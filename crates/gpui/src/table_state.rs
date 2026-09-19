//! Pure torrent-table state for the native shell.
//!
//! A port of the pure TypeScript modules the table is built from:
//! `src/store/selectors.ts` (filter/search/sort, sidebar counts, selection
//! summary) plus the selection and sort verbs in `src/store/ui.ts` and the
//! column list in `src/components/table/columns.tsx`. Kept independent of GPUI
//! so it can be unit-tested without building the toolkit.
//!
//! Hashes are the stable row keys (the DTO's `hash` field), so selection
//! survives sort, filter and refresh.

use std::collections::{HashMap, HashSet};

use rtorrent_core::file_index::FileIndex;
use rtorrent_core::types::{Status, TorrentDto};

/// Columns the table can sort by (the sortable subset of visible columns).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortColumn {
    #[default]
    Name,
    Size,
    Percent,
    Status,
    DownRate,
    UpRate,
    EtaSeconds,
    Ratio,
    StartedAt,
    FinishedAt,
}

impl SortColumn {
    /// Apply a header click. Names start ascending; numeric columns start
    /// descending because a first click normally means largest/fastest.
    #[must_use]
    pub fn clicked(self, current: Sort) -> Sort {
        if current.column == self {
            Sort {
                column: self,
                ascending: !current.ascending,
            }
        } else {
            Sort {
                column: self,
                ascending: matches!(self, Self::Name | Self::Status),
            }
        }
    }
}

/// A column and its direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort {
    pub column: SortColumn,
    pub ascending: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Sort {
    pub const DEFAULT: Self = Self {
        column: SortColumn::Name,
        ascending: true,
    };

    #[must_use]
    pub fn clicked(self, column: SortColumn) -> Self {
        column.clicked(self)
    }
}

/// The dimensions the table can be filtered on. Saved smart filters and text
/// live in the Tauri shell only; the native filter vocabulary is the sidebar's
/// single-dimension selection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Status(StatusKey),
    ErrorKind(String),
    Label(String),
    /// A tag every shown torrent must carry (V3-10).
    Tag(String),
    Tracker(String),
    View(String),
}

/// Sidebar status buckets. `Completed` is a superset (percent >= 100), as in
/// the TypeScript selectors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatusKey {
    Downloading,
    Seeding,
    Completed,
    Paused,
    Stalled,
    Checking,
    Error,
}

impl StatusKey {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Downloading => "downloading",
            Self::Seeding => "seeding",
            Self::Completed => "completed",
            Self::Paused => "paused",
            Self::Stalled => "stalled",
            Self::Checking => "checking",
            Self::Error => "error",
        }
    }
}

fn matches_status(torrent: &TorrentDto, status: StatusKey) -> bool {
    match status {
        StatusKey::Completed => torrent.percent >= 100.0,
        StatusKey::Error => torrent.status == Status::Error,
        StatusKey::Downloading => torrent.status == Status::Downloading,
        StatusKey::Seeding => torrent.status == Status::Seeding,
        StatusKey::Paused => torrent.status == Status::Paused,
        StatusKey::Stalled => torrent.status == Status::Stalled,
        StatusKey::Checking => torrent.status == Status::Checking,
    }
}

/// Case-insensitive substring match across name, hash, label, tags, tracker,
/// save path — and, when an index is supplied, a torrent's contained filenames
/// (V3-12).
fn matches_search(torrent: &TorrentDto, search: &str, files: Option<&FileIndex>) -> bool {
    if search.is_empty() {
        return true;
    }
    let query = search.to_lowercase();
    torrent.name.to_lowercase().contains(&query)
        || torrent.hash.to_lowercase().contains(&query)
        || torrent.label.to_lowercase().contains(&query)
        || torrent
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(&query))
        || torrent.tracker_host.to_lowercase().contains(&query)
        || torrent.save_path.to_lowercase().contains(&query)
        || files.is_some_and(|index| index.contains(&torrent.hash, &query))
}

fn matches_filter(torrent: &TorrentDto, filter: &Filter) -> bool {
    match filter {
        Filter::All => true,
        Filter::Status(status) => matches_status(torrent, *status),
        Filter::ErrorKind(kind) => torrent.error_kind == *kind,
        Filter::Label(label) => torrent.label == *label,
        Filter::Tag(tag) => torrent
            .tags
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(tag)),
        Filter::Tracker(host) => torrent.tracker_host == *host,
        Filter::View(view) => torrent.views.iter().any(|v| v == view),
    }
}

