//! Modal dialogs for the native shell.
//!
//! A port of `src/components/dialogs`: the shared `ModalBase` chrome (dimmed
//! backdrop, app-coloured panel, panel header and footer) plus the dialogs the
//! ported action set needs — Remove, Set label, Rate limit and Set location.
//! Each dialog owns its form state and emits one event; the shell applies it
//! through `TorrentsModel`.
//!
//! Escape cancels; Enter confirms where a dialog has no text field to submit.
//! A dialog grabs focus when it opens so those reach it, and a press inside the
//! panel never reaches the backdrop that would dismiss it.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, KeyDownEvent,
    MouseButton, Rgba, SharedString, Stateful, Window,
};

use std::path::PathBuf;
use std::sync::Arc;

use rtorrent_core::types::DaemonHealth;

use crate::actions::DismissOverlay;
use crate::format;
use crate::services::Services;
use crate::theme::{self, Palette, BORDER_BLACK, SCRIM};

const HEADER_HEIGHT: f32 = 33.;
const FOOTER_HEIGHT: f32 = 50.;

/// The dimmed, window-sized backdrop every modal sits on.
///
/// Callers attach their own dismiss handlers: Escape (through the bound
/// [`DismissOverlay`] action), and a press on the backdrop itself.
pub(crate) fn backdrop(focus: &FocusHandle) -> Stateful<Div> {
    div()
        .id("modal-backdrop")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme::color(SCRIM))
        .track_focus(focus)
}

/// The panel: app-coloured body, black border, 9 px radius, no shadow.
pub(crate) fn panel(id: &'static str, width: f32, palette: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(width))
        .max_h(px(560.))
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(9.))
        .border_1()
        .border_color(theme::color(BORDER_BLACK))
        .bg(theme::color(palette.app))
        // A press inside the panel must not reach the backdrop's dismiss.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

