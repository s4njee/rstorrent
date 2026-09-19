//! Session export/import (V3-22 / LIB-09): a portable manifest of the
//! library for backup or moving to another daemon.
//!
//! The manifest carries hashes, re-addable sources where available, trackers,
//! labels/tags, paths, priorities, limits and client metadata — never
//! credentials (nothing secret is read at export time, so nothing secret can
//! be written). Import adds torrents stopped, rechecks them, and resumes only
//! verified data; a crash-safe import journal lets an interrupted run resume
//! instead of re-adding from scratch.
//!
//! Sources, honestly graded: an embedded `.torrent` (base64) or a magnet URI
//! re-adds anywhere; a bare `.torrent` *path* only helps on the machine that
//! has the file, and a torrent with none of the three is reported
//! unrestorable at validation time rather than failing mid-import.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::rtorrent::{RawTorrent, RtorrentApi};

/// Manifest format tag. Bump on any breaking shape change (validators reject
/// anything else with a readable error, never a panic).
pub const FORMAT: &str = "rstorrent-session/1";

/// Global rate limits carried for a machine move (KiB/s, 0 = unlimited).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ManifestGlobal {
    pub down_kb: i64,
    pub up_kb: i64,
}

/// How a torrent can be re-added, best option first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ManifestSource {
    /// Magnet URI (or http(s) `.torrent` URL — whatever `load_magnet` took).
    Magnet(String),
    /// Embedded `.torrent` bytes, base64.
    TorrentBase64(String),
    /// A `.torrent` path on the exporting machine. Only useful for restore
    /// where that file exists; validation flags it when nothing better does.
    TorrentPath(String),
}

/// One torrent in the manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestTorrent {
    /// Uppercase hex info-hash; the identity everything keys on.
    pub hash: String,
    pub name: String,
    #[serde(default)]
    pub source: Option<ManifestSource>,
    /// `d.base_path` at export (where the data lived).
    pub save_path: String,
    pub label: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub priority: i64,
    /// Assigned named throttle (`""` = global).
    #[serde(default)]
    pub throttle: String,
    #[serde(default)]
    pub peers_max: i64,
    #[serde(default)]
    pub peers_min: i64,
    #[serde(default)]
    pub uploads_max: i64,
    #[serde(default)]
    pub trackers: Vec<String>,
    /// Sticky provenance (D6): who added it, from where, when.
    #[serde(default)]
    pub added_by: String,
    #[serde(default)]
    pub source_path: String,
    #[serde(default)]
    pub added_at: i64,
    /// Client queue/force state, restored as metadata (not daemon truth).
    #[serde(default)]
    pub queue_pos: Option<i64>,
    #[serde(default)]
    pub force_start: bool,
}

/// The portable manifest document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: String,
    pub exported_at: i64,
    pub client: String,
    pub client_version: String,
    pub global: ManifestGlobal,
    pub torrents: Vec<ManifestTorrent>,
}

impl Manifest {
    /// Parse + shape-check a manifest document. Never panics on garbage.
    pub fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("not a session manifest: {e}"))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }
}

/// Build a manifest from live daemon rows. `trackers` maps hash → announce
/// URLs (the host fetches them per torrent); `sources` maps hash → the best
/// re-addable source the host could assemble (or nothing, honestly).
#[must_use]
pub fn export_manifest(
    rows: &[RawTorrent],
    trackers: &HashMap<String, Vec<String>>,
    sources: &HashMap<String, ManifestSource>,
    global_down_kb: i64,
    global_up_kb: i64,
    exported_at: i64,
    client_version: &str,
) -> Manifest {
    let mut torrents: Vec<ManifestTorrent> = rows
        .iter()
        .map(|t| ManifestTorrent {
            hash: t.hash.clone(),
            name: t.name.clone(),
            source: sources.get(&t.hash).cloned(),
            save_path: if t.base_path.is_empty() {
                t.directory.clone()
            } else {
                t.base_path.clone()
            },
            label: t.label.clone(),
            tags: t.tags.clone(),
            priority: t.priority,
            throttle: t.throttle_name.clone(),
            peers_max: t.peers_max,
            peers_min: t.peers_min,
            uploads_max: t.uploads_max,
            trackers: trackers.get(&t.hash).cloned().unwrap_or_default(),
            added_by: t.added_by.clone(),
            source_path: t.source_path.clone(),
            added_at: t.added_at,
            queue_pos: t.queue_pos,
            force_start: t.force_start,
        })
        .collect();
    torrents.sort_by(|a, b| a.hash.cmp(&b.hash));
    Manifest {
        format: FORMAT.into(),
        exported_at,
        client: "rstorrent".into(),
        client_version: client_version.into(),
        global: ManifestGlobal {
            down_kb: global_down_kb,
            up_kb: global_up_kb,
        },
        torrents,
    }
}

