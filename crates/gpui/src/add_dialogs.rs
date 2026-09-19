//! The add-torrent and add-magnet dialogs.
//!
//! Ports of `AddTorrentDialog.tsx` and `AddMagnetDialog.tsx`. They share the
//! destination/label fields and the load options (`AddOptions`), and differ in
//! where the torrent comes from: a `.torrent` read from this machine, or a
//! magnet/URL handed to the daemon.
//!
//! Neither dialog talks to the daemon: each collects a source and its options and
//! emits [`AddEvent::Confirm`], and the shell applies it through the model, so an
//! add is logged, reported and followed by a re-poll like every other action.

use std::collections::HashSet;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, App, Context, Div, Entity, EventEmitter, FocusHandle, KeyDownEvent, MouseButton,
    PathPromptOptions, SharedString, Stateful, Subscription, Window,
};
use rtorrent_core::duplicates::{self, AddCandidate, Duplicate};
use rtorrent_core::rtorrent::{magnet_hash, magnet_name, magnet_trackers};
use rtorrent_core::types::{AddOptions, FileNode, TorrentMeta};

use crate::add_source;
use crate::detail_files::{self, TreeNode, TriState};
use crate::dialogs::{
    backdrop, body, button, checkbox, footer, form_field, header, panel, text_field, ButtonKind,
};
use crate::format;
use crate::model::TorrentsModel;
use crate::theme::{self, Palette};

/// Where a torrent comes from.
pub enum AddSource {
    /// A `.torrent` this machine read; the daemon is handed its bytes.
    Bytes(Vec<u8>),
    /// A magnet URI or torrent URL.
    Magnet(String),
}

/// What an add dialog decided.
pub enum AddEvent {
    Cancel,
    Confirm {
        source: AddSource,
        options: AddOptions,
    },
    /// An exact duplicate (V3-13): add these announce URLs to the existing
    /// torrent instead of adding a second copy.
    MergeTrackers {
        hash: String,
        urls: Vec<String>,
    },
    /// An exact duplicate: select the existing row and clear the filter/search.
    Reveal {
        hash: String,
    },
}

/// The warning for a detected duplicate, with no actions attached.
fn duplicate_note(duplicate: &Duplicate, palette: Palette) -> Div {
    let message = match duplicate {
        Duplicate::Exact { name, .. } => {
            format!("Already in your library as {name}.")
        }
        Duplicate::SameNameAndSize { name, .. } => {
            format!("Another torrent has the same name and size: {name}.")
        }
        Duplicate::DestinationInUse { name, path, .. } => {
            format!("The destination folder is already used by {name} ({path}).")
        }
    };
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .px(px(10.))
        .py(px(8.))
        .rounded(px(4.))
        .border_1()
        .border_color(theme::color(palette.accent_amber))
        .bg(theme::color(palette.track))
        .child(theme::text(
            message,
            11.,
            theme::color(palette.text_primary),
        ))
}

/// "Show it" / "Merge trackers" for a warning that blocks the add.
fn duplicate_actions(
    hash: &str,
    trackers: &[String],
    palette: Palette,
    cx: &mut Context<impl EventEmitter<AddEvent> + 'static>,
) -> Div {
    let reveal_hash = hash.to_owned();
    let merge_hash = hash.to_owned();
    let urls = trackers.to_vec();
    let mut actions = div().flex().gap(px(8.)).child(
        button(
            "add-reveal",
            "Show it",
            ButtonKind::Secondary,
            true,
            palette,
        )
        .on_click(cx.listener(move |_this, _, _, cx| {
            cx.emit(AddEvent::Reveal {
                hash: reveal_hash.clone(),
            });
        })),
    );
    if !urls.is_empty() {
        actions = actions.child(
            button(
                "add-merge",
                "Merge trackers",
                ButtonKind::Secondary,
                true,
                palette,
            )
            .on_click(cx.listener(move |_this, _, _, cx| {
                cx.emit(AddEvent::MergeTrackers {
                    hash: merge_hash.clone(),
                    urls: urls.clone(),
                });
            })),
        );
    }
    actions
}

