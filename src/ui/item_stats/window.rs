//! Opening stats windows: where each one goes, and Shift putting a new one in place of the last.

use std::rc::Rc;

use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, Global, TitlebarOptions, Window,
    WindowBounds, WindowOptions, px, size,
};

use super::{ItemStats, Request, Target};
use crate::ui::{Hub, Loaded, item_data::ensure_data};

/// pyfa's stats window size on GTK, where tabs take more room.
const WINDOW_W: f32 = 640.;
const WINDOW_H: f32 = 600.;
/// How far each new stats window sits from the previous one.
const CASCADE: f32 = 30.;

/// The open stats windows, for Shift to replace the last one used.
#[derive(Default)]
pub(super) struct StatsWindows {
    open: Vec<AnyWindowHandle>,
    pub(super) active: Option<AnyWindowHandle>,
    /// How many windows deep the cascade is.
    counter: u32,
}

impl Global for StatsWindows {}

impl StatsWindows {
    /// The window Shift replaces: the one last focused, else the newest still open.
    fn last(&mut self, cx: &App) -> Option<AnyWindowHandle> {
        let alive = cx.windows();
        self.open.retain(|w| alive.contains(w));
        self.active
            .filter(|w| self.open.contains(w))
            .or_else(|| self.open.last().copied())
    }
}

/// Open a stats window, or with `reuse` put it where the last one was and close that one.
pub fn open(
    loaded: &Rc<Loaded>,
    hub: &Entity<Hub>,
    request: Request,
    reuse: bool,
    window: &mut Window,
    cx: &mut App,
) {
    ensure_data(loaded, cx);
    let type_id = match request.target {
        Target::Type(id) => Some(id),
        Target::Fitted { item, .. } => request.fit.as_ref().and_then(|f| {
            // The pilot's implants only matter for an implant, which the fit lists itself
            // unless it uses the character's.
            let pilot = hub.read(cx).characters.pilot(f.character, &loaded.catalog);
            item.type_in(f, &pilot.implants)
        }),
    };
    let Some(type_id) = type_id else { return };

    let mut windows = cx
        .try_global::<StatsWindows>()
        .map_or_default(|w| StatsWindows {
            open: w.open.clone(),
            active: w.active,
            counter: w.counter,
        });
    let reused = if reuse {
        windows.last(cx).and_then(|last| {
            if last == window.window_handle() {
                let bounds = window.window_bounds();
                window.remove_window();
                Some(bounds)
            } else {
                last.update(cx, |_, window, _| {
                    let bounds = window.window_bounds();
                    window.remove_window();
                    bounds
                })
                .ok()
            }
        })
    } else {
        None
    };
    let bounds = reused.unwrap_or_else(|| {
        let parent = window.bounds();
        windows.counter += 1;
        let step = CASCADE * windows.counter as f32;
        let fits = step + WINDOW_W < f32::from(parent.size.width)
            && step + WINDOW_H < f32::from(parent.size.height);
        if !fits {
            windows.counter = 1;
        }
        let step = px(CASCADE * windows.counter as f32);
        WindowBounds::Windowed(Bounds {
            origin: gpui_kit::point(parent.origin.x + step, parent.origin.y + step),
            size: size(px(WINDOW_W), px(WINDOW_H)),
        })
    });

    let title = format!("efa: {} stats", loaded.catalog.name(type_id));
    let options = WindowOptions {
        window_bounds: Some(bounds),
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            ..Default::default()
        }),
        app_id: Some("efa".into()),
        ..Default::default()
    };
    let (loaded, hub) = (Rc::clone(loaded), hub.clone());
    let opened = crate::ui::chrome::open_window(options, cx, move |window, cx| {
        cx.new(|cx| ItemStats::new(loaded, hub, request, type_id, window, cx))
    });
    match opened {
        Ok((handle, _)) => {
            windows.open.push(handle);
            windows.active = Some(handle);
            cx.set_global(windows);
        }
        Err(e) => eprintln!("opening the stats window: {e}"),
    }
}
