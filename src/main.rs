use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use efa::ui::{Efa, Hub, Role};
use gpui_kit::{AppContext, WindowBounds, WindowOptions, px, size};

fn main() {
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    if first.as_deref() == Some("--build-pack".as_ref()) {
        build_pack(args.next(), args.next());
    }
    efa::persist::move_old_data();
    efa::errors::install_panic_hook();
    efa::errors::queue_last_crash();
    // An EFT file given on the command line is imported as a new fit.
    let import = first.map(|path| {
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
            let hub = cx.new(|cx| Hub::new(import, cx));
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| Efa::new(hub, Role::Main, window, cx))
            })
            .expect("the main window opens");
        });
}

/// `efa --build-pack <deimos dir> <out file>`: write the data pack and exit.
fn build_pack(dump: Option<OsString>, out: Option<OsString>) -> ! {
    let (Some(dump), Some(out)) = (dump, out) else {
        eprintln!("usage: efa --build-pack <deimos dir> <out file>");
        std::process::exit(2);
    };
    let start = std::time::Instant::now();
    match efa::pack::Pack::write(Path::new(&dump), Path::new(&out)) {
        Ok(size) => {
            println!(
                "wrote {} ({:.1} MB) in {:.1} s",
                Path::new(&out).display(),
                size as f64 / 1e6,
                start.elapsed().as_secs_f64()
            );
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("building the data pack: {e}");
            std::process::exit(1);
        }
    }
}