/// The options a dialog's form adds up to.
///
/// Off the view on purpose: which files are left out and which flags are set is
/// worth testing without a window.
#[must_use]
pub fn add_options(
    save_path: &str,
    label: &str,
    start: bool,
    top_of_queue: bool,
    skip_hash_check: bool,
    unselected_indexes: Vec<usize>,
) -> AddOptions {
    AddOptions {
        save_path: save_path.trim().to_owned(),
        label: label.trim().to_owned(),
        start,
        top_of_queue,
        // Not offered: rtorrent has no sequential-download switch.
        sequential: false,
        skip_hash_check,
        unselected_indexes,
    }
}

/// The destination and label the two dialogs share, with their own entity.
///
/// Its own entity because both dialogs present it identically — label above the
/// field, a Browse button beside the path — and neither should own the other's
/// text.
pub struct AddFields {
    save_path: Entity<InputState>,
    label: Entity<InputState>,
}

impl AddFields {
    pub fn new(
        default_save_path: &str,
        label_placeholder: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let save_path = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            if !default_save_path.is_empty() {
                state.set_value(default_save_path, window, cx);
            }
            state
        });
        let label = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(SharedString::from(label_placeholder.to_owned()))
        });
        cx.new(|_| Self { save_path, label })
    }

    #[must_use]
    pub fn save_path(&self, cx: &App) -> String {
        self.save_path.read(cx).value().to_string()
    }

    #[must_use]
    pub fn label(&self, cx: &App) -> String {
        self.label.read(cx).value().to_string()
    }

    /// Ask the platform for a directory and put it in the field.
    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose where to save".into()),
        });
        let save_path = self.save_path.clone();
        cx.spawn_in(window, async move |_this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await {
                if let Some(path) = paths.into_iter().next() {
                    let text = path.to_string_lossy().into_owned();
                    cx.update(|window, cx| {
                        save_path.update(cx, |state, cx| {
                            state.set_value(text, window, cx);
                        });
                    })
                    .ok();
                }
            }
        })
        .detach();
    }
}

impl Render for AddFields {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::dark();
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(form_field(
                "Save to",
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(text_field(&self.save_path, palette)),
                    )
                    .child(
                        button(
                            "add-browse",
                            "Browse…",
                            ButtonKind::Secondary,
                            true,
                            palette,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.browse(window, cx);
                        })),
                    ),
                palette,
            ))
            .child(form_field(
                "Label",
                text_field(&self.label, palette),
                palette,
            ))
    }
}

/// The add-magnet dialog: a magnet URI or a torrent URL.
pub struct AddMagnetDialog {
    palette: Palette,
    focus: FocusHandle,
    uri: Entity<InputState>,
    fields: Entity<AddFields>,
    /// The library, for duplicate detection (V3-13).
    model: Entity<TorrentsModel>,
    start: bool,
    top_of_queue: bool,
    subscriptions: Vec<Subscription>,
}