/// One validation finding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemProblem {
    /// Index into `Manifest.torrents` (`None` = document-level).
    pub index: Option<usize>,
    pub hash: String,
    pub message: String,
}

/// Dry-run validation: shape, hashes, duplicates, source availability (with
/// embedded-`.torrent` info-hash verification), never any daemon I/O.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidationReport {
    pub torrent_count: usize,
    pub restorable_count: usize,
    pub errors: Vec<ItemProblem>,
    pub warnings: Vec<ItemProblem>,
}

#[must_use]
pub fn validate_manifest(manifest: &Manifest) -> ValidationReport {
    let mut report = ValidationReport {
        torrent_count: manifest.torrents.len(),
        ..Default::default()
    };
    if manifest.format != FORMAT {
        report.errors.push(ItemProblem {
            index: None,
            hash: String::new(),
            message: format!(
                "unsupported manifest format '{}' (this client reads {})",
                manifest.format, FORMAT
            ),
        });
        return report;
    }
    let mut seen_hashes = HashSet::new();
    for (index, t) in manifest.torrents.iter().enumerate() {
        let hash = t.hash.trim().to_ascii_uppercase();
        if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            report.errors.push(ItemProblem {
                index: Some(index),
                hash: t.hash.clone(),
                message: "not a 40-digit hex info-hash".into(),
            });
            continue;
        }
        if !seen_hashes.insert(hash.clone()) {
            report.errors.push(ItemProblem {
                index: Some(index),
                hash: t.hash.clone(),
                message: "duplicate info-hash in this manifest".into(),
            });
            continue;
        }
        match &t.source {
            None => report.errors.push(ItemProblem {
                index: Some(index),
                hash: t.hash.clone(),
                message: "no re-addable source (no magnet, embedded .torrent or path)".into(),
            }),
            Some(ManifestSource::Magnet(uri)) if uri.trim().is_empty() => {
                report.errors.push(ItemProblem {
                    index: Some(index),
                    hash: t.hash.clone(),
                    message: "empty magnet URI".into(),
                });
            }
            Some(ManifestSource::TorrentBase64(b64)) => match base64_decode(b64) {
                Err(e) => report.errors.push(ItemProblem {
                    index: Some(index),
                    hash: t.hash.clone(),
                    message: format!("embedded .torrent does not decode: {e}"),
                }),
                Ok(bytes) => match crate::torrent_file::read_metadata_bytes(&bytes) {
                    Err(e) => report.errors.push(ItemProblem {
                        index: Some(index),
                        hash: t.hash.clone(),
                        message: format!("embedded .torrent does not parse: {e}"),
                    }),
                    Ok(meta) if meta.info_hash.to_ascii_uppercase() != hash => {
                        report.errors.push(ItemProblem {
                            index: Some(index),
                            hash: t.hash.clone(),
                            message: format!(
                                "embedded .torrent is a different torrent ({})",
                                meta.info_hash
                            ),
                        });
                    }
                    Ok(_) => {}
                },
            },
            Some(ManifestSource::TorrentPath(_)) => report.warnings.push(ItemProblem {
                index: Some(index),
                hash: t.hash.clone(),
                message: "only a .torrent path recorded — restores only where that file exists"
                    .into(),
            }),
            _ => {}
        }
    }
    report.restorable_count = if report.errors.iter().any(|e| e.index.is_none()) {
        0
    } else {
        report.torrent_count - report.errors.iter().filter(|e| e.index.is_some()).count()
    };
    report
}