fn sort_key(torrent: &TorrentDto, column: SortColumn) -> SortValue {
    match column {
        SortColumn::Name => SortValue::Text(torrent.name.to_lowercase()),
        // eta_seconds sorts finite values first in ascending order; the
        // unknown (None) case is effectively infinite.
        SortColumn::EtaSeconds => SortValue::Int(torrent.eta_seconds.unwrap_or(i64::MAX)),
        SortColumn::Size => SortValue::Int(torrent.size),
        SortColumn::Percent => SortValue::Float(torrent.percent),
        SortColumn::DownRate => SortValue::Int(torrent.down_rate),
        SortColumn::UpRate => SortValue::Int(torrent.up_rate),
        SortColumn::Ratio => SortValue::Float(torrent.ratio),
        SortColumn::StartedAt => SortValue::Int(torrent.started_at),
        SortColumn::FinishedAt => SortValue::Int(torrent.finished_at),
        SortColumn::Status => SortValue::Text(status_rank(torrent.status).to_string()),
    }
}

fn status_rank(status: Status) -> u8 {
    match status {
        Status::Downloading => 0,
        Status::Seeding => 1,
        Status::Completed => 2,
        Status::Paused => 3,
        Status::Stalled => 4,
        Status::Checking => 5,
        Status::Error => 6,
    }
}

#[derive(PartialEq)]
enum SortValue {
    Int(i64),
    Float(f64),
    Text(String),
}

impl PartialOrd for SortValue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a.partial_cmp(b),
            (Self::Float(a), Self::Float(b)) => a.partial_cmp(b),
            (Self::Text(a), Self::Text(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

/// Filter + search + sort. Returns references into `torrents` in display
/// order; the input is not mutated.
pub fn visible<'a>(
    torrents: &'a [TorrentDto],
    filter: &Filter,
    search: &str,
    sort: Sort,
) -> Vec<&'a TorrentDto> {
    visible_with_files(torrents, filter, search, sort, None)
}

/// [`visible`] with the lazily built file index (V3-12), so a search can match a
/// torrent's contained filenames as well as its own fields.
pub fn visible_with_files<'a>(
    torrents: &'a [TorrentDto],
    filter: &Filter,
    search: &str,
    sort: Sort,
    files: Option<&FileIndex>,
) -> Vec<&'a TorrentDto> {
    let mut rows: Vec<&TorrentDto> = torrents
        .iter()
        .filter(|t| matches_filter(t, filter) && matches_search(t, search, files))
        .collect();
    rows.sort_by(|a, b| {
        let ordering = sort_key(a, sort.column)
            .partial_cmp(&sort_key(b, sort.column))
            .unwrap_or(std::cmp::Ordering::Equal);
        if sort.ascending {
            ordering
        } else {
            ordering.reverse()
        }
    });
    rows
}

/// Sidebar counts over the *unfiltered* list, matching the TypeScript design:
/// counts stay global regardless of the active filter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SidebarCounts {
    pub all: usize,
    pub downloading: usize,
    pub seeding: usize,
    pub completed: usize,
    pub paused: usize,
    pub stalled: usize,
    pub error: usize,
    pub errors: Vec<(String, usize)>,
    pub labels: Vec<(String, usize)>,
    pub tags: Vec<(String, usize)>,
    pub trackers: Vec<(String, usize)>,
    pub views: Vec<(String, usize)>,
}

