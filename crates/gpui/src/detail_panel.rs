//! The bottom detail panel: the focused torrent's files, peers, trackers,
//! transfer, pieces and log, under the table.
//!
//! Its own entity, reading a payload the model keeps in its own field, so the
//! detail poll can never re-render the table. A port of the Tauri shell's
//! `src/components/details/`: what the daemon is asked for lives in
//! [`crate::detail_panes`], the rail's figures in [`crate::detail_facts`], and
//! the tree's shape in [`crate::detail_files`].

use std::collections::{HashMap, HashSet};

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};
use gpui_kit::component::Sizable;
use gpui_kit::prelude::*;
use gpui_kit::{
    canvas, div, point, px, rgba, AnyElement, Bounds, Context, Div, Entity, MouseButton,
    MouseDownEvent, PathBuilder, SharedString, Stateful, Subscription, Window,
};
use rtorrent_core::types::{
    DetailPayload, FileNode, GlobalStats, LogEntry, LogLevel, Status, TorrentDto,
};

use crate::actions;
use crate::detail_facts::{self, Tone};
use crate::detail_files::{self, TreeNode};
use crate::detail_panes::{Pane, PANES};
use crate::detail_pieces;
use crate::detail_rows;
use crate::format;
use crate::model::TorrentsModel;
use crate::rate_history::{self, RatePoint};
use crate::theme::{self, geometry, Palette};

/// How many columns the piece stripes are drawn with.
///
/// One column per few pixels of stripe: enough to read the shape of a torrent,
/// few enough that the row is a couple of hundred elements rather than the
/// hundreds of thousands of chunks behind it.
const PIECE_COLUMNS: usize = 220;

/// The Transfer pane's chart height.
const CHART_HEIGHT: f32 = 96.;

/// The panel.
pub struct DetailPanel {
    model: Entity<TorrentsModel>,
    palette: Palette,
    subscriptions: Vec<Subscription>,
    /// The torrent the per-subject state below belongs to. Switching rows resets
    /// it rather than carrying a selection or an open folder over.
    subject: Option<String>,
    /// Folders the tree has open, by path.
    expanded: HashSet<String>,
    /// The file rows selected in the tree, and the fixed end Shift extends from.
    file_selection: HashSet<usize>,
    file_anchor: Option<usize>,
    /// Priority overrides awaiting a poll that agrees with them, so a click shows
    /// at once without pretending the daemon has answered.
    pending: HashMap<usize, i64>,
    /// The leaves a file menu was opened over, recorded before the menu opens: a
    /// context menu carries no target of its own.
    file_menu_target: Vec<usize>,
    /// The peer and tracker rows their menus were opened over.
    peer_target: Option<String>,
    tracker_target: Option<usize>,
    /// The Trackers pane's announce-URL field.
    tracker_url: Entity<InputState>,
}

impl DetailPanel {
    pub fn new(model: Entity<TorrentsModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut panel = Self {
            model,
            palette: Palette::dark(),
            subscriptions: Vec::new(),
            subject: None,
            expanded: HashSet::new(),
            file_selection: HashSet::new(),
            file_anchor: None,
            pending: HashMap::new(),
            file_menu_target: Vec::new(),
            peer_target: None,
            tracker_target: None,
            tracker_url: cx.new(|cx| InputState::new(window, cx).placeholder("announce URL…")),
        };
        // The model notifies on every poll tick and selection change; the panel
        // follows it without the shell having to remember to relay.
        let model = panel.model.clone();
        panel
            .subscriptions
            .push(cx.observe_in(&model, window, |_panel, _, _, cx| {
                cx.notify();
            }));
        // Enter adds the URL, as the Tauri pane's form does.
        let url = panel.tracker_url.clone();
        panel.subscriptions.push(cx.subscribe_in(
            &url,
            window,
            |panel, _input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    panel.submit_tracker(window, cx);
                }
            },
        ));
        panel
    }

    /// Reset the per-subject state when the panel's subject changes.
    fn sync_subject(&mut self, subject: Option<&str>) {
        if self.subject.as_deref() == subject {
            return;
        }
        self.subject = subject.map(str::to_owned);
        self.expanded.clear();
        self.file_selection.clear();
        self.file_anchor = None;
        self.pending.clear();
        self.file_menu_target.clear();
        self.peer_target = None;
        self.tracker_target = None;
    }

    // --- The rows' verbs ----------------------------------------------------
    //
    // The shell routes the menus' actions here, because a menu item cannot carry
    // the row it was opened over.

    /// Record what a file menu acts on, and open it: the node's leaves, or the
    /// part of the selection inside it. A node that was not already selected
    /// replaces the selection, which is what the Tauri tree does on right-click.
    fn open_file_menu(&mut self, indexes: &[usize], cx: &mut Context<Self>) {
        let selected = indexes
            .first()
            .is_some_and(|index| self.file_selection.contains(index));
        self.file_menu_target = if selected {
            indexes
                .iter()
                .copied()
                .filter(|index| self.file_selection.contains(index))
                .collect()
        } else {
            self.file_selection = indexes.iter().copied().collect();
            self.file_anchor = indexes.first().copied();
            indexes.to_vec()
        };
        cx.notify();
    }

    /// Apply a priority to the leaves a file menu was opened over.
    pub fn set_file_menu_priority(&mut self, priority: i64, cx: &mut Context<Self>) {
        let indexes = self.file_menu_target.clone();
        self.set_priorities(&indexes, priority, cx);
    }

    pub fn act_on_peer(&mut self, verb: crate::services::PeerVerb, cx: &mut Context<Self>) {
        let (Some(hash), Some(peer)) = (self.subject.clone(), self.peer_target.clone()) else {
            return;
        };
        self.model
            .update(cx, |model, cx| model.peer_action(hash, peer, verb, cx));
    }

    /// Flip the tracker row the menu was opened over.
    pub fn toggle_tracker(&mut self, cx: &mut Context<Self>) {
        let (Some(hash), Some(index)) = (self.subject.clone(), self.tracker_target) else {
            return;
        };
        let enabled = self
            .model
            .read(cx)
            .detail()
            .and_then(|payload| payload.trackers.as_ref())
            .and_then(|rows| rows.iter().find(|row| row.index == index))
            .is_some_and(|row| !row.enabled);
        self.model.update(cx, |model, cx| {
            model.set_tracker_enabled(hash, index, enabled, cx);
        });
    }

    pub fn remove_tracker(&mut self, cx: &mut Context<Self>) {
        let (Some(hash), Some(index)) = (self.subject.clone(), self.tracker_target) else {
            return;
        };
        self.model
            .update(cx, |model, cx| model.remove_tracker(hash, index, cx));
    }

    /// Add an announce URL to the focused torrent.
    pub fn add_tracker(&mut self, url: String, cx: &mut Context<Self>) {
        let Some(hash) = self.subject.clone() else {
            return;
        };
        self.model
            .update(cx, |model, cx| model.add_tracker(hash, url, cx));
    }

    /// Announce the focused torrent now.
    pub fn reannounce(&mut self, cx: &mut Context<Self>) {
        let Some(hash) = self.subject.clone() else {
            return;
        };
        self.model
            .update(cx, |model, cx| model.reannounce(hash, cx));
    }

    fn set_priorities(&mut self, indexes: &[usize], priority: i64, cx: &mut Context<Self>) {
        let Some(hash) = self.subject.clone() else {
            return;
        };
        if indexes.is_empty() {
            return;
        }
        for index in indexes {
            self.pending.insert(*index, priority);
        }
        self.model.update(cx, |model, cx| {
            for index in indexes {
                model.set_file_priority(hash.clone(), *index, priority, cx);
            }
        });
        cx.notify();
    }

    /// Drop overrides the daemon has caught up with, so the poll is the truth
    /// again. Done on the render path rather than mid-poll.
    fn prune_pending(&mut self, files: &[FileNode]) {
        self.pending.retain(|index, priority| {
            files
                .get(*index)
                .is_some_and(|file| file.priority != *priority)
        });
        self.file_selection.retain(|index| *index < files.len());
    }

    /// The priority a leaf row shows: the override while one is in flight, else
    /// what the daemon last said.
    fn effective_priority(&self, index: usize, actual: i64) -> i64 {
        self.pending.get(&index).copied().unwrap_or(actual)
    }
}