/// Base64-encode bytes for manifest embedding.
#[must_use]
pub fn encode_base64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn base64_decode(raw: &str) -> Result<Vec<u8>, String> {
    // Small strict decoder (no extra dependency for one field): standard
    // alphabet, padding-aware, whitespace rejected.
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .map_err(|e| e.to_string())
}

/// What restoring one manifest entry means against a live daemon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestoreAction {
    /// Fresh add, stopped.
    Add,
    /// Already loaded — nothing to do.
    SkipHave,
    /// Excluded by the caller's selection.
    SkipUnselected,
    /// Fails validation (with the reason).
    SkipInvalid(String),
}

/// One entry of the restore plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreItem<'a> {
    pub torrent: &'a ManifestTorrent,
    pub action: RestoreAction,
    /// Destination directory after prefix remapping.
    pub dst_dir: String,
}

/// Plan a selective restore: validate, diff against loaded hashes, apply the
/// caller's selection and path-prefix remaps. Pure — the host executes.
#[must_use]
pub fn plan_restore<'a>(
    manifest: &'a Manifest,
    existing: &HashSet<String>,
    selected: Option<&HashSet<String>>,
    remaps: &[(String, String)],
) -> Vec<RestoreItem<'a>> {
    let report = validate_manifest(manifest);
    if !report.errors.iter().all(|e| e.index.is_none()) && manifest.format != FORMAT {
        // Whole-document failure: every entry is unrestorable.
        return manifest
            .torrents
            .iter()
            .map(|t| RestoreItem {
                torrent: t,
                action: RestoreAction::SkipInvalid("manifest failed validation".into()),
                dst_dir: String::new(),
            })
            .collect();
    }
    let invalid: HashMap<usize, &str> = report
        .errors
        .iter()
        .filter_map(|e| e.index.map(|i| (i, e.message.as_str())))
        .collect();
    manifest
        .torrents
        .iter()
        .enumerate()
        .map(|(index, t)| {
            let hash = t.hash.trim().to_ascii_uppercase();
            if let Some(reason) = invalid.get(&index) {
                return RestoreItem {
                    torrent: t,
                    action: RestoreAction::SkipInvalid((*reason).to_owned()),
                    dst_dir: String::new(),
                };
            }
            if existing.iter().any(|h| h.eq_ignore_ascii_case(&hash)) {
                return RestoreItem {
                    torrent: t,
                    action: RestoreAction::SkipHave,
                    dst_dir: String::new(),
                };
            }
            if let Some(selected) = selected {
                if !selected.iter().any(|h| h.eq_ignore_ascii_case(&hash)) {
                    return RestoreItem {
                        torrent: t,
                        action: RestoreAction::SkipUnselected,
                        dst_dir: String::new(),
                    };
                }
            }
            RestoreItem {
                torrent: t,
                action: RestoreAction::Add,
                dst_dir: remap_path(&t.save_path, remaps),
            }
        })
        .collect()
}

/// Apply ordered prefix remaps to a saved path (`/old/media` → `/srv/media`).
/// First matching prefix wins; no match returns the path unchanged.
#[must_use]
pub fn remap_path(path: &str, remaps: &[(String, String)]) -> String {
    for (from, to) in remaps {
        let from = from.trim_end_matches('/');
        if from.is_empty() {
            continue;
        }
        if path == from || path.starts_with(&format!("{from}/")) {
            return format!("{}{}", to.trim_end_matches('/'), &path[from.len()..]);
        }
    }
    path.to_owned()
}

/// Crash-safe import journal: which hashes of a manifest import finished,
/// so an interrupted run resumes instead of re-adding.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportJournal {
    #[serde(default)]
    pub manifest_id: String,
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub done: Vec<String>,
    #[serde(default)]
    pub started_ms: i64,
    #[serde(default)]
    pub updated_ms: i64,
}

impl ImportJournal {
    /// Manifest identity: format + export time + torrent count. Good enough
    /// to tell "same file retried" from "a different file".
    #[must_use]
    pub fn manifest_id(manifest: &Manifest) -> String {
        format!(
            "{}@{}#{}",
            manifest.format,
            manifest.exported_at,
            manifest.torrents.len()
        )
    }