pub(crate) fn header(
    title: &str,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    palette: Palette,
) -> Stateful<Div> {
    div()
        .id("modal-header")
        .h(px(HEADER_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(12.))
        .bg(theme::color(palette.panel))
        .border_b_1()
        .border_color(theme::color(BORDER_BLACK))
        .child(theme::text(
            title.to_owned(),
            12.,
            theme::color(palette.text_primary),
        ))
        .child(div().flex_1())
        .child(
            div()
                .id("modal-close")
                .w(px(16.))
                .h(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .cursor_pointer()
                .hover(|style| style.bg(theme::color(palette.selected)))
                .on_click(on_close)
                .child(crate::icons::glyph(
                    crate::icons::CLOSE,
                    10.,
                    theme::color(palette.text_dim),
                )),
        )
}

pub(crate) fn body() -> Stateful<Div> {
    div()
        .id("modal-body")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .gap(px(12.))
        .px(px(16.))
        .py(px(14.))
        .overflow_y_scroll()
}

pub(crate) fn footer(palette: Palette) -> Stateful<Div> {
    div()
        .id("modal-footer")
        .h(px(FOOTER_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(8.))
        .px(px(16.))
        .bg(theme::color(palette.panel))
        .border_t_1()
        .border_color(theme::color(BORDER_BLACK))
}

/// The dialog buttons from `ModalBase.module.css`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ButtonKind {
    Secondary,
    Primary,
    Danger,
}

/// A dialog button without a handler; callers chain their own `on_click`.
pub(crate) fn button(
    id: &'static str,
    label: &str,
    kind: ButtonKind,
    enabled: bool,
    palette: Palette,
) -> Stateful<Div> {
    let (border, background, foreground) = match kind {
        ButtonKind::Secondary => (palette.border_strong, palette.track, palette.text_body),
        ButtonKind::Primary => (
            palette.accent_cyan,
            palette.selected,
            palette.accent_cyan_bright,
        ),
        ButtonKind::Danger => (palette.danger_border, palette.danger_bg, palette.accent_red),
    };
    let (border, background, foreground) = if enabled {
        (border, background, foreground)
    } else {
        (palette.track, palette.track, palette.text_dim)
    };
    let hovered = palette.selected;
    let mut element = div()
        .id(id)
        .h(px(26.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .px(px(16.))
        .rounded(px(5.))
        .border_1()
        .border_color(theme::color(border))
        .bg(theme::color(background))
        .child(theme::text(label.to_owned(), 11., theme::color(foreground)));
    if enabled {
        element = element
            .cursor_pointer()
            .hover(move |style| style.bg(theme::color(hovered)));
    }
    element
}

/// A field with its label above it, which is how the add and create dialogs lay
/// their forms out (unlike `field`, whose label sits beside the control).
pub(crate) fn form_field(label: &str, control: impl IntoElement, palette: Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(theme::text(
            label.to_owned(),
            10.5,
            theme::color(palette.text_muted),
        ))
        .child(control)
}

/// A labelled row: a 66 px label beside a control, as `forms.module.css` has it.
fn field(label: &str, control: impl IntoElement, palette: Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(div().w(px(66.)).flex_none().child(theme::text(
            label.to_owned(),
            10.5,
            theme::color(palette.text_muted),
        )))
        .child(control)
}

/// A text field drawn in the design's `bg/field` box, with the kit's input
/// stripped of its own chrome so only this container is visible.
pub(crate) fn text_field(state: &Entity<InputState>, palette: Palette) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .px(px(9.))
        .h(px(26.))
        .rounded(px(4.))
        .border_1()
        .border_color(theme::color(palette.border_strong))
        .bg(theme::color(palette.field))
        .child(Input::new(state).appearance(false))
}

/// The design's checkbox: a 14 px box that reads a check when set.
pub(crate) fn checkbox(
    id: &'static str,
    label: &str,
    checked: bool,
    enabled: bool,
    palette: Palette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (box_border, box_background) = if checked {
        (palette.accent_cyan, palette.selected)
    } else {
        (palette.border_strong, palette.field)
    };
    let text = if !enabled {
        palette.text_muted
    } else if checked {
        palette.text_primary
    } else {
        palette.text_body
    };
    let mut element = div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(7.))
        .child(
            div()
                .w(px(14.))
                .h(px(14.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .border_1()
                .border_color(theme::color(box_border))
                .bg(theme::color(box_background))
                .when(checked, |boxy| {
                    boxy.child(crate::icons::glyph(
                        crate::icons::CHECK,
                        9.,
                        theme::color(palette.accent_cyan_bright),
                    ))
                }),
        )
        .child(theme::text(label.to_owned(), 11., theme::color(text)));
    if enabled {
        element = element.cursor_pointer().on_click(on_click);
    }
    element
}

/// A single line of explanatory text.
fn note(content: impl Into<SharedString>, color: Rgba) -> Div {
    theme::text(content.into(), 11., color).min_w_0().truncate()
}

/// A field of digits wired to `on_confirm`, used by the rate dialog.
fn numeric_field(
    placeholder: &'static str,
    value: Option<i64>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    cx.new(|cx| {
        let state = InputState::new(window, cx).placeholder(placeholder);
        match value {
            Some(value) if value > 0 => state.default_value(value.to_string()),
            _ => state,
        }
    })
}

/// Parse a whole number of KiB/s, or a connection cap, from a text field.
///
/// # Errors
///
/// Returns a message naming the offending value, for the dialog to show.
fn parse_kb(text: &str) -> Result<i64, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    trimmed
        .parse::<i64>()
        .map_err(|_| format!("'{trimmed}' is not a whole number"))
        .and_then(|value| {
            if value < 0 {
                Err("must be zero or greater".to_owned())
            } else {
                Ok(value)
            }
        })
}

// --- Remove -----------------------------------------------------------------

/// The Remove confirmation's outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoveEvent {
    Cancel,
    Confirm { delete_data: bool },
}

impl EventEmitter<RemoveEvent> for RemoveDialog {}

/// Confirms removing the selection, with optional data deletion.
pub struct RemoveDialog {
    palette: Palette,
    focus: FocusHandle,
    delete_data: bool,
    /// False when the daemon is remote, where its files are not ours to delete.
    can_delete_data: bool,
    count: usize,
    name: String,
    size: i64,
}

impl RemoveDialog {
    pub fn new(
        count: usize,
        name: String,
        size: i64,
        can_delete_data: bool,
        delete_data: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            palette: Palette::dark(),
            focus,
            // ⇧Del opens this already asking for the files; plain Del leaves the
            // box alone.
            delete_data: delete_data && can_delete_data,
            can_delete_data,
            count,
            name,
            size,
        }
    }

    /// The dialog's focus handle, for a test or a caller that wants it.
    #[must_use]
    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    fn cancel(&self, cx: &mut Context<Self>) {
        cx.emit(RemoveEvent::Cancel);
    }

    fn confirm(&self, cx: &mut Context<Self>) {
        cx.emit(RemoveEvent::Confirm {
            delete_data: self.delete_data && self.can_delete_data,
        });
    }
}

impl Render for RemoveDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette;
        let message = if self.count == 1 {
            format!("Remove {} from the transfer list?", self.name)
        } else {
            format!("Remove {} torrents from the transfer list?", self.count)
        };
        let delete_label = format!(
            "Also delete downloaded files ({})",
            format::bytes(self.size)
        );
        backdrop(&self.focus)
            .on_action(cx.listener(|this, _: &DismissOverlay, _, cx| this.cancel(cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.cancel(cx)),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                match event.keystroke.key.as_str() {
                    "enter" => {
                        cx.stop_propagation();
                        this.confirm(cx);
                    }
                    "escape" => {
                        cx.stop_propagation();
                        this.cancel(cx);
                    }
                    _ => {}
                }
            }))
            .child(
                panel("remove-dialog", 400., palette)
                    .child(header(
                        "Remove torrent",
                        cx.listener(|this, _, _, cx| this.cancel(cx)),
                        palette,
                    ))
                    .child(
                        body().child(
                            div()
                                .flex()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .w(px(18.))
                                        .h(px(18.))
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(9.))
                                        .border_1()
                                        .border_color(theme::color(palette.danger_border))
                                        .bg(theme::color(palette.danger_bg))
                                        .child(theme::text(
                                            "!".to_owned(),
                                            11.,
                                            theme::color(palette.accent_red),
                                        )),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(10.))
                                        .flex_1()
                                        .min_w_0()
                                        .child(note(message, theme::color(palette.text_body)))
                                        .child(checkbox(
                                            "remove-delete-data",
                                            &delete_label,
                                            self.delete_data && self.can_delete_data,
                                            self.can_delete_data,
                                            palette,
                                            cx.listener(|this, _, _, cx| {
                                                this.delete_data = !this.delete_data;
                                                cx.notify();
                                            }),
                                        ))
                                        .when(!self.can_delete_data, |column| {
                                            column.child(note(
                                                "The daemon is remote, so its files cannot be deleted from here.",
                                                theme::color(palette.text_dim),
                                            ))
                                        }),
                                ),
                        ),
                    )
                    .child(
                        footer(palette)
                            .child(
                                button(
                                    "remove-cancel",
                                    "Cancel",
                                    ButtonKind::Secondary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                            )
                            .child(
                                button(
                                    "remove-confirm",
                                    "Remove",
                                    ButtonKind::Danger,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
                            ),
                    ),
            )
    }
}