impl gpui_kit::Render for DetailPanel {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let palette = self.palette;
        // Copied out of the model before anything below needs `cx` mutably. The
        // panel re-renders on notify rather than per frame, so this is a handful
        // of clones a second, not a budget.
        let (subject, pane, payload, globals, samples, entries) = {
            let model = self.model.read(cx);
            let subject = model.focused_torrent().cloned();
            // The chart's samples are copied for the same reason as the payload:
            // the model borrow cannot outlive a call to `cx.listener`.
            let samples = subject.as_ref().map_or_else(Vec::new, |torrent| {
                model.rates().get(&torrent.hash).to_vec()
            });
            let entries: Vec<LogEntry> = model.log().entries().cloned().collect();
            (
                subject,
                model.active_tab(),
                model.detail().cloned(),
                model.globals().clone(),
                samples,
                entries,
            )
        };
        self.sync_subject(subject.as_ref().map(|torrent| torrent.hash.as_str()));
        if let Some(files) = payload
            .as_ref()
            .and_then(|payload| payload.files.as_deref())
        {
            self.prune_pending(files);
        }

        let body = match &subject {
            None => {
                let selection = self.model.read(cx).selection().hashes.len();
                placeholder(
                    if selection > 1 {
                        "select a torrent to see its details"
                    } else {
                        "select a torrent to see its files, peers and trackers"
                    },
                    palette,
                )
            }
            Some(torrent) => {
                let rules = self
                    .model
                    .read(cx)
                    .services()
                    .settings()
                    .bandwidth_rules
                    .clone();
                let mut row = div().flex_1().min_h_0().flex();
                row = row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .flex_col()
                        .child(self.error_banner(torrent, palette))
                        .child(self.pane_content(
                            pane,
                            torrent,
                            payload.as_ref(),
                            &samples,
                            &entries,
                            cx,
                        )),
                );
                if pane.has_rail() {
                    row =
                        row.child(self.rail(torrent, payload.as_ref(), &globals, &rules, palette));
                }
                row.into_any_element()
            }
        };

        div()
            .id("detail-panel")
            .h(px(geometry::DETAIL))
            .flex_none()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(theme::color(palette.border_mid))
            .bg(theme::color(palette.app))
            .child(self.tab_strip(subject.as_ref(), pane, cx))
            .child(body)
    }
}