impl AddMagnetDialog {
    pub fn new(
        default_save_path: &str,
        model: Entity<TorrentsModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let uri = cx.new(|cx| InputState::new(window, cx).placeholder("magnet:?xt=urn:btih:…"));
        let fields = AddFields::new(default_save_path, "(none)", window, cx);
        let dialog = Self {
            palette: Palette::dark(),
            focus,
            uri,
            fields,
            model,
            start: true,
            top_of_queue: false,
            subscriptions: Vec::new(),
        };
        // A magnet on the clipboard is what the user was about to paste; offer it
        // only when it would actually be accepted.
        if let Some(item) = cx.read_from_clipboard() {
            if let Some(text) = item.text() {
                if add_source::is_valid(&text) {
                    dialog.uri.update(cx, |state, cx| {
                        state.set_value(text.trim().to_owned(), window, cx);
                    });
                }
            }
        }
        // Enter in the source field adds, which is how a pasted magnet is
        // finished off.
        let mut dialog = dialog;
        let source = dialog.uri.clone();
        dialog.subscriptions.push(cx.subscribe_in(
            &source,
            window,
            |dialog, _input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    dialog.confirm(cx);
                }
            },
        ));
        dialog.focus.focus(window, cx);
        dialog
    }

    fn cancel(&self, cx: &mut Context<Self>) {
        cx.emit(AddEvent::Cancel);
    }

    fn confirm(&self, cx: &mut Context<Self>) {
        let uri = self.uri.read(cx).value().trim().to_owned();
        if !add_source::is_valid(&uri) {
            return;
        }
        // An exact duplicate is never added; the actions beside the warning are
        // the way forward (V3-13).
        if self
            .duplicate(cx)
            .is_some_and(|d| matches!(d, Duplicate::Exact { .. }))
        {
            return;
        }
        cx.emit(AddEvent::Confirm {
            source: AddSource::Magnet(uri),
            options: add_options(
                &self.fields.read(cx).save_path(cx),
                &self.fields.read(cx).label(cx),
                self.start,
                self.top_of_queue,
                false,
                Vec::new(),
            ),
        });
    }

    /// The duplicate finding for the current magnet, if any.
    fn duplicate(&self, cx: &App) -> Option<Duplicate> {
        let uri = self.uri.read(cx).value().trim().to_owned();
        if !add_source::is_valid(&uri) {
            return None;
        }
        let hash = magnet_hash(&uri);
        let name = magnet_name(&uri);
        let destination = self.fields.read(cx).save_path(cx);
        let candidate = AddCandidate {
            hash: hash.as_deref(),
            name: (!name.is_empty()).then_some(name.as_str()),
            size: None,
            destination: (!destination.is_empty()).then_some(destination.as_str()),
        };
        duplicates::detect(&candidate, self.model.read(cx).torrents())
    }
}

impl Render for AddMagnetDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette;
        let uri = self.uri.read(cx).value().trim().to_owned();
        let valid = add_source::is_valid(&uri);
        let show_invalid = !uri.is_empty() && !valid;

        // Duplicate detection (V3-13), built before the element chain so its
        // action buttons can take `cx` first.
        let duplicate = self.duplicate(cx);
        let trackers = magnet_trackers(&uri);
        let duplicate_block = duplicate.as_ref().map(|found| {
            let mut block = duplicate_note(found, palette);
            if let Duplicate::Exact { hash, .. } = found {
                block = block.child(duplicate_actions(hash, &trackers, palette, cx));
            }
            block
        });

        backdrop(&self.focus)
            .on_action(
                cx.listener(|this, _: &crate::actions::DismissOverlay, _, cx| this.cancel(cx)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.cancel(cx)),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                // Esc backs out. Enter is the source field's business, which
                // subscribes to it: a stray Enter on a checkbox must not add
                // something half-typed.
                if event.keystroke.key.as_str() == "escape" {
                    cx.stop_propagation();
                    this.cancel(cx);
                }
            }))
            .child(
                panel("add-magnet-dialog", 460., palette)
                    .child(header(
                        "Add magnet link",
                        cx.listener(|this, _, _, cx| this.cancel(cx)),
                        palette,
                    ))
                    .child(
                        body()
                            .child(form_field(
                                "Magnet URI or torrent URL",
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(text_field(&self.uri, palette))
                                    .when(show_invalid, |field| {
                                        field.child(theme::text(
                                            "not a valid magnet or torrent URL".to_owned(),
                                            10.5,
                                            theme::color(palette.accent_red),
                                        ))
                                    }),
                                palette,
                            ))
                            .when_some(duplicate_block, |body, block| body.child(block))
                            .child(self.fields.clone())
                            .child(
                                div()
                                    .flex()
                                    .gap(px(20.))
                                    .child(checkbox(
                                        "magnet-start",
                                        "Start torrent",
                                        self.start,
                                        true,
                                        palette,
                                        cx.listener(|this, _, _, cx| {
                                            this.start = !this.start;
                                            cx.notify();
                                        }),
                                    ))
                                    .child(checkbox(
                                        "magnet-top",
                                        "Add to top of queue",
                                        self.top_of_queue,
                                        true,
                                        palette,
                                        cx.listener(|this, _, _, cx| {
                                            this.top_of_queue = !this.top_of_queue;
                                            cx.notify();
                                        }),
                                    )),
                            ),
                    )
                    .child(
                        footer(palette)
                            .child(
                                button(
                                    "add-magnet-cancel",
                                    "Cancel",
                                    ButtonKind::Secondary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                            )
                            .child(
                                button(
                                    "add-magnet-confirm",
                                    "Add",
                                    ButtonKind::Primary,
                                    valid && !matches!(duplicate, Some(Duplicate::Exact { .. })),
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
                            ),
                    ),
            )
    }
}