// --- Set label --------------------------------------------------------------

/// The Set label dialog's outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LabelEvent {
    Cancel,
    Confirm { label: String },
}

impl EventEmitter<LabelEvent> for LabelDialog {}

/// Assigns (or clears) the selection's label. The labels already in use are
/// offered as chips so a typo cannot fork one label into two.
pub struct LabelDialog {
    palette: Palette,
    focus: FocusHandle,
    input: Entity<InputState>,
    existing: Vec<String>,
    count: usize,
}

impl LabelDialog {
    pub fn new(
        count: usize,
        existing: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("label")
                .submit_on_enter(true)
        });
        let handle = input.clone();
        handle.update(cx, |state, cx| state.focus(window, cx));
        Self {
            palette: Palette::dark(),
            focus,
            input,
            existing,
            count,
        }
    }

    fn cancel(&self, cx: &mut Context<Self>) {
        cx.emit(LabelEvent::Cancel);
    }

    fn confirm(&self, label: String, cx: &mut Context<Self>) {
        cx.emit(LabelEvent::Confirm {
            label: label.trim().to_owned(),
        });
    }
}

impl Render for LabelDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette;
        let existing = self.existing.clone();
        let mut chips = div().flex().flex_wrap().gap(px(6.));
        for label in existing {
            let value = label.clone();
            chips = chips.child(
                div()
                    .id(SharedString::from(format!("label-chip-{label}")))
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .px(px(8.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(theme::color(palette.border_strong))
                    .bg(theme::color(palette.track))
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::color(palette.selected)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let value = value.clone();
                        this.input.update(cx, |input, cx| {
                            input.set_value(value, window, cx);
                        });
                    }))
                    .child(theme::text(label, 10.5, theme::color(palette.text_body))),
            );
        }
        backdrop(&self.focus)
            .on_action(cx.listener(|this, _: &DismissOverlay, _, cx| this.cancel(cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.cancel(cx)),
            )
            .child(
                panel("label-dialog", 420., palette)
                    .child(header(
                        "Set label",
                        cx.listener(|this, _, _, cx| this.cancel(cx)),
                        palette,
                    ))
                    .child(
                        body()
                            .child(note(
                                format!("Apply a label to {} torrent(s).", self.count),
                                theme::color(palette.text_muted),
                            ))
                            .child(field("Label", text_field(&self.input, palette), palette))
                            .when(!self.existing.is_empty(), |column| {
                                column.child(field("In use", chips, palette))
                            }),
                    )
                    .child(
                        footer(palette)
                            .child(
                                button(
                                    "label-cancel",
                                    "Cancel",
                                    ButtonKind::Secondary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                            )
                            .child(
                                button(
                                    "label-clear",
                                    "Clear",
                                    ButtonKind::Secondary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.confirm(String::new(), cx);
                                    },
                                )),
                            )
                            .child(
                                button(
                                    "label-confirm",
                                    "Apply",
                                    ButtonKind::Primary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        let value = this.input.read(cx).value().to_string();
                                        this.confirm(value, cx);
                                    },
                                )),
                            ),
                    ),
            )
    }
}

