# MiniMTP

轻量 Rust MTP 文件传输工具。egui + OpenGL 界面，鼠尾草绿配色；MTP 后端使用纯 Rust 的 [mtp-rs](https://github.com/vdavid/mtp-rs)。支持 macOS、Windows 和 Linux。

## 使用

1. 用支持数据传输的 USB 线连接设备，解锁并选择「文件传输 / MTP」。
2. 打开程序，或点击 **Refresh** 扫描设备。左侧可切换设备与存储。
3. 双击文件夹进入，点击路径或 **Back** 返回。
4. 将文件或文件夹拖入窗口，上传到当前目录；**Upload** 按钮用于选择文件上传。
5. 选中项目后点击 **Download**，选择本地目录。支持文件夹递归下载。

Cmd/Ctrl 点击多选，Shift 点击范围选择，Cmd/Ctrl+A 选择当前搜索结果。传输时点击 **Cancel** 或按 Esc 取消，等待设备响应后即可继续。取消时保留已完成的文件与已创建的文件夹；当前未完成的下载会删除，未完成的上传尝试清理，清理失败会显示错误。

同名文件或文件夹会停止操作，避免静默覆盖或合并。上传不跟随符号链接。第一版界面使用英文，设备名、文件名和错误信息支持系统中文字体。

当前支持**拖入上传**；下载使用按钮，尚不支持拖出到 Finder/资源管理器。不包含删除、重命名、自动同步、后台服务或缩略图预览。

## 构建

Rust 1.88+，建议使用最新稳定版。提交的 Cargo.lock 固定依赖版本。

```sh
cargo run --locked
cargo build --release --locked
cargo test --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
```

macOS：

```sh
./scripts/package-macos.sh
```

生成 `dist/MiniMTP.app` 与 `dist/MiniMTP-macos.zip`。脚本使用系统 Swift/AppKit 绘制图标，并做本机 ad-hoc 签名；没有开发者签名或 Apple 公证。默认生成当前主机架构，当前交付为 Apple Silicon。

Windows：在 Windows 上执行 `cargo build --release --locked`，产物为 `target/release/minimtp.exe`。MTP 走系统 WPD 后端，不需要替换 USB 驱动。

Linux：需要 OpenGL 桌面环境及系统构建依赖，例如 Ubuntu/Debian：

```sh
sudo apt install build-essential pkg-config libudev-dev libxkbcommon-dev libwayland-dev libgl1-mesa-dev
cargo build --release --locked
```

文件选择器使用 XDG Desktop Portal，桌面环境需要相应 portal 服务。USB 设备还需要当前用户的访问权限；应按设备的 vendor/product ID 配置 udev 规则。中文文件名需要系统安装 CJK 字体，例如 `fonts-noto-cjk`。

## 体积与内存

- 发布配置使用 `opt-level=z`、LTO、单 codegen unit、去符号，生产包不启用虚拟设备依赖。
- 单工作线程拥有 MTP 会话，空闲时阻塞等待命令；状态变化唤醒 UI，无定时设备轮询。
- 上传按 64 KiB 分块读取；下载消费 MTP 分块流，不缓存完整文件。文件大小增加不会导致整文件内存分配。
- 列表只渲染可见行，只保留当前目录；搜索和排序仍需要当前目录元数据。大量条目会增加内存。
- 中文字体首次遇到 CJK 内容时从系统加载，不嵌入大字体。可设置 `MINIMTP_FONT=/absolute/path/font.ttf` 或 `.ttc` 指定字体。
- 空闲不持续重绘；传输期间进度事件最多约每秒 10 次，忙碌指示器会请求动画帧。

本机 Apple Silicon 发布二进制约 **3.1 MiB**，包含图标的 app 约 **3.4 MiB**，zip 约 **2 MiB**。本机 1060×720、Retina 2×、无设备的发布版空闲 RSS 实测约 **121 MiB**（`ps`），CPU **0.0%**；这包含界面/驱动等开销，并非只有传输缓冲区。内存应以实际窗口大小、显示缩放、驱动和字体加载情况测量，不能把 64 KiB 传输块等同于整个程序的内存。

## 验证与限制

已通过 macOS 构建、Clippy、虚拟设备的字节一致性往返、嵌套目录、中文路径、同名保护、取消入口和路径穿越校验；实际 UI 验证了设备列表、双击导航、系统上传选择器、上传后列表刷新。Windows x86_64 与 Linux x86_64 已通过 `cargo check` 交叉目标编译检查（Rust 1.94.1）；这不等于这两个系统的实际运行验证。

当前没有接入真实 USB MTP 设备，尚未验证实际设备兼容性、拔线恢复、大文件吞吐或跨平台拖拽行为。设备被 OpenMTP、Android File Transfer、照片或图像捕捉占用时，退出相应程序，重新插拔后刷新。程序不会主动结束其他进程或更改系统服务。

用于无硬件测试的虚拟设备需显式启用：

```sh
mkdir -p /tmp/minimtp-device
MINIMTP_VIRTUAL_DIR=/tmp/minimtp-device cargo run --features virtual-device
```

该目录会作为本地可写的模拟设备，上传/下载操作会真实读写目录。默认发布包没有这一功能。
