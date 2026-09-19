//! Session export/import dialog (V3-22 / LIB-09, LIB-10): the GPUI port of
//! `SessionDialog.tsx`.
//!
//! Export builds the portable manifest text (hashes, sources, trackers,
//! labels/tags, paths, priorities, limits, client metadata — never
//! credentials) and writes it to the user's picked path. Import reads a
//! manifest file — or discovers one from qBittorrent (`BT_backup`) or
//! Transmission (config dir) resume data — previews the restore plan with
//! per-item selection and path remapping, then runs the detached journaled
//! job (add stopped → recheck → resume verified) whose shared state this
//! dialog polls while open.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::input::InputState;
use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, App, Context, Div, Entity, EventEmitter, FocusHandle, KeyDownEvent, MouseButton,
    PathPromptOptions, Stateful, Subscription, Window,
};
use rtorrent_core::foreign::ScanReport;

use crate::actions::DismissOverlay;
use crate::dialogs::{
    backdrop, body, button, footer, form_field, header, panel, text_field, ButtonKind,
};
use crate::model::TorrentsModel;
use crate::services::{Services, SessionImportState};
use crate::theme::{self, Palette};

/// What the session dialog reports. `Imported` carries the finished summary
/// so the shell can log it like every other mutation.
#[derive(Clone, Debug)]
pub enum SessionEvent {
    Close,
    Imported {
        added: usize,
        skipped: usize,
        failed: Vec<String>,
    },
}

impl EventEmitter<SessionEvent> for SessionDialog {}

/// The import preview: the shared validation DTO plus the dialog's selection.
struct Preview {
    report: rtorrent_core::session::ValidationDto,
    selected: HashSet<String>,
}

pub struct SessionDialog {
    palette: Palette,
    focus: FocusHandle,
    services: Arc<Services>,
    model: Entity<TorrentsModel>,
    _subscriptions: Vec<Subscription>,
    // Export.
    export_msg: Option<String>,
    exporting: bool,
    // Import source.
    manifest_path: Entity<InputState>,
    manifest_text: Option<String>,
    scan_msg: Option<String>,
    scan_problems: Vec<(String, String)>,
    scanning: bool,
    // Preview + run.
    remap_from: Entity<InputState>,
    remap_to: Entity<InputState>,
    preview: Option<Preview>,
    validating: bool,
    error: Option<String>,
    job: SessionImportState,
    polling: bool,
}

