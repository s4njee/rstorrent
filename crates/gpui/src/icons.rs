//! The design's line-icon set, as embedded SVG.
//!
//! A port of `src/components/icons/index.tsx`: 12×12 viewBox, 1.6 px stroke,
//! `currentColor` so the caller tints each glyph, and no emoji or icon font.
//! The design's toolbar and menu glyphs are hand-drawn rather than taken from a
//! catalogue, so they are carried across verbatim instead of mapped onto
//! `gpui-kit`'s Lucide set.
//!
//! Glyphs are raw `svg` elements, not gpui-component `Icon`s: an `Icon` captures
//! its colour when it renders, so a button could not brighten its glyph on
//! hover.

use gpui_kit::prelude::*;
use gpui_kit::{div, px, Div, Rgba, Stateful, Svg};

/// A 12×12 line icon: the shared wrapper from the TypeScript set.
macro_rules! icon {
    ($name:ident, $body:literal, stroke) => {
        pub const $name: &[u8] = concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.6">"#,
            $body,
            "</svg>"
        )
        .as_bytes();
    };
    ($name:ident, $body:literal, fill) => {
        pub const $name: &[u8] = concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 12 12" fill="currentColor" stroke="none">"#,
            $body,
            "</svg>"
        )
        .as_bytes();
    };
}

