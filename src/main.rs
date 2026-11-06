use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result {
    let data_dir =
        std::env::var_os("GYFA_DATA").map_or_else(|| PathBuf::from("data/eve"), PathBuf::from);
    // An EFT file given on the command line is imported as a new fit.
    let import = match std::env::args_os().nth(1) {
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(e) => {
                eprintln!("{}: {e}", PathBuf::from(path).display());
                std::process::exit(1);
            }
        },
        None => None,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("gyfa")
            .with_app_id("gyfa")
            .with_inner_size([1400.0, 850.0]),
        ..Default::default()
    };
    eframe::run_native(
        "gyfa",
        options,
        Box::new(move |cc| Ok(Box::new(gyfa::app::App::new(cc, data_dir, import)))),
    )
}