impl DetailPanel {
    /// The tab strip, with the focused torrent named on the right.
    fn tab_strip(
        &self,
        subject: Option<&TorrentDto>,
        active: Pane,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let palette = self.palette;
        let mut strip = div()
            .id("detail-tabs")
            .h(px(geometry::DETAIL_TABS))
            .flex_none()
            .flex()
            .items_stretch()
            .gap(px(14.))
            .px(px(12.))
            .bg(theme::color(palette.panel))
            .border_b_1()
            .border_color(theme::color(palette.border_mid));

        for pane in PANES {
            let selected = pane == active;
            // Text tabs with an accent underline: the shell's own idiom rather
            // than the design handoff's filled tabs, which the Tauri shell took.
            strip = strip.child(
                div()
                    .id(SharedString::from(format!("detail-tab-{}", pane.key())))
                    .h_full()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |panel, _, _, cx| {
                            panel
                                .model
                                .update(cx, |model, cx| model.set_active_tab(pane, cx));
                        }),
                    )
                    .child(
                        theme::text(
                            pane.label(),
                            11.5,
                            theme::color(if selected {
                                palette.accent_cyan_bright
                            } else {
                                palette.text_dim
                            }),
                        )
                        .px(px(2.))
                        .pb(px(5.)),
                    )
                    .child(div().h(px(2.)).w_full().bg(if selected {
                        theme::color(palette.accent_cyan)
                    } else {
                        rgba(0x0000_0000)
                    })),
            );
        }

        strip.child(div().flex_1()).child(
            theme::text(
                subject.map_or_else(String::new, |torrent| torrent.name.clone()),
                11.,
                theme::color(palette.text_dim),
            )
            .max_w(px(420.))
            .truncate()
            .min_w_0(),
        )
    }

    /// Why the torrent is failing, wherever the panel happens to be.
    fn error_banner(&self, torrent: &TorrentDto, palette: Palette) -> Div {
        let mut banner = div().flex_none();
        if torrent.status != Status::Error || torrent.status_msg.is_empty() {
            return banner;
        }
        banner = banner
            .mx(px(10.))
            .mt(px(8.))
            .px(px(9.))
            .py(px(6.))
            .rounded(px(4.))
            .border_1()
            .border_color(theme::color(palette.danger_border))
            .bg(theme::color(palette.danger_bg))
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(theme::text(
                torrent.status_msg.clone(),
                11.,
                theme::color(palette.danger_text),
            ));
        if let Some(hint) = error_hint(&torrent.error_kind) {
            banner = banner.child(theme::text(hint, 10.5, theme::color(palette.text_muted)));
        }
        banner
    }

    /// The facts rail: the handoff's eleven, then ours, behind a divider.
    fn rail(
        &self,
        torrent: &TorrentDto,
        payload: Option<&DetailPayload>,
        globals: &GlobalStats,
        rules: &[rtorrent_core::bandwidth::BandwidthRule],
        palette: Palette,
    ) -> Stateful<Div> {
        let pieces = payload.and_then(|payload| payload.pieces.as_ref());
        let mut rail = div()
            .id("detail-rail")
            .w(px(geometry::DETAIL_RAIL))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .gap(px(3.))
            .px(px(10.))
            .py(px(8.))
            .border_l_1()
            .border_color(theme::color(palette.border_mid))
            .bg(theme::color(palette.panel))
            .overflow_y_scroll();

        for fact in detail_facts::primary(torrent, pieces) {
            rail = rail.child(fact_row(&fact, palette));
        }
        rail = rail.child(
            div()
                .h(px(1.))
                .flex_none()
                .my(px(2.))
                .bg(theme::color(palette.border_mid)),
        );
        for fact in detail_facts::extra(torrent, globals, rules) {
            rail = rail.child(fact_row(&fact, palette));
        }
        rail
    }

    /// The pane in view.
    fn pane_content(
        &self,
        pane: Pane,
        torrent: &TorrentDto,
        payload: Option<&DetailPayload>,
        samples: &[RatePoint],
        entries: &[LogEntry],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match pane {
            Pane::Files => self.files_pane(payload, cx).into_any_element(),
            Pane::Peers => self.peers_pane(payload, cx).into_any_element(),
            Pane::Trackers => self.trackers_pane(torrent, payload, cx).into_any_element(),
            Pane::Transfer => self.transfer_pane(samples, self.palette).into_any_element(),
            Pane::Pieces => self
                .pieces_pane(torrent, payload, self.palette)
                .into_any_element(),
            Pane::Log => self
                .log_pane(torrent, entries, self.palette)
                .into_any_element(),
        }
    }

    /// The Log pane: what the app has done, newest last, with the focused
    /// torrent's own entries picked out.
    fn log_pane(&self, torrent: &TorrentDto, entries: &[LogEntry], palette: Palette) -> AnyElement {
        if entries.is_empty() {
            return placeholder("no log entries yet", palette);
        }
        let mine = entries
            .iter()
            .filter(|entry| entry.hash.as_deref() == Some(torrent.hash.as_str()))
            .count();

        let mut list = div()
            .id("detail-log")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_y_scroll();

        for (index, entry) in entries.iter().enumerate() {
            let colour = match entry.level {
                LogLevel::Warn => palette.accent_amber,
                LogLevel::Error => palette.accent_red,
                LogLevel::Info => palette.text_body,
            };
            let selected = entry.hash.as_deref() == Some(torrent.hash.as_str());
            list = list.child(
                div()
                    .id(SharedString::from(format!("detail-log-{index}")))
                    .flex_none()
                    .flex()
                    .gap(px(8.))
                    .px(px(10.))
                    .py(px(2.))
                    .when(selected, |row| row.bg(theme::color(palette.selected)))
                    .child(
                        theme::mono(
                            format::clock(entry.time),
                            10.,
                            theme::color(palette.text_dim),
                        )
                        .w(px(58.))
                        .flex_none(),
                    )
                    .child(
                        theme::text(entry.message.clone(), 10.5, theme::color(colour))
                            .flex_1()
                            .min_w_0(),
                    ),
            );
        }

        div()
            .id("detail-log-pane")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(pane_header(
                &format!("Log — {} entries, {mine} for this torrent", entries.len()),
                palette,
            ))
            .child(list)
            .into_any_element()
    }

    /// The Pieces pane: what is on disk, and how well the swarm covers it.
    fn pieces_pane(
        &self,
        torrent: &TorrentDto,
        payload: Option<&DetailPayload>,
        palette: Palette,
    ) -> AnyElement {
        let Some(pieces) = payload.and_then(|payload| payload.pieces.as_ref()) else {
            return placeholder("waiting for the piece map…", palette);
        };
        if pieces.size_chunks <= 0 {
            return placeholder("waiting for the piece map…", palette);
        }

        let bytes = detail_pieces::bytes_from_hex(&pieces.bitfield);
        let fractions = detail_pieces::bucket_fractions(&bytes, pieces.size_chunks, PIECE_COLUMNS);
        let completed = pieces.completed_chunks.max(0);
        let total = pieces.size_chunks.max(1);
        #[allow(clippy::cast_precision_loss)]
        let percent = 100.0 * completed as f64 / total as f64;

        let mut pane = div()
            .id("detail-pieces")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(5.))
            .px(px(10.))
            .py(px(8.))
            .overflow_hidden()
            .child(caption(
                &format!(
                    "pieces: {} / {} · {percent:.1}%",
                    format::count(completed),
                    format::count(total)
                ),
                palette,
            ))
            .child(stripe(
                &fractions,
                1.0,
                palette.status_fill(torrent.status),
                palette,
                10.,
            ));

        // The availability map is optional: a closed torrent does not report one.
        if let Some(counts) = pieces
            .availability
            .as_ref()
            .map(|hex| detail_pieces::bytes_from_hex(hex))
            .filter(|counts| !counts.is_empty())
        {
            let (averages, peak) =
                detail_pieces::bucket_averages(&counts, pieces.size_chunks, PIECE_COLUMNS);
            let lowest = counts.iter().copied().min().unwrap_or(0);
            let highest = counts.iter().copied().max().unwrap_or(0);
            let copies = detail_pieces::distributed_copies(&counts, pieces.size_chunks);
            pane = pane
                .child(caption(
                    &format!("availability: {lowest}–{highest} peers/piece · {copies:.2} copies"),
                    palette,
                ))
                .child(stripe(&averages, peak, palette.accent_violet, palette, 8.));
        }

        pane.child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.))
                .mt(px(3.))
                .child(
                    div()
                        .flex()
                        .gap(px(14.))
                        .child(mini_fact(
                            "Chunks",
                            format!("{} / {}", format::count(completed), format::count(total)),
                            palette,
                        ))
                        .child(mini_fact(
                            "Chunk size",
                            if pieces.chunk_size > 0 {
                                format::bytes(pieces.chunk_size)
                            } else {
                                "—".to_owned()
                            },
                            palette,
                        )),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(14.))
                        .child(mini_fact(
                            "Piece map",
                            format::bytes(pieces.size_chunks * pieces.chunk_size),
                            palette,
                        ))
                        .child(mini_fact(
                            "Distributed copies",
                            format!(
                                "{:.2}",
                                detail_pieces::distributed_copies(
                                    &detail_pieces::bytes_from_hex(
                                        pieces.availability.as_deref().unwrap_or_default()
                                    ),
                                    pieces.size_chunks
                                )
                            ),
                            palette,
                        )),
                ),
        )
        .into_any_element()
    }

    /// The Transfer pane: the focused torrent's rates, from the samples this
    /// session has taken.
    fn transfer_pane(&self, samples: &[RatePoint], palette: Palette) -> AnyElement {
        if samples.len() < 2 {
            return placeholder("collecting speed data…", palette);
        }
        let peak = rate_history::peak(samples);
        let last = samples.last().copied().unwrap_or_default();
        let samples = samples.to_vec();

        // The plot: two filled areas with their strokes over them, and the
        // gridlines underneath. Geometry is computed on the first pass, where
        // the element's bounds are finally known.
        let chart = canvas(
            move |bounds, _window, _cx| {
                let width = bounds.size.width.as_f32();
                let height = bounds.size.height.as_f32();
                let down = rate_history::series_points(&samples, false, width, 0., height, peak);
                let up = rate_history::series_points(&samples, true, width, 0., height, peak);
                (bounds, down, up)
            },
            move |_bounds, (bounds, down, up), window, _cx| {
                let origin = bounds.origin;
                let width = bounds.size.width;
                let height = bounds.size.height;

                for fraction in [0.25_f32, 0.5, 0.75] {
                    let y = origin.y + px(height.as_f32() * fraction);
                    window.paint_quad(gpui_kit::fill(
                        Bounds::new(point(origin.x, y), gpui_kit::size(width, px(1.))),
                        theme::color(palette.row_line),
                    ));
                }

                for (points, colour) in [(&down, palette.accent_cyan), (&up, palette.accent_green)]
                {
                    let (Some(first), Some(last)) = (points.first(), points.last()) else {
                        continue;
                    };
                    let baseline = origin.y + height;

                    let mut area = PathBuilder::fill();
                    area.move_to(point(origin.x + px(first.0), origin.y + px(first.1)));
                    for (x, y) in points.iter().skip(1) {
                        area.line_to(point(origin.x + px(*x), origin.y + px(*y)));
                    }
                    area.line_to(point(origin.x + px(last.0), baseline));
                    area.line_to(point(origin.x + px(first.0), baseline));
                    area.close();
                    if let Ok(path) = area.build() {
                        // The palette is opaque, so the wash is a mix toward the
                        // panel rather than an alpha fill.
                        window.paint_path(
                            path,
                            theme::color(theme::blend(palette.app, colour, 0.22)),
                        );
                    }

                    let mut line = PathBuilder::stroke(px(1.5));
                    line.move_to(point(origin.x + px(first.0), origin.y + px(first.1)));
                    for (x, y) in points.iter().skip(1) {
                        line.line_to(point(origin.x + px(*x), origin.y + px(*y)));
                    }
                    if let Ok(path) = line.build() {
                        window.paint_path(path, theme::color(colour));
                    }
                }
            },
        )
        .w_full()
        .h(px(CHART_HEIGHT))
        .flex_none();

        div()
            .id("detail-transfer")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(10.))
            .py(px(8.))
            .child(chart)
            .child(caption(
                &format!(
                    "↓ {} · ↑ {} · peak {}",
                    format::rate(last.down),
                    format::rate(last.up),
                    format::rate(peak)
                ),
                palette,
            ))
            .into_any_element()
    }

    /// The Peers pane: who the torrent is connected to, and how much they have.
    fn peers_pane(&self, payload: Option<&DetailPayload>, cx: &mut Context<Self>) -> Stateful<Div> {
        let palette = self.palette;
        let peers = payload.and_then(|payload| payload.peers.as_deref());
        let mut pane = div()
            .id("detail-peers")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(pane_header(
                &format!("Peers — {} connected", peers.map_or(0, <[_]>::len)),
                palette,
            ));

        let Some(peers) = peers else {
            return pane.child(placeholder("waiting for the peer list…", palette));
        };
        if peers.is_empty() {
            return pane.child(placeholder("no peers", palette));
        }

        pane = pane.child(peer_header(palette));
        let mut list = div()
            .id("detail-peer-rows")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_y_scroll();

        for (row_index, peer) in peers.iter().enumerate() {
            let (ip, port) = detail_rows::split_address(&peer.address);
            // Copied out for the menu closure, which must own everything it
            // captures — the rows are borrowed from the payload.
            let has_id = !peer.id.is_empty();
            let row = div()
                .id(SharedString::from(format!("detail-peer-{row_index}")))
                .h(px(geometry::DETAIL_ROW))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(10.))
                .border_b_1()
                .border_color(theme::color(palette.row_line))
                .hover(|style| style.bg(theme::color(palette.row_alt)))
                // A row without an id has nothing for rtorrent to act on, so
                // its menu opens with the verbs disabled rather than absent.
                .on_mouse_down(MouseButton::Right, {
                    let id = peer.id.clone();
                    cx.listener(move |panel, _, _, cx| {
                        if !id.is_empty() {
                            panel.peer_target = Some(id.clone());
                        }
                        cx.notify();
                    })
                })
                .context_menu(move |menu, _, _| build_peer_menu(menu, has_id))
                .child(
                    theme::mono(ip, 10.5, theme::color(palette.text_body))
                        .w(px(120.))
                        .flex_none()
                        .truncate(),
                )
                .child(
                    theme::mono(
                        port.unwrap_or_else(|| "—".to_owned()),
                        10.5,
                        theme::color(palette.text_muted),
                    )
                    .w(px(52.))
                    .flex_none()
                    .text_right(),
                )
                .child(
                    theme::text(peer.client.clone(), 10.5, theme::color(palette.text_muted))
                        .flex_1()
                        .min_w_0()
                        .truncate(),
                )
                .child(have_bar(peer.progress, palette))
                .child(rate_cell(peer.down_rate, true, palette))
                .child(rate_cell(peer.up_rate, false, palette))
                .child(
                    theme::mono(
                        if peer.flags.is_empty() {
                            "—".to_owned()
                        } else {
                            peer.flags.clone()
                        },
                        10.,
                        theme::color(palette.text_dim),
                    )
                    .w(px(56.))
                    .flex_none()
                    .text_right(),
                );
            list = list.child(row);
        }
        pane.child(list)
    }

    /// The Trackers pane: the announce endpoints, the two peer sources, and the
    /// controls that manage them.
    fn trackers_pane(
        &self,
        torrent: &TorrentDto,
        payload: Option<&DetailPayload>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let palette = self.palette;
        let trackers = payload.and_then(|payload| payload.trackers.as_deref());
        let mut list = div()
            .id("detail-tracker-rows")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_y_scroll();

        match trackers {
            None => list = list.child(placeholder("waiting for the tracker list…", palette)),
            Some([]) => {
                list = list.child(placeholder("no tracker data", palette));
            }
            Some(trackers) => {
                for tracker in trackers {
                    let tone = detail_rows::tracker_tone(&tracker.status, tracker.enabled);
                    let index = tracker.index;
                    // Owned for the menu closure, which cannot borrow the row.
                    let enabled = tracker.enabled;
                    let selected = self.tracker_target == Some(index);
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("detail-tracker-{index}")))
                            .h(px(geometry::DETAIL_ROW))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .px(px(10.))
                            .border_b_1()
                            .border_color(theme::color(palette.row_line))
                            .when(selected, |row| row.bg(theme::color(palette.selected)))
                            .when(!selected, |row| {
                                row.hover(|style| style.bg(theme::color(palette.row_alt)))
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |panel, _, _, cx| {
                                    panel.tracker_target = Some(index);
                                    cx.notify();
                                }),
                            )
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |panel, _, _, cx| {
                                    panel.tracker_target = Some(index);
                                    cx.notify();
                                }),
                            )
                            .context_menu(move |menu, _, _| build_tracker_menu(menu, enabled))
                            .child(dot(tone, palette))
                            .child(
                                theme::text(
                                    tracker.url.clone(),
                                    10.5,
                                    theme::color(if tracker.enabled {
                                        palette.text_body
                                    } else {
                                        palette.text_dim
                                    }),
                                )
                                .flex_1()
                                .min_w_0()
                                .truncate(),
                            )
                            .child(
                                theme::text(
                                    detail_rows::tracker_word(&tracker.status, tracker.enabled),
                                    10.5,
                                    theme::color(tone_colour(tone, palette)),
                                )
                                .w(px(80.))
                                .flex_none()
                                .text_right(),
                            )
                            .child(
                                theme::mono(
                                    if tracker.enabled {
                                        format!("{} / {}", tracker.seeds, tracker.leeches)
                                    } else {
                                        "—".to_owned()
                                    },
                                    10.,
                                    theme::color(palette.text_muted),
                                )
                                .w(px(70.))
                                .flex_none()
                                .text_right(),
                            )
                            .child(
                                theme::mono(
                                    if tracker.enabled {
                                        format::countdown(
                                            tracker.next_announce,
                                            format::now_seconds(),
                                        )
                                    } else {
                                        "—".to_owned()
                                    },
                                    10.,
                                    theme::color(palette.text_dim),
                                )
                                .w(px(62.))
                                .flex_none()
                                .text_right(),
                            ),
                    );
                }
            }
        }

        // DHT and peer exchange are not trackers, but they are peer sources the
        // design lists with them: on without announcing, so they read in cyan —
        // and off altogether for a private torrent, which turns both off.
        let private = torrent.is_private;
        let tone = if private {
            detail_rows::Tone::Idle
        } else {
            detail_rows::Tone::Enabled
        };
        for name in detail_rows::PEER_SOURCES {
            list = list.child(
                div()
                    .id(SharedString::from(format!("detail-source-{name}")))
                    .h(px(geometry::DETAIL_ROW))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .px(px(10.))
                    .border_b_1()
                    .border_color(theme::color(palette.row_line))
                    .child(dot(tone, palette))
                    .child(
                        theme::text(name.to_owned(), 10.5, theme::color(palette.text_body))
                            .flex_1()
                            .min_w_0(),
                    )
                    .child(
                        theme::text(
                            detail_rows::peer_source_word(private).to_owned(),
                            10.5,
                            theme::color(tone_colour(tone, palette)),
                        )
                        .w(px(80.))
                        .flex_none()
                        .text_right(),
                    )
                    .child(div().w(px(70.)).flex_none())
                    .child(div().w(px(62.)).flex_none()),
            );
        }

        let has_url = !self.tracker_url.read(cx).value().trim().is_empty();
        let footer = div()
            .id("detail-tracker-footer")
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(10.))
            .py(px(6.))
            .border_t_1()
            .border_color(theme::color(palette.border_mid))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .px(px(7.))
                    .rounded(px(3.))
                    .border_1()
                    .border_color(theme::color(palette.border_strong))
                    .bg(theme::color(palette.field))
                    .child(Input::new(&self.tracker_url).appearance(false).small()),
            )
            .child(
                small_button("tracker-add", "Add", has_url, false, palette)
                    .on_click(cx.listener(|panel, _, window, cx| panel.submit_tracker(window, cx))),
            )
            .child(
                small_button(
                    "tracker-reannounce",
                    "Force reannounce",
                    true,
                    false,
                    palette,
                )
                .on_click(cx.listener(|panel, _, _, cx| panel.reannounce(cx))),
            )
            .child(
                small_button(
                    "tracker-remove",
                    "Remove",
                    self.tracker_target.is_some(),
                    true,
                    palette,
                )
                .on_click(cx.listener(|panel, _, _, cx| panel.remove_tracker(cx))),
            );

        div()
            .id("detail-trackers")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(list)
            .child(footer)
    }

    /// Announce-URL submit: the field's Enter or the Add button.
    fn submit_tracker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.tracker_url.clone();
        let text = input.read(cx).value().trim().to_owned();
        if text.is_empty() {
            return;
        }
        self.add_tracker(text, cx);
        input.update(cx, |state, cx| state.set_value("", window, cx));
    }

    /// The Files pane: the torrent's paths as the folders they describe.
    fn files_pane(&self, payload: Option<&DetailPayload>, cx: &mut Context<Self>) -> AnyElement {
        let palette = self.palette;
        let Some(files) = payload.and_then(|payload| payload.files.as_deref()) else {
            return placeholder("waiting for the file list…", palette);
        };
        if files.is_empty() {
            return placeholder("no file data", palette);
        }

        let tree = detail_files::build_tree(files);
        let mut list = div()
            .id("detail-files")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .child(file_header(palette));

        for (path, depth, node) in tree_rows(&tree, 0, "", &self.expanded) {
            let indexes = detail_files::leaf_indexes(node);
            let file_index = node.file_index;
            let priority = file_index.map_or(node.priority, |index| {
                self.effective_priority(index, node.priority)
            });
            let progress = file_index.map_or(node.progress, |index| {
                files.get(index).map_or(node.progress, |file| file.progress)
            });
            let selected = file_index.is_some_and(|index| self.file_selection.contains(&index));
            let is_dir = node.is_dir;
            let is_open = self.expanded.contains(&path);

            let row = div()
                .id(SharedString::from(format!("detail-file-{path}")))
                .h(px(geometry::DETAIL_ROW))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.))
                .pr(px(10.))
                .pl(px(10. + 12.0 * depth as f32))
                .border_b_1()
                .border_color(theme::color(palette.row_line))
                .when(selected, |row| row.bg(theme::color(palette.selected)))
                .when(!selected, |row| {
                    row.hover(|style| style.bg(theme::color(palette.row_alt)))
                })
                .on_mouse_down(MouseButton::Left, {
                    let path = path.clone();
                    let indexes = indexes.clone();
                    cx.listener(move |panel, event, _, cx| {
                        panel.file_mouse_down(&path, &indexes, file_index, event, cx);
                    })
                })
                .on_mouse_down(MouseButton::Right, {
                    let indexes = indexes.clone();
                    cx.listener(move |panel, _, _, cx| {
                        panel.open_file_menu(&indexes, cx);
                    })
                })
                .context_menu(move |menu, _, _| build_file_menu(menu))
                .child(
                    div()
                        .w(px(10.))
                        .flex_none()
                        .flex()
                        .justify_center()
                        .child(theme::text(
                            if is_dir {
                                if is_open {
                                    "▾"
                                } else {
                                    "▸"
                                }
                            } else {
                                "·"
                            },
                            9.,
                            theme::color(palette.text_dim),
                        )),
                )
                .child(
                    theme::text(
                        node.name.clone(),
                        11.,
                        theme::color(if priority == detail_files::PRIORITY_OFF {
                            palette.text_dim
                        } else {
                            palette.text_primary
                        }),
                    )
                    .flex_1()
                    .min_w_0()
                    .truncate(),
                )
                .child(
                    theme::mono(
                        format::bytes(node.size),
                        10.5,
                        theme::color(palette.text_muted),
                    )
                    .w(px(70.))
                    .flex_none()
                    .text_right(),
                )
                .child(div().w(px(90.)).flex_none().child(progress_bar(
                    progress,
                    detail_files::file_status(priority, progress),
                    90.,
                    palette,
                )))
                .child(
                    theme::mono(
                        format::bytes(percent_of(node.size, progress)),
                        10.5,
                        theme::color(palette.text_muted),
                    )
                    .w(px(60.))
                    .flex_none()
                    .text_right(),
                )
                .child(
                    div()
                        .w(px(70.))
                        .flex_none()
                        .flex()
                        .justify_end()
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, {
                            let indexes = indexes.clone();
                            cx.listener(move |panel, _, _, cx| {
                                let next = detail_files::next_priority(priority);
                                panel.set_priorities(&indexes, next, cx);
                            })
                        })
                        .child(priority_chip(priority, palette)),
                );

            list = list.child(row);
        }
        list.into_any_element()
    }

    /// A file row's click: select a leaf, extend with Shift, toggle with Cmd —
    /// or open and shut a folder, which has no single leaf to select.
    fn file_mouse_down(
        &mut self,
        path: &str,
        indexes: &[usize],
        file_index: Option<usize>,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = file_index else {
            let _ = indexes;
            if !self.expanded.remove(path) {
                self.expanded.insert(path.to_owned());
            }
            cx.notify();
            return;
        };

        if event.modifiers.shift {
            if let Some(anchor) = self.file_anchor {
                let (low, high) = if anchor <= index {
                    (anchor, index)
                } else {
                    (index, anchor)
                };
                self.file_selection.extend(low..=high);
            } else {
                self.file_selection.insert(index);
                self.file_anchor = Some(index);
            }
        } else if event.modifiers.secondary() {
            if !self.file_selection.remove(&index) {
                self.file_selection.insert(index);
            }
            self.file_anchor = Some(index);
        } else {
            self.file_selection.clear();
            self.file_selection.insert(index);
            self.file_anchor = Some(index);
        }
        cx.notify();
    }
}

