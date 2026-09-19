//! Import discovery for other clients (V3-22 / LIB-10): qBittorrent and
//! Transmission.
//!
//! Discovery reads the other client's resume data into a read-only plan —
//! hashes, `.torrent` sources, save paths, labels/categories/tags,
//! priorities — and converts it to a [`crate::session::Manifest`] so the
//! normal validate → preview → journaled-import path runs unchanged. Nothing
//! here touches the other client (read-only) or the daemon (no I/O at all).
//!
//! Key formats, verified against upstream source rather than memory:
//! - qBittorrent `BT_backup/<hash>.fastresume` + sibling `<hash>.torrent`
//!   (`bencoderesumedatastorage.cpp`): `qBt-name`, `qBt-savePath`,
//!   `qBt-downloadPath`, `qBt-category`, `qBt-tags` (list), `qBt-seedStatus`,
//!   `qBt-stopCondition`; libtorrent keys `save_path`, `name`. The file stem
//!   is the v1 info-hash in hex; name falls back `qBt-name` → `name` → the
//!   sibling `.torrent`'s `info.name` → the stem (the same chain qbt_migrate
//!   documents).
//! - Transmission `<config>/resume/<hash>.resume` + `torrents/<hash>.torrent`
//!   (`resume.cc`, `quark.cc`): kebab-case keys `destination`,
//!   `incomplete-dir`, `paused`, `labels` (list), `name`, `added-date`,
//!   `bandwidth-priority` (-1/0/1), `group`, `max-peers`. Queue position is
//!   deliberately *not* read — Transmission does not persist it in the
//!   resume file (upstream issue #1460), so claiming it would be fiction.
//!
//! Unknown or missing keys are absence, never an error: a newer client that
//! adds keys still scans. Anything that cannot be turned into an honest
//! manifest entry (bad hash, hash mismatch, no re-addable source) lands in
//! `problems` with a readable message instead of failing the whole scan.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::session::{encode_base64, Manifest, ManifestSource, ManifestTorrent};

// ---------------------------------------------------------------------------
// Minimal bencode reader (no new dependency for two scanners).
// ---------------------------------------------------------------------------

/// A bencoded value borrowing the input.
#[derive(Clone, Debug, PartialEq, Eq)]
enum BVal<'a> {
    Int(i64),
    Bytes(&'a [u8]),
    List(Vec<BVal<'a>>),
    Dict(Vec<(&'a [u8], BVal<'a>)>),
}

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a [u8]) -> Self {
        Parser { input, pos: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn take(&mut self) -> Result<u8, String> {
        let b = self
            .peek()
            .ok_or_else(|| "unexpected end of bencoded data".to_owned())?;
        self.pos += 1;
        Ok(b)
    }

    fn parse(&mut self) -> Result<BVal<'a>, String> {
        match self.take()? {
            b'i' => self.parse_int(),
            b'l' => {
                let mut items = Vec::new();
                while self.peek() != Some(b'e') {
                    if self.peek().is_none() {
                        return Err("unterminated list".into());
                    }
                    items.push(self.parse()?);
                }
                self.pos += 1; // 'e'
                Ok(BVal::List(items))
            }
            b'd' => {
                let mut pairs = Vec::new();
                while self.peek() != Some(b'e') {
                    if self.peek().is_none() {
                        return Err("unterminated dict".into());
                    }
                    let key = match self.parse()? {
                        BVal::Bytes(k) => k,
                        _ => return Err("non-string dict key".into()),
                    };
                    let value = self.parse()?;
                    pairs.push((key, value));
                }
                self.pos += 1; // 'e'
                Ok(BVal::Dict(pairs))
            }
            b'0'..=b'9' => {
                self.pos -= 1;
                self.parse_bytes()
            }
            other => Err(format!(
                "invalid bencode prefix '{other}' at offset {}",
                self.pos - 1
            )),
        }
    }

    fn parse_int(&mut self) -> Result<BVal<'a>, String> {
        let start = self.pos;
        while self.peek() != Some(b'e') {
            match self.take()? {
                b'-' | b'0'..=b'9' => {}
                _ => return Err("invalid integer".into()),
            }
        }
        self.pos += 1; // 'e'
        let raw = std::str::from_utf8(&self.input[start..self.pos - 1])
            .map_err(|_| "non-UTF8 integer".to_owned())?;
        raw.parse::<i64>()
            .map(BVal::Int)
            .map_err(|_| format!("integer out of range: {raw}"))
    }

    fn parse_bytes(&mut self) -> Result<BVal<'a>, String> {
        let start = self.pos;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.take()? != b':' {
            return Err("malformed string length".into());
        }
        let len: usize = std::str::from_utf8(&self.input[start..self.pos - 1])
            .map_err(|_| "non-UTF8 string length".to_owned())?
            .parse()
            .map_err(|_| "bad string length".to_owned())?;
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| "string length overflow".to_owned())?;
        if end > self.input.len() {
            return Err("string overruns input".into());
        }
        let bytes = &self.input[self.pos..end];
        self.pos = end;
        Ok(BVal::Bytes(bytes))
    }
}

