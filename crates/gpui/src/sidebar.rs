//! The filter sidebar: status / label / tracker / view rows with global
//! counts. Clicking a row sets the active filter; clicking the active row
//! again clears back to all.

use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, Context, Div, EventEmitter, MouseButton, MouseDownEvent, SharedString, Stateful,
    Window,
};

use crate::model::TorrentsModel;
use crate::table_state::{Filter, SidebarCounts, StatusKey};
use crate::theme::{self, Palette};

/// Events the shell subscribes to from a [`FilterSidebar`] entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterSidebarEvent {
    /// The active filter changed, so the shell persists the view.
    FilterChanged,
}

pub const ERROR_LABELS: [(&str, &str); 8] = [
    ("unregistered", "unregistered"),
    ("tracker_timeout", "timeout"),
    ("tracker_error", "trk error"),
    ("missing_files", "missing files"),
    ("no_space", "no space"),
    ("permission", "permission"),
    ("disk_error", "disk error"),
    ("other", "other"),
];

const STATUS_ROWS: [(Option<StatusKey>, &str); 7] = [
    (None, "all"),
    (Some(StatusKey::Downloading), "downloading"),
    (Some(StatusKey::Seeding), "seeding"),
    (Some(StatusKey::Completed), "completed"),
    (Some(StatusKey::Paused), "paused"),
    (Some(StatusKey::Stalled), "stalled"),
    (Some(StatusKey::Error), "error"),
];

fn status_count(counts: &SidebarCounts, key: Option<StatusKey>) -> usize {
    match key {
        None => counts.all,
        Some(StatusKey::Downloading) => counts.downloading,
        Some(StatusKey::Seeding) => counts.seeding,
        Some(StatusKey::Completed) => counts.completed,
        Some(StatusKey::Paused) => counts.paused,
        Some(StatusKey::Stalled) => counts.stalled,
        Some(StatusKey::Checking) => 0,
        Some(StatusKey::Error) => counts.error,
    }
}

pub struct FilterSidebar {
    model: gpui_kit::Entity<TorrentsModel>,
}

/// One sidebar row: what it is called, what it counts, and what a click sets.
struct Row {
    id: SharedString,
    label: String,
    count: usize,
    active: bool,
    filter: Filter,
    /// A colour square before the label (tags only, V3-10).
    dot: Option<u32>,
}

impl EventEmitter<FilterSidebarEvent> for FilterSidebar {}

impl FilterSidebar {
    pub fn new(model: gpui_kit::Entity<TorrentsModel>) -> Self {
        Self { model }
    }

    fn choose(&mut self, next: Filter, cx: &mut Context<Self>) {
        let active = self.model.read(cx).filter().clone();
        let cleared = active == next;
        self.model.update(cx, |model, cx| {
            model.set_filter(if cleared { Filter::All } else { next }, cx);
        });
        cx.emit(FilterSidebarEvent::FilterChanged);
        cx.notify();
    }

    fn section(title: &str, palette: Palette) -> Div {
        div()
            .flex_none()
            .px(px(10.))
            .pt(px(10.))
            .pb(px(2.))
            .child(theme::mono(title, 9.5, theme::color(palette.text_dim)))
    }

    fn row(&mut self, row: Row, palette: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let Row {
            id,
            label,
            count,
            active,
            filter,
            dot,
        } = row;
        div()
            .id(id)
            .h(px(22.))
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(10.))
            .rounded(px(4.))
            .cursor_pointer()
            .when(active, |row| row.bg(theme::color(palette.selected)))
            .when(!active, |row| {
                row.hover(|style| style.bg(theme::color(palette.selected)))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.choose(filter.clone(), cx);
                    cx.stop_propagation();
                }),
            )
            .when_some(dot, |row, colour| {
                row.child(
                    div()
                        .w(px(7.))
                        .h(px(7.))
                        .flex_none()
                        .rounded(px(2.))
                        .bg(theme::color(colour)),
                )
            })
            .child(
                theme::text(label, 11.5, theme::color(palette.text_body))
                    .flex_1()
                    .min_w_0()
                    .truncate(),
            )
            .child(theme::mono(
                count.to_string(),
                10.,
                theme::color(palette.text_muted),
            ))
    }
}

