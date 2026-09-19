//! The create-torrent dialog: build a `.torrent` from a file or folder here.
//!
//! A port of `CreateTorrentDialog.tsx`. The work is local — `rtorrent-core`
//! hashes the source and writes the metainfo — so this is the one dialog that
//! does its own file I/O; it runs off the UI thread because hashing a large
//! torrent takes as long as it takes. Telling the daemon to seed it afterwards
//! is the app's business (`Services::add_raw`), and the report is the shell's:
//! the dialog shows the result and emits it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::component::input::InputState;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};
use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, MouseButton,
    PathPromptOptions, SharedString, Subscription, Window,
};
use rtorrent_core::torrent_file;
use rtorrent_core::types::{AddOptions, CreateTorrentOptions};

use crate::actions;
use crate::dialogs::{
    backdrop, body, button, checkbox, footer, form_field, header, panel, text_field, ButtonKind,
};
use crate::format;
use crate::model::TorrentsModel;
use crate::services::Services;
use crate::theme::{self, Palette};

/// The piece sizes offered, `0` meaning "let the creator choose".
///
/// The same ladder the Tauri dialog offers, and the same default: a piece size
/// is a judgement about the swarm, and most people have no reason to make it.
pub const PIECE_SIZES: [(i64, &str); 13] = [
    (0, "Auto (optimal)"),
    (16_384, "16 KiB"),
    (32_768, "32 KiB"),
    (65_536, "64 KiB"),
    (131_072, "128 KiB"),
    (262_144, "256 KiB"),
    (524_288, "512 KiB"),
    (1_048_576, "1 MiB"),
    (2_097_152, "2 MiB"),
    (4_194_304, "4 MiB"),
    (8_388_608, "8 MiB"),
    (16_777_216, "16 MiB"),
    (33_554_432, "32 MiB"),
];

/// The label for a piece length.
#[must_use]
pub fn piece_label(bytes: i64) -> &'static str {
    PIECE_SIZES
        .iter()
        .find(|(value, _)| *value == bytes)
        .map_or("Auto (optimal)", |(_, label)| *label)
}

