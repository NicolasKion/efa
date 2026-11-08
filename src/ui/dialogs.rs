//! The small dialogs every window asks with: a line of text, a number, or yes or no.
//!
//! Each opens over the window it is given. The OK or Yes button and Enter both answer; a text
//! that does not pass its check keeps the dialog open with the problem under the field.

use std::{rc::Rc, str::FromStr};

use gpui_kit::component::{
    ActiveTheme, WindowExt,
    button::{Button, ButtonVariants},
    input::{Input, InputState},
    v_flex,
};
use gpui_kit::{
    App, AppContext, ClickEvent, ParentElement, SharedString, Styled, Window, div,
    prelude::FluentBuilder, px,
};

use super::widgets::{Submit, button_row, parse_decimal};

/// Width of a dialog unless it asks for another.
const WIDTH: f32 = 380.;

/// What a text prompt says and starts with.
#[derive(Debug, Clone)]
pub struct Prompt {
    pub title: SharedString,
    /// The line over the field; none leaves it out.
    pub message: Option<SharedString>,
    /// The field's text to start with, selected so typing replaces it.
    pub default: String,
    pub width: f32,
}

impl Prompt {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            message: None,
            default: String::new(),
            width: WIDTH,
        }
    }

    #[must_use]
    pub fn message(mut self, message: impl Into<SharedString>) -> Self {
        self.message = Some(message.into());
        self
    }

    #[must_use]
    pub fn default(mut self, default: impl Into<String>) -> Self {
        self.default = default.into();
        self
    }

    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }
}

/// Ask for a line of text. `check` gets the trimmed text and says what is wrong with it, which
/// keeps the dialog open; otherwise the dialog closes and `done` gets the trimmed text.
pub fn prompt_text(
    window: &mut Window,
    cx: &mut App,
    prompt: Prompt,
    check: impl Fn(&str, &App) -> Option<SharedString> + 'static,
    done: impl Fn(String, &mut Window, &mut App) + 'static,
) {
    let input = cx.new(|cx| InputState::new(window, cx).default_value(prompt.default.clone()));
    let error = cx.new(|_| None::<SharedString>);
    let (check, done) = (Rc::new(check), Rc::new(done));
    // Typing replaces the text, once the dialog is up and cannot take the focus back.
    let field = input.clone();
    window.defer(cx, move |window, cx| {
        field.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    });
    window.open_dialog(cx, move |dialog, _, cx| {
        let problem = error.read(cx).clone();
        let submit: Submit = {
            let (input, error, check, done) = (
                input.clone(),
                error.clone(),
                Rc::clone(&check),
                Rc::clone(&done),
            );
            Rc::new(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                let text = input.read(cx).value().trim().to_owned();
                if let Some(problem) = check(&text, cx) {
                    error.update(cx, |e, cx| {
                        *e = Some(problem);
                        cx.notify();
                    });
                    window.refresh();
                    return;
                }
                window.close_dialog(cx);
                done(text, window, cx);
            })
        };
        dialog
            .on_ok({
                let submit = Rc::clone(&submit);
                move |e, w, cx| {
                    submit(e, w, cx);
                    false
                }
            })
            .title(prompt.title.clone())
            .w(px(prompt.width))
            .child(
                v_flex()
                    .gap_3()
                    .children(prompt.message.clone())
                    .child(Input::new(&input))
                    .when_some(problem, |col, problem| {
                        col.child(div().text_sm().text_color(cx.theme().danger).child(problem))
                    })
                    .child(button_row().child(
                        Button::new("prompt-ok").primary().label("OK").on_click({
                            let submit = Rc::clone(&submit);
                            move |e: &ClickEvent, w: &mut Window, cx: &mut App| submit(e, w, cx)
                        }),
                    )),
            )
    });
}

/// Ask for a number, or anything else that parses. A decimal comma reads like a point, so
/// "0,5" is a half.
pub fn prompt_value<T: FromStr + 'static>(
    window: &mut Window,
    cx: &mut App,
    prompt: Prompt,
    done: impl Fn(T, &mut Window, &mut App) + 'static,
) {
    prompt_text(
        window,
        cx,
        prompt,
        |text, _| {
            parse_decimal::<T>(text)
                .is_none()
                .then(|| "Enter a number.".into())
        },
        move |text, window, cx| {
            if let Some(value) = parse_decimal(&text) {
                done(value, window, cx);
            }
        },
    );
}

/// What a yes or no question says.
#[derive(Debug, Clone)]
pub struct Confirm {
    pub title: SharedString,
    pub message: SharedString,
    pub yes: SharedString,
    pub no: SharedString,
    /// Yes destroys something, and its button says so in the danger color.
    pub danger: bool,
}

impl Confirm {
    /// A plain question with Yes and No.
    pub fn new(title: impl Into<SharedString>, message: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            yes: "Yes".into(),
            no: "No".into(),
            danger: false,
        }
    }

    /// pyfa's delete question: Delete and Cancel, Delete in the danger color.
    pub fn delete(message: impl Into<SharedString>) -> Self {
        Self {
            yes: "Delete".into(),
            no: "Cancel".into(),
            danger: true,
            ..Self::new("Confirm Delete", message)
        }
    }
}

/// Ask a yes or no question; `on_yes` runs once the dialog closed with yes. Enter answers yes,
/// Escape no.
pub fn confirm(
    window: &mut Window,
    cx: &mut App,
    question: Confirm,
    on_yes: impl Fn(&mut Window, &mut App) + 'static,
) {
    let on_yes = Rc::new(on_yes);
    window.open_dialog(cx, move |dialog, _, _| {
        let yes_button = {
            let on_yes = Rc::clone(&on_yes);
            Button::new("confirm-yes")
                .label(question.yes.clone())
                .map(|b| {
                    if question.danger {
                        b.danger()
                    } else {
                        b.primary()
                    }
                })
                .on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    on_yes(window, cx);
                })
        };
        dialog
            .on_ok({
                let on_yes = Rc::clone(&on_yes);
                move |_, window, cx| {
                    on_yes(window, cx);
                    true
                }
            })
            .title(question.title.clone())
            .w(px(WIDTH + 40.))
            .child(
                v_flex().gap_3().child(question.message.clone()).child(
                    button_row()
                        .child(
                            Button::new("confirm-no")
                                .label(question.no.clone())
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(yes_button),
                ),
            )
    });
}
