// Hide the console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod canvas;
mod model;
mod render;
mod svg;

use eframe::egui;

fn main() -> eframe::Result {
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("architect.ai")
            .with_inner_size([1100.0, 700.0])
            .with_maximized(true)
            .with_min_inner_size([640.0, 400.0]),
        ..Default::default()
    };

    // Some (older Intel) Vulkan drivers crash during wgpu setup. On Windows, use DirectX 12
    // unless the user picks a backend explicitly via the WGPU_BACKEND env var.
    #[cfg(windows)]
    if std::env::var_os("WGPU_BACKEND").is_none() {
        if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
            setup.instance_descriptor.backends = eframe::egui_wgpu::wgpu::Backends::DX12;
        }
    }

    eframe::run_native(
        "architect.ai",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_theme(egui::Theme::Light);
            // Ctrl +/- should not scale the whole UI.
            cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
            Ok(Box::new(app::App::default()))
        }),
    )
}
