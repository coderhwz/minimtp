use bytes::Bytes;
use futures::{executor::block_on, stream};
use mtp_rs::mtp::{
    ByteRange, MtpDevice, MtpDeviceInfo, NewObjectInfo, ObjectHandle, ObjectInfo, Storage,
    StorageId, StorageInfo,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    time::{Duration, Instant},
};

pub type Trail = Vec<(Option<ObjectHandle>, String)>;
pub enum Command {
    Scan,
    Connect(u64),
    List(StorageId, Trail),
    Upload(StorageId, Option<ObjectHandle>, Vec<PathBuf>),
    Download(StorageId, Vec<ObjectInfo>, PathBuf),
}
pub enum Event {
    Devices(Vec<MtpDeviceInfo>),
    Connected(String, Vec<StorageInfo>),
    Listed(Trail, Vec<ObjectInfo>),
    Progress(String, u64, u64),
    Finished(Result<String, String>),
}
#[derive(Clone)]
pub struct Reporter {
    tx: Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
}
impl Reporter {
    fn send(&self, event: Event) {
        if self.tx.send(event).is_ok() {
            (self.wake)();
        }
    }
}
pub fn start(
    wake: impl Fn() + Send + Sync + 'static,
) -> (Sender<Command>, Receiver<Event>, Arc<AtomicBool>) {
    let (commands, rx) = mpsc::channel();
    let (tx, events) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let reporter = Reporter {
        tx,
        wake: Arc::new(wake),
    };
    std::thread::Builder::new()
        .name("mtp-transfer".into())
        .spawn(move || {
            #[cfg(feature = "virtual-device")]
            if let Some(directory) = std::env::var_os("MINIMTP_VIRTUAL_DIR") {
                let _ = mtp_rs::register_virtual_device(&mtp_rs::VirtualDeviceConfig {
                    model: "Virtual MTP (local test)".into(),
                    storages: vec![mtp_rs::VirtualStorageConfig {
                        description: "Test storage".into(),
                        capacity: 64 * 1024 * 1024 * 1024,
                        backing_dir: PathBuf::from(directory),
                        read_only: false,
                    }],
                    ..Default::default()
                });
            }
            let mut device: Option<MtpDevice> = None;
            for command in rx {
                let result: Result<Option<String>, String> = block_on(async {
                    match command {
                        Command::Scan => {
                            if let Some(old) = device.take() {
                                let _ = old.close().await;
                            }
                            let devices = MtpDevice::list_devices().map_err(mtp_error)?;
                            reporter.send(Event::Devices(devices));
                            Ok(None)
                        }
                        Command::Connect(location) => {
                            if let Some(old) = device.take() {
                                let _ = old.close().await;
                            }
                            let connected = MtpDevice::open_by_location(location)
                                .await
                                .map_err(mtp_error)?;
                            let name = format!(
                                "{} {}",
                                connected.device_info().manufacturer,
                                connected.device_info().model
                            );
                            let storages = connected.storages().await.map_err(mtp_error)?;
                            if storages.is_empty() {
                                return Err("设备没有可用存储，请解锁设备并允许文件传输。".into());
                            }
                            let infos = storages.iter().map(|s| s.info().clone()).collect();
                            device = Some(connected);
                            reporter.send(Event::Connected(name, infos));
                            Ok(None)
                        }
                        command => {
                            let dev = device.as_ref().ok_or("请先连接设备")?;
                            let sid = match &command {
                                Command::List(s, _)
                                | Command::Upload(s, _, _)
                                | Command::Download(s, _, _) => *s,
                                _ => unreachable!(),
                            };
                            let storage = dev.storage(sid).await.map_err(mtp_error)?;
                            match command {
                                Command::List(_, trail) => {
                                    let parent = trail.last().and_then(|p| p.0);
                                    let mut files =
                                        storage.list_objects(parent).await.map_err(mtp_error)?;
                                    files.sort_by_cached_key(|f| {
                                        (!f.is_folder(), f.filename.to_lowercase())
                                    });
                                    reporter.send(Event::Listed(trail, files));
                                    Ok(None)
                                }
                                Command::Upload(_, parent, paths) => {
                                    upload(&storage, parent, paths, &flag, &reporter).await?;
                                    Ok(Some("上传完成".into()))
                                }
                                Command::Download(_, entries, destination) => {
                                    for entry in entries {
                                        safe_name(&entry.filename)?;
                                        let target = destination.join(&entry.filename);
                                        download(&storage, entry, target, &flag, &reporter).await?;
                                    }
                                    Ok(Some("下载完成".into()))
                                }
                                _ => unreachable!(),
                            }
                        }
                    }
                });
                match result {
                    Ok(Some(message)) => reporter.send(Event::Finished(Ok(message))),
                    Err(error) => reporter.send(Event::Finished(Err(error))),
                    Ok(None) => (),
                }
            }
            if let Some(device) = device {
                let _ = block_on(device.close());
            }
        })
        .expect("Cannot start MTP worker");
    (commands, events, cancel)
}
fn mtp_error(error: mtp_rs::Error) -> String {
    if error.is_exclusive_access() {
        "设备正被其他程序占用。请退出 Android File Transfer、照片或图像捕捉，重新插拔 USB 后再连接。".into()
    } else {
        format!("{error}。请检查 USB 连接，解锁设备并选择「文件传输 / MTP」。")
    }
}
fn cancelled(flag: &AtomicBool) -> Result<(), String> {
    if flag.load(Ordering::Relaxed) {
        Err("已取消；已完成的文件会保留".into())
    } else {
        Ok(())
    }
}
pub fn safe_name(name: &str) -> Result<(), String> {
    let mut parts = Path::new(name).components();
    if matches!(parts.next(), Some(Component::Normal(_)))
        && parts.next().is_none()
        && !name.contains(['\0', '\\', ':'])
    {
        Ok(())
    } else {
        Err(format!("设备返回了不安全的文件名：{name:?}"))
    }
}
struct Progress<'a> {
    reporter: &'a Reporter,
    name: String,
    last: Instant,
}
impl<'a> Progress<'a> {
    fn new(reporter: &'a Reporter, name: &str) -> Self {
        Self {
            reporter,
            name: name.into(),
            last: Instant::now() - Duration::from_secs(1),
        }
    }
    fn update(&mut self, done: u64, total: u64) {
        if self.last.elapsed() >= Duration::from_millis(100) || done == total {
            self.reporter
                .send(Event::Progress(self.name.clone(), done, total));
            self.last = Instant::now();
        }
    }
}
async fn upload(
    storage: &Storage,
    parent: Option<ObjectHandle>,
    paths: Vec<PathBuf>,
    cancel: &AtomicBool,
    reporter: &Reporter,
) -> Result<(), String> {
    let mut pending: Vec<_> = paths.into_iter().rev().map(|p| (p, parent)).collect();
    let mut listed_parent = None;
    let mut existing = std::collections::HashSet::new();
    while let Some((path, parent)) = pending.pop() {
        cancelled(cancel)?;
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err(format!("不跟随符号链接：{}", path.display()));
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("文件名需要是有效的 Unicode")?;
        safe_name(name)?;
        // Never silently replace existing device files or merge directories.
        if listed_parent != Some(parent) {
            existing = storage
                .list_objects(parent)
                .await
                .map_err(mtp_error)?
                .into_iter()
                .map(|f| f.filename)
                .collect();
            listed_parent = Some(parent);
        }
        if !existing.insert(name.to_owned()) {
            return Err(format!(
                "设备上已存在「{name}」，已停止以避免覆盖。已完成的文件会保留。"
            ));
        }
        if metadata.is_dir() {
            let folder = storage
                .create_folder(parent, name)
                .await
                .map_err(mtp_error)?;
            for child in fs::read_dir(&path).map_err(|e| e.to_string())? {
                pending.push((child.map_err(|e| e.to_string())?.path(), Some(folder)));
            }
        } else if metadata.is_file() {
            let mut file = File::open(&path).map_err(|e| e.to_string())?;
            let size = metadata.len();
            let data = stream::poll_fn(move |_| {
                let mut chunk = vec![0; 64 * 1024];
                std::task::Poll::Ready(match file.read(&mut chunk) {
                    Ok(0) => None,
                    Ok(n) => {
                        chunk.truncate(n);
                        Some(Ok(Bytes::from(chunk)))
                    }
                    Err(e) => Some(Err(e)),
                })
            });
            let mut progress = Progress::new(reporter, name);
            progress.update(0, size);
            let result = storage
                .upload_with_progress(parent, NewObjectInfo::file(name, size), data, |p| {
                    progress.update(p.bytes_transferred, size);
                    if cancel.load(Ordering::Relaxed) {
                        std::ops::ControlFlow::Break(())
                    } else {
                        std::ops::ControlFlow::Continue(())
                    }
                })
                .await;
            if let Err(error) = result {
                let cleanup = if let Some(partial) = error.partial {
                    storage.delete(partial).await.err()
                } else {
                    None
                };
                if let Some(cleanup) = cleanup {
                    return Err(format!(
                        "上传失败：{}；设备上的未完成文件清理失败：{cleanup}",
                        error.source
                    ));
                }
                cancelled(cancel)?;
                return Err(mtp_error(error.source));
            }
        } else {
            return Err(format!("不支持的文件类型：{}", path.display()));
        }
    }
    Ok(())
}
async fn download(
    storage: &Storage,
    entry: ObjectInfo,
    destination: PathBuf,
    cancel: &AtomicBool,
    reporter: &Reporter,
) -> Result<(), String> {
    let mut pending = vec![(entry, destination)];
    while let Some((entry, path)) = pending.pop() {
        cancelled(cancel)?;
        safe_name(&entry.filename)?;
        if entry.is_folder() {
            fs::create_dir(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            for child in storage
                .list_objects(Some(entry.handle))
                .await
                .map_err(mtp_error)?
            {
                safe_name(&child.filename)?;
                let target = path.join(&child.filename);
                pending.push((child, target));
            }
        } else {
            // create_new protects existing files, including symlinks. Incomplete new files are removed.
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let result = async {
                let mut data = storage
                    .download(entry.handle, ByteRange::Full)
                    .await
                    .map_err(mtp_error)?;
                let mut progress = Progress::new(reporter, &entry.filename);
                let mut written = 0;
                progress.update(0, data.size());
                while let Some(chunk) = data.next_chunk().await {
                    if cancel.load(Ordering::Relaxed) {
                        let _ = data.cancel(Duration::from_secs(2)).await;
                        return cancelled(cancel);
                    }
                    let chunk = match chunk {
                        Ok(chunk) => chunk,
                        Err(error) => {
                            let _ = data.cancel(Duration::from_secs(2)).await;
                            return Err(mtp_error(error));
                        }
                    };
                    if let Err(error) = file.write_all(&chunk) {
                        let _ = data.cancel(Duration::from_secs(2)).await;
                        return Err(error.to_string());
                    }
                    written += chunk.len() as u64;
                    progress.update(written, data.size());
                }
                if written != data.size() {
                    return Err("设备返回的文件长度不完整".into());
                }
                file.flush().map_err(|e| e.to_string())
            }
            .await;
            drop(file);
            if result.is_err() {
                let _ = fs::remove_file(&path);
            }
            result?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_device_path_traversal() {
        for name in [
            "",
            ".",
            "..",
            "../secret",
            "/etc/passwd",
            "a/b",
            "a\0b",
            "a\\b",
            "C:secret",
        ] {
            assert!(safe_name(name).is_err(), "{name:?}");
        }
        assert!(safe_name("照片 01.jpg").is_ok());
    }
    #[test]
    fn streamed_round_trip_and_collision_protection() {
        use mtp_rs::{VirtualDeviceConfig, VirtualStorageConfig};
        let root = std::env::temp_dir().join(format!("minimtp-test-{}", std::process::id()));
        fs::create_dir_all(root.join("device")).unwrap();
        fs::create_dir_all(root.join("out")).unwrap();
        let input = root.join("sample.bin");
        let payload: Vec<u8> = (0..200_123).map(|n| (n % 251) as u8).collect();
        fs::write(&input, &payload).unwrap();
        let (tx, _) = mpsc::channel();
        let reporter = Reporter {
            tx,
            wake: Arc::new(|| {}),
        };
        let cancel = AtomicBool::new(false);
        block_on(async {
            let dev = MtpDevice::builder()
                .open_virtual(VirtualDeviceConfig {
                    storages: vec![VirtualStorageConfig {
                        backing_dir: root.join("device"),
                        description: "Test storage".into(),
                        capacity: 1024 * 1024 * 1024,
                        read_only: false,
                    }],
                    ..Default::default()
                })
                .await
                .unwrap();
            let storage = dev.storages().await.unwrap().remove(0);
            upload(&storage, None, vec![input.clone()], &cancel, &reporter)
                .await
                .unwrap();
            assert!(upload(&storage, None, vec![input], &cancel, &reporter)
                .await
                .is_err());
            let entry = storage.list_objects(None).await.unwrap().remove(0);
            let target = root.join("out/sample.bin");
            download(&storage, entry.clone(), target.clone(), &cancel, &reporter)
                .await
                .unwrap();
            assert_eq!(fs::read(&target).unwrap(), payload);
            assert!(
                download(&storage, entry, target.clone(), &cancel, &reporter)
                    .await
                    .is_err()
            );
            assert_eq!(fs::read(target).unwrap(), payload);
            let folder = root.join("nested");
            fs::create_dir_all(folder.join("child")).unwrap();
            fs::write(folder.join("child/照片.txt"), b"nested content").unwrap();
            upload(&storage, None, vec![folder], &cancel, &reporter)
                .await
                .unwrap();
            let folder = storage
                .list_objects(None)
                .await
                .unwrap()
                .into_iter()
                .find(|f| f.is_folder())
                .unwrap();
            download(
                &storage,
                folder,
                root.join("out/nested"),
                &cancel,
                &reporter,
            )
            .await
            .unwrap();
            assert_eq!(
                fs::read(root.join("out/nested/child/照片.txt")).unwrap(),
                b"nested content"
            );
            cancel.store(true, Ordering::Relaxed);
            assert!(upload(
                &storage,
                None,
                vec![root.join("sample.bin")],
                &cancel,
                &reporter
            )
            .await
            .is_err());
            dev.close().await.unwrap();
        });
        fs::remove_dir_all(root).unwrap();
    }
}