impl EventEmitter<AddEvent> for AddMagnetDialog {}

/// The add-torrent dialog: a `.torrent` from this machine, with its contents.
pub struct AddTorrentDialog {
    palette: Palette,
    focus: FocusHandle,
    fields: Entity<AddFields>,
    /// The library, for duplicate detection (V3-13).
    model: Entity<TorrentsModel>,
    /// The file as read, kept so the add does not read it a second time.
    bytes: Option<Vec<u8>>,
    meta: Option<TorrentMeta>,
    error: Option<String>,
    /// The files that will be loaded, out of the torrent's own list.
    selected: HashSet<usize>,
    expanded: HashSet<String>,
    start: bool,
    top_of_queue: bool,
    skip_hash_check: bool,
}

impl AddTorrentDialog {
    pub fn new(
        default_save_path: &str,
        model: Entity<TorrentsModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let fields = AddFields::new(default_save_path, "(none)", window, cx);
        let mut dialog = Self {
            palette: Palette::dark(),
            focus,
            fields,
            model,
            bytes: None,
            meta: None,
            error: None,
            selected: HashSet::new(),
            expanded: HashSet::new(),
            start: true,
            top_of_queue: false,
            skip_hash_check: false,
        };
        dialog.focus.focus(window, cx);
        dialog.pick(window, cx);
        dialog
    }