/// The announce URLs a block of text holds: one per line, blanks ignored.
///
/// Tiers are not expressible in the options rtorrent-core takes, so a blank line
/// is just a separator to the eye rather than something that changes the file.
#[must_use]
pub fn trackers_from(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The last path element of a source, which is what the torrent is named after.
#[must_use]
pub fn source_name(source: &str) -> String {
    Path::new(source.trim_end_matches(['/', '\\']))
        .file_name()
        .map_or_else(
            || "new".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        )
}

/// Where to offer to save the `.torrent`: the configured save path if there is
/// one, else alongside the source.
#[must_use]
pub fn suggested_output(source: &str, default_dir: &str) -> String {
    let trimmed = source.trim_end_matches(['/', '\\']);
    if default_dir.is_empty() {
        // Where the source is, named after it: `/iso/debian.iso` becomes
        // `/iso/debian.iso.torrent`.
        return format!("{trimmed}.torrent");
    }
    Path::new(default_dir)
        .join(format!("{}.torrent", source_name(source)))
        .to_string_lossy()
        .into_owned()
}

/// What the dialog decided, for the shell to report.
pub enum CreateEvent {
    Cancel,
    /// The torrent exists: the message is one line, the hash names it.
    Done {
        message: String,
        hash: Option<String>,
    },
}

/// What creating one produced.
struct Created {
    name: String,
    info_hash: String,
    total_size: i64,
    piece_count: i64,
    output_path: String,
}

/// The create dialog.
pub struct CreateTorrentDialog {
    model: Entity<TorrentsModel>,
    palette: Palette,
    focus: FocusHandle,
    source: Entity<InputState>,
    output: Entity<InputState>,
    tracker_field: Entity<InputState>,
    source_tag: Entity<InputState>,
    comment: Entity<InputState>,
    /// The announce URLs added so far; the field adds one at a time.
    trackers: Vec<String>,
    piece_length: i64,
    is_private: bool,
    start_seeding: bool,
    busy: bool,
    error: Option<String>,
    created: Option<Created>,
    subscriptions: Vec<Subscription>,
}

impl CreateTorrentDialog {
    pub fn new(model: Entity<TorrentsModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let source =
            cx.new(|cx| InputState::new(window, cx).placeholder("path to a file or folder"));
        let output =
            cx.new(|cx| InputState::new(window, cx).placeholder("where to save the .torrent"));
        let tracker_field = cx.new(|cx| {
            InputState::new(window, cx).placeholder("http://tracker.example.com:80/announce")
        });
        let source_tag = cx.new(|cx| InputState::new(window, cx).placeholder("optional"));
        let comment = cx.new(|cx| InputState::new(window, cx).placeholder("optional"));

        let focus = cx.focus_handle();
        // Seeding means handing the file to the daemon, which only makes sense
        // when its files are on this machine.
        let start_seeding = model.read(cx).services().is_local();
        let mut dialog = Self {
            model,
            palette: Palette::dark(),
            focus,
            source,
            output,
            tracker_field,
            source_tag,
            comment,
            trackers: Vec::new(),
            piece_length: 0,
            is_private: false,
            start_seeding,
            busy: false,
            error: None,
            created: None,
            subscriptions: Vec::new(),
        };

        // Enter in the tracker field adds it, the way the Trackers pane does.
        let field = dialog.tracker_field.clone();
        dialog.subscriptions.push(cx.subscribe_in(
            &field,
            window,
            |dialog, _input, event: &gpui_kit::component::input::InputEvent, window, cx| {
                if matches!(
                    event,
                    gpui_kit::component::input::InputEvent::PressEnter { .. }
                ) {
                    dialog.add_tracker(window, cx);
                }
            },
        ));
        dialog.focus.focus(window, cx);
        dialog
    }

    /// Set the piece length the menu picked.
    pub fn set_piece_length(&mut self, bytes: i64, cx: &mut Context<Self>) {
        self.piece_length = bytes;
        cx.notify();
    }

    /// Take the URL in the field into the list.
    fn add_tracker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = self.tracker_field.clone();
        let url = field.read(cx).value().trim().to_owned();
        if url.is_empty() {
            return;
        }
        self.trackers.push(url);
        field.update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
    }

    fn cancel(&self, cx: &mut Context<Self>) {
        cx.emit(CreateEvent::Cancel);
    }

    /// Ask for a source file, a source folder, or somewhere to save.
    fn pick(&mut self, what: Pick, window: &mut Window, cx: &mut Context<Self>) {
        let source = self.source.clone();
        let output = self.output.clone();
        let default_dir = self.model.read(cx).services().settings().default_save_path;

        match what {
            Pick::SourceFile | Pick::SourceFolder => {
                let directories = matches!(what, Pick::SourceFolder);
                let chosen = cx.prompt_for_paths(PathPromptOptions {
                    files: !directories,
                    directories,
                    multiple: false,
                    prompt: Some(if directories {
                        "Choose a folder to seed".into()
                    } else {
                        "Choose a file to seed".into()
                    }),
                });
                cx.spawn_in(window, async move |_this, cx| {
                    if let Ok(Ok(Some(paths))) = chosen.await {
                        if let Some(path) = paths.into_iter().next() {
                            let text = path.to_string_lossy().into_owned();
                            let suggested = suggested_output(&text, &default_dir);
                            cx.update(|window, cx| {
                                // A source suggests where the .torrent goes, but
                                // never overwrites a path already typed.
                                let current = output.read(cx).value().to_string();
                                source.update(cx, |state, cx| state.set_value(text, window, cx));
                                if current.trim().is_empty() {
                                    output.update(cx, |state, cx| {
                                        state.set_value(suggested, window, cx)
                                    });
                                }
                            })
                            .ok();
                        }
                    }
                })
                .detach();
            }
            Pick::Output => {
                let directory = PathBuf::from(if default_dir.is_empty() {
                    std::env::var("HOME").unwrap_or_default()
                } else {
                    default_dir
                });
                let name = {
                    let typed = output.read(cx).value().trim().to_owned();
                    if typed.is_empty() {
                        format!("{}.torrent", source_name(&source.read(cx).value()))
                    } else {
                        Path::new(&typed).file_name().map_or_else(
                            || "new.torrent".to_owned(),
                            |n| n.to_string_lossy().into_owned(),
                        )
                    }
                };
                let chosen = cx.prompt_for_new_path(&directory, Some(&name));
                cx.spawn_in(window, async move |_this, cx| {
                    if let Ok(Ok(Some(path))) = chosen.await {
                        let text = path.to_string_lossy().into_owned();
                        cx.update(|window, cx| {
                            output.update(cx, |state, cx| state.set_value(text, window, cx));
                        })
                        .ok();
                    }
                })
                .detach();
            }
        }
    }

    /// Create the torrent, off the UI thread, then report.
    fn confirm(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let source = self.source.read(cx).value().trim().to_owned();
        if source.is_empty() {
            self.error = Some("choose a source file or folder first".to_owned());
            cx.notify();
            return;
        }
        let output = self.output.read(cx).value().trim().to_owned();
        let comment = self.comment.read(cx).value().trim().to_owned();
        let tag = self.source_tag.read(cx).value().trim().to_owned();
        let local = self.model.read(cx).services().is_local();
        let start_seeding = self.start_seeding && local;

        let options = CreateTorrentOptions {
            source_path: PathBuf::from(&source),
            piece_length: (self.piece_length > 0).then_some(self.piece_length),
            trackers: self.trackers.clone(),
            is_private: self.is_private,
            comment: (!comment.is_empty()).then_some(comment),
            source: (!tag.is_empty()).then_some(tag),
            created_by: Some("Blackbird".to_owned()),
        };

        let services = Arc::clone(self.model.read(cx).services());
        let runtime = services.runtime();
        self.busy = true;
        self.error = None;
        self.created = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let outcome = runtime
                .spawn(async move { create(options, output, start_seeding, services).await })
                .await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match outcome {
                    // The task itself could not run: that is a bug, not a bad form.
                    Err(join) => this.error = Some(format!("the create task failed: {join}")),
                    Ok(Err(error)) => this.error = Some(error),
                    Ok(Ok(created)) => {
                        let message = format!(
                            "created {} ({} pieces, {})",
                            created.name,
                            created.piece_count,
                            format::bytes(created.total_size)
                        );
                        let hash = created.info_hash.clone();
                        this.created = Some(created);
                        cx.emit(CreateEvent::Done {
                            message,
                            hash: Some(hash),
                        });
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// Which picker was asked for.
#[derive(Clone, Copy)]
enum Pick {
    SourceFile,
    SourceFolder,
    Output,
}

/// Create the file, write it, and hand it to the daemon to seed when asked.
///
/// Runs on the service runtime's blocking pool: hashing a large source is CPU
/// and disk work that would otherwise stall whatever thread it landed on.
async fn create(
    options: CreateTorrentOptions,
    output: String,
    start_seeding: bool,
    services: Arc<Services>,
) -> Result<Created, String> {
    // rtorrent seeds a torrent from the directory that holds its data, which is
    // where the source we hashed lives.
    let seed_directory = options
        .source_path
        .parent()
        .map_or_else(String::new, |parent| parent.to_string_lossy().into_owned());

    let hashed = tokio::task::spawn_blocking(move || torrent_file::create_torrent(options))
        .await
        .map_err(|error| format!("the create task failed: {error}"))?;
    let (torrent, bytes) = hashed?;

    if !output.is_empty() {
        let path = PathBuf::from(&output);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
            }
        }
        std::fs::write(&path, &bytes)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    }

    if start_seeding {
        services
            .add_raw(
                bytes,
                AddOptions {
                    save_path: seed_directory,
                    label: String::new(),
                    start: true,
                    top_of_queue: false,
                    sequential: false,
                    // The data is already on disk; re-hashing it would only cost
                    // time before the first announce.
                    skip_hash_check: true,
                    unselected_indexes: Vec::new(),
                },
            )
            .await
            .map_err(|error| error.to_string())?;
    }

    Ok(Created {
        name: torrent.name.clone(),
        info_hash: torrent.info_hash().to_uppercase(),
        total_size: torrent.length,
        piece_count: i64::try_from(torrent.pieces.len()).unwrap_or(0),
        output_path: output,
    })
}

impl EventEmitter<CreateEvent> for CreateTorrentDialog {}

impl Render for CreateTorrentDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette;
        let has_source = !self.source.read(cx).value().trim().is_empty();
        let piece_length = self.piece_length;
        let trackers = self.trackers.clone();
        let created = self.created.as_ref().map(|created| {
            (
                created.name.clone(),
                created.info_hash.clone(),
                created.piece_count,
                created.output_path.clone(),
            )
        });

        let mut pane = body();
        if let Some(error) = self.error.clone() {
            pane = pane.child(theme::text(error, 10.5, theme::color(palette.accent_red)));
        }
        if let Some((name, hash, pieces, output_path)) = created {
            pane = pane.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .p(px(9.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(theme::color(palette.border_mid))
                    .bg(theme::color(palette.panel))
                    .child(theme::text(
                        format!("Created {name} ({pieces} pieces)"),
                        11.,
                        theme::color(palette.text_primary),
                    ))
                    .child(theme::mono(
                        format!("info-hash {hash}"),
                        10.5,
                        theme::color(palette.text_muted),
                    ))
                    .when(!output_path.is_empty(), |block| {
                        block.child(theme::mono(
                            output_path,
                            10.5,
                            theme::color(palette.text_dim),
                        ))
                    }),
            );
        }

        pane = pane
            .child(form_field(
                "Source",
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(text_field(&self.source, palette)),
                    )
                    .child(
                        button(
                            "create-source-file",
                            "File…",
                            ButtonKind::Secondary,
                            true,
                            palette,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.pick(Pick::SourceFile, window, cx);
                        })),
                    )
                    .child(
                        button(
                            "create-source-folder",
                            "Folder…",
                            ButtonKind::Secondary,
                            true,
                            palette,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.pick(Pick::SourceFolder, window, cx);
                        })),
                    ),
                palette,
            ))
            .child(form_field(
                "Save .torrent to",
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(text_field(&self.output, palette)),
                    )
                    .child(
                        button(
                            "create-output",
                            "Browse…",
                            ButtonKind::Secondary,
                            true,
                            palette,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.pick(Pick::Output, window, cx);
                        })),
                    ),
                palette,
            ))
            .child(form_field(
                "Trackers",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(text_field(&self.tracker_field, palette)),
                            )
                            .child(
                                button(
                                    "create-tracker-add",
                                    "Add",
                                    ButtonKind::Secondary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.add_tracker(window, cx);
                                    },
                                )),
                            ),
                    )
                    .children(trackers.iter().enumerate().map(|(index, url)| {
                        div()
                            .id(SharedString::from(format!("create-tracker-{index}")))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                theme::text(url.clone(), 10.5, theme::color(palette.text_body))
                                    .flex_1()
                                    .min_w_0()
                                    .truncate(),
                            )
                            .child(
                                theme::text("×".to_owned(), 11., theme::color(palette.text_dim))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            this.trackers.remove(index);
                                            cx.notify();
                                        }),
                                    ),
                            )
                    })),
                palette,
            ))
            .child(
                div()
                    .flex()
                    .gap(px(14.))
                    .child(
                        div().flex_1().min_w_0().child(form_field(
                            "Piece size",
                            div()
                                .id("create-piece-length")
                                .flex()
                                .items_center()
                                .justify_between()
                                .w_full()
                                .h(px(24.))
                                .px(px(8.))
                                .rounded(px(4.))
                                .border_1()
                                .border_color(theme::color(palette.border_strong))
                                .bg(theme::color(palette.field))
                                .cursor_pointer()
                                .context_menu(move |menu, _, _| {
                                    build_piece_menu(menu, piece_length)
                                })
                                .child(theme::text(
                                    piece_label(self.piece_length).to_owned(),
                                    11.,
                                    theme::color(palette.text_body),
                                ))
                                .child(theme::text(
                                    "▾".to_owned(),
                                    9.,
                                    theme::color(palette.text_dim),
                                )),
                            palette,
                        )),
                    )
                    .child(div().flex_1().min_w_0().child(form_field(
                        "Source (optional)",
                        text_field(&self.source_tag, palette),
                        palette,
                    ))),
            )
            .child(form_field(
                "Comment",
                text_field(&self.comment, palette),
                palette,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(checkbox(
                        "create-private",
                        "Private torrent (no DHT or peer exchange)",
                        self.is_private,
                        true,
                        palette,
                        cx.listener(|this, _, _, cx| {
                            this.is_private = !this.is_private;
                            cx.notify();
                        }),
                    ))
                    .child(checkbox(
                        "create-seed",
                        "Start seeding in the daemon",
                        self.start_seeding,
                        true,
                        palette,
                        cx.listener(|this, _, _, cx| {
                            this.start_seeding = !this.start_seeding;
                            cx.notify();
                        }),
                    )),
            );

        backdrop(&self.focus)
            .on_action(cx.listener(|this, _: &actions::DismissOverlay, _, cx| this.cancel(cx)))
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
                panel("create-torrent-dialog", 560., palette)
                    .child(header(
                        "Create torrent",
                        cx.listener(|this, _, _, cx| this.cancel(cx)),
                        palette,
                    ))
                    .child(pane)
                    .child(
                        footer(palette)
                            .child(
                                button(
                                    "create-cancel",
                                    "Cancel",
                                    ButtonKind::Secondary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                            )
                            .child(
                                button(
                                    "create-confirm",
                                    if self.busy { "Creating…" } else { "Create" },
                                    ButtonKind::Primary,
                                    has_source && !self.busy,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
                            ),
                    ),
            )
    }
}