/// Parse bencoded data; trailing bytes after the first value are rejected so
/// a half-written resume file never scans as valid.
fn parse_bencode(input: &[u8]) -> Result<BVal<'_>, String> {
    let mut parser = Parser::new(input);
    let value = parser.parse()?;
    if parser.pos != input.len() {
        return Err(format!(
            "trailing bytes after bencoded value ({} of {})",
            parser.pos,
            input.len()
        ));
    }
    Ok(value)
}

/// Look up a key in a parsed dict.
fn lookup<'a, 'b>(pairs: &'b [(&'a [u8], BVal<'a>)], key: &str) -> Option<&'b BVal<'a>> {
    pairs
        .iter()
        .find(|(k, _)| *k == key.as_bytes())
        .map(|(_, v)| v)
}

/// Best-effort UTF-8 for paths and names (foreign clients are lossy here too).
fn bstr(value: &BVal<'_>) -> Option<String> {
    match value {
        BVal::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
        _ => None,
    }
}

fn bint(value: &BVal<'_>) -> Option<i64> {
    match value {
        BVal::Int(i) => Some(*i),
        _ => None,
    }
}

fn blist_strs(value: &BVal<'_>) -> Vec<String> {
    match value {
        BVal::List(items) => items
            .iter()
            .filter_map(|item| match item {
                BVal::Bytes(b) => {
                    let s = String::from_utf8_lossy(b).into_owned();
                    if s.trim().is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Scan report (JSON-ready for the dialog).
// ---------------------------------------------------------------------------

/// Which client a directory was scanned as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForeignClient {
    QBittorrent,
    Transmission,
}

impl std::str::FromStr for ForeignClient {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "qbittorrent" | "qb" | "qbit" => Ok(ForeignClient::QBittorrent),
            "transmission" | "tr" => Ok(ForeignClient::Transmission),
            _ => Err(format!(
                "unknown client '{raw}' — expected \"qbittorrent\" or \"transmission\""
            )),
        }
    }
}

impl ForeignClient {
    #[must_use]
    pub fn id(&self) -> &'static str {
        match self {
            ForeignClient::QBittorrent => "qbittorrent",
            ForeignClient::Transmission => "transmission",
        }
    }

    /// Provenance stamp written into each manifest entry's `added_by`.
    #[must_use]
    pub fn added_by(&self) -> &'static str {
        match self {
            ForeignClient::QBittorrent => "qbittorrent-import",
            ForeignClient::Transmission => "transmission-import",
        }
    }
}

/// One unreadable-or-unrestorable entry. The scan continues past it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProblem {
    pub file: String,
    pub message: String,
}

/// Per-entry preview metadata for the dialog (the full entries travel inside
/// `manifest_text` and are previewed through the normal validate path).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanEntryMeta {
    pub hash: String,
    pub name: String,
    pub has_source: bool,
    /// Whether the torrent looked stopped/paused over there (display only —
    /// imports always add stopped and resume after recheck).
    pub stopped: bool,
}