    /// Ask for a `.torrent` and read it. Off the UI thread: the file can be
    /// megabytes and parsing it is not the shell's business.
    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a .torrent file".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let picked = match chosen.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                // Cancelled, or the platform has no picker: nothing to add.
                _ => None,
            };
            let Some(path) = picked else {
                this.update(cx, |_, cx| cx.emit(AddEvent::Cancel)).ok();
                return;
            };
            let bytes = cx
                .background_executor()
                .spawn(async move { std::fs::read(path) })
                .await;
            this.update(cx, |this, cx| this.read(bytes, cx)).ok();
        })
        .detach();
    }

    /// Take the file's bytes, or say why they could not be read.
    fn read(&mut self, bytes: Result<Vec<u8>, std::io::Error>, cx: &mut Context<Self>) {
        match bytes {
            Err(error) => self.error = Some(format!("could not read the file: {error}")),
            Ok(bytes) => match rtorrent_core::torrent_file::read_metadata_bytes(&bytes) {
                Err(error) => self.error = Some(error),
                Ok(meta) => {
                    // Everything starts selected, with the top level open.
                    self.selected = (0..meta.files.len()).collect();
                    self.expanded = detail_files::build_tree(&meta.files)
                        .iter()
                        .filter(|node| node.is_dir)
                        .map(|node| format!("/{}", node.name))
                        .collect();
                    self.bytes = Some(bytes);
                    self.meta = Some(meta);
                }
            },
        }
        cx.notify();
    }

    fn cancel(&self, cx: &mut Context<Self>) {
        cx.emit(AddEvent::Cancel);
    }

    fn confirm(&self, cx: &mut Context<Self>) {
        let (Some(bytes), Some(meta)) = (self.bytes.as_ref(), self.meta.as_ref()) else {
            return;
        };
        // An exact duplicate is never added; the actions beside the warning are
        // the way forward (V3-13).
        if self
            .duplicate(cx)
            .is_some_and(|found| matches!(found, Duplicate::Exact { .. }))
        {
            return;
        }
        // Everything not ticked is loaded at priority 0 rather than omitted.
        let unselected: Vec<usize> = (0..meta.files.len())
            .filter(|index| !self.selected.contains(index))
            .collect();
        cx.emit(AddEvent::Confirm {
            source: AddSource::Bytes(bytes.clone()),
            options: add_options(
                &self.fields.read(cx).save_path(cx),
                &self.fields.read(cx).label(cx),
                self.start,
                self.top_of_queue,
                self.skip_hash_check,
                unselected,
            ),
        });
    }

    /// The duplicate finding for the inspected torrent, if any.
    fn duplicate(&self, cx: &App) -> Option<Duplicate> {
        let meta = self.meta.as_ref()?;
        let destination = self.fields.read(cx).save_path(cx);
        let candidate = AddCandidate {
            hash: Some(&meta.info_hash),
            name: Some(&meta.name),
            size: Some(meta.size),
            destination: (!destination.is_empty()).then_some(destination.as_str()),
        };
        duplicates::detect(&candidate, self.model.read(cx).torrents())
    }

    /// Tick or untick every file under a node.
    fn toggle(&mut self, node: &TreeNode, on: bool) {
        for index in detail_files::leaf_indexes(node) {
            if on {
                self.selected.insert(index);
            } else {
                self.selected.remove(&index);
            }
        }
    }

    fn set_all(&mut self, on: bool, cx: &mut Context<Self>) {
        self.selected = match self.meta.as_ref() {
            Some(meta) if on => (0..meta.files.len()).collect(),
            _ => HashSet::new(),
        };
        cx.notify();
    }

    /// The contents tree: one row per file, folders rolling their files up.
    fn contents(
        &self,
        nodes: &[TreeNode],
        depth: u32,
        parent: &str,
        cx: &mut Context<Self>,
    ) -> Vec<Stateful<Div>> {
        let palette = self.palette;
        let mut rows = Vec::new();
        for node in nodes {
            let path = format!("{parent}/{}", node.name);
            let is_open = self.expanded.contains(&path);
            let state = detail_files::folder_state(node, &self.selected);
            let mark = match state {
                TriState::Checked => "✓",
                TriState::Indeterminate => "–",
                TriState::Unchecked => "",
            };
            let (box_bg, box_border) = match state {
                TriState::Unchecked => (palette.field, palette.border_strong),
                _ => (palette.accent_cyan, palette.accent_cyan),
            };
            let node_for_toggle = node.clone();
            let path_for_expand = path.clone();
            let name = node.name.clone();
            let size = node.size;
            let is_dir = node.is_dir;

            rows.push(
                div()
                    .id(SharedString::from(format!("add-file-{path}")))
                    .h(px(geometry_row()))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .pl(px(6. + 14.0 * depth as f32))
                    .pr(px(8.))
                    .hover(|style| style.bg(theme::color(palette.row_alt)))
                    .child(
                        div()
                            .w(px(12.))
                            .h(px(12.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(2.))
                            .border_1()
                            .border_color(theme::color(box_border))
                            .bg(theme::color(box_bg))
                            .cursor_pointer()
                            .child(theme::text(mark.to_owned(), 9., theme::color(palette.app)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    let on = detail_files::folder_state(
                                        &node_for_toggle,
                                        &this.selected,
                                    ) != TriState::Checked;
                                    this.toggle(&node_for_toggle.clone(), on);
                                    cx.notify();
                                }),
                            ),
                    )
                    .child(
                        div()
                            .w(px(10.))
                            .flex_none()
                            .flex()
                            .justify_center()
                            .cursor_pointer()
                            .when(is_dir, |twisty| {
                                twisty.on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        if !this.expanded.remove(&path_for_expand) {
                                            this.expanded.insert(path_for_expand.clone());
                                        }
                                        cx.notify();
                                    }),
                                )
                            })
                            .child(theme::text(
                                if is_dir && is_open {
                                    "▾".to_owned()
                                } else if is_dir {
                                    "▸".to_owned()
                                } else {
                                    String::new()
                                },
                                9.,
                                theme::color(palette.text_dim),
                            )),
                    )
                    .child(
                        theme::text(
                            name,
                            11.,
                            theme::color(if is_dir {
                                palette.text_body
                            } else {
                                palette.text_primary
                            }),
                        )
                        .flex_1()
                        .min_w_0()
                        .truncate(),
                    )
                    .child(
                        theme::mono(format::bytes(size), 10., theme::color(palette.text_muted))
                            .w(px(70.))
                            .flex_none()
                            .text_right(),
                    ),
            );

            if is_dir && is_open {
                rows.extend(self.contents(&node.children, depth + 1, &path, cx));
            }
        }
        rows
    }
}

