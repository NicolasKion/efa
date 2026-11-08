//! efa's own window header: the minimize, maximize and close buttons drawn into the top bar
//! instead of the system's title bar. Windows always gets it, since the native bar there is a
//! plain strip above the app; on Linux it is a preference, so it can be tried on a desktop that
//! draws no decorations (or turned off where the desktop draws its own). macOS keeps its
//! traffic lights.
//!
//! On Windows the buttons and the empty bar are native hit areas, so snapping, the system menu
//! and double click to maximize work as with any window. Elsewhere they are plain buttons, and
//! the bar moves the window itself.

use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyView, AnyWindowHandle, App, AppContext, Context, Entity, Global, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Task, TitlebarOptions, WeakEntity, Window, WindowBounds,
    WindowControlArea, WindowOptions, div, prelude::FluentBuilder, px, size,
};

/// The header's height, the menu bar's included.
pub const HEIGHT: f32 = 32.;

/// Whether this window draws its own header with the window buttons.
pub fn builtin(cx: &App) -> bool {
    cfg!(target_os = "windows")
        || (cfg!(target_os = "linux") && super::hub::preferences(cx).general.window_controls)
}

/// `options` for a window that draws its own header: on Windows the system title bar goes, the
/// title stays for the taskbar.
pub fn options(options: WindowOptions) -> WindowOptions {
    if !cfg!(target_os = "windows") {
        return options;
    }
    let title = options.titlebar.as_ref().and_then(|t| t.title.clone());
    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title,
            appears_transparent: true,
            traffic_light_position: None,
        }),
        ..options
    }
}

/// [`gpui_kit::open_window`] for a window without a menu bar: the view comes under a header
/// with the window's title and buttons whenever [`builtin`] is on.
pub fn open_window<V: Render>(
    options: WindowOptions,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> gpui_kit::Result<(AnyWindowHandle, Entity<V>)> {
    let mut view_out = None;
    let (handle, _) = gpui_kit::open_window(self::options(options), cx, |window, cx| {
        let view = build(window, cx);
        view_out = Some(view.clone());
        cx.new(|_| Framed { view: view.into() })
    })?;
    Ok((handle, view_out.expect("open_window ran its build closure")))
}

/// Options for an efa window of `width` by `height`, centered on the screen.
pub fn window_options(width: f32, height: f32, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(width), px(height)), cx)),
        app_id: Some("efa".into()),
        ..Default::default()
    }
}

/// The open window of view type `V`, for windows there is only ever one of.
struct Single<V: 'static>(Option<(AnyWindowHandle, WeakEntity<V>)>);

impl<V: 'static> Global for Single<V> {}

/// Open the one window of view type `V`, titled "efa: `title`", or bring it forward when it is
/// open already, after running `on_existing` on its view.
///
/// The window opens once the current update is over, so `build` and the first draw may read
/// any entity, including the one asking. The task resolves to why the window could not be
/// opened, if it could not; the failure is logged either way, so dropping the task's result
/// (with `detach`) loses nothing but the status line. Dropping the task itself cancels the open.
pub fn open_single<V: Render + 'static>(
    title: impl Into<SharedString>,
    (width, height): (f32, f32),
    cx: &mut App,
    on_existing: impl FnOnce(&mut V, &mut Window, &mut Context<V>) + 'static,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V + 'static,
) -> Task<Result<(), SharedString>> {
    let title = title.into();
    cx.spawn(async move |cx| {
        cx.update(|cx| {
            if let Some((handle, view)) = cx.try_global::<Single<V>>().and_then(|s| s.0.clone())
                && let Some(view) = view.upgrade()
            {
                let shown = handle.update(cx, |_, window, cx| {
                    view.update(cx, |v, cx| on_existing(v, window, cx));
                    window.activate_window();
                });
                if shown.is_ok() {
                    return Ok(());
                }
            }
            let options = window_options(width, height, cx);
            let opened = open_window(options, cx, {
                let title = title.clone();
                move |window, cx| {
                    window.set_window_title(&format!("efa: {title}"));
                    cx.new(|cx| build(window, cx))
                }
            });
            match opened {
                Ok((handle, view)) => {
                    let id = view.entity_id();
                    // Gone with its view, so the next open makes a new one.
                    cx.observe_release(&view, move |_, cx| {
                        let ours = cx
                            .try_global::<Single<V>>()
                            .and_then(|s| s.0.as_ref())
                            .is_some_and(|(_, v)| v.entity_id() == id);
                        if ours {
                            cx.set_global(Single::<V>(None));
                        }
                    })
                    .detach();
                    cx.set_global(Single::<V>(Some((handle, view.downgrade()))));
                    Ok(())
                }
                Err(e) => {
                    eprintln!("opening {title}: {e}");
                    Err(format!("Could not open {title}: {e}").into())
                }
            }
        })
    })
}