pub fn sidebar_counts(torrents: &[TorrentDto]) -> SidebarCounts {
    let mut counts = SidebarCounts {
        all: torrents.len(),
        ..SidebarCounts::default()
    };
    let mut errors: HashMap<&str, usize> = HashMap::new();
    let mut labels: HashMap<&str, usize> = HashMap::new();
    let mut tags: HashMap<&str, usize> = HashMap::new();
    let mut trackers: HashMap<&str, usize> = HashMap::new();
    let mut views: HashMap<&str, usize> = HashMap::new();
    for torrent in torrents {
        match torrent.status {
            Status::Downloading => counts.downloading += 1,
            Status::Seeding => counts.seeding += 1,
            Status::Paused => counts.paused += 1,
            Status::Stalled => counts.stalled += 1,
            Status::Error => counts.error += 1,
            Status::Checking | Status::Completed => {}
        }
        if torrent.percent >= 100.0 {
            counts.completed += 1;
        }
        if torrent.status == Status::Error && !torrent.error_kind.is_empty() {
            *errors.entry(torrent.error_kind.as_str()).or_default() += 1;
        }
        if !torrent.label.is_empty() {
            *labels.entry(torrent.label.as_str()).or_default() += 1;
        }
        // A torrent can carry several tags, so each membership counts (V3-10).
        for tag in &torrent.tags {
            *tags.entry(tag.as_str()).or_default() += 1;
        }
        if !torrent.tracker_host.is_empty() {
            *trackers.entry(torrent.tracker_host.as_str()).or_default() += 1;
        }
        for view in &torrent.views {
            *views.entry(view.as_str()).or_default() += 1;
        }
    }
    counts.errors = sorted(entries(errors));
    counts.labels = sorted(entries(labels));
    counts.tags = sorted(entries(tags));
    counts.trackers = sorted(entries(trackers));
    counts.views = sorted(entries(views));
    counts
}

/// Group the selected hashes by the tag list each should end up with, after
/// adding `add` and removing `remove` (V3-10). One entry per distinct list, so a
/// bulk edit is a few calls rather than one per torrent.
#[must_use]
pub fn tag_edits(
    torrents: &[TorrentDto],
    hashes: &[String],
    add: &[String],
    remove: &[String],
) -> Vec<(Vec<String>, Vec<String>)> {
    let removed: Vec<String> = remove.iter().map(|tag| tag.to_lowercase()).collect();
    let mut groups: Vec<(Vec<String>, Vec<String>)> = Vec::new();
    for hash in hashes {
        let Some(torrent) = torrents
            .iter()
            .find(|torrent| torrent.hash.eq_ignore_ascii_case(hash))
        else {
            continue;
        };
        let mut next: Vec<String> = torrent
            .tags
            .iter()
            .filter(|tag| !removed.contains(&tag.to_lowercase()))
            .cloned()
            .collect();
        for tag in add {
            if !next
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(tag))
            {
                next.push(tag.clone());
            }
        }
        if let Some(group) = groups.iter_mut().find(|(_, tags)| tags == &next) {
            group.0.push(hash.clone());
        } else {
            groups.push((vec![hash.clone()], next));
        }
    }
    groups
}

fn entries(map: HashMap<&str, usize>) -> Vec<(String, usize)> {
    map.into_iter()
        .map(|(value, count)| (value.to_owned(), count))
        .collect()
}

fn sorted(mut rows: Vec<(String, usize)>) -> Vec<(String, usize)> {
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

/// Aggregate figures for the multi-selection summary bar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionSummary {
    pub count: usize,
    pub size: i64,
    pub down_rate: i64,
    pub up_rate: i64,
    /// How many of the selected torrents are stopped, driving Resume/Pause.
    pub paused: usize,
}

/// Summarize the selected torrents. Selection can name hashes that are gone (a
/// removal between snapshot and render), so this counts what actually resolves.
pub fn selection_summary(torrents: &[TorrentDto], selection: &HashSet<String>) -> SelectionSummary {
    let mut summary = SelectionSummary::default();
    for torrent in torrents {
        if !selection.contains(&torrent.hash) {
            continue;
        }
        summary.count += 1;
        summary.size += torrent.size;
        summary.down_rate += torrent.down_rate;
        summary.up_rate += torrent.up_rate;
        if torrent.status == Status::Paused {
            summary.paused += 1;
        }
    }
    summary
}

/// Selection state. Hashes, rather than indexes, survive sort/filter/refresh.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub hashes: HashSet<String>,
    /// The fixed end used by Shift-click and Shift-arrow.
    pub anchor: Option<String>,
}

/// Modifier keys used by a click.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub toggle: bool,
}