impl EventEmitter<AddEvent> for AddTorrentDialog {}

/// A contents row's height.
fn geometry_row() -> f32 {
    18.
}

impl Render for AddTorrentDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette;
        let ready = self.meta.is_some();
        let selected_size = self.meta.as_ref().map_or(0, |meta| {
            detail_files::selected_size(&meta.files, &self.selected)
        });

        // Duplicate detection (V3-13), built before the body chain so its action
        // buttons can take `cx` first.
        let duplicate = self.duplicate(cx);
        let duplicate_trackers = self
            .meta
            .as_ref()
            .map(|meta| meta.trackers.clone())
            .unwrap_or_default();
        let duplicate_block = duplicate.as_ref().map(|found| {
            let mut block = duplicate_note(found, palette);
            if let Duplicate::Exact { hash, .. } = found {
                block = block.child(duplicate_actions(hash, &duplicate_trackers, palette, cx));
            }
            block
        });

        let mut pane = body().child(self.fields.clone());
        if let Some(block) = duplicate_block {
            pane = pane.child(block);
        }
        if let Some(error) = self.error.clone() {
            pane = pane.child(theme::text(error, 10.5, theme::color(palette.accent_red)));
        } else if let Some(meta) = self.meta.clone() {
            pane = pane
                .child(form_field(
                    "Torrent",
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            theme::text(
                                meta.name.clone(),
                                11.5,
                                theme::color(palette.text_primary),
                            )
                            .truncate()
                            .min_w_0(),
                        )
                        .child(theme::text(
                            format!(
                                "{} · {} file{}",
                                format::bytes(meta.size),
                                meta.files.len(),
                                if meta.files.len() == 1 { "" } else { "s" }
                            ),
                            10.,
                            theme::color(palette.text_muted),
                        )),
                    palette,
                ))
                .child(
                    div()
                        .flex()
                        .gap(px(20.))
                        .child(checkbox(
                            "add-start",
                            "Start torrent",
                            self.start,
                            true,
                            palette,
                            cx.listener(|this, _, _, cx| {
                                this.start = !this.start;
                                cx.notify();
                            }),
                        ))
                        .child(checkbox(
                            "add-skip-hash",
                            "Skip hash check",
                            self.skip_hash_check,
                            true,
                            palette,
                            cx.listener(|this, _, _, cx| {
                                this.skip_hash_check = !this.skip_hash_check;
                                cx.notify();
                            }),
                        ))
                        .child(checkbox(
                            "add-top",
                            "Add to top of queue",
                            self.top_of_queue,
                            true,
                            palette,
                            cx.listener(|this, _, _, cx| {
                                this.top_of_queue = !this.top_of_queue;
                                cx.notify();
                            }),
                        )),
                )
                .child(
                    div()
                        .id("add-contents")
                        .flex()
                        .flex_col()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(theme::color(palette.border_strong))
                        .overflow_hidden()
                        .child(
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_between()
                                .px(px(8.))
                                .py(px(5.))
                                .bg(theme::color(palette.panel))
                                .border_b_1()
                                .border_color(theme::color(palette.border_mid))
                                .child(theme::mono(
                                    format!("Contents · {} selected", format::bytes(selected_size)),
                                    10.,
                                    theme::color(palette.text_muted),
                                ))
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(8.))
                                        .child(
                                            theme::text(
                                                "select all".to_owned(),
                                                10.,
                                                theme::color(palette.accent_cyan_bright),
                                            )
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _, _, cx| {
                                                    this.set_all(true, cx)
                                                }),
                                            ),
                                        )
                                        .child(
                                            theme::text(
                                                "none".to_owned(),
                                                10.,
                                                theme::color(palette.accent_cyan_bright),
                                            )
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _, _, cx| {
                                                    this.set_all(false, cx)
                                                }),
                                            ),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .id("add-contents-tree")
                                .max_h(px(150.))
                                .overflow_y_scroll()
                                .children(self.contents(
                                    &detail_files::build_tree(&meta.files),
                                    0,
                                    "",
                                    cx,
                                )),
                        ),
                );
        } else {
            pane = pane.child(theme::text(
                "reading torrent…".to_owned(),
                11.,
                theme::color(palette.text_muted),
            ));
        }

        backdrop(&self.focus)
            .on_action(
                cx.listener(|this, _: &crate::actions::DismissOverlay, _, cx| this.cancel(cx)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.cancel(cx)),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key.as_str() == "escape" {
                    cx.stop_propagation();
                    this.cancel(cx);
                }
            }))
            .child(
                panel("add-torrent-dialog", 620., palette)
                    .child(header(
                        "Add torrent",
                        cx.listener(|this, _, _, cx| this.cancel(cx)),
                        palette,
                    ))
                    .child(pane)
                    .child(
                        footer(palette)
                            .child(
                                button(
                                    "add-torrent-cancel",
                                    "Cancel",
                                    ButtonKind::Secondary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                            )
                            .child(
                                button(
                                    "add-torrent-confirm",
                                    "Add",
                                    ButtonKind::Primary,
                                    ready && !matches!(duplicate, Some(Duplicate::Exact { .. })),
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
                            ),
                    ),
            )
    }
}