/// The read-only discovery result. `manifest_text` is a session manifest in
/// the LIB-09 shape, ready for validate → preview → journaled import.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub client: String,
    pub manifest_text: String,
    pub entry_count: usize,
    pub restorable_count: usize,
    pub entries: Vec<ScanEntryMeta>,
    pub problems: Vec<ScanProblem>,
}

/// Scan a client directory into a manifest. `dir` is the `BT_backup` folder
/// for qBittorrent, or the config dir (or its `resume/` subdir) for
/// Transmission. `now` seeds `exported_at` (hosts pass wall-clock time).
#[must_use]
pub fn scan(client: ForeignClient, dir: &Path, now: i64) -> ScanReport {
    let mut problems = Vec::new();
    let mut torrents = Vec::new();
    let mut metas = Vec::new();
    let mut seen = HashSet::new();
    match client {
        ForeignClient::QBittorrent => {
            scan_qb(dir, &mut torrents, &mut metas, &mut problems, &mut seen)
        }
        ForeignClient::Transmission => {
            scan_transmission(dir, &mut torrents, &mut metas, &mut problems, &mut seen);
        }
    }
    torrents.sort_by(|a: &ManifestTorrent, b: &ManifestTorrent| a.hash.cmp(&b.hash));
    metas.sort_by(|a, b| a.hash.cmp(&b.hash));
    let restorable_count = torrents.iter().filter(|t| t.source.is_some()).count();
    let manifest = Manifest {
        format: crate::session::FORMAT.into(),
        exported_at: now,
        client: format!("{}-import", client.id()),
        client_version: String::new(),
        global: Default::default(),
        torrents,
    };
    ScanReport {
        client: client.id().into(),
        manifest_text: manifest.to_json(),
        entry_count: manifest_entry_count(&manifest),
        restorable_count,
        entries: metas,
        problems,
    }
}

fn manifest_entry_count(manifest: &Manifest) -> usize {
    manifest.torrents.len()
}

/// One discovered resume entry before identity checks. Bundled so the
/// collector stays readable as fields grow per client.
struct IncomingEntry {
    file_label: String,
    hash: String,
    name: String,
    save_path: String,
    label: String,
    tags: Vec<String>,
    priority: i64,
    added_at: i64,
    stopped: bool,
    torrent_bytes: Option<(PathBuf, Vec<u8>)>,
    added_by: &'static str,
}

fn push_entry(
    torrents: &mut Vec<ManifestTorrent>,
    metas: &mut Vec<ScanEntryMeta>,
    problems: &mut Vec<ScanProblem>,
    seen: &mut HashSet<String>,
    incoming: IncomingEntry,
) {
    let IncomingEntry {
        file_label,
        hash,
        name,
        save_path,
        label,
        tags,
        priority,
        added_at,
        stopped,
        torrent_bytes,
        added_by,
    } = incoming;
    let hash = hash.to_ascii_uppercase();
    if hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        problems.push(ScanProblem {
            file: file_label,
            message: format!(
                "{name}: v2-only info-hash — rtorrent speaks v1, so this entry is skipped"
            ),
        });
        return;
    }
    if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        problems.push(ScanProblem {
            file: file_label,
            message: format!("{name}: not a v1 info-hash, skipped"),
        });
        return;
    }
    if !seen.insert(hash.clone()) {
        problems.push(ScanProblem {
            file: file_label,
            message: format!("{name}: duplicate of an entry already scanned, skipped"),
        });
        return;
    }
    let source = match torrent_bytes {
        Some((path, bytes)) => match crate::torrent_file::read_metadata_bytes(&bytes) {
            Err(e) => {
                problems.push(ScanProblem {
                    file: file_label,
                    message: format!(
                        "{name}: .torrent does not parse ({e}) — no re-addable source"
                    ),
                });
                None
            }
            Ok(meta) if meta.info_hash.to_ascii_uppercase() != hash => {
                let _ = path;
                problems.push(ScanProblem {
                    file: file_label,
                    message: format!(
                        "{name}: .torrent is a different torrent ({}) — no re-addable source",
                        meta.info_hash
                    ),
                });
                None
            }
            Ok(_) => Some(ManifestSource::TorrentBase64(encode_base64(&bytes))),
        },
        None => {
            problems.push(ScanProblem {
                file: file_label,
                message: format!(
                    "{name}: no .torrent file beside the resume data (magnet with no metadata?) — unrestorable"
                ),
            });
            None
        }
    };
    metas.push(ScanEntryMeta {
        hash: hash.clone(),
        name: name.clone(),
        has_source: source.is_some(),
        stopped,
    });
    torrents.push(ManifestTorrent {
        hash,
        name,
        source,
        save_path,
        label,
        tags,
        priority,
        throttle: String::new(),
        peers_max: 0,
        peers_min: 0,
        uploads_max: 0,
        trackers: Vec::new(),
        added_by: added_by.into(),
        source_path: String::new(),
        added_at,
        queue_pos: None,
        force_start: false,
    });
}