/// The Files pane's column headings.
fn file_header(palette: Palette) -> Stateful<Div> {
    let cell = |label: &str, width: f32, right: bool| {
        let mut cell = theme::text(label.to_owned(), 10., theme::color(palette.text_dim));
        cell = if right {
            cell.w(px(width)).flex_none().text_right()
        } else {
            cell.w(px(width)).flex_none()
        };
        cell
    };
    div()
        .id("detail-files-header")
        .h(px(geometry::DETAIL_HEADER))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(10.))
        .border_b_1()
        .border_color(theme::color(palette.border_mid))
        .child(
            theme::text("File", 10., theme::color(palette.text_dim))
                .flex_1()
                .min_w_0(),
        )
        .child(cell("Size", 70., true))
        .child(cell("Progress", 90., false))
        .child(cell("Done", 60., true))
        .child(cell("Priority", 70., true))
}

/// One rail row: the key, then the value in whatever tone it carries.
fn fact_row(fact: &detail_facts::Fact, palette: Palette) -> Div {
    let colour = match fact.tone {
        Tone::Plain => palette.text_body,
        Tone::Rate => palette.accent_cyan_bright,
        Tone::Up => palette.accent_green,
    };
    div()
        .flex()
        .flex_none()
        .gap(px(8.))
        .child(
            theme::text(fact.key, 11., theme::color(palette.text_dim))
                .w(px(geometry::DETAIL_RAIL_KEY))
                .flex_none(),
        )
        .child(
            theme::text(fact.value.clone(), 11., theme::color(colour))
                .flex_1()
                .min_w_0()
                .truncate(),
        )
}