// --- Edit tags (V3-10) ------------------------------------------------------

/// The tag editor's outcome: tags to add and tags to remove, applied to the
/// whole selection. `add`/`remove` are disjoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagEvent {
    Cancel,
    Confirm {
        add: Vec<String>,
        remove: Vec<String>,
    },
}

impl EventEmitter<TagEvent> for TagDialog {}

/// Bulk tag editor: the selection's tags as chips (click to mark for removal)
/// and a field for new ones. Add/remove rather than an absolute set, because
/// "some torrents have it" has no single list to write back.
pub struct TagDialog {
    palette: Palette,
    focus: FocusHandle,
    input: Entity<InputState>,
    /// The union of tags on the selection, sorted.
    union: Vec<String>,
    /// Tags marked for removal.
    removed: Vec<String>,
    count: usize,
}

impl TagDialog {
    pub fn new(
        count: usize,
        union: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("comma-separated tags")
                .submit_on_enter(true)
        });
        let handle = input.clone();
        handle.update(cx, |state, cx| state.focus(window, cx));
        Self {
            palette: Palette::dark(),
            focus,
            input,
            union,
            removed: Vec::new(),
            count,
        }
    }

    fn cancel(&self, cx: &mut Context<Self>) {
        cx.emit(TagEvent::Cancel);
    }

    fn toggle(&mut self, tag: String, cx: &mut Context<Self>) {
        if self
            .removed
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(&tag))
        {
            self.removed
                .retain(|candidate| !candidate.eq_ignore_ascii_case(&tag));
        } else {
            self.removed.push(tag);
        }
        cx.notify();
    }

    fn confirm(&self, add: Vec<String>, remove: Vec<String>, cx: &mut Context<Self>) {
        cx.emit(TagEvent::Confirm { add, remove });
    }
}

impl Render for TagDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette;
        let mut chips = div().flex().flex_wrap().gap(px(6.));
        for tag in &self.union {
            let mark = tag.clone();
            let removed = self
                .removed
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(tag));
            let colour = theme::tag_colour(tag);
            chips = chips.child(
                div()
                    .id(SharedString::from(format!("tag-chip-{tag}")))
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .px(px(8.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(theme::color(if removed {
                        palette.accent_red
                    } else {
                        palette.border_strong
                    }))
                    .bg(theme::color(if removed {
                        palette.track
                    } else {
                        theme::blend(palette.app, colour, 0.22)
                    }))
                    .when(removed, |chip| chip.opacity(0.55))
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::color(palette.selected)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle(mark.clone(), cx);
                    }))
                    .child(theme::text(
                        tag.clone(),
                        10.5,
                        theme::color(if removed { palette.text_dim } else { colour }),
                    )),
            );
        }
        backdrop(&self.focus)
            .on_action(cx.listener(|this, _: &DismissOverlay, _, cx| this.cancel(cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.cancel(cx)),
            )
            .child(
                panel("tag-dialog", 440., palette)
                    .child(header(
                        "Edit tags",
                        cx.listener(|this, _, _, cx| this.cancel(cx)),
                        palette,
                    ))
                    .child(
                        body()
                            .child(note(
                                format!(
                                    "Add or remove tags on {} torrent(s). Click a tag to mark it for removal.",
                                    self.count
                                ),
                                theme::color(palette.text_muted),
                            ))
                            .child(field(
                                "Add tags",
                                text_field(&self.input, palette),
                                palette,
                            ))
                            .when(!self.union.is_empty(), |column| {
                                column.child(field("Tags", chips, palette))
                            })
                            .child(note(
                                "Tags follow the daemon session, so every client sees them.",
                                theme::color(palette.text_dim),
                            )),
                    )
                    .child(
                        footer(palette)
                            .child(
                                button(
                                    "tag-cancel",
                                    "Cancel",
                                    ButtonKind::Secondary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                            )
                            .child(
                                button(
                                    "tag-clear",
                                    "Clear all",
                                    ButtonKind::Secondary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    // Removing the whole union empties every
                                    // selected torrent's tags.
                                    let union = this.union.clone();
                                    this.confirm(Vec::new(), union, cx);
                                })),
                            )
                            .child(
                                button(
                                    "tag-confirm",
                                    "Apply",
                                    ButtonKind::Primary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let typed = this.input.read(cx).value().to_string();
                                    let add: Vec<String> = rtorrent_core::tags::normalise(
                                        typed.split(',').map(str::to_owned),
                                    )
                                    .into_iter()
                                    .filter(|tag| {
                                        !this.removed.iter().any(|removed| {
                                            removed.eq_ignore_ascii_case(tag)
                                        })
                                    })
                                    .collect();
                                    let remove = this.removed.clone();
                                    this.confirm(add, remove, cx);
                                })),
                            ),
                    ),
            )
    }
}

