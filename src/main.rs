mod transfer;
mod ui;
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("MiniMTP")
            .with_inner_size([1060., 720.])
            .with_min_inner_size([820., 560.]),
        renderer: eframe::Renderer::Glow,
        multisampling: 0,
        depth_buffer: 0,
        stencil_buffer: 0,
        vsync: true,
        ..Default::default()
    };
    eframe::run_native(
        "MiniMTP",
        options,
        Box::new(|cc| Ok(Box::new(ui::App::new(cc)))),
    )
}