/// Apply a row click to selection state, mirroring `src/store/ui.ts`.
#[must_use]
pub fn click(current: &Selection, order: &[String], hash: &str, modifiers: Modifiers) -> Selection {
    if modifiers.toggle {
        let mut hashes = current.hashes.clone();
        if !hashes.remove(hash) {
            hashes.insert(hash.to_owned());
        }
        return Selection {
            hashes,
            anchor: Some(hash.to_owned()),
        };
    }
    if modifiers.shift {
        if let Some(anchor) = current.anchor.as_deref() {
            let a = order.iter().position(|h| h == anchor);
            let b = order.iter().position(|h| h == hash);
            if let (Some(a), Some(b)) = (a, b) {
                let (lo, hi) = if a < b { (a, b) } else { (b, a) };
                return Selection {
                    hashes: order[lo..=hi].iter().cloned().collect(),
                    anchor: Some(anchor.to_owned()),
                };
            }
        }
        return Selection {
            hashes: [hash.to_owned()].into_iter().collect(),
            anchor: Some(hash.to_owned()),
        };
    }
    Selection {
        hashes: [hash.to_owned()].into_iter().collect(),
        anchor: Some(hash.to_owned()),
    }
}

/// The result of a keyboard move: the new selection and the row to reveal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Move {
    pub selection: Selection,
    pub reveal: Option<usize>,
}

fn focus_index(order: &[String], selection: &Selection) -> Option<usize> {
    selection
        .anchor
        .as_deref()
        .and_then(|hash| order.iter().position(|h| h == hash))
        .or_else(|| {
            selection
                .hashes
                .iter()
                .filter_map(|hash| order.iter().position(|h| h == hash))
                .max()
        })
}

fn move_to(order: &[String], index: isize) -> Move {
    if order.is_empty() {
        return Move {
            selection: Selection::default(),
            reveal: None,
        };
    }
    let clamped = index.clamp(0, order.len() as isize - 1) as usize;
    let hash = order[clamped].clone();
    Move {
        selection: Selection {
            hashes: [hash.clone()].into_iter().collect(),
            anchor: Some(hash),
        },
        reveal: Some(clamped),
    }
}

/// Move by rows. An empty selection enters at the first/last row.
#[must_use]
pub fn step(selection: &Selection, order: &[String], delta: isize) -> Move {
    if order.is_empty() {
        return Move {
            selection: selection.clone(),
            reveal: None,
        };
    }
    match focus_index(order, selection) {
        Some(index) => move_to(order, index as isize + delta),
        None => move_to(
            order,
            if delta > 0 {
                0
            } else {
                order.len() as isize - 1
            },
        ),
    }
}

/// Extend the far end by rows.
#[must_use]
pub fn extend_by(selection: &Selection, order: &[String], delta: isize) -> Move {
    if order.is_empty() {
        return Move {
            selection: selection.clone(),
            reveal: None,
        };
    }
    let Some(anchor) = selection.anchor.as_deref() else {
        return step(selection, order, delta);
    };
    let Some(anchor_index) = order.iter().position(|h| h == anchor) else {
        return step(selection, order, delta);
    };
    let far = selection
        .hashes
        .iter()
        .filter_map(|hash| order.iter().position(|h| h == hash))
        .max_by_key(|&index| index.abs_diff(anchor_index))
        .unwrap_or(anchor_index);
    let target = (far as isize + delta).clamp(0, order.len() as isize - 1) as usize;
    let (lo, hi) = if anchor_index < target {
        (anchor_index, target)
    } else {
        (target, anchor_index)
    };
    Move {
        selection: Selection {
            hashes: order[lo..=hi].iter().cloned().collect(),
            anchor: Some(anchor.to_owned()),
        },
        reveal: Some(target),
    }
}