impl gpui_kit::Render for FilterSidebar {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let palette = Palette::dark();
        let filter = self.model.read(cx).filter().clone();
        let counts = crate::table_state::sidebar_counts(self.model.read(cx).torrents());
        let mut body = div()
            .id("filter-sidebar")
            .size_full()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .bg(theme::color(palette.panel))
            .child(Self::section("STATUS", palette));
        for (key, label) in STATUS_ROWS {
            let candidate = match key {
                None => Filter::All,
                Some(status) => Filter::Status(status),
            };
            let count = status_count(&counts, key);
            let active = filter == candidate;
            body = body.child(self.row(
                Row {
                    id: SharedString::from(format!("sidebar-status-{label}")),
                    label: label.to_owned(),
                    count,
                    active,
                    filter: candidate,
                    dot: None,
                },
                palette,
                cx,
            ));
        }
        if !counts.errors.is_empty() {
            body = body.child(Self::section("ERRORS", palette));
            for (value, count) in &counts.errors {
                let label = ERROR_LABELS
                    .iter()
                    .find(|(key, _)| key == value)
                    .map_or(value.as_str(), |(_, label)| label);
                let candidate = Filter::ErrorKind(value.clone());
                let active = filter == candidate;
                body = body.child(self.row(
                    Row {
                        id: SharedString::from(format!("sidebar-error-{value}")),
                        label: label.to_owned(),
                        count: *count,
                        active,
                        filter: candidate,
                        dot: None,
                    },
                    palette,
                    cx,
                ));
            }
        }
        if !counts.labels.is_empty() {
            body = body.child(Self::section("LABELS", palette));
            for (value, count) in &counts.labels {
                let candidate = Filter::Label(value.clone());
                let active = filter == candidate;
                body = body.child(self.row(
                    Row {
                        id: SharedString::from(format!("sidebar-label-{value}")),
                        label: value.clone(),
                        count: *count,
                        active,
                        filter: candidate,
                        dot: None,
                    },
                    palette,
                    cx,
                ));
            }
        }
        if !counts.tags.is_empty() {
            body = body.child(Self::section("TAGS", palette));
            for (value, count) in &counts.tags {
                let candidate = Filter::Tag(value.clone());
                let active = filter == candidate;
                body = body.child(self.row(
                    Row {
                        id: SharedString::from(format!("sidebar-tag-{value}")),
                        label: value.clone(),
                        count: *count,
                        active,
                        filter: candidate,
                        dot: Some(theme::tag_colour(value)),
                    },
                    palette,
                    cx,
                ));
            }
        }
        if !counts.trackers.is_empty() {
            body = body.child(Self::section("TRACKERS", palette));
            for (value, count) in &counts.trackers {
                let candidate = Filter::Tracker(value.clone());
                let active = filter == candidate;
                body = body.child(self.row(
                    Row {
                        id: SharedString::from(format!("sidebar-tracker-{value}")),
                        label: value.clone(),
                        count: *count,
                        active,
                        filter: candidate,
                        dot: None,
                    },
                    palette,
                    cx,
                ));
            }
        }
        if !counts.views.is_empty() {
            body = body.child(Self::section("VIEWS", palette));
            for (value, count) in &counts.views {
                let candidate = Filter::View(value.clone());
                let active = filter == candidate;
                body = body.child(self.row(
                    Row {
                        id: SharedString::from(format!("sidebar-view-{value}")),
                        label: value.clone(),
                        count: *count,
                        active,
                        filter: candidate,
                        dot: None,
                    },
                    palette,
                    cx,
                ));
            }
        }
        body
    }
}
