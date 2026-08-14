#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod controller;
mod controllers;
mod model;
mod view;
mod views;

fn main() {
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1_200.0, 760.0]),
        ..Default::default()
    };

    if let Err(error) = eframe::run_native(
        "BFFmpeg",
        native_options,
        Box::new(|creation_context| Ok(Box::new(view::EncoderApp::new(creation_context)))),
    ) {
        eprintln!("Failed to start BFFmpeg: {error}");
    }
}