/// Drop hashes that no longer exist (after a snapshot/removal).
pub fn prune(selection: &mut Selection, existing: &HashSet<String>) {
    selection.hashes.retain(|hash| existing.contains(hash));
    if selection
        .anchor
        .as_ref()
        .is_some_and(|anchor| !existing.contains(anchor))
    {
        selection.anchor = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn torrent(hash: &str, name: &str) -> TorrentDto {
        TorrentDto {
            hash: hash.into(),
            name: name.into(),
            size: 0,
            bytes_done: 0,
            percent: 0.0,
            status: Status::Downloading,
            status_msg: String::new(),
            seeds_connected: 0,
            peers_connected: 0,
            seeds_swarm: 0,
            peers_swarm: 0,
            down_rate: 0,
            up_rate: 0,
            eta_seconds: None,
            ratio: 0.0,
            label: String::new(),
            tracker_host: String::new(),
            save_path: String::new(),
            priority: 0,
            is_private: false,
            throttle_name: String::new(),
            down_rate_limit: None,
            up_rate_limit: None,
            started_at: 0,
            finished_at: 0,
            peers_max: 0,
            peers_min: 0,
            uploads_max: 0,
            connection_type: String::new(),
            added_by: String::new(),
            source_path: String::new(),
            added_at: 0,
            tags: Vec::new(),
            force_start: false,
            throttle_rule: String::new(),
            views: vec![],
            error_kind: String::new(),
            is_open: true,
            is_active: true,
        }
    }

    #[test]
    fn search_matches_name_label_and_tracker_case_insensitively() {
        let mut t = torrent("H", "Ubuntu ISO");
        t.label = "Linux".into();
        t.tracker_host = "tracker.example.com".into();
        assert!(matches_search(&t, "ubuntu", None));
        assert!(matches_search(&t, "LINUX", None));
        assert!(matches_search(&t, "example", None));
        assert!(!matches_search(&t, "debian", None));
    }

    #[test]
    fn visible_filters_searches_and_sorts() {
        let a = torrent("A", "banana");
        let mut b = torrent("B", "apple");
        b.status = Status::Seeding;
        let torrents = vec![a, b];
        let rows = visible(&torrents, &Filter::All, "", Sort::DEFAULT);
        assert_eq!(rows[0].hash, "B");
        let rows = visible(
            &torrents,
            &Filter::Status(StatusKey::Seeding),
            "",
            Sort::DEFAULT,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].hash, "B");
        let rows = visible(&torrents, &Filter::All, "banana", Sort::DEFAULT);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].hash, "A");
    }

    #[test]
    fn completed_filter_is_a_percent_superset() {
        let mut a = torrent("A", "a");
        a.percent = 100.0;
        a.status = Status::Seeding;
        let b = torrent("B", "b");
        let torrents = vec![a, b];
        let rows = visible(
            &torrents,
            &Filter::Status(StatusKey::Completed),
            "",
            Sort::DEFAULT,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].hash, "A");
    }

    #[test]
    fn null_eta_sorts_last_ascending() {
        let mut a = torrent("A", "a");
        a.eta_seconds = None;
        let mut b = torrent("B", "b");
        b.eta_seconds = Some(60);
        let torrents = vec![a, b];
        let rows = visible(
            &torrents,
            &Filter::All,
            "",
            Sort {
                column: SortColumn::EtaSeconds,
                ascending: true,
            },
        );
        assert_eq!(rows[0].hash, "B");
    }

    #[test]
    fn sidebar_counts_stay_global_and_completed_overlaps() {
        let mut a = torrent("A", "a");
        a.status = Status::Downloading;
        a.label = "video".into();
        let mut b = torrent("B", "b");
        b.status = Status::Seeding;
        b.percent = 100.0;
        b.label = "video".into();
        b.tracker_host = "t.example".into();
        b.views = vec!["main".into()];
        let counts = sidebar_counts(&[a, b]);
        assert_eq!(counts.all, 2);
        assert_eq!(counts.downloading, 1);
        assert_eq!(counts.seeding, 1);
        assert_eq!(counts.completed, 1);
        assert_eq!(counts.labels, vec![("video".into(), 2)]);
        assert_eq!(counts.trackers, vec![("t.example".into(), 1)]);
        assert_eq!(counts.views, vec![("main".into(), 1)]);
    }

    #[test]
    fn clicks_follow_native_list_conventions() {
        let order = ["A".to_owned(), "B".into(), "C".into()];
        let empty = Selection::default();
        let single = click(&empty, &order, "B", Modifiers::default());
        assert_eq!(single.anchor.as_deref(), Some("B"));
        let toggled = click(
            &single,
            &order,
            "C",
            Modifiers {
                shift: false,
                toggle: true,
            },
        );
        assert!(toggled.hashes.contains("B") && toggled.hashes.contains("C"));
        let ranged = click(
            &single,
            &order,
            "C",
            Modifiers {
                shift: true,
                toggle: false,
            },
        );
        assert_eq!(ranged.hashes.len(), 2);
        assert!(ranged.hashes.contains("B") && ranged.hashes.contains("C"));
    }

    #[test]
    fn keyboard_step_enters_and_moves() {
        let order = ["A".to_owned(), "B".into()];
        let entered = step(&Selection::default(), &order, 1);
        assert_eq!(entered.reveal, Some(0));
        let next = step(&entered.selection, &order, 1);
        assert_eq!(next.reveal, Some(1));
        let extended = extend_by(&next.selection, &order, -1);
        assert_eq!(extended.selection.hashes.len(), 2);
    }

    #[test]
    fn prune_drops_removed_hashes_and_anchor() {
        let mut selection = Selection {
            hashes: ["A".to_owned(), "GONE".into()].into_iter().collect(),
            anchor: Some("GONE".into()),
        };
        prune(&mut selection, &["A".to_owned()].into_iter().collect());
        assert!(!selection.hashes.contains("GONE"));
        assert!(selection.anchor.is_none());
    }

    #[test]
    fn search_matches_tags_case_insensitively() {
        let mut t = torrent("H", "Ubuntu ISO");
        t.tags = vec!["Linux".into(), "server".into()];
        assert!(matches_search(&t, "linu", None));
        assert!(matches_search(&t, "SERVER", None));
        assert!(!matches_search(&t, "desktop", None));
    }

    #[test]
    fn the_tag_filter_is_case_insensitive_and_requires_membership() {
        let mut tagged = torrent("A", "a");
        tagged.tags = vec!["Linux".into()];
        let plain = torrent("B", "b");
        let rows = vec![tagged, plain];
        let shown = visible(&rows, &Filter::Tag("linux".into()), "", Sort::DEFAULT);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].hash, "A");
        let none = visible(&rows, &Filter::Tag("iso".into()), "", Sort::DEFAULT);
        assert!(none.is_empty());
    }

    #[test]
    fn sidebar_counts_each_tag_membership() {
        let mut a = torrent("A", "a");
        a.tags = vec!["linux".into(), "iso".into()];
        let mut b = torrent("B", "b");
        b.tags = vec!["linux".into()];
        let counts = sidebar_counts(&[a, b]);
        assert_eq!(
            counts.tags,
            vec![("iso".to_owned(), 1), ("linux".to_owned(), 2)]
        );
    }

    #[test]
    fn tag_edits_group_by_the_resulting_list() {
        let mut a = torrent("A", "a");
        a.tags = vec!["linux".into()];
        let mut b = torrent("B", "b");
        b.tags = vec!["linux".into(), "iso".into()];
        let mut c = torrent("C", "c");
        c.tags = vec!["iso".into()];
        let rows = vec![a, b, c];

        // Add "archive" to all, remove "iso": A → [linux, archive], B → [linux,
        // archive], C → [archive]. Two groups: [A, B] and [C].
        let groups = tag_edits(
            &rows,
            &["A".into(), "B".into(), "C".into()],
            &["archive".into()],
            &["iso".into()],
        );
        assert_eq!(groups.len(), 2);
        let ab = groups
            .iter()
            .find(|(hashes, _)| hashes.contains(&"A".to_owned()))
            .unwrap();
        assert_eq!(ab.1, vec!["linux".to_owned(), "archive".to_owned()]);
        assert_eq!(ab.0.len(), 2);
        let c_group = groups
            .iter()
            .find(|(hashes, _)| hashes.contains(&"C".to_owned()))
            .unwrap();
        assert_eq!(c_group.1, vec!["archive".to_owned()]);
    }

    #[test]
    fn search_matches_hash_save_path_and_indexed_filenames() {
        let mut a = torrent("AAAA", "alpha");
        a.save_path = "/srv/movies".into();
        let b = torrent("BBBB", "beta");
        let rows = vec![a, b];

        // The torrent's own fields are matched without an index.
        assert_eq!(visible(&rows, &Filter::All, "aaaa", Sort::DEFAULT).len(), 1);
        assert_eq!(
            visible(&rows, &Filter::All, "movies", Sort::DEFAULT).len(),
            1
        );
        // A filename match needs the index; without one it is not a match.
        assert!(visible(&rows, &Filter::All, "s02e04", Sort::DEFAULT).is_empty());

        let mut index = FileIndex::default();
        index.record("BBBB", ["Show/S02E04.mkv"]);
        let matched =
            visible_with_files(&rows, &Filter::All, "s02e04", Sort::DEFAULT, Some(&index));
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].hash, "BBBB");
    }
}