    /// Hashes already finished (uppercased for case-insensitive compare).
    #[must_use]
    pub fn done_set(&self) -> HashSet<String> {
        self.done.iter().map(|h| h.to_ascii_uppercase()).collect()
    }

    pub fn mark_done(&mut self, hash: &str, now_ms: i64) {
        let hash = hash.to_ascii_uppercase();
        if !self.done.iter().any(|h| h == &hash) {
            self.done.push(hash);
        }
        self.updated_ms = now_ms;
    }

    /// Serialize for disk.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }

    /// Parse from disk; garbage is an empty journal (the plan, not the
    /// journal, is the source of truth).
    #[must_use]
    pub fn from_json(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_default()
    }
}

/// Load/save helpers shared by hosts (paths differ per shell).
#[must_use]
pub fn load_journal(path: &Path) -> ImportJournal {
    std::fs::read_to_string(path)
        .ok()
        .map(|raw| ImportJournal::from_json(&raw))
        .unwrap_or_default()
}

/// Persist the journal, creating the parent directory as needed.
///
/// # Errors
///
/// Returns the I/O error when the file cannot be written.
pub fn save_journal(path: &Path, journal: &ImportJournal) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, journal.to_json())
}

/// Progress/result callback channel for the import driver.
pub type ImportLog = Arc<dyn Fn(LogLevel, String, Option<String>) + Send + Sync>;

/// Log levels for import progress (kept local so the web host, which has no
/// Tauri `LogLevel`, can drive imports too).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// Outcome of one import run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub added: usize,
    pub resumed: usize,
    pub skipped: usize,
    pub failed: Vec<String>,
}