/// A stripe of `values.len()` columns, each mixed between the trough and the
/// full colour by its share of `scale`.
///
/// A column stands for a slice of the torrent rather than one chunk, so the
/// fraction has to survive into the paint: the mix is that fraction made
/// visible, the way the canvas bars the Tauri shell drew carried it as alpha.
fn stripe(values: &[f64], scale: f64, full: u32, palette: Palette, height: f32) -> Div {
    let mut row = div()
        .w_full()
        .h(px(height))
        .flex_none()
        .flex()
        .rounded(px(1.))
        .bg(theme::color(palette.track))
        .overflow_hidden();
    if scale <= 0.0 {
        return row;
    }
    for value in values {
        #[allow(clippy::cast_possible_truncation)]
        let ratio = (value / scale).clamp(0.0, 1.0) as f32;
        row = row.child(div().flex_1().h_full().bg(theme::color(theme::blend(
            palette.track,
            full,
            ratio,
        ))));
    }
    row
}

/// One line of figures under a pane's bars.
fn caption(text: &str, palette: Palette) -> Div {
    theme::mono(text.to_owned(), 10., theme::color(palette.text_muted))
}

/// A key and its value in the Pieces pane's grid.
fn mini_fact(key: &str, value: String, palette: Palette) -> Div {
    div()
        .w(px(200.))
        .flex_none()
        .flex()
        .gap(px(6.))
        .child(theme::text(
            key.to_owned(),
            10.5,
            theme::color(palette.text_dim),
        ))
        .child(
            theme::mono(value, 10.5, theme::color(palette.text_body))
                .flex_1()
                .min_w_0()
                .truncate(),
        )
}