// --- Rate limit -------------------------------------------------------------

/// The Rate limit dialog's outcome: a rate pair plus the connection caps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateLimitEvent {
    Cancel,
    Confirm {
        down_kb: i64,
        up_kb: i64,
        peers_max: i64,
        peers_min: i64,
        uploads_max: i64,
    },
}

impl EventEmitter<RateLimitEvent> for RateLimitDialog {}

/// Per-torrent rate limits and connection caps.
pub struct RateLimitDialog {
    palette: Palette,
    focus: FocusHandle,
    down: Entity<InputState>,
    up: Entity<InputState>,
    peers_max: Entity<InputState>,
    peers_min: Entity<InputState>,
    uploads_max: Entity<InputState>,
    error: Option<String>,
}

impl RateLimitDialog {
    pub fn new(
        torrent: Option<&rtorrent_core::types::TorrentDto>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        // The DTO carries limits in bytes/s; the dialog speaks KiB/s.
        let down_kb = torrent
            .and_then(|torrent| torrent.down_rate_limit)
            .map(|bytes| bytes / 1024);
        let up_kb = torrent
            .and_then(|torrent| torrent.up_rate_limit)
            .map(|bytes| bytes / 1024);
        let down = numeric_field("KiB/s, 0 = unlimited", down_kb, window, cx);
        let up = numeric_field("KiB/s, 0 = unlimited", up_kb, window, cx);
        let peers_max = numeric_field("0 = default", torrent.map(|t| t.peers_max), window, cx);
        let peers_min = numeric_field("0 = default", torrent.map(|t| t.peers_min), window, cx);
        let uploads_max = numeric_field("0 = default", torrent.map(|t| t.uploads_max), window, cx);
        focus.focus(window, cx);
        Self {
            palette: Palette::dark(),
            focus,
            down,
            up,
            peers_max,
            peers_min,
            uploads_max,
            error: None,
        }
    }

    fn cancel(&self, cx: &mut Context<Self>) {
        cx.emit(RateLimitEvent::Cancel);
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        let fields = [
            ("download", self.down.read(cx).value().to_string()),
            ("upload", self.up.read(cx).value().to_string()),
            ("max peers", self.peers_max.read(cx).value().to_string()),
            ("min peers", self.peers_min.read(cx).value().to_string()),
            (
                "upload slots",
                self.uploads_max.read(cx).value().to_string(),
            ),
        ];
        let mut parsed = [0_i64; 5];
        for (index, (name, text)) in fields.iter().enumerate() {
            match parse_kb(text) {
                Ok(value) => parsed[index] = value,
                Err(message) => {
                    self.error = Some(format!("{name}: {message}"));
                    cx.notify();
                    return;
                }
            }
        }
        let [down_kb, up_kb, peers_max, peers_min, uploads_max] = parsed;
        cx.emit(RateLimitEvent::Confirm {
            down_kb,
            up_kb,
            peers_max,
            peers_min,
            uploads_max,
        });
    }
}

impl Render for RateLimitDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette;
        backdrop(&self.focus)
            .on_action(cx.listener(|this, _: &DismissOverlay, _, cx| this.cancel(cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.cancel(cx)),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.cancel(cx);
                }
            }))
            .child(
                panel("rate-limit-dialog", 400., palette)
                    .child(header(
                        "Limit rates",
                        cx.listener(|this, _, _, cx| this.cancel(cx)),
                        palette,
                    ))
                    .child(
                        body()
                            .child(field("Download", text_field(&self.down, palette), palette))
                            .child(field("Upload", text_field(&self.up, palette), palette))
                            .child(field(
                                "Max peers",
                                text_field(&self.peers_max, palette),
                                palette,
                            ))
                            .child(field(
                                "Min peers",
                                text_field(&self.peers_min, palette),
                                palette,
                            ))
                            .child(field(
                                "Upload slots",
                                text_field(&self.uploads_max, palette),
                                palette,
                            ))
                            .child(note(
                                "Zero means unlimited for the rates, and the daemon's default for the caps.",
                                theme::color(palette.text_dim),
                            ))
                            .when_some(self.error.clone(), |column, error| {
                                column.child(theme::text(
                                    error,
                                    10.5,
                                    theme::color(palette.accent_red),
                                ))
                            }),
                    )
                    .child(
                        footer(palette)
                            .child(
                                button(
                                    "rate-cancel",
                                    "Cancel",
                                    ButtonKind::Secondary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                            )
                            .child(
                                button(
                                    "rate-confirm",
                                    "Apply",
                                    ButtonKind::Primary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
                            ),
                    ),
            )
    }
}

