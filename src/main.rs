use std::path::PathBuf;

use gpui_kit::{AppContext, WindowBounds, WindowOptions, px, size};

fn main() {
    let data_dir =
        std::env::var_os("GYFA_DATA").map_or_else(|| PathBuf::from("data/eve"), PathBuf::from);
    // An EFT file given on the command line is imported as a new fit.
    let import = std::env::args_os().nth(1).map(|path| {
        std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("{}: {e}", PathBuf::from(path).display());
            std::process::exit(1);
        })
    });

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1500.), px(900.)), cx)),
                app_id: Some("gyfa".into()),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| gyfa::ui::Gyfa::new(data_dir, import, window, cx))
            })
            .expect("the main window opens");
        });
}