/// A dash of text where a pane has nothing to show.
fn placeholder(message: &str, palette: Palette) -> AnyElement {
    div()
        .id(SharedString::from(format!("detail-placeholder-{message}")))
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .child(theme::text(
            message.to_owned(),
            11.5,
            theme::color(palette.text_dim),
        ))
        .into_any_element()
}

/// The bytes a row has finished, from its size and progress.
fn percent_of(size: i64, progress: f64) -> i64 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let done = (size as f64 * (progress / 100.0)).round() as i64;
    done
}

/// The tree as flat rows: the node, its depth, and the path that keys it.
fn tree_rows<'a>(
    nodes: &'a [TreeNode],
    depth: u32,
    parent: &str,
    expanded: &HashSet<String>,
) -> Vec<(String, u32, &'a TreeNode)> {
    let mut rows = Vec::new();
    for node in nodes {
        let path = format!("{parent}/{}", node.name);
        rows.push((path.clone(), depth, node));
        if node.is_dir && expanded.contains(&path) {
            rows.extend(tree_rows(&node.children, depth + 1, &path, expanded));
        }
    }
    rows
}

/// A row's progress bar: 6 px of trough with the completed fraction filled in
/// the status colour, the way the table draws a torrent's.
fn progress_bar(percent: f64, status: Status, width: f32, palette: Palette) -> Div {
    let fraction = (percent / 100.0).clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation)]
    let filled = width * fraction as f32;
    div()
        .w_full()
        .h(px(6.))
        .flex_none()
        .rounded(px(1.))
        .bg(theme::color(palette.track))
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(px(filled))
                .rounded(px(1.))
                .bg(theme::color(palette.status_fill(status))),
        )
}