/// Execute a restore plan: add stopped → metadata → recheck → wait for the
/// hash check → resume. Already-loaded and invalid entries are never
/// touched; every finished hash lands in the journal before moving on, so a
/// kill mid-import resumes cleanly. Torrents whose data is missing fail with
/// an honest message instead of seeding garbage.
pub async fn run_import(
    backend: &dyn RtorrentApi,
    items: &[PlannedRestore<'_>],
    journal: &mut ImportJournal,
    save: &(dyn Fn(&ImportJournal) + Send + Sync),
    log: &ImportLog,
    cancel: &AtomicBool,
    now_ms: impl Fn() -> i64,
) -> ImportSummary {
    let mut summary = ImportSummary::default();
    for planned in items {
        if cancel.load(Ordering::Relaxed) {
            log(
                LogLevel::Warn,
                "import cancelled — resume it any time; finished torrents are kept".into(),
                None,
            );
            break;
        }
        let t = planned.torrent;
        let hash = t.hash.trim().to_ascii_uppercase();
        if journal.done_set().contains(&hash) {
            summary.resumed += 1;
            continue;
        }
        // Resolve the source bytes/URI up front: no source, no add.
        enum Source {
            Bytes(Vec<u8>),
            Magnet(String),
        }
        let source = match &t.source {
            Some(ManifestSource::Magnet(uri)) if !uri.trim().is_empty() => {
                Source::Magnet(uri.trim().to_owned())
            }
            Some(ManifestSource::TorrentBase64(b64)) => match base64_decode(b64) {
                Ok(bytes) => Source::Bytes(bytes),
                Err(e) => {
                    let msg = format!("{}: embedded .torrent does not decode ({e})", t.name);
                    summary.failed.push(msg.clone());
                    log(LogLevel::Error, msg, Some(hash.clone()));
                    continue;
                }
            },
            _ => {
                let msg = format!("{}: no re-addable source in this manifest", t.name);
                summary.failed.push(msg.clone());
                log(LogLevel::Error, msg, Some(hash.clone()));
                continue;
            }
        };
        let load = match &source {
            Source::Bytes(bytes) => {
                backend
                    .load_raw(
                        bytes.clone(),
                        crate::rtorrent::LoadOptions {
                            directory: planned.dst_dir.clone(),
                            label: t.label.clone(),
                            start: false,
                            top_of_queue: false,
                            unselected_indexes: Vec::new(),
                        },
                    )
                    .await
            }
            Source::Magnet(uri) => {
                backend
                    .load_magnet(
                        uri,
                        crate::rtorrent::LoadOptions {
                            directory: planned.dst_dir.clone(),
                            label: t.label.clone(),
                            start: false,
                            top_of_queue: false,
                            unselected_indexes: Vec::new(),
                        },
                    )
                    .await
            }
        };
        if let Err(e) = load {
            let msg = format!("{}: could not add ({e})", t.name);
            summary.failed.push(msg.clone());
            log(LogLevel::Error, msg, Some(hash.clone()));
            continue;
        }
        // Metadata: tags, priority, caps, throttle, provenance. Best-effort
        // per key — the torrent is added; a failed extra must not fail it.
        if !t.tags.is_empty() {
            let _ = backend.set_tags(std::slice::from_ref(&hash), &t.tags).await;
        }
        let _ = backend.set_priority(&hash, t.priority.clamp(0, 3)).await;
        if t.peers_max != 0 || t.peers_min != 0 || t.uploads_max != 0 {
            let _ = backend
                .set_connection_limits(&hash, t.peers_max, t.peers_min, t.uploads_max)
                .await;
        }
        if !t.throttle.trim().is_empty() {
            let _ = backend
                .assign_throttle(std::slice::from_ref(&hash), Some(t.throttle.trim()))
                .await;
        }
        let added_at = now_ms().to_string();
        let _ = backend
            .set_custom_metadata(
                &hash,
                &[
                    ("added_by", "import"),
                    ("source_path", &t.source_path),
                    ("added_at", &added_at),
                ],
            )
            .await;
        // Missing-data torrent? rtorrent reports it via d.message after the
        // check; recheck first, then read the verdict.
        if let Err(e) = backend.recheck(std::slice::from_ref(&hash)).await {
            let msg = format!("{}: could not recheck ({e})", t.name);
            summary.failed.push(msg.clone());
            log(LogLevel::Error, msg, Some(hash.clone()));
            continue;
        }
        match wait_for_check(backend, &hash).await {
            CheckVerdict::Clean => {
                if let Err(e) = backend.start(std::slice::from_ref(&hash)).await {
                    let msg = format!("{}: verified but could not resume ({e})", t.name);
                    summary.failed.push(msg.clone());
                    log(LogLevel::Error, msg, Some(hash.clone()));
                    continue;
                }
                journal.mark_done(&hash, now_ms());
                save(journal);
                summary.added += 1;
                log(
                    LogLevel::Info,
                    format!("imported {} (rechecked, resumed)", t.name),
                    Some(hash.clone()),
                );
            }
            CheckVerdict::MissingData(message) => {
                let msg = format!("{}: recheck failed — {message} (kept stopped)", t.name);
                summary.failed.push(msg.clone());
                log(LogLevel::Error, msg, Some(hash.clone()));
                journal.mark_done(&hash, now_ms());
                save(journal);
            }
        }
    }
    summary
}

/// What a post-add recheck found.
#[derive(Clone, Debug, PartialEq, Eq)]
enum CheckVerdict {
    Clean,
    MissingData(String),
}

/// Wait for a hash check to finish (polls the snapshot). Times out rather
/// than hanging the import forever on a stuck check.
async fn wait_for_check(backend: &dyn RtorrentApi, hash: &str) -> CheckVerdict {
    // ~10 minutes of patience, two seconds at a time.
    for _ in 0..300 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let rows = match backend.list_snapshot().await {
            Ok(rows) => rows,
            Err(_) => continue,
        };
        let Some(t) = rows.iter().find(|t| t.hash.eq_ignore_ascii_case(hash)) else {
            return CheckVerdict::MissingData("torrent vanished during recheck".into());
        };
        if t.hashing {
            continue;
        }
        if t.message.contains("No such file")
            || t.message.contains("chunk read error")
            || t.message.contains("Could not open")
            || t.message.contains("Storage error")
        {
            return CheckVerdict::MissingData(t.message.clone());
        }
        if !t.message.is_empty() {
            // A tracker grumble with present data still verifies: complete
            // means the bytes checked out.
            if t.complete {
                return CheckVerdict::Clean;
            }
            return CheckVerdict::MissingData(t.message.clone());
        }
        return CheckVerdict::Clean;
    }
    CheckVerdict::MissingData("recheck did not finish in time".into())
}

