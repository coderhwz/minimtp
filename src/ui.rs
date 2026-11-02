use crate::transfer::{self, Command, Event, Trail};
use eframe::egui::{
    self, Align, Color32, FontId, Frame, Layout, Margin, RichText, ScrollArea, Stroke, Vec2,
};
use mtp_rs::mtp::{MtpDeviceInfo, ObjectInfo, StorageInfo};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender},
        Arc,
    },
};

const INK: Color32 = Color32::from_rgb(35, 49, 48);
const MUTED: Color32 = Color32::from_rgb(125, 137, 132);
const GREEN: Color32 = Color32::from_rgb(47, 109, 87);
const PAPER: Color32 = Color32::from_rgb(250, 250, 246);
const LINE: Color32 = Color32::from_rgb(229, 233, 225);

pub struct App {
    commands: Sender<Command>,
    events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    devices: Vec<MtpDeviceInfo>,
    device_index: usize,
    storages: Vec<StorageInfo>,
    storage_index: usize,
    entries: Vec<ObjectInfo>,
    trail: Trail,
    selected: BTreeSet<usize>,
    search: String,
    connected: bool,
    busy: bool,
    transferring: bool,
    status: String,
    error: Option<String>,
    progress: Option<(String, u64, u64)>,
    notice: Option<String>,
    cjk_loaded: bool,
}
impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::light();
        style.visuals.panel_fill = PAPER;
        style.visuals.window_fill = PAPER;
        style.visuals.override_text_color = Some(INK);
        style.visuals.selection.bg_fill = Color32::from_rgb(219, 235, 220);
        style.visuals.selection.stroke = Stroke::new(1., GREEN);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(239, 242, 234);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(239, 242, 234);
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1., LINE);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(226, 234, 221);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(226, 234, 221);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(209, 225, 208);
        style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(7);
        style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(7);
        style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(7);
        style.spacing.item_spacing = Vec2::new(12., 10.);
        style.spacing.button_padding = Vec2::new(13., 8.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(13.));
        cc.egui_ctx.set_style(style);
        let ctx = cc.egui_ctx.clone();
        let (commands, events, cancel) = transfer::start(move || ctx.request_repaint());
        let mut app = Self {
            commands,
            events,
            cancel,
            devices: vec![],
            device_index: 0,
            storages: vec![],
            storage_index: 0,
            entries: vec![],
            trail: vec![],
            selected: BTreeSet::new(),
            search: String::new(),
            connected: false,
            busy: false,
            transferring: false,
            status: "Looking for a device…".into(),
            error: None,
            progress: None,
            notice: None,
            cjk_loaded: false,
        };
        app.begin(Command::Scan, "Looking for a device…", false);
        app
    }
    fn begin(&mut self, command: Command, message: &str, transfer: bool) {
        self.cancel.store(false, Ordering::Relaxed);
        self.busy = true;
        self.transferring = transfer;
        self.progress = None;
        self.status = message.into();
        self.notice = None;
        if self.commands.send(command).is_err() {
            self.busy = false;
            self.error = Some("The transfer worker stopped. Please reopen MiniMTP.".into());
        }
    }
    fn storage(&self) -> Option<&StorageInfo> {
        self.storages.get(self.storage_index)
    }
    fn can_upload(&self) -> bool {
        !self.busy && self.connected && self.storage().is_some_and(|s| s.is_writable)
    }
    fn list(&mut self, trail: Trail) {
        if let Some(s) = self.storage() {
            self.begin(Command::List(s.id, trail), "Reading folder…", false);
        }
    }
    fn root(&mut self) {
        self.entries.clear();
        self.selected.clear();
        self.search.clear();
        if let Some(s) = self.storage() {
            self.list(vec![(None, s.description.clone())]);
        }
    }
    fn upload(&mut self, paths: Vec<PathBuf>) {
        if !self.can_upload() || paths.is_empty() {
            return;
        }
        if let Some(s) = self.storage() {
            let command = Command::Upload(s.id, self.trail.last().and_then(|p| p.0), paths);
            self.begin(command, "Uploading…", true);
        }
    }
    fn refresh(&mut self) {
        self.connected = false;
        self.entries.clear();
        self.selected.clear();
        self.storages.clear();
        self.trail.clear();
        self.begin(Command::Scan, "Looking for a device…", false);
    }
    fn receive(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Devices(devices) => {
                    self.devices = devices;
                    self.device_index = 0;
                    self.busy = false;
                    if let Some(device) = self.devices.first() {
                        self.begin(Command::Connect(device.location_id), "Connecting…", false);
                    } else {
                        self.status = "No MTP device found. Connect a device, then refresh.".into();
                    }
                }
                Event::Connected(name, storages) => {
                    self.storages = storages;
                    self.storage_index = 0;
                    self.connected = true;
                    self.busy = false;
                    self.status = format!("Connected to {name}");
                    self.root();
                }
                Event::Listed(trail, entries) => {
                    self.trail = trail;
                    self.entries = entries;
                    self.selected.clear();
                    self.busy = false;
                    if let Some(message) = self.notice.take() {
                        self.status = message;
                    } else {
                        self.status = "Drop files to upload · Double-click a folder to open".into();
                    }
                }
                Event::Progress(name, done, total) => {
                    self.progress = Some((name, done, total));
                }
                Event::Finished(result) => {
                    let was_transfer = self.transferring;
                    self.busy = false;
                    self.transferring = false;
                    match result {
                        Ok(message) => {
                            self.notice = Some(message);
                        }
                        Err(error) => {
                            if !was_transfer {
                                self.connected = false;
                            }
                            self.status = error.clone();
                            self.error = Some(error);
                        }
                    }
                    self.progress = None;
                    // Refresh after errors too: cancellation may leave completed files/folders.
                    if was_transfer && self.connected {
                        let notice = self.notice.take();
                        self.list(self.trail.clone());
                        self.notice = notice;
                    }
                }
            }
        }
    }
    fn choose_upload(&mut self) {
        // Dialogs run on the UI thread; the worker never calls platform UI APIs.
        if let Some(paths) = rfd::FileDialog::new()
            .set_title("Choose files to upload")
            .pick_files()
        {
            self.upload(paths);
        }
    }
    fn choose_download(&mut self) {
        let entries: Vec<_> = self
            .selected
            .iter()
            .filter_map(|i| self.entries.get(*i).cloned())
            .collect();
        if entries.is_empty() {
            return;
        }
        if let Some(destination) = rfd::FileDialog::new()
            .set_title("Download to folder")
            .pick_folder()
        {
            if let Some(s) = self.storage() {
                self.begin(
                    Command::Download(s.id, entries, destination),
                    "Downloading…",
                    true,
                );
            }
        }
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar").exact_width(222.).resizable(false)
            .frame(Frame::new().fill(Color32::from_rgb(239,242,234)).inner_margin(Margin::symmetric(22,28))).show(ctx,|ui| {
            ui.horizontal(|ui| {
                let (r,_) = ui.allocate_exact_size(Vec2::splat(36.),egui::Sense::hover());
                ui.painter().rect_filled(r,10.,GREEN);
                draw_transfer(ui,r.center(),Color32::WHITE,10.);
                ui.vertical(|ui| {
                    ui.label(RichText::new("MiniMTP").size(21.).strong());
                    ui.label(RichText::new("LESS, BUT BETTER.").size(8.5).color(MUTED));
                });
            });
            ui.add_space(39.);
            section(ui,"DEVICE");
            ui.add_space(8.);
            let old_device=self.device_index;
            ui.add_enabled_ui(!self.busy && !self.devices.is_empty(),|ui| {
                egui::ComboBox::from_id_salt("devices").width(169.).selected_text(self.devices.get(self.device_index).and_then(|d|d.product.as_deref()).unwrap_or("No device connected")).show_ui(ui,|ui| {
                    for (i,d) in self.devices.iter().enumerate() {ui.selectable_value(&mut self.device_index,i,d.product.as_deref().unwrap_or("MTP device"));}
                });
            });
            if old_device!=self.device_index {
                self.connected=false; self.entries.clear(); self.selected.clear();
                self.begin(Command::Connect(self.devices[self.device_index].location_id),"Connecting…",false);
            }
            ui.add_space(7.);
            ui.horizontal(|ui| {
                let (r,_) = ui.allocate_exact_size(Vec2::splat(7.),egui::Sense::hover());
                ui.painter().circle_filled(r.center(),3.,if self.connected {GREEN} else {MUTED});
                ui.label(RichText::new(if self.connected {"Connected via USB"} else {"Waiting for a connection"}).size(11.).color(MUTED));
            });
            ui.add_space(34.);
            section(ui,"STORAGE"); ui.add_space(8.);
            let old_storage=self.storage_index;
            ui.add_enabled_ui(self.connected && !self.busy,|ui| {
                egui::ComboBox::from_id_salt("storages").width(169.).selected_text(self.storage().map(|s|s.description.as_str()).unwrap_or("No storage available")).show_ui(ui,|ui| {
                    for (i,s) in self.storages.iter().enumerate() {ui.selectable_value(&mut self.storage_index,i,&s.description);}
                });
            });
            if old_storage!=self.storage_index {self.root();}
            if let Some(s) = self.storage() {
                ui.add_space(12.);
                let used = if s.total_capacity==0 {0.} else {1. - s.free_space as f32/s.total_capacity as f32};
                ui.add(egui::ProgressBar::new(used).fill(GREEN).desired_height(4.));
                ui.label(RichText::new(format!("{} free of {}",size(s.free_space),size(s.total_capacity))).size(10.).color(MUTED));
                if !s.is_writable {ui.label(RichText::new("Read-only storage").size(11.).color(MUTED));}
            } else {ui.label(RichText::new("Your space, at a glance.").size(11.).color(MUTED));}
            ui.with_layout(Layout::bottom_up(Align::LEFT),|ui| {
                ui.label(RichText::new("USB DIRECT  /  NO CLOUD").size(9.).color(MUTED));
                ui.add_space(20.);
                Frame::new().fill(Color32::from_rgb(225,233,218)).corner_radius(12.).inner_margin(16.).show(ui,|ui| {
                    ui.set_width(146.);
                    ui.set_height(132.);
                    ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                    ui.label(RichText::new("A little drag.\nA lot less effort.").size(17.).strong().color(GREEN));
                    ui.add_space(9.);
                    ui.label(RichText::new("Drop files or folders into the window to upload. Select items to download.").size(11.).color(Color32::from_rgb(108,128,113)));
                    });
                });
            });
        });
    }
    fn footer(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .exact_height(if self.transferring { 90. } else { 48. })
            .frame(
                Frame::new()
                    .fill(PAPER)
                    .inner_margin(Margin::symmetric(28, 12))
                    .stroke(Stroke::new(1., LINE)),
            )
            .show(ctx, |ui| {
                if let Some((name, done, total)) = &self.progress {
                    let fraction = if *total == 0 {
                        0.
                    } else {
                        *done as f32 / *total as f32
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(name).size(12.).strong());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("{} / {}", size(*done), size(*total)))
                                    .size(11.)
                                    .color(MUTED),
                            );
                        });
                    });
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .fill(GREEN)
                            .desired_height(4.),
                    );
                }
                ui.horizontal(|ui| {
                    if self.busy {
                        ui.spinner();
                    }
                    ui.add(
                        egui::Label::new(RichText::new(&self.status).size(11.).color(MUTED))
                            .truncate(),
                    )
                    .on_hover_text(&self.status);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.transferring {
                            if ui.small_button("Cancel").clicked() {
                                self.cancel.store(true, Ordering::Relaxed);
                                self.status = "Cancelling, waiting for the device…".into();
                            }
                        } else if self.connected {
                            ui.label(
                                RichText::new(format!("{} items", self.entries.len()))
                                    .size(11.)
                                    .color(MUTED),
                            );
                        }
                    });
                });
            });
    }
    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(if self.connected {
                        self.trail
                            .last()
                            .map(|p| p.1.as_str())
                            .unwrap_or("Your files")
                    } else {
                        "Your files. A little closer."
                    })
                    .size(28.)
                    .strong(),
                );
                ui.add_space(4.);
                ui.horizontal(|ui| {
                    if self.connected {
                        let mut navigate = None;
                        for (i, (_, name)) in self.trail.iter().enumerate() {
                            if i > 0 {
                                ui.label(RichText::new("/").color(MUTED));
                            }
                            if ui
                                .add_enabled(
                                    !self.busy,
                                    egui::Button::new(RichText::new(name).size(11.).color(MUTED))
                                        .frame(false),
                                )
                                .clicked()
                            {
                                navigate = Some(i);
                            }
                        }
                        if let Some(i) = navigate {
                            self.search.clear();
                            self.list(self.trail[..=i].to_vec());
                        }
                    } else {
                        ui.label(
                            RichText::new("A quiet space for everything you carry.")
                                .size(12.)
                                .color(MUTED),
                        );
                    }
                });
            });
        });
        ui.add_space(25.);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.busy && self.trail.len() > 1,
                    egui::Button::new("Back"),
                )
                .on_hover_text("Parent folder")
                .clicked()
            {
                self.search.clear();
                self.list(self.trail[..self.trail.len() - 1].to_vec());
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Refresh"))
                .clicked()
            {
                self.refresh();
            }
            if ui
                .add_enabled(
                    self.can_upload(),
                    egui::Button::new(RichText::new("Upload").color(Color32::WHITE)).fill(GREEN),
                )
                .clicked()
            {
                self.choose_upload();
            }
            if ui
                .add_enabled(
                    !self.busy && !self.selected.is_empty(),
                    egui::Button::new("Download"),
                )
                .clicked()
            {
                self.choose_download();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Search this folder")
                        .desired_width((ui.available_width() - 18.).clamp(90., 220.))
                        .margin(Vec2::new(10., 8.)),
                );
            });
        });
        ui.add_space(20.);
    }
    fn files(&mut self, ui: &mut egui::Ui) {
        let query = self.search.to_lowercase();
        let visible: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, f)| f.filename.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect();
        if !self.connected || visible.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height()/2.-120.).max(12.));
                let (r,_) = ui.allocate_exact_size(Vec2::splat(88.),egui::Sense::hover());
                ui.painter().rect_filled(r,26.,Color32::from_rgb(234,239,225));
                draw_transfer(ui,r.center(),GREEN,23.);
                ui.add_space(23.);
                ui.label(RichText::new(if !self.connected {"Good things travel light."} else if query.is_empty() {"Room for something new."} else {"Nothing found."}).size(23.).strong());
                ui.add_space(11.);
                ui.label(RichText::new(if !self.connected {"Connect your device with a USB data cable.\nUnlock it and choose File transfer / MTP."} else if query.is_empty() {"Drop files or folders here to upload."} else {"Try a different search."}).size(13.).color(MUTED));
                if !self.connected {
                    ui.add_space(22.);
                    if ui.add_enabled(!self.busy,egui::Button::new("Find my device").fill(Color32::from_rgb(229,237,221))).clicked() {self.refresh();}
                }
            });
            return;
        }
        let width = ui.available_width();
        let name_width = (width - 258.).max(150.);
        let (header, _) = ui.allocate_exact_size(Vec2::new(width, 22.), egui::Sense::hover());
        for (name, x) in [
            ("NAME", 42.),
            ("SIZE", name_width + 12.),
            ("MODIFIED", name_width + 108.),
        ] {
            ui.painter().text(
                egui::pos2(header.left() + x, header.center().y),
                egui::Align2::LEFT_CENTER,
                name,
                FontId::proportional(10.),
                MUTED,
            );
        }
        ui.separator();
        let mut open = None;
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, 47., visible.len(), |ui, range| {
                for row in range {
                    let index = visible[row];
                    let entry = &self.entries[index];
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::new(width, 47.), egui::Sense::click());
                    let selected = self.selected.contains(&index);
                    if selected || response.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            7.,
                            if selected {
                                Color32::from_rgb(226, 237, 219)
                            } else {
                                Color32::from_rgb(242, 244, 236)
                            },
                        );
                    }
                    draw_file(
                        ui,
                        egui::pos2(rect.left() + 13., rect.center().y - 10.),
                        entry.is_folder(),
                    );
                    let name_rect = egui::Rect::from_min_size(
                        egui::pos2(rect.left() + 42., rect.top()),
                        Vec2::new(name_width - 48., 47.),
                    );
                    let mut name_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(name_rect)
                            .layout(Layout::left_to_right(Align::Center)),
                    );
                    name_ui.add(
                        egui::Label::new(RichText::new(&entry.filename).size(13.))
                            .selectable(false)
                            .truncate(),
                    );
                    ui.painter().text(
                        egui::pos2(rect.left() + name_width + 12., rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        if entry.is_folder() {
                            "—".into()
                        } else {
                            size(entry.size)
                        },
                        FontId::proportional(12.),
                        MUTED,
                    );
                    let date = entry
                        .modified
                        .map(|d| format!("{}-{:02}-{:02}", d.year, d.month, d.day))
                        .unwrap_or_else(|| "—".into());
                    ui.painter().text(
                        egui::pos2(rect.left() + name_width + 108., rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        date,
                        FontId::proportional(12.),
                        MUTED,
                    );
                    if response.clicked() {
                        if ui.input(|i| i.modifiers.command || i.modifiers.ctrl) {
                            if !self.selected.remove(&index) {
                                self.selected.insert(index);
                            }
                        } else if ui.input(|i| i.modifiers.shift) {
                            let anchor = self.selected.last().copied().unwrap_or(index);
                            self.selected.extend(
                                visible
                                    .iter()
                                    .copied()
                                    .filter(|i| *i >= anchor.min(index) && *i <= anchor.max(index)),
                            );
                        } else {
                            self.selected.clear();
                            self.selected.insert(index);
                        }
                    }
                    if response.double_clicked() && entry.is_folder() && !self.busy {
                        open = Some((entry.handle, entry.filename.clone()));
                    }
                    response.on_hover_text(&entry.filename);
                }
            });
        if let Some((handle, name)) = open {
            self.search.clear();
            let mut trail = self.trail.clone();
            trail.push((Some(handle), name));
            self.list(trail);
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive();
        if !self.cjk_loaded
            && (self.entries.iter().any(|f| needs_cjk(&f.filename))
                || self.storages.iter().any(|s| needs_cjk(&s.description))
                || needs_cjk(&self.status)
                || self.error.as_ref().is_some_and(|e| needs_cjk(e)))
        {
            load_cjk_font(ctx);
            self.cjk_loaded = true;
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.transferring {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.error=Some("A transfer is in progress. Cancel it and wait for the device before closing MiniMTP.".into());
        }
        self.sidebar(ctx);
        self.footer(ctx);
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(PAPER)
                    .inner_margin(Margin::symmetric(28, 28)),
            )
            .show(ctx, |ui| {
                self.header(ui);
                self.files(ui);
            });
        let (hovered, dropped) =
            ctx.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.clone()));
        if hovered {
            let rect = ctx.screen_rect().shrink(12.);
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drop"),
            ));
            painter.rect_filled(
                rect,
                16.,
                Color32::from_rgba_unmultiplied(239, 246, 229, 245),
            );
            painter.rect_stroke(rect, 16., Stroke::new(2., GREEN), egui::StrokeKind::Inside);
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if self.can_upload() {
                    "Release to upload to this folder"
                } else {
                    "Connect a writable device and finish the current task first"
                },
                FontId::proportional(21.),
                GREEN,
            );
        }
        if !dropped.is_empty() {
            if self.can_upload() {
                self.upload(dropped.into_iter().filter_map(|f| f.path).collect());
            } else {
                self.error = Some("请先连接可写设备，并等待当前操作完成后再拖入文件。".into());
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.transferring {
            self.cancel.store(true, Ordering::Relaxed);
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::A))
            && !ctx.wants_keyboard_input()
        {
            self.selected = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, f)| {
                    f.filename
                        .to_lowercase()
                        .contains(&self.search.to_lowercase())
                })
                .map(|(i, _)| i)
                .collect();
        }
        if let Some(error) = self.error.clone() {
            egui::Window::new("Couldn’t complete the operation")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .default_width(420.)
                .show(ctx, |ui| {
                    ui.label(&error);
                    ui.add_space(12.);
                    if ui.button("OK").clicked() {
                        self.error = None;
                    }
                });
        }
    }
}
fn section(ui: &mut egui::Ui, s: &str) {
    ui.label(RichText::new(s).size(10.).strong().color(MUTED));
}
fn size(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1048576 {
        format!("{:.1} KB", n as f64 / 1024.)
    } else if n < 1073741824 {
        format!("{:.1} MB", n as f64 / 1048576.)
    } else {
        format!("{:.1} GB", n as f64 / 1073741824.)
    }
}
fn draw_transfer(ui: &egui::Ui, c: egui::Pos2, color: Color32, s: f32) {
    let p = ui.painter();
    let stroke = Stroke::new((s / 7.).max(1.6), color);
    for (x, direction) in [(-s * 0.35, -1.), (s * 0.35, 1.)] {
        let tip = c + Vec2::new(x, direction * s * 0.65);
        p.line_segment([c + Vec2::new(x, -direction * s * 0.65), tip], stroke);
        p.line_segment(
            [tip, tip + Vec2::new(-s * 0.28, -direction * s * 0.28)],
            stroke,
        );
        p.line_segment(
            [tip, tip + Vec2::new(s * 0.28, -direction * s * 0.28)],
            stroke,
        );
    }
}
fn draw_file(ui: &egui::Ui, pos: egui::Pos2, folder: bool) {
    let p = ui.painter();
    if folder {
        p.rect_filled(
            egui::Rect::from_min_size(pos, Vec2::new(10., 6.)),
            2.,
            Color32::from_rgb(138, 166, 111),
        );
        p.rect_filled(
            egui::Rect::from_min_size(pos + Vec2::new(0., 4.), Vec2::new(23., 16.)),
            3.,
            Color32::from_rgb(157, 182, 127),
        );
    } else {
        let r = egui::Rect::from_min_size(pos + Vec2::new(3., 0.), Vec2::new(16., 21.));
        p.rect_filled(r, 3., Color32::from_rgb(231, 236, 226));
        p.rect_stroke(
            r,
            3.,
            Stroke::new(1., Color32::from_rgb(163, 181, 160)),
            egui::StrokeKind::Inside,
        );
        for y in [8., 12., 16.] {
            p.line_segment(
                [pos + Vec2::new(7., y), pos + Vec2::new(15., y)],
                Stroke::new(1., Color32::from_rgb(163, 181, 160)),
            );
        }
    }
}

// Load a CJK fallback only when a device name, filename or error needs it.
// System fonts keep the distribution small; no giant font is embedded in the binary.
fn needs_cjk(s: &str) -> bool {
    s.chars().any(|c| c >= '\u{2e80}')
}

fn load_cjk_font(ctx: &egui::Context) {
    let custom = std::env::var_os("MINIMTP_FONT").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let candidates = [
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
    ];
    #[cfg(target_os = "windows")]
    let candidates = ["C:/Windows/Fonts/msyh.ttc", "C:/Windows/Fonts/simsun.ttc"];
    #[cfg(target_os = "linux")]
    let candidates = [
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ];
    for path in custom
        .into_iter()
        .chain(candidates.iter().map(PathBuf::from))
    {
        if let Ok(data) = std::fs::read(&path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("cjk".into(), egui::FontData::from_owned(data).into());
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push("cjk".into());
            }
            ctx.set_fonts(fonts);
            break;
        }
    }
}
