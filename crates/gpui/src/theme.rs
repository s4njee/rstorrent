//! rstorrent's native-shell design tokens.
//!
//! The values mirror `src/theme/tokens.css` ("Dark Ops"), the single source of
//! truth for the Tauri UI. Keeping the palette in one small module makes the
//! shell's views share a vocabulary instead of scattering raw colours.

use gpui_kit::{rgb, ParentElement, Rgba, Styled};

/// Theme-aware colours used by the shell and its torrent surfaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub app: u32,
    pub panel: u32,
    pub field: u32,
    pub row_alt: u32,
    pub selected: u32,
    pub track: u32,
    pub border_strong: u32,
    pub border_mid: u32,
    pub row_line: u32,
    pub text_primary: u32,
    pub text_body: u32,
    pub text_muted: u32,
    pub text_dim: u32,
    pub accent_cyan: u32,
    pub accent_cyan_bright: u32,
    pub accent_green: u32,
    pub accent_green_soft: u32,
    pub accent_amber: u32,
    pub accent_red: u32,
    pub accent_violet: u32,
    pub danger_bg: u32,
    pub danger_border: u32,
    pub danger_text: u32,
}

impl Palette {
    /// The "Dark Ops" tokens from `tokens.css`. The app ships one theme, so
    /// there is no light variant to keep in sync.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            app: 0x0014_161a,
            panel: 0x0019_1c21,
            field: 0x000f_1114,
            row_alt: 0x0017_1a1f,
            selected: 0x001d_2b33,
            track: 0x0022_262d,
            border_strong: 0x002a_2f38,
            border_mid: 0x0023_272e,
            row_line: 0x001a_1d22,
            text_primary: 0x00d6_dae2,
            text_body: 0x00c8_cdd6,
            text_muted: 0x008b_93a2,
            text_dim: 0x0056_5e6b,
            accent_cyan: 0x0058_c4dd,
            accent_cyan_bright: 0x008f_dcee,
            accent_green: 0x0057_d597,
            accent_green_soft: 0x007f_d8a4,
            accent_amber: 0x00d5_a04c,
            accent_red: 0x00e0_5d5d,
            accent_violet: 0x009b_8cf0,
            danger_bg: 0x003a_1f1f,
            danger_border: 0x006b_2f2f,
            danger_text: 0x00e0_a5a5,
        }
    }

    /// Fill colour for a torrent status's progress bar.
    #[must_use]
    pub const fn status_fill(self, status: rtorrent_core::types::Status) -> u32 {
        use rtorrent_core::types::Status::{
            Checking, Completed, Downloading, Error, Paused, Seeding, Stalled,
        };
        match status {
            Downloading | Checking => self.accent_cyan,
            Seeding | Completed => self.accent_green,
            Paused => 0x004a_515c,
            Stalled => self.accent_amber,
            Error => self.accent_red,
        }
    }

    /// Text colour for a torrent status cell.
    #[must_use]
    pub const fn status_text(self, status: rtorrent_core::types::Status) -> u32 {
        use rtorrent_core::types::Status::{
            Checking, Completed, Downloading, Error, Paused, Seeding, Stalled,
        };
        match status {
            Downloading => self.accent_cyan,
            Seeding | Completed => self.accent_green,
            Paused => self.text_dim,
            Stalled => self.accent_amber,
            Checking => self.accent_cyan_bright,
            Error => self.accent_red,
        }
    }
}

#[inline]
#[must_use]
pub fn color(value: u32) -> Rgba {
    rgb(value)
}

/// Mix two tokens, `ratio` of the way from `from` to `to`.
///
/// For the piece stripes and the availability bar, where a column stands for a
/// slice of the torrent rather than one chunk: the fraction has to survive into
/// the paint, and a palette value has no alpha to spend on it.
#[must_use]
pub fn blend(from: u32, to: u32, ratio: f32) -> u32 {
    let ratio = ratio.clamp(0.0, 1.0);
    let channel = |shift: u32| {
        let a = f32::from(((from >> shift) & 0xff) as u8);
        let b = f32::from(((to >> shift) & 0xff) as u8);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let mixed = (a + (b - a) * ratio).round().clamp(0.0, 255.0) as u32;
        mixed << shift
    };
    channel(16) | channel(8) | channel(0)
}

pub const SANS: &str = "IBM Plex Sans";
pub const MONO: &str = "IBM Plex Mono";