icon!(ADD, r#"<path d="M6 1v10M1 6h10"/>"#, stroke);
icon!(
    MAGNET,
    r#"<path d="M2.5 1v5a3.5 3.5 0 0 0 7 0V1"/><path d="M1 1.5h3M8 1.5h3"/>"#,
    stroke
);
icon!(REMOVE, r#"<path d="M1 6h10"/>"#, stroke);
icon!(PLAY, r#"<polygon points="3,1.5 10.5,6 3,10.5"/>"#, fill);
icon!(
    PAUSE,
    r#"<rect x="2.5" y="1.5" width="2.6" height="9"/><rect x="7" y="1.5" width="2.6" height="9"/>"#,
    fill
);
icon!(UP, r#"<path d="M6 10.5V2M2.8 5.2 6 2l3.2 3.2"/>"#, stroke);
icon!(
    TOP,
    r#"<path d="M6 10.5V4M2.8 7 6 4l3.2 3M2.8 3.4 6 .4l3.2 3"/>"#,
    stroke
);
icon!(
    DOWN,
    r#"<path d="M6 1.5V10M2.8 6.8 6 10l3.2-3.2"/>"#,
    stroke
);
icon!(
    BOTTOM,
    r#"<path d="M6 1.5V8M2.8 5 6 8l3.2-3M2.8 8.6 6 11.6l3.2-3"/>"#,
    stroke
);
icon!(
    RATE_LIMIT,
    r#"<path d="M1.5 3h9M1.5 9h9"/><path d="m3.5 1.5-2 1.5 2 1.5M8.5 7.5l2 1.5-2 1.5"/>"#,
    stroke
);
icon!(
    RECHECK,
    r#"<path d="M10 3.5A4.5 4.5 0 1 0 10.5 7"/><path d="M10.5 1.5V4H8"/>"#,
    stroke
);
icon!(
    LABEL,
    r#"<path d="M1.5 1.5h5l4 4-5 5-4-4z"/><circle cx="3.6" cy="3.6" r="0.7" fill="currentColor" stroke="none"/>"#,
    stroke
);
icon!(FOLDER, r#"<path d="M1 3h3l1 1.2h6V10H1z"/>"#, stroke);
icon!(
    LINK,
    r#"<path d="M4.5 7.5 7.5 4.5"/><path d="M5 2.5 6.5 1a2 2 0 0 1 2.8 2.8L7.8 5.3"/><path d="M7 9.5 5.5 11a2 2 0 0 1-2.8-2.8L4.2 6.7"/>"#,
    stroke
);
icon!(
    OPEN,
    r#"<path d="M4.5 1.5H1.5v9h9v-3"/><path d="M7 1.5h3.5V5M10.5 1.5 5.5 6.5"/>"#,
    stroke
);
icon!(CLOSE, r#"<path d="M2 2l8 8M10 2l-8 8"/>"#, stroke);
icon!(
    CHEVRON_RIGHT,
    r#"<path d="M4.5 2.5 8 6l-3.5 3.5"/>"#,
    stroke
);
icon!(
    CREATE_TORRENT,
    r#"<path d="M2 1.5h5l3 3V10.5H2z"/><path d="M7 1.5v3h3"/><path d="M4 7h4M6 5v4"/>"#,
    stroke
);
icon!(
    COLUMNS,
    r#"<path d="M1.5 2.5h9v7h-9z"/><path d="M4.5 2.5v7M7.75 2.5v7"/>"#,
    stroke
);
icon!(CHECK, r#"<path d="m2 6.5 3 3 5-7"/>"#, stroke);

/// A glyph at `size`, tinted by the caller.
pub fn glyph(icon: &'static [u8], size: f32, color: Rgba) -> Svg {
    gpui_kit::svg()
        .data(icon)
        .size(px(size))
        .flex_none()
        .text_color(color)
}

/// A 26×24 toolbar button: a glyph that brightens with its background.
///
/// `toolbar_button` returns the button without a click handler so callers can
/// attach one, and paints the disabled state from `enabled` — the design greys
/// both the glyph and the hover background rather than hiding the control.
pub fn toolbar_button(
    id: &'static str,
    icon: &'static [u8],
    enabled: bool,
    palette: crate::theme::Palette,
) -> Stateful<Div> {
    use crate::theme::{self, geometry};

    let glyph_color = if enabled {
        palette.text_body
    } else {
        palette.text_dim
    };
    let hovered = palette.selected;
    let mut button = div()
        .id(id)
        .h(px(geometry::TOOLBAR_BUTTON_H))
        .min_w(px(geometry::TOOLBAR_BUTTON))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .child(glyph(icon, 13., theme::color(glyph_color)));
    if enabled {
        button = button
            .cursor_pointer()
            .hover(move |style| style.bg(theme::color(hovered)));
    }
    button
}

/// A fixed-width separator between toolbar groups.
pub fn separator(palette: crate::theme::Palette) -> Div {
    use crate::theme;

    div()
        .w(px(1.))
        .h(px(16.))
        .flex_none()
        .bg(theme::color(palette.border_mid))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every glyph must be a parseable, non-empty SVG with the shared viewBox;
    /// a malformed literal would otherwise paint nothing at runtime.
    #[test]
    fn every_glyph_is_a_12x12_svg() {
        let all: [(&str, &[u8]); 20] = [
            ("add", ADD),
            ("magnet", MAGNET),
            ("remove", REMOVE),
            ("play", PLAY),
            ("pause", PAUSE),
            ("up", UP),
            ("top", TOP),
            ("down", DOWN),
            ("bottom", BOTTOM),
            ("rate-limit", RATE_LIMIT),
            ("recheck", RECHECK),
            ("label", LABEL),
            ("folder", FOLDER),
            ("link", LINK),
            ("open", OPEN),
            ("close", CLOSE),
            ("chevron-right", CHEVRON_RIGHT),
            ("create-torrent", CREATE_TORRENT),
            ("columns", COLUMNS),
            ("check", CHECK),
        ];
        for (name, bytes) in all {
            let svg = std::str::from_utf8(bytes).expect("glyph is UTF-8");
            assert!(svg.starts_with("<svg"), "{name} is not an SVG");
            assert!(svg.ends_with("</svg>"), "{name} is not closed");
            assert!(svg.contains(r#"viewBox="0 0 12 12""#), "{name} viewBox");
            assert!(svg.contains("currentColor"), "{name} is not tintable");
        }
    }
}
