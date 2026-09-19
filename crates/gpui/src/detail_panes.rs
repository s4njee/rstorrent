//! The detail panel's panes.
//!
//! A *pane* is what the panel shows; a [`DetailTab`] is what the daemon can be
//! asked for. They are not the same set — the panel has six panes and two of
//! them read the daemon's `general` payload — so the mapping lives here rather
//! than being implied by a union of both vocabularies.
//!
//! A port of `src/utils/panes.ts`, plus the focused-torrent rule the panel needs
//! (`focusedHashOf` in the Tauri tree).

use rtorrent_core::types::DetailTab;

use crate::table_state::Selection;

/// One tab of the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Files,
    Peers,
    Trackers,
    Transfer,
    Pieces,
    Log,
}

/// The panes in tab-strip order: the design's five, with the log last (it is
/// ours, not the design's, and losing it would cost the only view of what the
/// app has done).
pub const PANES: [Pane; 6] = [
    Pane::Files,
    Pane::Peers,
    Pane::Trackers,
    Pane::Transfer,
    Pane::Pieces,
    Pane::Log,
];

/// The pane the panel opens on.
pub const DEFAULT_PANE: Pane = Pane::Files;

impl Pane {
    /// The tab's label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Files => "Files",
            Self::Peers => "Peers",
            Self::Trackers => "Trackers",
            Self::Transfer => "Transfer",
            Self::Pieces => "Pieces",
            Self::Log => "Log",
        }
    }

    /// The daemon tab whose payload this pane renders.
    ///
    /// `General` carries the piece map, which is why Pieces reads the same tab
    /// as Transfer rather than having one of its own.
    #[must_use]
    pub const fn daemon_tab(self) -> DetailTab {
        match self {
            Self::Files => DetailTab::Content,
            Self::Peers => DetailTab::Peers,
            Self::Trackers => DetailTab::Trackers,
            Self::Transfer | Self::Pieces => DetailTab::General,
            Self::Log => DetailTab::Log,
        }
    }

    /// Whether the facts rail shares the body with this pane.
    #[must_use]
    pub const fn has_rail(self) -> bool {
        matches!(self, Self::Files | Self::Transfer)
    }

    /// The persisted name, also the serde name of the daemon's tabs.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Peers => "peers",
            Self::Trackers => "trackers",
            Self::Transfer => "transfer",
            Self::Pieces => "pieces",
            Self::Log => "log",
        }
    }

    /// Parse a persisted pane, accepting the pre-console names so a view saved
    /// by an older build opens on the equivalent pane instead of the default.
    /// `speed` was the per-torrent chart, which the design folds into Transfer.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "files" => Some(Self::Files),
            "peers" => Some(Self::Peers),
            "trackers" => Some(Self::Trackers),
            "transfer" => Some(Self::Transfer),
            "pieces" => Some(Self::Pieces),
            "log" => Some(Self::Log),
            "content" => Some(Self::Files),
            "general" | "speed" => Some(Self::Transfer),
            _ => None,
        }
    }
}

/// The torrent the panel describes.
///
/// A multi-selection keeps its subject: the anchor is the row that was clicked
/// last, so a shift-range or a cmd-click set still shows something instead of
/// blanking the panel. When the anchor is no longer selected — a toggle removed
/// it — the lowest hash stands in: the selection is a set, so there is no
/// "first" to fall back to, and picking one arbitrarily would make the panel
/// jump between renders.
#[must_use]
pub fn focused_hash(selection: &Selection) -> Option<String> {
    if selection.hashes.is_empty() {
        return None;
    }
    match &selection.anchor {
        Some(anchor) if selection.hashes.contains(anchor) => Some(anchor.clone()),
        _ => selection.hashes.iter().min().cloned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection(hashes: &[&str], anchor: Option<&str>) -> Selection {
        Selection {
            hashes: hashes.iter().map(|hash| (*hash).to_owned()).collect(),
            anchor: anchor.map(str::to_owned),
        }
    }

    #[test]
    fn the_tab_strip_is_the_designs_five_plus_the_log() {
        let labels: Vec<&str> = PANES.iter().map(|pane| pane.label()).collect();
        assert_eq!(
            labels,
            ["Files", "Peers", "Trackers", "Transfer", "Pieces", "Log"]
        );
        assert_eq!(DEFAULT_PANE, Pane::Files);
    }

    #[test]
    fn each_pane_knows_the_daemon_tab_it_reads() {
        assert_eq!(Pane::Files.daemon_tab(), DetailTab::Content);
        assert_eq!(Pane::Peers.daemon_tab(), DetailTab::Peers);
        assert_eq!(Pane::Trackers.daemon_tab(), DetailTab::Trackers);
        assert_eq!(Pane::Transfer.daemon_tab(), DetailTab::General);
        // The piece map travels with the general payload.
        assert_eq!(Pane::Pieces.daemon_tab(), DetailTab::General);
        assert_eq!(Pane::Log.daemon_tab(), DetailTab::Log);
    }

    #[test]
    fn the_facts_rail_joins_only_the_panes_the_design_gives_one() {
        let with_rail: Vec<Pane> = PANES.into_iter().filter(|pane| pane.has_rail()).collect();
        assert_eq!(with_rail, [Pane::Files, Pane::Transfer]);
    }

    #[test]
    fn a_persisted_pane_round_trips() {
        for pane in PANES {
            assert_eq!(Pane::parse(pane.key()), Some(pane));
        }
        // The persisted name is the daemon's serde name, so the two cannot drift.
        assert_eq!(Pane::Files.key(), "files");
        assert_eq!(Pane::Transfer.key(), "transfer");
    }

    #[test]
    fn an_older_builds_pane_opens_on_the_equivalent_one() {
        assert_eq!(Pane::parse("content"), Some(Pane::Files));
        assert_eq!(Pane::parse("general"), Some(Pane::Transfer));
        assert_eq!(Pane::parse("speed"), Some(Pane::Transfer));
        // Case is not significant.
        assert_eq!(Pane::parse("Peers"), Some(Pane::Peers));
    }

    #[test]
    fn an_unplaceable_pane_is_none() {
        assert_eq!(Pane::parse(""), None);
        assert_eq!(Pane::parse("nope"), None);
    }

    #[test]
    fn the_panel_has_no_subject_without_a_selection() {
        assert_eq!(focused_hash(&Selection::default()), None);
        // An anchor alone is not a subject: the set is what is selected.
        assert_eq!(focused_hash(&selection(&[], Some("A"))), None);
    }

    #[test]
    fn a_multi_selection_keeps_the_row_clicked_last() {
        let selected = selection(&["A", "B", "C"], Some("C"));
        assert_eq!(focused_hash(&selected), Some("C".to_owned()));
    }

    #[test]
    fn a_deselected_anchor_falls_back_to_what_is_still_selected() {
        let selected = selection(&["A", "B"], Some("Z"));
        assert_eq!(focused_hash(&selected), Some("A".to_owned()));
    }

    #[test]
    fn one_selected_row_is_the_subject_whether_it_is_the_anchor_or_not() {
        assert_eq!(
            focused_hash(&selection(&["B"], Some("B"))),
            Some("B".to_owned())
        );
        assert_eq!(focused_hash(&selection(&["B"], None)), Some("B".to_owned()));
    }
}