/// A window's view under efa's header, on the theme's background in its text color and size.
struct Framed {
    view: AnyView,
}

impl Render for Framed {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = builtin(cx).then(|| {
            let theme = cx.theme();
            h_flex()
                .id("window-header")
                .flex_none()
                .h(px(HEIGHT))
                .border_b_1()
                .border_color(theme.border)
                .bg(theme.title_bar)
                .child(
                    drag_area(div().id("window-title"))
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .items_center()
                        .px_3()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(div().truncate().child(window.window_title())),
                )
                .child(controls(window, cx))
        });
        // Every window's body starts from the theme's background, text color and size, so views
        // only set what differs.
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .children(header)
            .child(div().flex_1().min_h_0().child(self.view.clone()))
    }
}

/// `el` as the part of the header that moves the window, and maximizes it on a double click.
pub fn drag_area<E>(el: E) -> E
where
    E: InteractiveElement + StatefulInteractiveElement,
{
    if cfg!(target_os = "windows") {
        el.window_control_area(WindowControlArea::Drag)
    } else {
        el.on_mouse_down(MouseButton::Left, |event: &MouseDownEvent, window, _| {
            if event.click_count >= 2 {
                window.zoom_window();
            } else {
                window.start_window_move();
            }
        })
        .on_mouse_down(MouseButton::Right, |event: &MouseDownEvent, window, _| {
            window.show_window_menu(event.position);
        })
    }
}

/// Minimize, maximize or restore, and close, flush with the header's right edge.
pub fn controls(window: &Window, cx: &App) -> impl IntoElement {
    let maximized = window.is_maximized();
    h_flex()
        .flex_none()
        .h_full()
        .child(button(Control::Minimize, cx))
        .child(button(
            if maximized {
                Control::Restore
            } else {
                Control::Maximize
            },
            cx,
        ))
        .child(button(Control::Close, cx))
}

#[derive(Clone, Copy)]
enum Control {
    Minimize,
    Maximize,
    Restore,
    Close,
}

fn button(control: Control, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (id, icon, area) = match control {
        Control::Minimize => ("minimize", IconName::WindowMinimize, WindowControlArea::Min),
        Control::Maximize => ("maximize", IconName::WindowMaximize, WindowControlArea::Max),
        Control::Restore => ("restore", IconName::WindowRestore, WindowControlArea::Max),
        Control::Close => ("close", IconName::WindowClose, WindowControlArea::Close),
    };
    let close = matches!(control, Control::Close);
    let (hover_bg, hover_text, active_bg) = if close {
        (theme.danger, theme.danger_foreground, theme.danger_active)
    } else {
        (
            theme.secondary_hover,
            theme.secondary_foreground,
            theme.secondary_active,
        )
    };
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(46.))
        .h_full()
        .text_color(theme.foreground)
        .hover(move |s| s.bg(hover_bg).text_color(hover_text))
        .active(move |s| s.bg(active_bg).text_color(hover_text))
        .map(|el| {
            if cfg!(target_os = "windows") {
                // The system handles the press, so hovering shows snap layouts too.
                el.window_control_area(area)
            } else {
                el.on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    match control {
                        Control::Minimize => window.minimize_window(),
                        Control::Maximize | Control::Restore => window.zoom_window(),
                        Control::Close => window.remove_window(),
                    }
                })
            }
        })
        .child(Icon::new(icon).small())
}