impl SessionDialog {
    pub fn new(
        services: Arc<Services>,
        model: Entity<TorrentsModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let manifest_path = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("manifest .json path")
                .submit_on_enter(false)
        });
        let remap_from = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("/old/media")
                .submit_on_enter(false)
        });
        let remap_to = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("/srv/media")
                .submit_on_enter(false)
        });
        Self {
            palette: Palette::dark(),
            focus,
            services,
            model,
            _subscriptions: Vec::new(),
            export_msg: None,
            exporting: false,
            manifest_path,
            manifest_text: None,
            scan_msg: None,
            scan_problems: Vec::new(),
            scanning: false,
            remap_from,
            remap_to,
            preview: None,
            validating: false,
            error: None,
            job: SessionImportState::default(),
            polling: false,
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(SessionEvent::Close);
    }

    fn input_value(entity: &Entity<InputState>, cx: &App) -> String {
        entity.read(cx).value().to_string()
    }

    // --- export ---

    fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.exporting {
            return;
        }
        self.exporting = true;
        self.export_msg = None;
        cx.notify();
        let services = Arc::clone(&self.services);
        let picker =
            cx.prompt_for_new_path(&std::path::PathBuf::from("session-manifest.json"), None);
        cx.spawn_in(window, async move |this, cx| {
            let text = services.export_session_text().await;
            let text = match text {
                Ok(text) => text,
                Err(e) => {
                    this.update(cx, |this: &mut Self, cx| {
                        this.exporting = false;
                        this.export_msg = Some(format!("export failed: {e}"));
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            let count: usize = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("torrents")?.as_array().map(|a| a.len()))
                .unwrap_or(0);
            // The save path is the user's choice, asked on the UI thread.
            let path = match picker.await {
                Ok(Ok(Some(path))) => path,
                // Cancelled, or the platform has no picker: nothing to write.
                _ => {
                    this.update(cx, |this: &mut Self, cx| {
                        this.exporting = false;
                        this.export_msg = Some("export cancelled.".into());
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            let written = cx
                .background_executor()
                .spawn(async move { std::fs::write(&path, text).map_err(|e| e.to_string()) })
                .await;
            this.update(cx, |this: &mut Self, cx| {
                this.exporting = false;
                this.export_msg = Some(match written {
                    Ok(()) => format!("exported {count} torrent(s)."),
                    Err(e) => format!("could not write file: {e}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // --- import source ---

    fn reset_run(&mut self) {
        self.preview = None;
        self.error = None;
        self.job = self.services.session_import_state();
    }

    fn choose_manifest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a session manifest".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let path = match chosen.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(path) = path else { return };
            let text = cx
                .background_executor()
                .spawn(async move { std::fs::read_to_string(path).map_err(|e| e.to_string()) })
                .await;
            this.update(cx, |this: &mut Self, cx| {
                match text {
                    Ok(text) => {
                        this.reset_run();
                        this.manifest_text = Some(text);
                        this.scan_msg = None;
                        this.scan_problems.clear();
                    }
                    Err(e) => this.error = Some(format!("could not read file: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn scan(&mut self, client: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        self.scan_msg = None;
        cx.notify();
        let prompt = if client == "qbittorrent" {
            "Choose the BT_backup folder"
        } else {
            "Choose the Transmission config folder (or its resume/ dir)"
        };
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        let services = Arc::clone(&self.services);
        cx.spawn_in(window, async move |this, cx| {
            let dir = match chosen.await {
                Ok(Ok(Some(paths))) => paths
                    .into_iter()
                    .next()
                    .map(|p| p.to_string_lossy().into_owned()),
                _ => None,
            };
            let Some(dir) = dir else {
                this.update(cx, |this: &mut Self, cx| {
                    this.scanning = false;
                    cx.notify();
                })
                .ok();
                return;
            };
            let report: Result<ScanReport, _> = services.scan_foreign(client.into(), dir).await;
            this.update(cx, |this: &mut Self, cx| {
                this.scanning = false;
                match report {
                    Ok(report) => {
                        this.reset_run();
                        this.manifest_text = Some(report.manifest_text.clone());
                        this.scan_problems = report
                            .problems
                            .iter()
                            .take(8)
                            .map(|p| (p.file.clone(), p.message.clone()))
                            .collect();
                        this.scan_msg = Some(format!(
                            "found {} torrent(s), {} with sources{}",
                            report.entry_count,
                            report.restorable_count,
                            if report.problems.len() > 8 {
                                format!(" — {} notes (first 8 shown)", report.problems.len())
                            } else if !report.problems.is_empty() {
                                format!(" — {} note(s)", report.problems.len())
                            } else {
                                String::new()
                            }
                        ));
                    }
                    Err(e) => this.scan_msg = Some(format!("scan failed: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The manifest to validate/import: scanned/uploaded text wins over the
    /// typed path, which the host reads.
    fn source_text(&self, cx: &App) -> Option<String> {
        if let Some(text) = &self.manifest_text {
            return Some(text.clone());
        }
        let path = Self::input_value(&self.manifest_path, cx);
        if path.trim().is_empty() {
            return None;
        }
        std::fs::read_to_string(path.trim()).ok()
    }

    fn remaps(&self, cx: &App) -> Vec<(String, String)> {
        let from = Self::input_value(&self.remap_from, cx);
        if from.trim().is_empty() {
            Vec::new()
        } else {
            vec![(from, Self::input_value(&self.remap_to, cx))]
        }
    }

    // --- validate / import ---

    fn validate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.validating {
            return;
        }
        let Some(text) = self.source_text(cx) else {
            self.error = Some("choose a manifest file or scan another client first.".into());
            cx.notify();
            return;
        };
        self.validating = true;
        self.error = None;
        cx.notify();
        let services = Arc::clone(&self.services);
        let remaps = self.remaps(cx);
        cx.spawn_in(window, async move |this, cx| {
            let report = services.validate_session(text, None, remaps).await;
            this.update(cx, |this: &mut Self, cx| {
                this.validating = false;
                match report {
                    Ok(report) => {
                        let selected = report
                            .items
                            .iter()
                            .filter(|i| i.action == "add")
                            .map(|i| i.hash.clone())
                            .collect();
                        this.preview = Some(Preview { report, selected });
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn start_import(&mut self, window: &mut Window, cx: &mut Context<Self>, resume: bool) {
        let Some(preview) = &self.preview else { return };
        if preview.selected.is_empty() {
            self.error = Some("nothing selected — tick at least one entry.".into());
            cx.notify();
            return;
        }
        let Some(text) = self.source_text(cx) else {
            self.error = Some("the manifest is gone — choose it again.".into());
            cx.notify();
            return;
        };
        let journal = self.model.read(cx).import_journal_path();
        match self.services.start_session_import(
            journal,
            text,
            Some(preview.selected.clone()),
            self.remaps(cx),
            resume,
        ) {
            Err(e) => {
                // A running job is rejoined, not forked: pick up its state.
                self.error = Some(e);
                self.job = self.services.session_import_state();
                self.poll(window, cx);
            }
            Ok(()) => {
                self.error = None;
                self.poll(window, cx);
            }
        }
        cx.notify();
    }

    /// Poll the shared job state until the run ends, then report it.
    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.polling {
            return;
        }
        self.polling = true;
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let done = this
                    .update(cx, |this: &mut Self, cx| {
                        this.job = this.services.session_import_state();
                        cx.notify();
                        !this.job.running
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
            }
            this.update(cx, |this: &mut Self, cx| {
                this.polling = false;
                let job = this.job.clone();
                if job.done {
                    cx.emit(SessionEvent::Imported {
                        added: job.added,
                        skipped: job.skipped,
                        failed: job.failed.clone(),
                    });
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn toggle(&mut self, hash: &str, cx: &mut Context<Self>) {
        if let Some(preview) = &mut self.preview {
            if preview.selected.contains(hash) {
                preview.selected.remove(hash);
            } else {
                preview.selected.insert(hash.to_owned());
            }
            cx.notify();
        }
    }
}

impl Render for SessionDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette;
        let dim = theme::color(palette.text_dim);
        let muted = theme::color(palette.text_muted);
        let body_text = theme::color(palette.text_body);

        let addable = self
            .preview
            .as_ref()
            .map(|p| p.report.items.iter().filter(|i| i.action == "add").count())
            .unwrap_or(0);
        let running = self.job.running;

        // The preview list, built before the panel so `cx` listeners borrow
        // nothing while rendering.
        let mut rows = div().flex().flex_col().gap(px(4.));
        if let Some(preview) = &self.preview {
            for item in &preview.report.items {
                let hash = item.hash.clone();
                let selected = preview.selected.contains(&item.hash);
                let mut detail = match item.action.as_str() {
                    "add" => "will add".to_owned(),
                    "have" => "already here".to_owned(),
                    "skip" => "skipped — not selected".to_owned(),
                    _ => format!("cannot restore — {}", item.reason),
                };
                if !item.dst_dir.is_empty() {
                    detail.push_str(&format!(" → {}", item.dst_dir));
                }
                if item.action == "add" {
                    rows = rows.child(plan_checkbox(
                        hash.clone(),
                        item.name.clone(),
                        selected,
                        !running,
                        detail,
                        palette,
                        cx.listener(move |this, _, _, cx| this.toggle(&hash, cx)),
                    ));
                } else {
                    rows = rows.child(
                        div()
                            .flex()
                            .items_start()
                            .gap(px(8.))
                            .child(theme::text("·".to_owned(), 11., dim))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        theme::text(item.name.clone(), 11., body_text)
                                            .min_w_0()
                                            .truncate(),
                                    )
                                    .child(theme::text(detail, 10.5, muted).min_w_0().truncate()),
                            ),
                    );
                }
            }
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
                panel("session-dialog", 620., palette)
                    .child(header(
                        "Session export / import",
                        cx.listener(|this, _, _, cx| this.close(cx)),
                        palette,
                    ))
                    .child(
                        body()
                            .child(theme::text("Export".to_owned(), 11., body_text))
                            .child(theme::text(
                                "A portable manifest — hashes, sources, trackers, labels/tags, paths, priorities, limits, client metadata. Never credentials.",
                                10.5,
                                muted,
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        button(
                                            "session-export",
                                            if self.exporting { "Exporting…" } else { "Export to file…" },
                                            ButtonKind::Primary,
                                            !self.exporting,
                                            palette,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.export(window, cx);
                                        })),
                                    )
                                    .when_some(self.export_msg.clone(), |row, msg| {
                                        row.child(theme::text(msg, 10.5, muted))
                                    }),
                            )
                            .child(theme::text("Import".to_owned(), 11., body_text))
                            .child(theme::text(
                                "Adds stopped, rechecks, and resumes only verified data. A crash-safe journal resumes interrupted runs.",
                                10.5,
                                muted,
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        button(
                                            "session-pick",
                                            "Choose manifest…",
                                            ButtonKind::Secondary,
                                            true,
                                            palette,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.choose_manifest(window, cx);
                                        })),
                                    )
                                    .child(text_field(&self.manifest_path, palette))
                                    .when_some(
                                        self.manifest_text.as_ref().map(|t| t.len()),
                                        |row, len| {
                                            row.child(theme::text(
                                                format!("manifest text loaded ({} bytes).", len),
                                                10.5,
                                                muted,
                                            ))
                                        },
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(theme::text("…or discover:".to_owned(), 10.5, muted))
                                    .child(
                                        button(
                                            "session-scan-qb",
                                            "Scan qBittorrent…",
                                            ButtonKind::Secondary,
                                            !self.scanning,
                                            palette,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.scan("qbittorrent", window, cx);
                                        })),
                                    )
                                    .child(
                                        button(
                                            "session-scan-tr",
                                            "Scan Transmission…",
                                            ButtonKind::Secondary,
                                            !self.scanning,
                                            palette,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.scan("transmission", window, cx);
                                        })),
                                    ),
                            )
                            .when_some(self.scan_msg.clone(), |column, msg| {
                                column.child(theme::text(msg, 10.5, muted))
                            })
                            .child({
                                let mut problems = div().flex().flex_col().gap(px(2.));
                                for (file, message) in &self.scan_problems {
                                    let short = file.rsplit(['/', '\\']).next().unwrap_or(file);
                                    problems = problems.child(theme::text(
                                        format!("{short}: {message}"),
                                        10.5,
                                        muted,
                                    ));
                                }
                                problems
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(form_field(
                                        "Move paths from",
                                        text_field(&self.remap_from, palette),
                                        palette,
                                    ))
                                    .child(form_field(
                                        "to",
                                        text_field(&self.remap_to, palette),
                                        palette,
                                    ))
                                    .child(
                                        button(
                                            "session-preview",
                                            if self.validating { "Checking…" } else { "Preview" },
                                            ButtonKind::Secondary,
                                            !self.validating,
                                            palette,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.validate(window, cx);
                                        })),
                                    ),
                            )
                            .when_some(self.error.clone(), |column, message| {
                                column.child(theme::text(message, 11., theme::color(palette.accent_red)))
                            })
                            .when_some(
                                self.preview.as_ref().map(|p| {
                                    (
                                        p.report.torrent_count,
                                        p.report.restorable_count,
                                        p.report.errors.len(),
                                        p.report.warnings.len(),
                                    )
                                }),
                                |column, (total, restorable, errors, warnings)| {
                                    column.child(theme::text(
                                        format!(
                                            "{total} in manifest · {restorable} restorable · {addable} to add · {errors} error(s) · {warnings} warning(s)"
                                        ),
                                        10.5,
                                        muted,
                                    ))
                                },
                            )
                            .child(rows)
                            .when(self.job.done || self.job.running, |column| {
                                let job = self.job.clone();
                                column.child(theme::text(
                                    if job.running {
                                        format!(
                                            "importing{}… {} added, {} skipped, {} failed",
                                            if job.current.is_empty() {
                                                String::new()
                                            } else {
                                                format!(" {}", &job.current[..8.min(job.current.len())])
                                            },
                                            job.added,
                                            job.skipped,
                                            job.failed.len()
                                        )
                                    } else {
                                        format!(
                                            "finished: {} added, {} resumed, {} skipped, {} failed",
                                            job.added,
                                            job.resumed,
                                            job.skipped,
                                            job.failed.len()
                                        )
                                    },
                                    10.5,
                                    muted,
                                ))
                            })
                            .child({
                                let mut log = div().flex().flex_col().gap(px(2.));
                                for line in self.job.log.iter().rev().take(4) {
                                    log = log.child(theme::text(line.clone(), 10.5, dim));
                                }
                                log
                            }),
                    )
                    .child(
                        footer(palette)
                            .child(
                                button(
                                    "session-close",
                                    "Close",
                                    ButtonKind::Secondary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                            )
                            .child(
                                button(
                                    "session-resume",
                                    "Resume previous",
                                    ButtonKind::Secondary,
                                    !running && addable > 0,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.start_import(window, cx, true);
                                })),
                            )
                            .child(
                                button(
                                    "session-import",
                                    if running {
                                        "Importing…"
                                    } else {
                                        "Import stopped"
                                    },
                                    ButtonKind::Primary,
                                    !running && addable > 0,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.start_import(window, cx, false);
                                })),
                            )
                            .when(running, |row| {
                                row.child(
                                    button(
                                        "session-cancel",
                                        "Cancel",
                                        ButtonKind::Danger,
                                        true,
                                        palette,
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.services.cancel_session_import();
                                        cx.notify();
                                    })),
                                )
                            }),
                    ),
            )
    }
}

/// A plan row with a dynamic element id (the shared `checkbox` helper takes a
/// `&'static str`, which a per-torrent list cannot mint without leaking one
/// id per render). Visuals match the shared helper.
fn plan_checkbox(
    hash: String,
    name: String,
    checked: bool,
    enabled: bool,
    detail: String,
    palette: Palette,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
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
        .id(gpui_kit::SharedString::from(format!("session-{hash}")))
        .flex()
        .items_start()
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
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    theme::text(name, 11., theme::color(text))
                        .min_w_0()
                        .truncate(),
                )
                .child(
                    theme::text(detail, 10.5, theme::color(palette.text_muted))
                        .min_w_0()
                        .truncate(),
                ),
        );
    if enabled {
        element = element.cursor_pointer().on_click(on_click);
    }
    element
}