/// The files a torrent would load, for the shell's decision — kept here so the
/// dialog's own `selected` set never leaves it.
#[must_use]
pub fn unselected(files: &[FileNode], selected: &HashSet<usize>) -> Vec<usize> {
    (0..files.len())
        .filter(|index| !selected.contains(index))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_options_carry_the_form_and_the_files_left_out() {
        let options = add_options("  /downloads/iso ", " iso ", true, false, true, vec![2, 4]);
        assert_eq!(options.save_path, "/downloads/iso");
        assert_eq!(options.label, "iso");
        assert!(options.start);
        assert!(!options.top_of_queue);
        assert!(options.skip_hash_check);
        // rtorrent has no sequential switch, so it is never set.
        assert!(!options.sequential);
        assert_eq!(options.unselected_indexes, vec![2, 4]);
    }

    #[test]
    fn an_empty_form_still_produces_usable_options() {
        let options = add_options("", "", false, true, false, Vec::new());
        assert!(options.save_path.is_empty());
        assert!(options.label.is_empty());
        assert!(options.top_of_queue);
        assert!(options.unselected_indexes.is_empty());
    }

    #[test]
    fn the_unselected_list_is_the_complement_of_the_selection() {
        let files: Vec<FileNode> = (0..5)
            .map(|index| FileNode {
                path: format!("f{index}"),
                size: 1,
                priority: 1,
                progress: 0.0,
                is_dir: false,
            })
            .collect();
        let selected = HashSet::from([0, 2]);
        assert_eq!(unselected(&files, &selected), vec![1, 3, 4]);
        assert_eq!(unselected(&files, &HashSet::new()), vec![0, 1, 2, 3, 4]);
        let all: HashSet<usize> = (0..5).collect();
        assert!(unselected(&files, &all).is_empty());
    }
}