/// Tag chip colours (V3-10), the same palette the web console uses
/// (`src/utils/tags.ts`). A tag's colour is its name hashed into this list, so
/// it looks identical in both shells without a settings round-trip.
const TAG_COLOURS: [u32; 8] = [
    0x004f_9ecf,
    0x007d_aea3,
    0x00c9_a86a,
    0x00b4_7cc7,
    0x00d0_8989,
    0x008f_bf76,
    0x00c7_9a5b,
    0x006f_a8b8,
];

/// A stable colour for a tag name, mirroring `tagColour` in the web client.
#[must_use]
pub fn tag_colour(tag: &str) -> u32 {
    let mut hash: u32 = 0;
    for byte in tag.to_lowercase().bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(u32::from(byte));
    }
    TAG_COLOURS[(hash as usize) % TAG_COLOURS.len()]
}

pub mod geometry {
    pub const TITLE_BAR: f32 = 34.;
    pub const TOOLBAR: f32 = 44.;
    pub const TOOLBAR_BUTTON: f32 = 26.;
    pub const TOOLBAR_BUTTON_H: f32 = 24.;
    pub const COLUMN_HEADER: f32 = 24.;
    pub const ROW: f32 = 23.;
    pub const STATUS_BAR: f32 = 24.;
    pub const SELECTION_BAR: f32 = 26.;
    pub const SIDEBAR: f32 = 206.;
    /// The detail panel's total height, under the table's workspace.
    pub const DETAIL: f32 = 220.;
    /// The panel's tab strip.
    pub const DETAIL_TABS: f32 = 26.;
    /// A pane's own header strip — `Peers — 12 connected`.
    pub const DETAIL_HEADER: f32 = 21.;
    /// A row in the Files tree or the Peers table.
    pub const DETAIL_ROW: f32 = 22.;
    /// The facts rail's width, and its key column.
    pub const DETAIL_RAIL: f32 = 260.;
    pub const DETAIL_RAIL_KEY: f32 = 84.;
}

/// The scrim behind a modal: `rgba(0, 0, 0, 0.5)`.
pub const SCRIM: u32 = 0x0000_0080;
/// `--border-black`: the window and panel border in the design.
pub const BORDER_BLACK: u32 = 0x0000_0000;

/// Dress gpui-component in the Dark Ops palette.
///
/// The kit's own widgets — the context menu, popovers, inputs and scrollbars —
/// otherwise paint its stock dark theme, which does not match `tokens.css`.
/// The kit sizes everything in rems against `font_size`, so 13 px puts its
/// small text at the design's ~11.5 px; the shell's own views are sized in
/// pixels and are unaffected.
pub fn apply_to_components(cx: &mut gpui_kit::App) {
    use gpui_kit::component::Theme;

    let palette = Palette::dark();
    let theme = Theme::global_mut(cx);
    theme.font_family = SANS.into();
    theme.mono_font_family = MONO.into();
    theme.font_size = gpui_kit::px(13.);
    theme.mono_font_size = gpui_kit::px(11.);
    theme.radius = gpui_kit::px(4.);
    theme.radius_lg = gpui_kit::px(6.);
    // The design has no shadows: 1 px borders instead (design README §tokens).
    theme.shadow = false;

    let colors = &mut theme.colors;
    colors.background = color(palette.app).into();
    colors.foreground = color(palette.text_primary).into();
    colors.border = color(palette.border_strong).into();
    colors.muted = color(palette.panel).into();
    colors.muted_foreground = color(palette.text_muted).into();
    colors.popover = color(palette.panel).into();
    colors.popover_foreground = color(palette.text_primary).into();
    // The kit's `accent` is a hover/highlight background, not a brand colour.
    colors.accent = color(palette.selected).into();
    colors.accent_foreground = color(palette.text_primary).into();
    colors.primary = color(palette.accent_cyan).into();
    colors.primary_hover = color(palette.accent_cyan_bright).into();
    colors.primary_active = color(palette.accent_cyan).into();
    colors.primary_foreground = color(palette.app).into();
    colors.secondary = color(palette.panel).into();
    colors.secondary_hover = color(palette.selected).into();
    colors.secondary_active = color(palette.selected).into();
    colors.secondary_foreground = color(palette.text_primary).into();
    colors.danger = color(palette.accent_red).into();
    colors.danger_hover = color(palette.danger_border).into();
    colors.danger_active = color(palette.danger_border).into();
    colors.danger_foreground = color(palette.danger_text).into();
    colors.input = color(palette.field).into();
    colors.ring = color(palette.accent_cyan).into();
    colors.caret = color(palette.accent_cyan).into();
    colors.selection = color(palette.selected).into();
    colors.list = color(palette.panel).into();
    colors.list_hover = color(palette.selected).into();
    colors.list_active = color(palette.selected).into();
    colors.list_active_border = color(palette.accent_cyan).into();
    colors.list_even = color(palette.row_alt).into();
    colors.table = color(palette.app).into();
    colors.table_head = color(palette.panel).into();
    colors.table_head_foreground = color(palette.text_dim).into();
    colors.table_hover = color(palette.selected).into();
    colors.table_active = color(palette.selected).into();
    colors.table_even = color(palette.row_alt).into();
    colors.table_row_border = color(palette.row_line).into();
    colors.title_bar = color(palette.panel).into();
    colors.title_bar_border = color(palette.border_mid).into();
    colors.status_bar = color(palette.panel).into();
    colors.status_bar_border = color(palette.border_mid).into();
    colors.sidebar = color(palette.panel).into();
    colors.sidebar_border = color(palette.border_mid).into();
    colors.sidebar_foreground = color(palette.text_body).into();
    colors.tab_bar = color(palette.panel).into();
    colors.tab = color(palette.panel).into();
    colors.tab_active = color(palette.app).into();
    colors.tab_active_foreground = color(palette.text_primary).into();
    colors.tab_foreground = color(palette.text_muted).into();
    colors.progress_bar = color(palette.accent_cyan).into();
    colors.scrollbar = color(palette.app).into();
    colors.scrollbar_thumb = color(palette.border_strong).into();
    colors.scrollbar_thumb_hover = color(palette.text_dim).into();
    colors.switch = color(palette.accent_cyan).into();
    colors.switch_thumb = color(palette.text_primary).into();
    colors.window_border = color(palette.border_strong).into();
    colors.overlay = color(SCRIM).into();
}

