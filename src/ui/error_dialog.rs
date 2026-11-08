//! pyfa's error dialog (gui/errorDialog.py): what went wrong, with details to copy into a bug
//! report. The main window opens it for whatever [`crate::errors`] queued.

use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Textarea, TextareaState},
    v_flex,
};
use gpui_kit::{
    App, AppContext, ClipboardItem, Context, Entity, FontWeight, IntoElement, ParentElement,
    Render, SharedString, Styled, Window, div,
};

use crate::errors::Report;

pub struct ErrorDialog {
    title: SharedString,
    details: Entity<TextareaState>,
    text: String,
    copied: bool,
}

/// Open a window for the reports. Call it outside any window's update.
pub fn open(reports: &[Report], cx: &mut App) {
    let Some(first) = reports.first() else { return };
    let title = if reports.len() == 1 {
        first.title.clone()
    } else {
        format!("{} (and {} more)", first.title, reports.len() - 1)
    };
    let text = reports
        .iter()
        .map(|r| format!("{}\n{}", r.title, r.details))
        .collect::<Vec<_>>()
        .join("\n\n");
    let options = crate::ui::chrome::window_options(640., 460., cx);
    let opened = crate::ui::chrome::open_window(options, cx, move |window, cx| {
        window.set_window_title("efa: Error");
        cx.new(|cx| ErrorDialog::new(title, text, window, cx))
    });
    if let Err(e) = opened {
        eprintln!("opening the error dialog: {e}");
    }
}

impl ErrorDialog {
    fn new(title: String, text: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let details = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx).rows(3);
            state.set_value(text.clone(), window, cx);
            state
        });
        Self {
            title: title.into(),
            details,
            text,
            copied: false,
        }
    }
}

impl Render for ErrorDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.danger)
                    .child(self.title.clone()),
            )
            .child(div().text_color(theme.muted_foreground).child(
                "Something went wrong that efa did not expect. If it keeps happening, \
                     please report it with the details below.",
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .child(Textarea::new(&self.details).h_full()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("error-copy")
                            .small()
                            .label(if self.copied { "Copied" } else { "Copy" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(this.text.clone()));
                                this.copied = true;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("error-close")
                            .small()
                            .primary()
                            .label("Close")
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            )
    }
}