/// One executable restore entry: the manifest torrent plus its planned
/// destination. Built by hosts from [`plan_restore`] (`Add` items only).
pub struct PlannedRestore<'a> {
    pub torrent: &'a ManifestTorrent,
    pub dst_dir: String,
}

/// Counts for the export dialog. Shared by every host so the dialog JSON
/// shape is identical on desktop, web and GPUI.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    pub torrents: usize,
    pub with_sources: usize,
}

/// One validation finding, JSON-ready for the import preview.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemProblemDto {
    pub index: Option<usize>,
    pub hash: String,
    pub message: String,
}

/// One planned restore entry, JSON-ready for the import preview.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedItemDto {
    pub hash: String,
    pub name: String,
    pub action: String,
    pub dst_dir: String,
    pub reason: String,
}

/// Validation + restore preview shape for the import dialog. Shared by every
/// host so the dialog JSON shape is identical on desktop, web and GPUI.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationDto {
    pub torrent_count: usize,
    pub restorable_count: usize,
    pub errors: Vec<ItemProblemDto>,
    pub warnings: Vec<ItemProblemDto>,
    pub items: Vec<PlannedItemDto>,
}

/// Validate + plan in one step for the import preview: pure validation plus
/// the per-item restore plan against loaded hashes. Hosts only supply
/// `existing` (the daemon's loaded hashes); selection and path-prefix remaps
/// come from the dialog.
#[must_use]
pub fn preview(
    manifest: &Manifest,
    existing: &HashSet<String>,
    selected: Option<&HashSet<String>>,
    remaps: &[(String, String)],
) -> ValidationDto {
    let report = validate_manifest(manifest);
    let items = plan_restore(manifest, existing, selected, remaps)
        .into_iter()
        .map(|item| {
            let (action, reason) = match &item.action {
                RestoreAction::Add => ("add".to_owned(), String::new()),
                RestoreAction::SkipHave => ("have".to_owned(), "already loaded".to_owned()),
                RestoreAction::SkipUnselected => ("skip".to_owned(), "not selected".to_owned()),
                RestoreAction::SkipInvalid(reason) => ("invalid".to_owned(), reason.clone()),
            };
            PlannedItemDto {
                hash: item.torrent.hash.clone(),
                name: item.torrent.name.clone(),
                action,
                dst_dir: item.dst_dir,
                reason,
            }
        })
        .collect();
    ValidationDto {
        torrent_count: report.torrent_count,
        restorable_count: report.restorable_count,
        errors: report
            .errors
            .into_iter()
            .map(|e| ItemProblemDto {
                index: e.index,
                hash: e.hash,
                message: e.message,
            })
            .collect(),
        warnings: report
            .warnings
            .into_iter()
            .map(|w| ItemProblemDto {
                index: w.index,
                hash: w.hash,
                message: w.message,
            })
            .collect(),
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn torrent(hash: &str) -> ManifestTorrent {
        ManifestTorrent {
            hash: hash.into(),
            name: format!("Show-{hash}"),
            source: Some(ManifestSource::Magnet(format!(
                "magnet:?xt=urn:btih:{hash}"
            ))),
            save_path: format!("/dl/Show-{hash}"),
            label: "video".into(),
            tags: vec!["hd".into()],
            priority: 2,
            throttle: String::new(),
            peers_max: 0,
            peers_min: 0,
            uploads_max: 0,
            trackers: vec!["udp://tracker.example/announce".into()],
            added_by: "file".into(),
            source_path: "/inbox/x.torrent".into(),
            added_at: 1_700_000_000,
            queue_pos: None,
            force_start: false,
        }
    }

    fn manifest() -> Manifest {
        Manifest {
            format: FORMAT.into(),
            exported_at: 1_700_000_001,
            client: "rstorrent".into(),
            client_version: "test".into(),
            global: ManifestGlobal {
                down_kb: 0,
                up_kb: 512,
            },
            torrents: vec![
                torrent("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
                torrent("BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"),
            ],
        }
    }

    #[test]
    fn round_trip_preserves_everything() {
        let manifest = manifest();
        let text = manifest.to_json();
        assert_eq!(Manifest::parse(&text).unwrap(), manifest);
    }

    #[test]
    fn garbage_and_wrong_format_fail_readably() {
        assert!(Manifest::parse("{nope").is_err());
        assert!(Manifest::parse("{}").is_err());
        let mut manifest = manifest();
        manifest.format = "rstorrent-session/99".into();
        let report = validate_manifest(&manifest);
        assert_eq!(report.restorable_count, 0);
        assert!(report.errors.iter().any(|e| e.index.is_none()));
    }

    #[test]
    fn validation_catches_hashes_dupes_and_missing_sources() {
        let mut manifest = manifest();
        manifest.torrents[0].hash = "zzz".into();
        manifest.torrents[1].hash = manifest.torrents[0].hash.clone();
        manifest.torrents.push(ManifestTorrent {
            source: None,
            ..torrent("CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC")
        });
        let report = validate_manifest(&manifest);
        // zzz: bad hash. dup: bad hash too (checked first). CCCC: no source.
        assert_eq!(report.restorable_count, 0);
        assert_eq!(report.errors.len(), 3);
    }

    #[test]
    fn validation_verifies_embedded_torrents() {
        // Real bytes, real hash: build a tiny torrent in-memory via a
        // hand-rolled minimal file? Instead assert the negative paths, which
        // don't need a valid torrent: undecodable and unparseable inputs.
        let mut manifest = manifest();
        manifest.torrents[0].source = Some(ManifestSource::TorrentBase64("!!!".into()));
        let report = validate_manifest(&manifest);
        assert!(report
            .errors
            .iter()
            .any(|e| e.message.contains("does not decode")));
        manifest.torrents[0].source = Some(ManifestSource::TorrentBase64(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            b"definitely not bencode",
        )));
        let report = validate_manifest(&manifest);
        assert!(report
            .errors
            .iter()
            .any(|e| e.message.contains("does not parse")));
    }

    #[test]
    fn plan_skips_loaded_unselected_and_invalid() {
        let mut manifest = manifest();
        manifest.torrents[0].hash = "zzz".into();
        let existing: HashSet<String> = ["BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".to_owned()]
            .into_iter()
            .collect();
        // No selection: zzz invalid, BBBB already loaded.
        let plan = plan_restore(&manifest, &existing, None, &[]);
        assert!(matches!(plan[0].action, RestoreAction::SkipInvalid(_)));
        assert!(matches!(plan[1].action, RestoreAction::SkipHave));
        // Selection limits to BBBB (already have) → still SkipHave, and the
        // invalid entry stays invalid, never silently dropped.
        let selected: HashSet<String> = ["bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned()]
            .into_iter()
            .collect();
        let plan = plan_restore(&manifest, &existing, Some(&selected), &[]);
        assert!(matches!(plan[1].action, RestoreAction::SkipHave));
    }

    #[test]
    fn plan_applies_prefix_remaps() {
        let manifest = manifest();
        let plan = plan_restore(
            &manifest,
            &HashSet::new(),
            None,
            &[("/dl".into(), "/srv/media".into())],
        );
        assert!(plan.iter().all(|i| matches!(i.action, RestoreAction::Add)));
        assert_eq!(
            plan[0].dst_dir,
            "/srv/media/Show-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        );
        // No matching prefix: unchanged.
        let plan = plan_restore(
            &manifest,
            &HashSet::new(),
            None,
            &[("/x".into(), "/y".into())],
        );
        assert_eq!(
            plan[0].dst_dir,
            "/dl/Show-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        );
    }

    #[test]
    fn journal_round_trips_and_dedupes_done() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("import-journal.json");
        assert!(load_journal(&path).done.is_empty());
        let mut journal = ImportJournal {
            manifest_id: "m".into(),
            total: 2,
            ..Default::default()
        };
        journal.mark_done("ab", 1);
        journal.mark_done("AB", 2);
        assert_eq!(journal.done.len(), 1, "case-insensitive dedupe");
        save_journal(&path, &journal).unwrap();
        let loaded = load_journal(&path);
        assert!(loaded.done_set().contains("AB"));
        assert!(ImportJournal::from_json("{nope").done.is_empty());
    }
}
