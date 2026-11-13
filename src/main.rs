// Hide the console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod canvas;
mod model;
mod render;
mod svg;
mod tabs;
mod validate;

use eframe::egui;

fn main() -> eframe::Result {
    // `alien_artefact --validate diagram.json`: check a diagram against the language rules without
    // opening a window. Prints one line per problem; exit code 1 when there are any.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--validate") {
        let Some(path) = args.get(1) else {
            eprintln!("usage: alien_artefact --validate <project.json>");
            std::process::exit(2);
        };
        let loaded = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|s| model::load_project(&s, "diagram"));
        let diagrams: Vec<(String, model::Diagram)> = match loaded {
            Ok(project) => project
                .items
                .into_iter()
                .filter_map(|i| match i.content {
                    model::ItemContent::Diagram(d) => Some((i.name, d)),
                    _ => None,
                })
                .collect(),
            Err(e) => {
                eprintln!("{path}: {e}");
                std::process::exit(2);
            }
        };
        let mut total = 0;
        for (name, diagram) in &diagrams {
            let issues = validate::validate(diagram);
            for i in &issues {
                // Name the diagram when the project has several.
                if diagrams.len() > 1 {
                    println!("[{name}] {:<4} {}", i.rule, i.message);
                } else {
                    println!("{:<4} {}", i.rule, i.message);
                }
            }
            total += issues.len();
        }
        println!("{total} problem(s)");
        std::process::exit(if total == 0 { 0 } else { 1 });
    }

    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Alien Artefact")
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
        "Alien Artefact",
        options,
        Box::new(|cc| {
            // Image tabs show PNG/JPEG files.
            egui_extras::install_image_loaders(&cc.egui_ctx);
            // Ctrl +/- should not scale the whole UI.
            cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
            let mut app = app::App::default();
            // Optional diagram file to open: `alien_artefact project.json`
            if let Some(path) = std::env::args_os().nth(1) {
                app.open_path(path.into());
            }
            Ok(Box::new(app))
        }),
    )
}