// ---------------------------------------------------------------------------
// qBittorrent BT_backup.
// ---------------------------------------------------------------------------

fn scan_qb(
    dir: &Path,
    torrents: &mut Vec<ManifestTorrent>,
    metas: &mut Vec<ScanEntryMeta>,
    problems: &mut Vec<ScanProblem>,
    seen: &mut HashSet<String>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            problems.push(ScanProblem {
                file: dir.display().to_string(),
                message: format!("cannot list directory ({e}). qBittorrent keeps resume data in BT_backup — e.g. ~/.local/share/qBittorrent/BT_backup on Linux, ~/Library/Application Support/qBittorrent/BT_backup on macOS, %LOCALAPPDATA%\\qBittorrent\\BT_backup on Windows"),
            });
            return;
        }
    };
    let mut fast_resumes: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("fastresume"))
        {
            fast_resumes.push(path);
        }
    }
    fast_resumes.sort();
    if fast_resumes.is_empty() {
        problems.push(ScanProblem {
            file: dir.display().to_string(),
            message: "no .fastresume files here — pick the BT_backup folder itself".into(),
        });
        return;
    }
    for path in fast_resumes {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let file_label = path.display().to_string();
        let raw = match std::fs::read(&path) {
            Ok(raw) => raw,
            Err(e) => {
                problems.push(ScanProblem {
                    file: file_label,
                    message: format!("cannot read ({e})"),
                });
                continue;
            }
        };
        let dict = match parse_bencode(&raw) {
            Ok(BVal::Dict(pairs)) => pairs,
            Ok(_) => {
                problems.push(ScanProblem {
                    file: file_label,
                    message: "not a resume dict, skipped".into(),
                });
                continue;
            }
            Err(e) => {
                problems.push(ScanProblem {
                    file: file_label,
                    message: format!("does not parse ({e}) — qBittorrent must be closed for a clean read; it rewrites these on exit"),
                });
                continue;
            }
        };
        let str_key = |key: &str| lookup(&dict, key).and_then(bstr);
        // Sibling metadata qBittorrent split out of the resume data.
        let sibling = path.with_extension("torrent");
        let sibling_bytes = std::fs::read(&sibling).ok();
        let sibling_name = sibling_bytes
            .as_deref()
            .and_then(|bytes| crate::torrent_file::read_metadata_bytes(bytes).ok())
            .map(|meta| meta.name)
            .unwrap_or_default();
        let name = str_key("qBt-name")
            .filter(|s| !s.trim().is_empty())
            .or_else(|| str_key("name").filter(|s| !s.trim().is_empty()))
            .or(if sibling_name.trim().is_empty() {
                None
            } else {
                Some(sibling_name)
            })
            .unwrap_or_else(|| stem.clone());
        let save_path = str_key("qBt-savePath")
            .filter(|s| !s.trim().is_empty())
            .or_else(|| str_key("save_path").filter(|s| !s.trim().is_empty()))
            .unwrap_or_default();
        let label = str_key("qBt-category").unwrap_or_default();
        let tags = lookup(&dict, "qBt-tags")
            .map(blist_strs)
            .unwrap_or_default();
        let stopped = lookup(&dict, "qBt-stopCondition")
            .and_then(bstr)
            .is_some_and(|s| !s.trim().is_empty() && s.trim() != "None");
        let source_path = sibling.clone();
        let torrent_bytes = sibling_bytes.map(|bytes| (source_path, bytes));
        push_entry(
            torrents,
            metas,
            problems,
            seen,
            IncomingEntry {
                file_label,
                hash: stem.clone(),
                name,
                save_path,
                label,
                tags,
                priority: 1,
                added_at: 0,
                stopped,
                torrent_bytes,
                added_by: ForeignClient::QBittorrent.added_by(),
            },
        );
        // Record the sibling path as provenance when a source was embedded.
        if let Some(last) = torrents.last_mut() {
            if last.source.is_some() {
                last.source_path = sibling.display().to_string();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Transmission config dir.
// ---------------------------------------------------------------------------

fn scan_transmission(
    dir: &Path,
    torrents: &mut Vec<ManifestTorrent>,
    metas: &mut Vec<ScanEntryMeta>,
    problems: &mut Vec<ScanProblem>,
    seen: &mut HashSet<String>,
) {
    // `dir` is either the config dir (holding `resume/` + `torrents/`) or the
    // `resume/` dir itself.
    let dir_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase());
    let (resume_dir, torrents_dir) = if dir_name.as_deref() == Some("resume") {
        (
            dir.to_path_buf(),
            dir.parent()
                .map(|p| p.join("torrents"))
                .unwrap_or_else(|| dir.join("torrents")),
        )
    } else {
        (dir.join("resume"), dir.join("torrents"))
    };
    // Single-file edge: someone pointed at one .resume directly.
    if dir.is_file() {
        scan_transmission_resume(dir, &torrents_dir, torrents, metas, problems, seen);
        return;
    }
    let entries = match std::fs::read_dir(&resume_dir) {
        Ok(entries) => entries,
        Err(e) => {
            problems.push(ScanProblem {
                file: resume_dir.display().to_string(),
                message: format!("cannot list resume data ({e}). Point at the Transmission config dir (holding resume/ + torrents/) or at resume/ itself — e.g. ~/.config/transmission on Linux, ~/Library/Application Support/Transmission on macOS"),
            });
            return;
        }
    };
    let mut resumes: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("resume"))
        {
            resumes.push(path);
        }
    }
    resumes.sort();
    if resumes.is_empty() {
        problems.push(ScanProblem {
            file: resume_dir.display().to_string(),
            message: "no .resume files here".into(),
        });
        return;
    }
    for path in resumes {
        scan_transmission_resume(&path, &torrents_dir, torrents, metas, problems, seen);
    }
}