/// Build a text element using the shell's bundled sans face.
pub fn text(content: impl Into<gpui_kit::SharedString>, size: f32, colour: Rgba) -> gpui_kit::Div {
    gpui_kit::div()
        .font_family(SANS)
        .text_size(gpui_kit::px(size))
        .line_height(gpui_kit::px(size + 3.))
        .text_color(colour)
        .child(content.into())
}

/// Build compact metadata text in the bundled mono face.
pub fn mono(content: impl Into<gpui_kit::SharedString>, size: f32, colour: Rgba) -> gpui_kit::Div {
    gpui_kit::div()
        .font_family(MONO)
        .text_size(gpui_kit::px(size))
        .line_height(gpui_kit::px(size + 3.))
        .text_color(colour)
        .child(content.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blend_moves_each_channel_by_the_ratio() {
        // The ends are exact, so a full column paints the colour and an empty
        // one paints the trough.
        assert_eq!(blend(0x0010_2030, 0x00f0_e0d0, 0.0), 0x0010_2030);
        assert_eq!(blend(0x0010_2030, 0x00f0_e0d0, 1.0), 0x00f0_e0d0);
        // Halfway is the midpoint of every channel, not of the number.
        assert_eq!(blend(0x0000_0000, 0x00ff_ff00, 0.5), 0x0080_8000);
        // Out-of-range ratios are clamped rather than wrapping a channel.
        assert_eq!(blend(0x0000_0000, 0x00ff_ffff, 2.0), 0x00ff_ffff);
        assert_eq!(blend(0x0000_0000, 0x00ff_ffff, -1.0), 0x0000_0000);
    }

    #[test]
    fn status_colours_come_from_the_token_map() {
        use rtorrent_core::types::Status;
        let palette = Palette::dark();
        assert_eq!(
            palette.status_fill(Status::Downloading),
            palette.accent_cyan
        );
        assert_eq!(palette.status_fill(Status::Seeding), palette.accent_green);
        assert_eq!(palette.status_text(Status::Error), palette.accent_red);
        assert_eq!(
            palette.status_text(Status::Checking),
            palette.accent_cyan_bright
        );
    }

    #[test]
    fn tag_colour_is_stable_and_case_insensitive() {
        assert_eq!(tag_colour("Linux"), tag_colour("linux"));
        assert_ne!(tag_colour("linux"), tag_colour("iso"));
        // The web client's `tagColour` hashes identically, so both shells agree.
        assert_eq!(tag_colour("linux"), 0x00d0_8989);
        assert_eq!(tag_colour("iso"), 0x008f_bf76);
    }
}
