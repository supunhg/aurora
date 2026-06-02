use crate::theme;
use eframe::{egui, NativeOptions};

pub fn run() {
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(egui::Vec2::new(1200.0, 800.0))
            .with_min_inner_size(egui::Vec2::new(800.0, 500.0))
            .with_title("Aurora Editor"),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "Aurora",
        options,
        Box::new(|cc| {
            theme::setup_aurora_theme(&cc.egui_ctx);
            Box::new(crate::app::AuroraApp::new(cc))
        }),
    );
}