fn scan_transmission_resume(
    path: &Path,
    torrents_dir: &Path,
    torrents: &mut Vec<ManifestTorrent>,
    metas: &mut Vec<ScanEntryMeta>,
    problems: &mut Vec<ScanProblem>,
    seen: &mut HashSet<String>,
) {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file_label = path.display().to_string();
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(e) => {
            problems.push(ScanProblem {
                file: file_label,
                message: format!("cannot read ({e})"),
            });
            return;
        }
    };
    let dict = match parse_bencode(&raw) {
        Ok(BVal::Dict(pairs)) => pairs,
        Ok(_) => {
            problems.push(ScanProblem {
                file: file_label,
                message: "not a resume dict, skipped".into(),
            });
            return;
        }
        Err(e) => {
            problems.push(ScanProblem {
                file: file_label,
                message: format!("does not parse ({e}) — stop Transmission for a clean read; it rewrites these while running"),
            });
            return;
        }
    };
    let str_key = |key: &str| lookup(&dict, key).and_then(bstr);
    let name = str_key("name")
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| stem.clone());
    let save_path = str_key("destination").unwrap_or_default();
    let tags = lookup(&dict, "labels").map(blist_strs).unwrap_or_default();
    let stopped = lookup(&dict, "paused").and_then(bint).unwrap_or(0) != 0;
    let added_at = lookup(&dict, "added-date").and_then(bint).unwrap_or(0);
    // Transmission -1/0/1 → rstorrent 0..3 scale, documented at the call site.
    let priority = match lookup(&dict, "bandwidth-priority")
        .and_then(bint)
        .unwrap_or(0)
    {
        i if i < 0 => 0,
        1.. => 2,
        _ => 1,
    };
    // Sibling metadata, trying the stem as-is plus case variants (Windows
    // clients have written either).
    let mut sibling_bytes = None;
    let mut sibling_used = PathBuf::new();
    for candidate in [
        torrents_dir.join(format!("{stem}.torrent")),
        torrents_dir.join(format!("{}.torrent", stem.to_ascii_lowercase())),
        torrents_dir.join(format!("{}.torrent", stem.to_ascii_uppercase())),
    ] {
        if let Ok(bytes) = std::fs::read(&candidate) {
            sibling_used = candidate;
            sibling_bytes = Some(bytes);
            break;
        }
    }
    let torrent_bytes = sibling_bytes.map(|bytes| (sibling_used.clone(), bytes));
    push_entry(
        torrents,
        metas,
        problems,
        seen,
        IncomingEntry {
            file_label,
            hash: stem.clone(),
            name,
            save_path,
            // Transmission groups are bandwidth groups, not categories; keep
            // them out of the label rather than misfiling the library.
            label: String::new(),
            tags,
            priority,
            added_at,
            stopped,
            torrent_bytes,
            added_by: ForeignClient::Transmission.added_by(),
        },
    );
    if let Some(last) = torrents.last_mut() {
        if last.source.is_some() {
            last.source_path = sibling_used.display().to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::torrent_file::{create_torrent, read_metadata_bytes};
    use crate::types::CreateTorrentOptions;
    use std::io::Write;

    // --- tiny bencode writer for fixtures (keys pre-sorted by the caller) ---

    fn enc_str(out: &mut Vec<u8>, s: &[u8]) {
        out.extend_from_slice(format!("{}:", s.len()).as_bytes());
        out.extend_from_slice(s);
    }

    fn enc_value(out: &mut Vec<u8>, key: &str, val: &Val) {
        enc_str(out, key.as_bytes());
        match val {
            Val::Int(i) => out.extend_from_slice(format!("i{i}e").as_bytes()),
            Val::Str(s) => enc_str(out, s.as_bytes()),
            Val::List(items) => {
                out.push(b'l');
                for item in items {
                    enc_str(out, item.as_bytes());
                }
                out.push(b'e');
            }
        }
    }

    enum Val {
        Int(i64),
        Str(String),
        List(Vec<String>),
    }

    /// Encode a dict; sorts keys byte-wise like every real writer does.
    fn enc_dict(pairs: Vec<(&str, Val)>) -> Vec<u8> {
        let mut sorted: Vec<(&str, Val)> = pairs;
        sorted.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        let mut out = vec![b'd'];
        for (key, val) in &sorted {
            enc_value(&mut out, key, val);
        }
        out.push(b'e');
        out
    }

    /// A real single-file .torrent plus its v1 info-hash (uppercase hex).
    fn fixture_torrent(name: &str, content: &[u8]) -> (Vec<u8>, String) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join(name);
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(content).unwrap();
        let (_torrent, bytes) = create_torrent(CreateTorrentOptions {
            source_path: file_path,
            piece_length: Some(16384),
            trackers: vec!["udp://tracker.example/announce".into()],
            is_private: false,
            comment: None,
            source: None,
            created_by: None,
        })
        .unwrap();
        let meta = read_metadata_bytes(&bytes).unwrap();
        (bytes, meta.info_hash.to_ascii_uppercase())
    }

    #[test]
    fn bencode_round_trip_and_rejections() {
        let raw = enc_dict(vec![
            ("name", Val::Str("Show".into())),
            ("paused", Val::Int(1)),
            ("labels", Val::List(vec!["a".into(), "b".into()])),
        ]);
        let parsed = parse_bencode(&raw).unwrap();
        let BVal::Dict(pairs) = parsed else {
            panic!("expected dict");
        };
        assert_eq!(
            lookup(&pairs, "name").and_then(bstr).as_deref(),
            Some("Show")
        );
        assert_eq!(lookup(&pairs, "paused").and_then(bint), Some(1));
        assert_eq!(
            lookup(&pairs, "labels").map(blist_strs).unwrap(),
            vec!["a".to_owned(), "b".to_owned()]
        );
        assert!(parse_bencode(b"i12").is_err(), "unterminated int");
        assert!(parse_bencode(b"3:ab").is_err(), "overrun string");
        assert!(parse_bencode(b"li1eeeEXTRA").is_err(), "trailing bytes");
        assert!(parse_bencode(b"{nope}").is_err(), "bad prefix");
        assert!(parse_bencode(b"le").is_ok());
    }

    #[test]
    fn qb_scan_reads_verified_key_shapes() {
        let (_bytes, hash) = fixture_torrent("episode.mkv", b"fake video bytes for qb");
        // Rebuild bytes (fixture_torrent drops the tempdir, so remake them).
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("episode.mkv");
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(b"fake video bytes for qb").unwrap();
        let (_t, torrent_bytes) = create_torrent(CreateTorrentOptions {
            source_path: file_path,
            piece_length: Some(16384),
            trackers: vec![],
            is_private: false,
            comment: None,
            source: None,
            created_by: None,
        })
        .unwrap();

        let backup = tempfile::tempdir().unwrap();
        std::fs::write(
            backup.path().join(format!("{hash}.torrent")),
            &torrent_bytes,
        )
        .unwrap();
        std::fs::write(
            backup.path().join(format!("{hash}.fastresume")),
            enc_dict(vec![
                ("name", Val::Str("libtorrent name".into())),
                ("qBt-category", Val::Str("video".into())),
                ("qBt-name", Val::Str(String::new())),
                ("qBt-savePath", Val::Str("/dl/qb".into())),
                ("qBt-stopCondition", Val::Str("None".into())),
                ("qBt-tags", Val::List(vec!["hd".into(), "sonarr".into()])),
                ("save_path", Val::Str("/dl/qb".into())),
            ]),
        )
        .unwrap();
        // A magnet with no metadata: resume but no sibling .torrent.
        let magnet_hash = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        std::fs::write(
            backup.path().join(format!("{magnet_hash}.fastresume")),
            enc_dict(vec![("qBt-savePath", Val::Str("/dl/qb".into()))]),
        )
        .unwrap();

        let report = scan(ForeignClient::QBittorrent, backup.path(), 1_700_000_000);
        assert_eq!(report.entry_count, 2);
        assert_eq!(
            report.restorable_count, 1,
            "magnet-only entry has no source"
        );
        let manifest = Manifest::parse(&report.manifest_text).unwrap();
        let entry = manifest.torrents.iter().find(|t| t.hash == hash).unwrap();
        assert_eq!(entry.name, "libtorrent name");
        assert_eq!(entry.save_path, "/dl/qb");
        assert_eq!(entry.label, "video");
        assert_eq!(entry.tags, vec!["hd".to_owned(), "sonarr".to_owned()]);
        assert!(matches!(
            entry.source,
            Some(ManifestSource::TorrentBase64(_))
        ));
        assert_eq!(entry.added_by, "qbittorrent-import");
        assert!(
            !entry.source_path.is_empty(),
            "sibling path kept as provenance"
        );
        assert!(
            report
                .problems
                .iter()
                .any(|p| p.message.contains("no .torrent file")),
            "magnet-only entry reported, not dropped: {:?}",
            report.problems
        );
        // The scanned manifest validates through the normal path.
        let validation = crate::session::validate_manifest(&manifest);
        assert_eq!(validation.restorable_count, 1);
    }

    #[test]
    fn qb_scan_rejects_mismatch_v2_and_garbage() {
        let backup = tempfile::tempdir().unwrap();
        // Hash mismatch: resume stem does not match the sibling .torrent.
        let (torrent_bytes, _real) = fixture_torrent("x.bin", b"mismatch content here!!");
        let wrong = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
        std::fs::write(
            backup.path().join(format!("{wrong}.torrent")),
            &torrent_bytes,
        )
        .unwrap();
        std::fs::write(
            backup.path().join(format!("{wrong}.fastresume")),
            enc_dict(vec![("save_path", Val::Str("/dl".into()))]),
        )
        .unwrap();
        // v2-only stem.
        let v2 = "C".repeat(64);
        std::fs::write(
            backup.path().join(format!("{v2}.fastresume")),
            enc_dict(vec![("save_path", Val::Str("/dl".into()))]),
        )
        .unwrap();
        // Garbage resume.
        std::fs::write(
            backup
                .path()
                .join("DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD.fastresume"),
            b"{definitely not bencode}",
        )
        .unwrap();

        let report = scan(ForeignClient::QBittorrent, backup.path(), 0);
        assert_eq!(
            report.entry_count, 1,
            "only the mismatched entry lands, sourceless"
        );
        assert_eq!(report.restorable_count, 0);
        assert_eq!(report.problems.len(), 3, "{:?}", report.problems);
        assert!(report
            .problems
            .iter()
            .any(|p| p.message.contains("different torrent")));
        assert!(report
            .problems
            .iter()
            .any(|p| p.message.contains("v2-only")));
        assert!(report
            .problems
            .iter()
            .any(|p| p.message.contains("does not parse")));
    }

    #[test]
    fn transmission_scan_reads_kebab_keys() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("movie.mkv");
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(b"fake video bytes for transmission!!").unwrap();
        let (_t, torrent_bytes) = create_torrent(CreateTorrentOptions {
            source_path: file_path,
            piece_length: Some(16384),
            trackers: vec![],
            is_private: false,
            comment: None,
            source: None,
            created_by: None,
        })
        .unwrap();
        let meta = read_metadata_bytes(&torrent_bytes).unwrap();

        let config = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(config.path().join("resume")).unwrap();
        std::fs::create_dir_all(config.path().join("torrents")).unwrap();
        std::fs::write(
            config
                .path()
                .join(format!("resume/{}.resume", meta.info_hash)),
            enc_dict(vec![
                ("added-date", Val::Int(1_700_000_100)),
                ("bandwidth-priority", Val::Int(1)),
                ("destination", Val::Str("/dl/tr".into())),
                ("labels", Val::List(vec!["movies".into()])),
                ("name", Val::Str("Movie".into())),
                ("paused", Val::Int(1)),
            ]),
        )
        .unwrap();
        std::fs::write(
            config
                .path()
                .join(format!("torrents/{}.torrent", meta.info_hash)),
            &torrent_bytes,
        )
        .unwrap();

        // Config-dir layout…
        let report = scan(ForeignClient::Transmission, config.path(), 0);
        assert_eq!(report.entry_count, 1);
        assert_eq!(report.restorable_count, 1);
        let manifest = Manifest::parse(&report.manifest_text).unwrap();
        let entry = &manifest.torrents[0];
        assert_eq!(entry.name, "Movie");
        assert_eq!(entry.save_path, "/dl/tr");
        assert_eq!(entry.tags, vec!["movies".to_owned()]);
        assert_eq!(entry.priority, 2, "transmission high (1) maps above normal");
        assert_eq!(entry.added_at, 1_700_000_100);
        assert!(report.entries[0].stopped, "paused surfaces as stopped");
        // …and pointing straight at resume/ works too.
        let direct = scan(
            ForeignClient::Transmission,
            &config.path().join("resume"),
            0,
        );
        assert_eq!(direct.entry_count, 1);
    }

    #[test]
    fn transmission_empty_dir_explains_itself() {
        let dir = tempfile::tempdir().unwrap();
        let report = scan(ForeignClient::Transmission, dir.path(), 0);
        assert_eq!(report.entry_count, 0);
        assert!(!report.problems.is_empty());
        let qb = scan(ForeignClient::QBittorrent, dir.path(), 0);
        assert!(qb.problems.iter().any(|p| p.message.contains("BT_backup")));
    }
}