/// The priority chip: high is the accent, normal is neutral, skip recedes.
fn priority_chip(priority: i64, palette: Palette) -> Div {
    let (bg, fg) = match priority {
        detail_files::PRIORITY_HIGH => (palette.selected, palette.accent_cyan_bright),
        detail_files::PRIORITY_NORMAL => (palette.track, palette.text_muted),
        _ => (palette.track, palette.text_dim),
    };
    div()
        .h(px(15.))
        .flex_none()
        .flex()
        .items_center()
        .px(px(6.))
        .rounded(px(2.))
        .bg(theme::color(bg))
        .child(theme::text(
            detail_files::priority_label(priority),
            10.,
            theme::color(fg),
        ))
}

/// The pointer's guidance for an error kind, when there is one.
fn error_hint(kind: &str) -> Option<&'static str> {
    match kind {
        "missing_files" => Some("Files missing — try Force recheck or Set location."),
        "no_space" => Some("No space left on device — free space or move the torrent."),
        "permission" => Some("Permission denied — check the save path's permissions."),
        "disk_error" => Some("Storage error — check disk health and permissions."),
        "unregistered" => Some("Tracker says unregistered — the passkey may be wrong."),
        "tracker_timeout" => Some("Tracker timed out — transient; try Reannounce."),
        "tracker_error" => Some("Tracker error — try Reannounce, or see the Trackers tab."),
        _ => None,
    }
}

