use std::path::PathBuf;

use efa::ui::{Efa, Hub, Role};
use gpui_kit::{AppContext, WindowBounds, WindowOptions, px, size};

fn main() {
    efa::persist::move_old_data();
    efa::errors::install_panic_hook();
    efa::errors::queue_last_crash();
    efa::desktop::install();
    let data_dir =
        std::env::var_os("EFA_DATA").map_or_else(|| PathBuf::from("data/eve"), PathBuf::from);
    // An EFT file given on the command line is imported as a new fit.
    let import = std::env::args_os().nth(1).map(|path| {
        std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("{}: {e}", PathBuf::from(path).display());
            std::process::exit(1);
        })
    });

    gpui_kit::application()
        // The whole Lucide set, so any icon can be used, e.g. the overheat flame.
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            efa::ui::init_menus(cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1500.), px(900.)), cx)),
                app_id: Some("efa".into()),
                ..Default::default()
            };
            // Loading and the fits live in the hub, which every window shares.
            let hub = cx.new(|cx| Hub::new(data_dir, import, cx));
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| Efa::new(hub, Role::Main, window, cx))
            })
            .expect("the main window opens");
        });
}