// --- Set location -----------------------------------------------------------

/// The Set location dialog's outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetLocationEvent {
    Cancel,
    Confirm { path: String, move_data: bool },
}

impl EventEmitter<SetLocationEvent> for SetLocationDialog {}

/// Relocates a torrent's download directory.
pub struct SetLocationDialog {
    palette: Palette,
    focus: FocusHandle,
    input: Entity<InputState>,
    move_data: bool,
    /// False for a remote daemon, whose files this machine cannot move.
    can_move: bool,
    current: String,
}

impl SetLocationDialog {
    pub fn new(
        current: String,
        can_move: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let initial = current.clone();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("download directory")
                .default_value(initial)
                .submit_on_enter(true)
        });
        let handle = input.clone();
        handle.update(cx, |state, cx| state.focus(window, cx));
        Self {
            palette: Palette::dark(),
            focus,
            input,
            move_data: can_move,
            can_move,
            current,
        }
    }

    fn cancel(&self, cx: &mut Context<Self>) {
        cx.emit(SetLocationEvent::Cancel);
    }

    fn confirm(&self, cx: &mut Context<Self>) {
        let path = self.input.read(cx).value().to_string();
        cx.emit(SetLocationEvent::Confirm {
            path: path.trim().to_owned(),
            move_data: self.move_data && self.can_move,
        });
    }
}

impl Render for SetLocationDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette;
        backdrop(&self.focus)
            .on_action(cx.listener(|this, _: &DismissOverlay, _, cx| this.cancel(cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.cancel(cx)),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.cancel(cx);
                }
            }))
            .child(
                panel("set-location-dialog", 460., palette)
                    .child(header(
                        "Set location",
                        cx.listener(|this, _, _, cx| this.cancel(cx)),
                        palette,
                    ))
                    .child(
                        body()
                            .child(note(
                                format!("Currently in {}", self.current),
                                theme::color(palette.text_muted),
                            ))
                            .child(field("Location", text_field(&self.input, palette), palette))
                            .child(checkbox(
                                "set-location-move",
                                "Move files to the new location",
                                self.move_data && self.can_move,
                                self.can_move,
                                palette,
                                cx.listener(|this, _, _, cx| {
                                    this.move_data = !this.move_data;
                                    cx.notify();
                                }),
                            ))
                            .when(!self.can_move, |column| {
                                column.child(note(
                                    "The daemon is remote, so its files cannot be moved from here.",
                                    theme::color(palette.text_dim),
                                ))
                            }),
                    )
                    .child(
                        footer(palette)
                            .child(
                                button(
                                    "location-cancel",
                                    "Cancel",
                                    ButtonKind::Secondary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                            )
                            .child(
                                button(
                                    "location-confirm",
                                    "Move",
                                    ButtonKind::Primary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
                            ),
                    ),
            )
    }
}

// --- Statistics -------------------------------------------------------------

/// The Statistics dialog's outcome. It is read-only, so closing is the only
/// thing it reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatisticsEvent {
    Close,
}

impl EventEmitter<StatisticsEvent> for StatisticsDialog {}

/// The dialog's two tabs, matching the Tauri `StatisticsDialog`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatsTab {
    Statistics,
    Daemon,
}

/// The figures the Statistics tab shows: the daemon's session counters plus the
/// app-persisted "since install" totals (see [`crate::stats`]).
#[derive(Clone, Copy, Debug, Default)]
struct StatsView {
    session_down: i64,
    session_up: i64,
    all_time_down: i64,
    all_time_up: i64,
    connected_peers: i64,
    session_waste: i64,
    cache_hit_pct: Option<f64>,
    buffer_size: Option<i64>,
    cache_overload_pct: Option<f64>,
    queued_io: Option<i64>,
}

/// Statistics: the daemon's session and cache counters, and its self-report.
///
/// Read-only, so there is no Apply: it fetches once on open and renders what
/// the daemon answered, with `—` for anything this rtorrent build doesn't
/// expose. The all-time totals come from the persisted counters beside
/// `settings.json`, folded in on each open.
pub struct StatisticsDialog {
    palette: Palette,
    focus: FocusHandle,
    tab: StatsTab,
    stats: Option<StatsView>,
    health: Option<DaemonHealth>,
    health_error: Option<String>,
    error: Option<String>,
}