/// The file rows' menu, built fresh for each open.
fn build_file_menu(menu: PopupMenu) -> PopupMenu {
    menu.menu("Skip", Box::new(actions::SetFilePriority(0)))
        .menu("Normal", Box::new(actions::SetFilePriority(1)))
        .menu("High", Box::new(actions::SetFilePriority(2)))
}

/// The peer rows' menu.
fn build_peer_menu(menu: PopupMenu, can_act: bool) -> PopupMenu {
    menu.menu_with_disabled("Snub", Box::new(actions::PeerSnub), !can_act)
        .menu_with_disabled("Disconnect", Box::new(actions::PeerDisconnect), !can_act)
        .separator()
        .menu_with_disabled("Ban", Box::new(actions::PeerBan), !can_act)
}

/// The tracker rows' menu: the toggle says which way it will go.
fn build_tracker_menu(menu: PopupMenu, enabled: bool) -> PopupMenu {
    menu.menu(
        if enabled { "Disable" } else { "Enable" },
        Box::new(actions::TrackerToggle),
    )
    .separator()
    .menu("Remove", Box::new(actions::TrackerRemove))
}

/// A pane's own header strip: what the table below it is showing.
fn pane_header(text: &str, palette: Palette) -> Stateful<Div> {
    div()
        .id("detail-pane-header")
        .h(px(geometry::DETAIL_HEADER))
        .flex_none()
        .flex()
        .items_center()
        .px(px(10.))
        .bg(theme::color(palette.panel))
        .border_b_1()
        .border_color(theme::color(palette.row_line))
        .child(theme::text(
            text.to_owned(),
            10.5,
            theme::color(palette.text_muted),
        ))
}

/// The Peers table's column headings.
fn peer_header(palette: Palette) -> Stateful<Div> {
    let cell = |label: &str, width: f32, right: bool| {
        let cell = theme::text(label.to_owned(), 9.5, theme::color(palette.text_dim));
        if right {
            cell.w(px(width)).flex_none().text_right()
        } else {
            cell.w(px(width)).flex_none()
        }
    };
    div()
        .id("detail-peer-header")
        .h(px(geometry::DETAIL_HEADER))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(10.))
        .border_b_1()
        .border_color(theme::color(palette.border_mid))
        .child(
            theme::text("IP", 9.5, theme::color(palette.text_dim))
                .w(px(120.))
                .flex_none(),
        )
        .child(cell("Port", 52., true))
        .child(
            theme::text("Client", 9.5, theme::color(palette.text_dim))
                .flex_1()
                .min_w_0(),
        )
        .child(cell("Have", 100., false))
        .child(cell("Down", 70., true))
        .child(cell("Up", 70., true))
        .child(cell("Flags", 56., true))
}

/// A peer's Have cell: a 6 px trough in the accent, with the percentage.
fn have_bar(progress: f64, palette: Palette) -> Div {
    #[allow(clippy::cast_possible_truncation)]
    let filled = 60.0 * (progress / 100.0).clamp(0.0, 1.0) as f32;
    div()
        .w(px(100.))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .flex_1()
                .h(px(6.))
                .rounded(px(1.))
                .bg(theme::color(palette.track))
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(px(filled))
                        .bg(theme::color(palette.accent_cyan)),
                ),
        )
        .child(theme::mono(
            format!("{progress:.0}%"),
            10.,
            theme::color(palette.text_muted),
        ))
}

/// A peer's rate cell: the direction's colour when it is moving, a dim dash
/// when it is not. Download is the accent, upload the cyan.
fn rate_cell(rate: i64, down: bool, palette: Palette) -> Div {
    let colour = if rate > 0 {
        if down {
            palette.accent_cyan_bright
        } else {
            palette.accent_green
        }
    } else {
        palette.text_dim
    };
    theme::mono(
        format::rate(rate).replace("0 B/s", "—"),
        10.,
        theme::color(colour),
    )
    .w(px(70.))
    .flex_none()
    .text_right()
}

/// A 6 px status dot.
fn dot(tone: detail_rows::Tone, palette: Palette) -> Div {
    div()
        .w(px(6.))
        .h(px(6.))
        .flex_none()
        .rounded_full()
        .bg(theme::color(tone_colour(tone, palette)))
}

/// The colour a tone paints its dot, word and percentage.
fn tone_colour(tone: detail_rows::Tone, palette: Palette) -> u32 {
    match tone {
        detail_rows::Tone::Active => palette.accent_cyan_bright,
        detail_rows::Tone::Warn => palette.accent_amber,
        detail_rows::Tone::Error => palette.accent_red,
        detail_rows::Tone::Enabled => palette.accent_cyan,
        detail_rows::Tone::Idle => palette.text_dim,
    }
}

/// A small button for a pane's footer, sized to sit inside a 22 px row.
fn small_button(
    id: &'static str,
    label: &str,
    enabled: bool,
    danger: bool,
    palette: Palette,
) -> Stateful<Div> {
    let (border, text) = match (enabled, danger) {
        (false, _) => (palette.border_mid, palette.text_dim),
        (true, true) => (palette.danger_border, palette.danger_text),
        (true, false) => (palette.border_strong, palette.text_body),
    };
    div()
        .id(id)
        .h(px(20.))
        .flex_none()
        .flex()
        .items_center()
        .px(px(9.))
        .rounded(px(3.))
        .border_1()
        .border_color(theme::color(border))
        .bg(theme::color(if enabled {
            palette.track
        } else {
            palette.panel
        }))
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|style| style.bg(theme::color(palette.selected)))
        })
        .child(theme::text(label.to_owned(), 10.5, theme::color(text)))
}
