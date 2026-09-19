//! Pure torrent-table column state.
//!
//! A port of `src/components/table/columns.ts`. Definitions, width clamping,
//! visibility rules and persistence normalization live here, free of GPUI, so
//! the table's rendering code stays about rendering and malformed persisted
//! data never reaches it.

use crate::table_state::SortColumn;

/// Every column the table knows, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColumnId {
    Name,
    Size,
    Done,
    Status,
    Seeds,
    Peers,
    Down,
    Up,
    Eta,
    Ratio,
    Label,
    Tracker,
    Started,
    Finished,
}

impl ColumnId {
    /// Display order, which is also the persisted order.
    pub const ALL: [Self; 14] = [
        Self::Name,
        Self::Size,
        Self::Done,
        Self::Status,
        Self::Seeds,
        Self::Peers,
        Self::Down,
        Self::Up,
        Self::Eta,
        Self::Ratio,
        Self::Label,
        Self::Tracker,
        Self::Started,
        Self::Finished,
    ];

    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The stable key used in `settings.json`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Size => "size",
            Self::Done => "done",
            Self::Status => "status",
            Self::Seeds => "seeds",
            Self::Peers => "peers",
            Self::Down => "down",
            Self::Up => "up",
            Self::Eta => "eta",
            Self::Ratio => "ratio",
            Self::Label => "label",
            Self::Tracker => "tracker",
            Self::Started => "started",
            Self::Finished => "finished",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|id| id.as_str() == value)
    }
}

/// One column's fixed properties.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColumnDefinition {
    pub id: ColumnId,
    pub label: &'static str,
    pub default_width: f32,
    pub min_width: f32,
    /// Takes the leftover width instead of a fixed width (Name only).
    pub flexible: bool,
    /// Hidden until the user enables it, rather than shown on first run.
    pub default_hidden: bool,
    /// Site of the header click that sorts by this column, when it sorts.
    pub sort: Option<SortColumn>,
}

/// The definition table, matching `COLUMN_DEFINITIONS`.
pub const COLUMN_DEFINITIONS: [ColumnDefinition; 14] = [
    ColumnDefinition {
        id: ColumnId::Name,
        label: "Name",
        default_width: 200.,
        min_width: 120.,
        flexible: true,
        default_hidden: false,
        sort: Some(SortColumn::Name),
    },
    ColumnDefinition {
        id: ColumnId::Size,
        label: "Size",
        default_width: 70.,
        min_width: 40.,
        flexible: false,
        default_hidden: false,
        sort: Some(SortColumn::Size),
    },
    ColumnDefinition {
        id: ColumnId::Done,
        label: "Done",
        default_width: 92.,
        min_width: 40.,
        flexible: false,
        default_hidden: false,
        sort: Some(SortColumn::Percent),
    },
    ColumnDefinition {
        id: ColumnId::Status,
        label: "Status",
        default_width: 84.,
        min_width: 64.,
        flexible: false,
        default_hidden: false,
        sort: Some(SortColumn::Status),
    },
    ColumnDefinition {
        id: ColumnId::Seeds,
        label: "S",
        default_width: 52.,
        min_width: 40.,
        flexible: false,
        default_hidden: false,
        sort: None,
    },
    ColumnDefinition {
        id: ColumnId::Peers,
        label: "P",
        default_width: 52.,
        min_width: 40.,
        flexible: false,
        default_hidden: false,
        sort: None,
    },
    ColumnDefinition {
        id: ColumnId::Down,
        label: "Down",
        default_width: 76.,
        min_width: 40.,
        flexible: false,
        default_hidden: false,
        sort: Some(SortColumn::DownRate),
    },
    ColumnDefinition {
        id: ColumnId::Up,
        label: "Up",
        default_width: 76.,
        min_width: 40.,
        flexible: false,
        default_hidden: false,
        sort: Some(SortColumn::UpRate),
    },
    ColumnDefinition {
        id: ColumnId::Eta,
        label: "ETA",
        default_width: 62.,
        min_width: 40.,
        flexible: false,
        default_hidden: false,
        sort: Some(SortColumn::EtaSeconds),
    },
    ColumnDefinition {
        id: ColumnId::Ratio,
        label: "Ratio",
        default_width: 46.,
        min_width: 40.,
        flexible: false,
        default_hidden: false,
        sort: Some(SortColumn::Ratio),
    },
    ColumnDefinition {
        id: ColumnId::Label,
        label: "Label",
        default_width: 72.,
        min_width: 52.,
        flexible: false,
        default_hidden: false,
        sort: None,
    },
    ColumnDefinition {
        id: ColumnId::Tracker,
        label: "Tracker",
        default_width: 110.,
        min_width: 70.,
        flexible: false,
        default_hidden: false,
        sort: None,
    },
    ColumnDefinition {
        id: ColumnId::Started,
        label: "Started",
        default_width: 72.,
        min_width: 52.,
        flexible: false,
        default_hidden: true,
        sort: Some(SortColumn::StartedAt),
    },
    ColumnDefinition {
        id: ColumnId::Finished,
        label: "Finished",
        default_width: 72.,
        min_width: 52.,
        flexible: false,
        default_hidden: true,
        sort: Some(SortColumn::FinishedAt),
    },
];