impl StatisticsDialog {
    pub fn new(
        services: Arc<Services>,
        stats_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);

        // Session counters + the persisted all-time totals. The fold touches a
        // small file, so it runs on the background executor rather than the UI
        // thread.
        let stats_services = Arc::clone(&services);
        cx.spawn(async move |this, cx| {
            let raw = stats_services.statistics().await;
            let folded = cx
                .background_executor()
                .spawn(async move {
                    raw.map(|raw| {
                        let (all_time_down, all_time_up) =
                            crate::stats::accumulate(&stats_path, raw.session_down, raw.session_up);
                        StatsView {
                            session_down: raw.session_down,
                            session_up: raw.session_up,
                            all_time_down,
                            all_time_up,
                            connected_peers: raw.connected_peers,
                            session_waste: raw.session_waste,
                            cache_hit_pct: raw.cache_hit_pct,
                            buffer_size: raw.buffer_size,
                            cache_overload_pct: raw.cache_overload_pct,
                            queued_io: raw.queued_io,
                        }
                    })
                })
                .await;
            this.update(cx, |this, cx| {
                match folded {
                    Ok(view) => this.stats = Some(view),
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();

        // The daemon's self-report is independent, so a slow one never delays
        // the counters.
        let health_services = Arc::clone(&services);
        cx.spawn(async move |this, cx| {
            let health = health_services.daemon_health().await;
            this.update(cx, |this, cx| {
                match health {
                    Ok(health) => this.health = Some(health),
                    Err(error) => this.health_error = Some(error.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();

        Self {
            palette: Palette::dark(),
            focus,
            tab: StatsTab::Statistics,
            stats: None,
            health: None,
            health_error: None,
            error: None,
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(StatisticsEvent::Close);
    }

    /// The placeholder row before a fetch resolves: "Unavailable" once an error
    /// has landed, otherwise "Loading…".
    fn pending_label(&self) -> String {
        if self.error.is_some() {
            "Unavailable".to_owned()
        } else {
            "Loading…".to_owned()
        }
    }

    fn statistics_rows(&self) -> Vec<(String, String, Option<u32>)> {
        let palette = self.palette;
        let Some(stats) = self.stats else {
            return vec![(self.pending_label(), String::new(), None)];
        };
        let ratio = (stats.all_time_down > 0)
            .then(|| stats.all_time_up as f64 / stats.all_time_down as f64);
        let mut rows = vec![
            (
                "Session download".to_owned(),
                crate::format::bytes(stats.session_down),
                None,
            ),
            (
                "Session upload".to_owned(),
                crate::format::bytes(stats.session_up),
                None,
            ),
            (
                "All-time download".to_owned(),
                crate::format::bytes(stats.all_time_down),
                None,
            ),
            (
                "All-time upload".to_owned(),
                crate::format::bytes(stats.all_time_up),
                None,
            ),
            (
                "All-time share ratio".to_owned(),
                ratio.map_or_else(|| "—".to_owned(), crate::format::ratio),
                Some(if ratio.is_some_and(|r| r >= 1.0) {
                    palette.accent_green_soft
                } else {
                    palette.text_primary
                }),
            ),
            (
                "Session waste".to_owned(),
                crate::format::bytes(stats.session_waste),
                None,
            ),
            (
                "Connected peers".to_owned(),
                stats.connected_peers.to_string(),
                None,
            ),
        ];
        rows.push((
            "Read cache hits".to_owned(),
            stats
                .cache_hit_pct
                .map_or_else(|| "—".to_owned(), |p| format!("{p:.1}%")),
            None,
        ));
        rows.push((
            "Total buffer size".to_owned(),
            stats
                .buffer_size
                .map_or_else(|| "—".to_owned(), crate::format::bytes),
            None,
        ));
        rows.push((
            "Write cache overload".to_owned(),
            stats
                .cache_overload_pct
                .map_or_else(|| "—".to_owned(), |p| format!("{p:.1}%")),
            None,
        ));
        rows.push((
            "Queued I/O jobs".to_owned(),
            stats
                .queued_io
                .map_or_else(|| "—".to_owned(), |n| n.to_string()),
            None,
        ));
        rows
    }

    fn daemon_rows(&self) -> Vec<(String, String, Option<u32>)> {
        let Some(health) = self.health.clone() else {
            let label = if self.health_error.is_some() {
                "Unavailable"
            } else {
                "Loading…"
            };
            return vec![(label.to_owned(), String::new(), None)];
        };
        let num = |value: i64| {
            if value > 0 {
                value.to_string()
            } else {
                "—".to_owned()
            }
        };
        let text = |value: &str| {
            if value.is_empty() {
                "—".to_owned()
            } else {
                value.to_owned()
            }
        };
        vec![
            (
                "rtorrent version".to_owned(),
                text(&health.client_version),
                None,
            ),
            ("XML-RPC API".to_owned(), text(&health.api_version), None),
            ("Session path".to_owned(), text(&health.session_path), None),
            (
                "Piece cache".to_owned(),
                if health.memory_max > 0 {
                    format!(
                        "{} / {}",
                        crate::format::bytes(health.memory_current),
                        crate::format::bytes(health.memory_max)
                    )
                } else {
                    "—".to_owned()
                },
                None,
            ),
            ("Open sockets".to_owned(), num(health.open_sockets), None),
            (
                "Max open sockets".to_owned(),
                num(health.max_open_sockets),
                None,
            ),
            (
                "Max open files".to_owned(),
                num(health.max_open_files),
                None,
            ),
            ("HTTP max open".to_owned(), num(health.http_max_open), None),
        ]
    }
}

/// A group of `key … value` rows under a dim caption.
fn stat_section(title: &str, rows: Vec<(String, String, Option<u32>)>, palette: Palette) -> Div {
    let mut block = div()
        .flex()
        .flex_col()
        .child(theme::text(title.to_owned(), 10., theme::color(palette.text_dim)).pb(px(6.)));
    for (key, value, colour) in rows {
        block = block.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .py(px(3.))
                .border_b_1()
                .border_color(theme::color(palette.row_line))
                .child(theme::text(key, 11., theme::color(palette.text_muted)))
                .child(div().min_w_0().text_right().child(theme::text(
                    value,
                    11.,
                    theme::color(colour.unwrap_or(palette.text_primary)),
                ))),
        );
    }
    block
}

impl Render for StatisticsDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette;
        let mut column = body().child(
            div()
                .flex()
                .gap(px(14.))
                .child(statistics_tab(
                    "stats-tab-statistics",
                    "Statistics",
                    self.tab == StatsTab::Statistics,
                    palette,
                    cx.listener(|this, _, _, cx| {
                        this.tab = StatsTab::Statistics;
                        cx.notify();
                    }),
                ))
                .child(statistics_tab(
                    "stats-tab-daemon",
                    "Daemon",
                    self.tab == StatsTab::Daemon,
                    palette,
                    cx.listener(|this, _, _, cx| {
                        this.tab = StatsTab::Daemon;
                        cx.notify();
                    }),
                )),
        );
        match self.tab {
            StatsTab::Statistics => {
                let rows = self.statistics_rows();
                let cache_at = 7.min(rows.len());
                let (user, cache) = rows.split_at(cache_at);
                column = column
                    .child(stat_section("User Statistics", user.to_vec(), palette))
                    .child(stat_section("Cache Statistics", cache.to_vec(), palette));
            }
            StatsTab::Daemon => {
                column = column.child(stat_section("Daemon", self.daemon_rows(), palette));
            }
        }
        if let Some(error) = self.error.clone() {
            column = column.child(theme::text(error, 10.5, theme::color(palette.accent_red)));
        }

        backdrop(&self.focus)
            .on_action(cx.listener(|this, _: &DismissOverlay, _, cx| this.close(cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close(cx)),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.close(cx);
                }
            }))
            .child(
                panel("statistics-dialog", 420., palette)
                    .child(header(
                        "Statistics",
                        cx.listener(|this, _, _, cx| this.close(cx)),
                        palette,
                    ))
                    .child(column)
                    .child(
                        footer(palette).child(
                            button(
                                "statistics-close",
                                "Close",
                                ButtonKind::Primary,
                                true,
                                palette,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                        ),
                    ),
            )
    }
}

/// One tab label with the shell's accent-underline idiom.
fn statistics_tab(
    id: &'static str,
    label: &str,
    selected: bool,
    palette: Palette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .flex()
        .flex_col()
        .cursor_pointer()
        .on_click(on_click)
        .child(
            theme::text(
                label.to_owned(),
                11.,
                theme::color(if selected {
                    palette.accent_cyan_bright
                } else {
                    palette.text_dim
                }),
            )
            .px(px(2.))
            .pb(px(4.)),
        )
        .child(div().h(px(2.)).w_full().bg(if selected {
            theme::color(palette.accent_cyan)
        } else {
            theme::color(0x0000_0000)
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_fields_parse_and_reject_nonsense() {
        assert_eq!(parse_kb(""), Ok(0));
        assert_eq!(parse_kb(" 512 "), Ok(512));
        assert_eq!(parse_kb("0"), Ok(0));
        assert!(parse_kb("-1").is_err());
        assert!(parse_kb("fast").is_err());
    }
}