/// The piece-size menu, with the current choice ticked.
fn build_piece_menu(menu: PopupMenu, selected: i64) -> PopupMenu {
    let mut menu = menu;
    for (bytes, label) in PIECE_SIZES {
        menu = menu.menu_with_check_and_disabled(
            label,
            bytes == selected,
            Box::new(actions::SetPieceLength(bytes)),
            false,
        );
    }
    menu
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_piece_ladder_matches_the_tauri_dialog() {
        assert_eq!(PIECE_SIZES.len(), 13);
        assert_eq!(PIECE_SIZES[0], (0, "Auto (optimal)"));
        assert_eq!(PIECE_SIZES[12], (33_554_432, "32 MiB"));
        // Every value is a power of two and the labels ascend with them.
        for window in PIECE_SIZES.windows(2) {
            let (smaller, _) = window[0];
            let (larger, _) = window[1];
            assert!(smaller < larger, "{smaller} < {larger}");
        }
    }

    #[test]
    fn a_piece_length_reads_back_as_its_label() {
        assert_eq!(piece_label(0), "Auto (optimal)");
        assert_eq!(piece_label(262_144), "256 KiB");
        // A value the ladder does not carry falls back to the default.
        assert_eq!(piece_label(1234), "Auto (optimal)");
    }

    #[test]
    fn trackers_come_one_per_line_and_blanks_fall_away() {
        let text = "http://a/announce\n\n  udp://b:1337/announce  \n\n";
        assert_eq!(
            trackers_from(text),
            ["http://a/announce", "udp://b:1337/announce"]
        );
        assert!(trackers_from("\n \n").is_empty());
        assert!(trackers_from("").is_empty());
    }

    #[test]
    fn the_output_is_suggested_from_the_source_name() {
        assert_eq!(
            suggested_output("/downloads/iso/debian.iso", "/torrents"),
            "/torrents/debian.iso.torrent"
        );
        // Trailing separators are part of the path, not the name.
        assert_eq!(
            suggested_output("/downloads/iso/", "/torrents"),
            "/torrents/iso.torrent"
        );
        // With no configured save path it goes beside the source.
        assert_eq!(
            suggested_output("/downloads/debian.iso", ""),
            "/downloads/debian.iso.torrent"
        );
    }

    #[test]
    fn a_path_with_no_name_still_suggests_something() {
        assert_eq!(source_name("/"), "new");
        assert_eq!(source_name(""), "new");
        assert_eq!(source_name("/downloads/"), "downloads");
    }

    /// A scratch directory with a source file in it.
    fn scratch(label: &str) -> (PathBuf, PathBuf) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "rstorrent-create-{label}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let source = dir.join("payload.bin");
        std::fs::write(&source, vec![7_u8; 4096]).expect("a source file");
        (dir, source)
    }

    fn options_for(source: &Path) -> CreateTorrentOptions {
        CreateTorrentOptions {
            source_path: source.to_path_buf(),
            piece_length: None,
            trackers: vec!["http://tracker.example/announce".to_owned()],
            is_private: true,
            comment: Some("a test".to_owned()),
            source: None,
            created_by: Some("test".to_owned()),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn creating_writes_the_file_and_hands_it_to_the_daemon() {
        let (dir, source) = scratch("seed");
        let output = dir.join("payload.torrent");
        let services = Arc::new(Services::mock().expect("runtime"));
        let before = services.list_snapshot().await.expect("listing").len();

        let created = create(
            options_for(&source),
            output.to_string_lossy().into_owned(),
            true,
            Arc::clone(&services),
        )
        .await
        .expect("creates the torrent");

        assert_eq!(created.name, "payload.bin");
        assert_eq!(created.total_size, 4096);
        assert!(created.piece_count >= 1);
        assert!(!created.info_hash.is_empty());
        // The metainfo is on disk where it was asked for…
        let written = std::fs::read(&output).expect("the .torrent was written");
        assert_eq!(
            &written[0..11],
            b"d8:announce",
            "a bencoded torrent starts with its announce list"
        );
        // …and the daemon was handed it, which the mock records.
        assert_eq!(
            services.list_snapshot().await.expect("listing").len(),
            before + 1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn without_seeding_nothing_reaches_the_daemon() {
        let (dir, source) = scratch("noseed");
        let services = Arc::new(Services::mock().expect("runtime"));
        let before = services.list_snapshot().await.expect("listing").len();

        // No output path either: the bytes are produced and dropped.
        create(
            options_for(&source),
            String::new(),
            false,
            Arc::clone(&services),
        )
        .await
        .expect("creates the torrent");

        assert_eq!(
            services.list_snapshot().await.expect("listing").len(),
            before,
            "the daemon was left alone"
        );
        assert!(!dir.join("payload.torrent").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_source_that_is_not_there_is_an_error_not_a_panic() {
        let (dir, _) = scratch("missing");
        let services = Arc::new(Services::mock().expect("runtime"));
        let missing = dir.join("gone.bin");

        let outcome = create(
            options_for(&missing),
            String::new(),
            false,
            Arc::clone(&services),
        )
        .await;
        assert!(outcome.is_err(), "a missing source is reported");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