/// The definition of one column.
#[must_use]
pub fn definition(id: ColumnId) -> ColumnDefinition {
    COLUMN_DEFINITIONS[id.index()]
}

/// One column's persisted width and visibility.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnPref {
    pub id: String,
    pub width: f32,
    pub visible: bool,
}

/// The table's live column state.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnState {
    widths: [f32; 14],
    visibility: [bool; 14],
}

impl Default for ColumnState {
    fn default() -> Self {
        let mut widths = [0.; 14];
        let mut visibility = [false; 14];
        for id in ColumnId::ALL {
            let definition = definition(id);
            widths[id.index()] = definition.default_width;
            visibility[id.index()] = !definition.default_hidden;
        }
        Self { widths, visibility }
    }
}

impl ColumnState {
    /// Clamp a width to its column's usable minimum, rounding to whole pixels.
    #[must_use]
    pub fn clamp_width(id: ColumnId, width: f32) -> f32 {
        let definition = definition(id);
        if width.is_finite() {
            width.max(definition.min_width).round()
        } else {
            definition.default_width
        }
    }

    #[must_use]
    pub fn width(&self, id: ColumnId) -> f32 {
        Self::clamp_width(id, self.widths[id.index()])
    }

    #[must_use]
    pub fn is_visible(&self, id: ColumnId) -> bool {
        // Name is the row's identity and cannot be hidden.
        id == ColumnId::Name || self.visibility[id.index()]
    }

    /// Set one column's width, clamped.
    pub fn resize(&mut self, id: ColumnId, width: f32) {
        self.widths[id.index()] = Self::clamp_width(id, width);
    }

    /// Set one column's visibility; Name stays visible.
    pub fn set_visible(&mut self, id: ColumnId, visible: bool) {
        if id == ColumnId::Name {
            self.visibility[id.index()] = true;
            return;
        }
        self.visibility[id.index()] = visible;
    }

    pub fn toggle(&mut self, id: ColumnId) {
        let next = !self.is_visible(id);
        self.set_visible(id, next);
    }

    /// Restore every column to its definition defaults.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The columns currently in the grid, in display order.
    pub fn visible(&self) -> Vec<ColumnId> {
        ColumnId::ALL
            .into_iter()
            .filter(|id| self.is_visible(*id))
            .collect()
    }

    /// The persisted form: one entry per column, always in display order so a
    /// settings diff reads sensibly.
    #[must_use]
    pub fn prefs(&self) -> Vec<ColumnPref> {
        ColumnId::ALL
            .into_iter()
            .map(|id| ColumnPref {
                id: id.as_str().to_owned(),
                width: self.width(id),
                visible: self.is_visible(id),
            })
            .collect()
    }

    /// Rebuild from persisted state, falling back per field and entirely on
    /// unknown input, as `deserializeColumnState` does in the TypeScript tree.
    #[must_use]
    pub fn from_prefs(prefs: &[ColumnPref]) -> Self {
        let mut state = Self::default();
        for pref in prefs {
            let Some(id) = ColumnId::parse(&pref.id) else {
                continue;
            };
            state.resize(id, pref.width);
            state.set_visible(id, pref.visible);
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_hide_only_the_date_columns() {
        let state = ColumnState::default();
        assert!(!state.is_visible(ColumnId::Started));
        assert!(!state.is_visible(ColumnId::Finished));
        assert_eq!(state.visible().len(), 12);
        assert_eq!(state.visible()[0], ColumnId::Name);
    }

    #[test]
    fn name_cannot_be_hidden() {
        let mut state = ColumnState::default();
        state.set_visible(ColumnId::Name, false);
        state.toggle(ColumnId::Name);
        assert!(state.is_visible(ColumnId::Name));
    }

    #[test]
    fn widths_clamp_to_the_definition() {
        let mut state = ColumnState::default();
        state.resize(ColumnId::Name, 10.);
        assert_eq!(state.width(ColumnId::Name), 120.);
        state.resize(ColumnId::Size, 123.4);
        assert_eq!(state.width(ColumnId::Size), 123.);
        state.resize(ColumnId::Size, f32::NAN);
        assert_eq!(state.width(ColumnId::Size), 70.);
    }

    #[test]
    fn prefs_round_trip_and_ignore_unknown_columns() {
        let mut state = ColumnState::default();
        state.resize(ColumnId::Tracker, 200.);
        state.set_visible(ColumnId::Started, true);
        state.set_visible(ColumnId::Ratio, false);

        let mut prefs = state.prefs();
        prefs.push(ColumnPref {
            id: "nonexistent".into(),
            width: 40.,
            visible: true,
        });
        let restored = ColumnState::from_prefs(&prefs);
        assert_eq!(restored, state);
        assert!(restored.is_visible(ColumnId::Started));
        assert!(!restored.is_visible(ColumnId::Ratio));
        assert_eq!(restored.visible().len(), 12);
    }

    #[test]
    fn only_numeric_columns_are_sortable_headers() {
        assert_eq!(definition(ColumnId::Done).sort, Some(SortColumn::Percent));
        assert_eq!(
            definition(ColumnId::Finished).sort,
            Some(SortColumn::FinishedAt)
        );
        assert_eq!(definition(ColumnId::Seeds).sort, None);
        assert_eq!(definition(ColumnId::Tracker).sort, None);
    }
}
