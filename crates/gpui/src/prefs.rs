//! The native Preferences dialog (G6-S1–S5).
//!
//! A port of `src/components/dialogs/PreferencesDialog.tsx`: the nine-pane
//! nav (Behavior, Downloads, Connection, Speed, BitTorrent, Network, RSS, Web
//! UI, Advanced) over a working copy of [`Settings`]. Text lives in kit input
//! entities, toggles and option rows edit the draft directly, and Apply
//! validates every numeric field before emitting — the first failure names
//! its field and keeps the dialog open, so a typo can never persist a
//! half-parsed document.
//!
//! What Apply does downstream lives in
//! [`TorrentsModel::apply_preferences`](crate::model::TorrentsModel::apply_preferences):
//! persist through the single settings writer, rebuild the backend when the
//! transport changed, and push daemon-affecting keys (port range, DHT,
//! network prefs) without waiting for a reconnect.

use gpui_kit::component::input::InputState;
use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, KeyDownEvent,
    MouseButton, SharedString, Stateful, Window,
};

use std::sync::Arc;

use rtorrent_core::complete::{CollisionPolicy, MoveRule};
use rtorrent_core::types::Transport;

use crate::actions::DismissOverlay;
use crate::dialogs::{
    backdrop, button, checkbox, footer, form_field, header, panel, text_field, ButtonKind,
};
use crate::model::TorrentsModel;
use crate::services::Services;
use crate::settings::{
    EncryptionMode, LabelDefault, RssFeed, RssRule, SeedGoalAction, Settings, WatchFolder,
};
use crate::theme::{self, Palette};

/// The Preferences dialog's outcome: the validated draft to persist (boxed —
/// a whole `Settings` is hundreds of bytes and this enum crosses the event
/// channel by value).
#[derive(Clone, Debug)]
pub enum PreferencesEvent {
    Cancel,
    Apply(Box<Settings>),
}

impl EventEmitter<PreferencesEvent> for PreferencesDialog {}

/// The nine panes, in the desktop order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pane {
    Behavior,
    Downloads,
    Connection,
    Speed,
    BitTorrent,
    Network,
    Rss,
    WebUi,
    Advanced,
}

impl Pane {
    const ALL: [Pane; 9] = [
        Pane::Behavior,
        Pane::Downloads,
        Pane::Connection,
        Pane::Speed,
        Pane::BitTorrent,
        Pane::Network,
        Pane::Rss,
        Pane::WebUi,
        Pane::Advanced,
    ];

    fn label(self) -> &'static str {
        match self {
            Pane::Behavior => "Behavior",
            Pane::Downloads => "Downloads",
            Pane::Connection => "Connection",
            Pane::Speed => "Speed",
            Pane::BitTorrent => "BitTorrent",
            Pane::Network => "Network",
            Pane::Rss => "RSS",
            Pane::WebUi => "Web UI",
            Pane::Advanced => "Advanced",
        }
    }
}

/// Which transport form the Connection pane edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransportKind {
    Unix,
    Tcp,
    Http,
}

impl TransportKind {
    fn of(transport: &Transport) -> Self {
        match transport {
            Transport::UnixSocket { .. } => TransportKind::Unix,
            Transport::Tcp { .. } => TransportKind::Tcp,
            Transport::Http { .. } => TransportKind::Http,
        }
    }
}

/// The test-connection probe's state, shown inline under the button.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum TestState {
    #[default]
    Idle,
    Testing,
    Ok(String),
    Err(String),
}

/// One editable row of a watched folder.
struct WatchRow {
    path: Entity<InputState>,
    label: Entity<InputState>,
    save_path: Entity<InputState>,
}

/// One per-label default save path (C11).
struct LabelRow {
    label: Entity<InputState>,
    save_path: Entity<InputState>,
}

/// One move-on-complete rule (V3-14): a tag match or a label match.
struct MoveRow {
    kind_tag: bool,
    key: Entity<InputState>,
    destination: Entity<InputState>,
}

/// One bandwidth rule (V3-18): a tag/label match plus caps.
struct BandwidthRow {
    id: String,
    kind_tag: bool,
    key: Entity<InputState>,
    down: Entity<InputState>,
    up: Entity<InputState>,
    peers_max: Entity<InputState>,
    peers_min: Entity<InputState>,
    uploads_max: Entity<InputState>,
}

/// One scheduler grid window (V3-18): weekdays, a half-open range, and
/// limit-vs-pause mode with its rates.
struct SchedRow {
    days: Vec<u8>,
    start: Entity<InputState>,
    end: Entity<InputState>,
    pause: bool,
    down: Entity<InputState>,
    up: Entity<InputState>,
}

/// The temporary override editor: a mode plus minutes-from-now and rates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverrideMode {
    Off,
    Pause,
    Limit,
}

struct OverrideRow {
    mode: OverrideMode,
    minutes: Entity<InputState>,
    down: Entity<InputState>,
    up: Entity<InputState>,
}

/// One RSS feed.
struct FeedRow {
    id: String,
    name: Entity<InputState>,
    url: Entity<InputState>,
    enabled: bool,
}

/// One RSS auto-download rule.
struct RuleRow {
    id: String,
    name: Entity<InputState>,
    feed_id: String,
    must: Entity<InputState>,
    must_not: Entity<InputState>,
    regex: Entity<InputState>,
    seasons: Entity<InputState>,
    episodes: Entity<InputState>,
    min_size: Entity<InputState>,
    max_size: Entity<InputState>,
    prefer_quality: Entity<InputState>,
    smart: bool,
    label: Entity<InputState>,
    tags: Entity<InputState>,
    save_path: Entity<InputState>,
    start: bool,
    top: bool,
    interval: Entity<InputState>,
    enabled: bool,
}

/// One per-label seed-goal override.
struct SeedRow {
    label: Entity<InputState>,
    ratio: Entity<InputState>,
    hours: Entity<InputState>,
}

/// Make a text input with placeholder and initial value.
fn input(
    cx: &mut Context<PreferencesDialog>,
    window: &mut Window,
    placeholder: &'static str,
    initial: &str,
) -> Entity<InputState> {
    cx.new(|cx| {
        let mut state = InputState::new(window, cx).placeholder(placeholder);
        if !initial.is_empty() {
            state = state.default_value(initial);
        }
        state
    })
}

/// A group heading + its rows, matching the desktop `Group` title style.
fn group(title: &str, palette: Palette) -> Div {
    div().flex().flex_col().gap(px(8.)).child(theme::text(
        title.to_owned(),
        11.,
        theme::color(palette.text_primary),
    ))
}

/// A single line of explanatory text.
fn note(content: String, palette: Palette) -> Div {
    theme::text(content, 10.5, theme::color(palette.text_dim)).min_w_0()
}

/// The design's checkbox with a caller-owned id, for rows the dialog builds
/// dynamically (the shared helper needs `&'static str` ids).
fn dynamic_checkbox(
    id: String,
    label: String,
    checked: bool,
    enabled: bool,
    palette: Palette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
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
        .id(SharedString::from(id))
        .flex()
        .items_center()
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
        .child(theme::text(label, 11., theme::color(text)));
    if enabled {
        element = element.cursor_pointer().on_click(on_click);
    }
    element
}

/// An error line for validation and probe failures.
fn error_line(message: &str, palette: Palette) -> Div {
    theme::text(message.to_owned(), 10.5, theme::color(palette.accent_red)).min_w_0()
}

/// A selectable option chip (radio behaviour without a radio widget).
fn option_chip(
    id: String,
    label: &str,
    selected: bool,
    palette: Palette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(SharedString::from(id))
        .h(px(22.))
        .flex()
        .items_center()
        .px(px(10.))
        .rounded(px(4.))
        .border_1()
        .border_color(theme::color(if selected {
            palette.accent_cyan
        } else {
            palette.border_strong
        }))
        .bg(theme::color(if selected {
            palette.selected
        } else {
            palette.track
        }))
        .cursor_pointer()
        .hover(|style| style.bg(theme::color(palette.selected)))
        .on_click(on_click)
        .child(theme::text(
            label.to_owned(),
            10.5,
            theme::color(if selected {
                palette.accent_cyan_bright
            } else {
                palette.text_body
            }),
        ))
}

/// A small row button for list editors (add/remove live in the footer style).
fn row_button(
    id: String,
    label: &str,
    palette: Palette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(SharedString::from(id))
        .h(px(24.))
        .flex_none()
        .flex()
        .items_center()
        .px(px(10.))
        .rounded(px(4.))
        .border_1()
        .border_color(theme::color(palette.border_strong))
        .bg(theme::color(palette.track))
        .cursor_pointer()
        .hover(|style| style.bg(theme::color(palette.selected)))
        .on_click(on_click)
        .child(theme::text(
            label.to_owned(),
            10.5,
            theme::color(palette.text_body),
        ))
}

/// A labelled row: a fixed-width label beside a control, as
/// `forms.module.css` lays them out on the desktop.
fn field_row(label: &str, control: impl IntoElement, palette: Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(div().w(px(120.)).flex_none().child(theme::text(
            label.to_owned(),
            10.5,
            theme::color(palette.text_muted),
        )))
        .child(control)
}

/// The full Preferences dialog: nine panes over a working copy of Settings.
pub struct PreferencesDialog {
    palette: Palette,
    focus: FocusHandle,
    pane: Pane,
    /// Everything that is not a text field edits here directly.
    draft: Settings,
    services: Arc<Services>,
    model: Entity<TorrentsModel>,
    /// RSS rule-test report (V3-23): rule id plus rows, or a fetch error.
    rss_test: Option<(String, Result<crate::rss::TestReport, String>)>,
    rss_testing: bool,
    /// Labels in use, for the notification-exclusion checkboxes.
    known_labels: Vec<String>,
    transport_kind: TransportKind,
    // --- text fields, by pane ---
    in_sock_path: Entity<InputState>,
    in_tcp_host: Entity<InputState>,
    in_tcp_port: Entity<InputState>,
    in_http_url: Entity<InputState>,
    in_http_user: Entity<InputState>,
    in_poll_ms: Entity<InputState>,
    in_stall_window_s: Entity<InputState>,
    in_down_limit: Entity<InputState>,
    in_up_limit: Entity<InputState>,
    in_max_peers: Entity<InputState>,
    in_max_uploads: Entity<InputState>,
    in_max_downloads: Entity<InputState>,
    in_max_active: Entity<InputState>,
    in_max_up_active: Entity<InputState>,
    in_max_total: Entity<InputState>,
    in_slow_limit: Entity<InputState>,
    in_turtle_down: Entity<InputState>,
    in_turtle_up: Entity<InputState>,
    in_turtle_start: Entity<InputState>,
    in_turtle_end: Entity<InputState>,
    in_port_range: Entity<InputState>,
    in_seed_ratio: Entity<InputState>,
    in_seed_hours: Entity<InputState>,
    in_proxy: Entity<InputState>,
    in_bind: Entity<InputState>,
    in_local: Entity<InputState>,
    in_save_path: Entity<InputState>,
    in_incomplete_dir: Entity<InputState>,
    in_run_on_complete: Entity<InputState>,
    in_rss_poll: Entity<InputState>,
    // --- list editors ---
    watch_folders: Vec<WatchRow>,
    label_defaults: Vec<LabelRow>,
    move_rules: Vec<MoveRow>,
    bandwidth: Vec<BandwidthRow>,
    sched_windows: Vec<SchedRow>,
    sched_override: OverrideRow,
    feeds: Vec<FeedRow>,
    rules: Vec<RuleRow>,
    seed_overrides: Vec<SeedRow>,
    id_seq: usize,
    test: TestState,
    /// Apply-blocking validation failure, naming its field.
    error: Option<String>,
}

impl PreferencesDialog {
    /// Open on the current settings; the draft starts as a clone so Cancel
    /// never touches the live document.
    pub fn new(
        services: Arc<Services>,
        model: Entity<TorrentsModel>,
        settings: &Settings,
        known_labels: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let draft = settings.clone();
        let mut dialog = Self {
            palette: Palette::dark(),
            focus,
            pane: Pane::Downloads,
            transport_kind: TransportKind::of(&draft.transport),
            in_sock_path: input(cx, window, "~/.rtorrent/rpc.socket", &sock_path(&draft)),
            in_tcp_host: input(cx, window, "127.0.0.1", &tcp_host(&draft)),
            in_tcp_port: input(cx, window, "5000", &tcp_port(&draft)),
            in_http_url: input(
                cx,
                window,
                "https://seedbox.example/RPC2",
                &http_url(&draft),
            ),
            in_http_user: input(cx, window, "(none)", &http_user(&draft)),
            in_poll_ms: input(cx, window, "1000", &draft.poll_ms.to_string()),
            in_stall_window_s: input(cx, window, "30", &draft.stall_window_s.to_string()),
            in_down_limit: input(
                cx,
                window,
                "0 = unlimited",
                &draft.down_limit_kb.to_string(),
            ),
            in_up_limit: input(cx, window, "0 = unlimited", &draft.up_limit_kb.to_string()),
            in_max_peers: input(cx, window, "0 = default", &draft.max_peers.to_string()),
            in_max_uploads: input(
                cx,
                window,
                "0 = unlimited",
                &draft.max_uploads_global.to_string(),
            ),
            in_max_downloads: input(
                cx,
                window,
                "0 = unlimited",
                &draft.max_downloads_global.to_string(),
            ),
            in_max_active: input(
                cx,
                window,
                "0 = no queue",
                &draft.max_active_downloads.to_string(),
            ),
            in_max_up_active: input(
                cx,
                window,
                "0 = no cap",
                &draft.max_active_uploads.to_string(),
            ),
            in_max_total: input(
                cx,
                window,
                "0 = no cap",
                &draft.max_active_torrents.to_string(),
            ),
            in_slow_limit: input(
                cx,
                window,
                "0 = off",
                &draft.queue_slow_limit_kbs.to_string(),
            ),
            in_turtle_down: input(
                cx,
                window,
                "0 = unlimited",
                &draft.turtle_down_kb.to_string(),
            ),
            in_turtle_up: input(cx, window, "0 = unlimited", &draft.turtle_up_kb.to_string()),
            in_turtle_start: input(
                cx,
                window,
                "HH:MM",
                &mins_to_hhmm(draft.turtle_schedule.start_min),
            ),
            in_turtle_end: input(
                cx,
                window,
                "HH:MM",
                &mins_to_hhmm(draft.turtle_schedule.end_min),
            ),
            in_port_range: input(cx, window, "6881-6899", &draft.port_range),
            in_seed_ratio: input(
                cx,
                window,
                "0 = off",
                &float_text(draft.global_seed_goal.stop_ratio),
            ),
            in_seed_hours: input(
                cx,
                window,
                "0 = off",
                &float_text(draft.global_seed_goal.seed_hours),
            ),
            in_proxy: input(cx, window, "host:port", &draft.proxy_address),
            in_bind: input(cx, window, "(daemon default)", &draft.bind_address),
            in_local: input(cx, window, "(daemon default)", &draft.local_address),
            in_save_path: input(cx, window, "~/Downloads", &draft.default_save_path),
            in_incomplete_dir: input(cx, window, "(disabled)", &draft.incomplete_dir),
            in_run_on_complete: input(cx, window, "(disabled)", &draft.run_on_complete),
            in_rss_poll: input(cx, window, "15", &draft.rss_poll_minutes.to_string()),
            watch_folders: Vec::new(),
            label_defaults: Vec::new(),
            move_rules: Vec::new(),
            bandwidth: Vec::new(),
            sched_windows: Vec::new(),
            sched_override: OverrideRow {
                mode: OverrideMode::Off,
                minutes: input(cx, window, "30", "30"),
                down: input(cx, window, "0 = ∞", ""),
                up: input(cx, window, "0 = ∞", ""),
            },
            feeds: Vec::new(),
            rules: Vec::new(),
            seed_overrides: Vec::new(),
            id_seq: 0,
            services,
            model,
            known_labels,
            rss_test: None,
            rss_testing: false,
            test: TestState::Idle,
            error: None,
            draft,
        };
        // Seed the list editors from the draft (entities need the cx that
        // struct construction already has).
        let watch: Vec<(String, String, String)> = dialog
            .draft
            .watch_folders
            .iter()
            .map(|f| (f.path.clone(), f.label.clone(), f.save_path.clone()))
            .collect();
        for (path, label, save_path) in watch {
            dialog.watch_folders.push(WatchRow {
                path: input(cx, window, "folder to watch", &path),
                label: input(cx, window, "(none)", &label),
                save_path: input(cx, window, "(label / global default)", &save_path),
            });
        }
        let defaults: Vec<(String, String)> = dialog
            .draft
            .label_defaults
            .iter()
            .map(|d| (d.label.clone(), d.save_path.clone()))
            .collect();
        for (label, save_path) in defaults {
            dialog.label_defaults.push(LabelRow {
                label: input(cx, window, "label", &label),
                save_path: input(cx, window, "save path", &save_path),
            });
        }
        let rules: Vec<(bool, String, String)> = dialog
            .draft
            .move_rules
            .iter()
            .map(|r| {
                if let Some(tag) = r.tag.as_deref() {
                    (true, tag.to_owned(), r.destination.clone())
                } else {
                    (
                        false,
                        r.label.clone().unwrap_or_default(),
                        r.destination.clone(),
                    )
                }
            })
            .collect();
        for (kind_tag, key, destination) in rules {
            dialog.move_rules.push(MoveRow {
                kind_tag,
                key: input(cx, window, "tag or label", &key),
                destination: input(cx, window, "destination", &destination),
            });
        }
        for index in 0..dialog.draft.bandwidth_rules.len() {
            let rule = dialog.draft.bandwidth_rules[index].clone();
            dialog.id_seq += 1;
            let id = if rule.id.trim().is_empty() {
                format!("bw-{}", dialog.id_seq)
            } else {
                rule.id.clone()
            };
            dialog.bandwidth.push(BandwidthRow {
                id,
                kind_tag: rule.tag.is_some(),
                key: input(
                    cx,
                    window,
                    "tag or label",
                    &rule.tag.clone().or(rule.label.clone()).unwrap_or_default(),
                ),
                down: input(cx, window, "0 = ∞", &rule.down_kb.to_string()),
                up: input(cx, window, "0 = ∞", &rule.up_kb.to_string()),
                peers_max: input(cx, window, "0", &rule.peers_max.to_string()),
                peers_min: input(cx, window, "0", &rule.peers_min.to_string()),
                uploads_max: input(cx, window, "0", &rule.uploads_max.to_string()),
            });
        }
        for index in 0..dialog.draft.schedule.windows.len() {
            let saved = dialog.draft.schedule.windows[index].clone();
            dialog.sched_windows.push(SchedRow {
                days: saved.days.clone(),
                start: input(cx, window, "HH:MM", &mins_to_hhmm(saved.start_min)),
                end: input(cx, window, "HH:MM", &mins_to_hhmm(saved.end_min)),
                pause: saved.pause,
                down: input(cx, window, "0 = ∞", &saved.down_kb.to_string()),
                up: input(cx, window, "0 = ∞", &saved.up_kb.to_string()),
            });
        }
        // A live override shows its remaining minutes; an expired one opens
        // cleared (build drops expired overrides anyway).
        if let Some(over) = dialog.draft.schedule.temp_override.clone() {
            let remaining = over
                .until_ms
                .saturating_sub(now_millis())
                .div_euclid(60_000)
                .max(1);
            dialog.sched_override = OverrideRow {
                mode: if over.pause {
                    OverrideMode::Pause
                } else {
                    OverrideMode::Limit
                },
                minutes: input(cx, window, "30", &remaining.to_string()),
                down: input(cx, window, "0 = ∞", &over.down_kb.to_string()),
                up: input(cx, window, "0 = ∞", &over.up_kb.to_string()),
            };
        }
        let feeds: Vec<(String, String, String, bool)> = dialog
            .draft
            .rss_feeds
            .iter()
            .map(|f| (f.id.clone(), f.name.clone(), f.url.clone(), f.enabled))
            .collect();
        for (id, name, url, enabled) in feeds {
            dialog.id_seq += 1;
            let id = if id.is_empty() {
                format!("feed-{}", dialog.id_seq)
            } else {
                id
            };
            dialog.feeds.push(FeedRow {
                id,
                name: input(cx, window, "feed name", &name),
                url: input(cx, window, "https://…/feed", &url),
                enabled,
            });
        }
        let auto: Vec<usize> = (0..dialog.draft.rss_rules.len()).collect();
        for index in auto {
            let rule = dialog.draft.rss_rules[index].clone();
            dialog.id_seq += 1;
            let id = if rule.id.is_empty() {
                format!("rule-{}", dialog.id_seq)
            } else {
                rule.id.clone()
            };
            dialog.rules.push(RuleRow {
                id,
                name: input(cx, window, "rule name", &rule.name),
                feed_id: rule.feed_id.clone(),
                must: input(cx, window, "must contain", &rule.must_contain),
                must_not: input(cx, window, "must not contain", &rule.must_not_contain),
                regex: input(cx, window, "optional regex", &rule.regex_match),
                seasons: input(cx, window, "e.g. 1-3", &rule.season_range),
                episodes: input(cx, window, "e.g. 4-12", &rule.episode_range),
                min_size: input(cx, window, "MiB, 0 = off", &rule.min_size_mb.to_string()),
                max_size: input(cx, window, "MiB, 0 = off", &rule.max_size_mb.to_string()),
                prefer_quality: input(cx, window, "e.g. 1080p", &rule.prefer_quality),
                smart: rule.smart_episode,
                label: input(cx, window, "(none)", &rule.label),
                tags: input(cx, window, "comma-separated", &rule.tags.join(", ")),
                save_path: input(cx, window, "(label / global default)", &rule.save_path),
                start: rule.start,
                top: rule.top_of_queue,
                interval: input(cx, window, "0 = global", &rule.poll_minutes.to_string()),
                enabled: rule.enabled,
            });
        }
        let seeds: Vec<(String, String, String)> = dialog
            .draft
            .label_seed_goals
            .iter()
            .map(|g| {
                (
                    g.label.clone(),
                    float_text(g.stop_ratio),
                    float_text(g.seed_hours),
                )
            })
            .collect();
        for (label, ratio, hours) in seeds {
            dialog.seed_overrides.push(SeedRow {
                label: input(cx, window, "label", &label),
                ratio: input(cx, window, "0 = off", &ratio),
                hours: input(cx, window, "0 = off", &hours),
            });
        }
        dialog
    }

    fn cancel(&self, cx: &mut Context<Self>) {
        cx.emit(PreferencesEvent::Cancel);
    }

    fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        self.error = Some(message);
        cx.notify();
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        match self.build(cx) {
            Ok(settings) => cx.emit(PreferencesEvent::Apply(Box::new(settings))),
            Err(message) => self.fail(message, cx),
        }
    }

    /// Test one rule against its feeds (V3-23): fetch + explain, showing the
    /// verdict and first failing clause per item. Runs on the services
    /// runtime (plain HTTPS needs a reactor).
    fn test_rule(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rules.get(index) else {
            return;
        };
        let rule = rtorrent_core::rss::Rule {
            id: row.id.clone(),
            name: row.name.read(cx).value().to_string(),
            enabled: true,
            feed_id: row.feed_id.clone(),
            must_contain: row.must.read(cx).value().to_string(),
            must_not_contain: row.must_not.read(cx).value().to_string(),
            regex_match: row.regex.read(cx).value().to_string(),
            season_range: row.seasons.read(cx).value().to_string(),
            episode_range: row.episodes.read(cx).value().to_string(),
            min_size_mb: row
                .min_size
                .read(cx)
                .value()
                .to_string()
                .parse()
                .unwrap_or(0),
            max_size_mb: row
                .max_size
                .read(cx)
                .value()
                .to_string()
                .parse()
                .unwrap_or(0),
            prefer_quality: row.prefer_quality.read(cx).value().to_string(),
            smart_episode: row.smart,
            label: row.label.read(cx).value().to_string(),
            tags: Vec::new(),
            save_path: row.save_path.read(cx).value().to_string(),
            start: row.start,
            top_of_queue: row.top,
            poll_minutes: 0,
        };
        // Invalid regex fails here with the message instead of fetching.
        if let Err(err) = rtorrent_core::rss::validate_regex(&rule.regex_match) {
            self.rss_test = Some((rule.id.clone(), Err(err)));
            self.rss_testing = false;
            cx.notify();
            return;
        }
        let feeds: Vec<rtorrent_core::rss::Feed> = self
            .draft
            .rss_feeds
            .iter()
            .map(|f| rtorrent_core::rss::Feed {
                id: f.id.clone(),
                name: f.name.clone(),
                url: f.url.clone(),
                enabled: f.enabled,
            })
            .collect();
        let services = Arc::clone(&self.services);
        let id = rule.id.clone();
        self.rss_testing = true;
        self.rss_test = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let report = services.rss_test(rule, feeds).await;
            this.update(cx, |this, cx| {
                this.rss_testing = false;
                this.rss_test = Some((id, report.map_err(|e| e.to_string())));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Read every field into a validated `Settings`. The first failure names
    /// its field; nothing is emitted until everything parses.
    fn build(&self, cx: &mut Context<Self>) -> Result<Settings, String> {
        let text = |input: &Entity<InputState>| input.read(cx).value().to_string();
        let int = |input: &Entity<InputState>, field: &str| -> Result<i64, String> {
            parse_int(&text(input), field)
        };
        let float = |input: &Entity<InputState>, field: &str| -> Result<f64, String> {
            parse_float(&text(input), field)
        };

        let transport = match self.transport_kind {
            TransportKind::Unix => {
                let path = text(&self.in_sock_path);
                if path.trim().is_empty() {
                    return Err("Socket path is empty".to_owned());
                }
                Transport::UnixSocket {
                    path: path.trim().to_owned(),
                }
            }
            TransportKind::Tcp => {
                let host = text(&self.in_tcp_host);
                if host.trim().is_empty() {
                    return Err("TCP host is empty".to_owned());
                }
                Transport::Tcp {
                    host: host.trim().to_owned(),
                    port: parse_port(&text(&self.in_tcp_port))?,
                }
            }
            TransportKind::Http => {
                let url = text(&self.in_http_url);
                if url.trim().is_empty() {
                    return Err("HTTP URL is empty".to_owned());
                }
                Transport::Http {
                    url: url.trim().to_owned(),
                    username: text(&self.in_http_user).trim().to_owned(),
                }
            }
        };

        let poll_ms = int(&self.in_poll_ms, "Poll interval")?;
        if poll_ms < 250 {
            return Err("Poll interval must be at least 250 ms".to_owned());
        }
        let stall_window_s = int(&self.in_stall_window_s, "Stall window")?;
        let rss_poll_minutes = int(&self.in_rss_poll, "RSS poll interval")?;

        let turtle_schedule = crate::settings::TurtleSchedule {
            enabled: self.draft.turtle_schedule.enabled,
            start_min: parse_hhmm(&text(&self.in_turtle_start), "Turtle window start")?,
            end_min: parse_hhmm(&text(&self.in_turtle_end), "Turtle window end")?,
            days: self.draft.turtle_schedule.days.clone(),
        };

        let mut windows = Vec::with_capacity(self.sched_windows.len());
        for (index, row) in self.sched_windows.iter().enumerate() {
            let start = parse_hhmm(&text(&row.start), &format!("Window {} start", index + 1))?;
            let end = parse_hhmm(&text(&row.end), &format!("Window {} end", index + 1))?;
            if start == end {
                return Err(format!(
                    "Window {} is zero-length (start == end)",
                    index + 1
                ));
            }
            windows.push(rtorrent_core::schedule::SchedWindow {
                days: row.days.clone(),
                start_min: start,
                end_min: end,
                pause: row.pause,
                down_kb: int(&row.down, &format!("Window {} download cap", index + 1))?,
                up_kb: int(&row.up, &format!("Window {} upload cap", index + 1))?,
            });
        }
        let temp_override = match self.sched_override.mode {
            OverrideMode::Off => None,
            OverrideMode::Pause | OverrideMode::Limit => {
                let minutes = int(&self.sched_override.minutes, "Override minutes")?;
                if minutes < 1 {
                    return Err("Override minutes must be at least 1".to_owned());
                }
                Some(rtorrent_core::schedule::TempOverride {
                    pause: self.sched_override.mode == OverrideMode::Pause,
                    down_kb: int(&self.sched_override.down, "Override download cap")?,
                    up_kb: int(&self.sched_override.up, "Override upload cap")?,
                    until_ms: now_millis() + minutes.saturating_mul(60_000),
                })
            }
        };
        let schedule = rtorrent_core::schedule::Schedule {
            windows,
            temp_override,
        };

        let mut watch_folders = Vec::with_capacity(self.watch_folders.len());
        for (index, row) in self.watch_folders.iter().enumerate() {
            watch_folders.push(WatchFolder {
                path: text(&row.path),
                label: text(&row.label).trim().to_owned(),
                save_path: text(&row.save_path).trim().to_owned(),
            });
            let _ = index;
        }
        let mut label_defaults = Vec::with_capacity(self.label_defaults.len());
        for row in &self.label_defaults {
            label_defaults.push(LabelDefault {
                label: text(&row.label).trim().to_owned(),
                save_path: text(&row.save_path).trim().to_owned(),
            });
        }
        let mut move_rules = Vec::with_capacity(self.move_rules.len());
        for row in &self.move_rules {
            let key = text(&row.key).trim().to_owned();
            let destination = text(&row.destination).trim().to_owned();
            if key.is_empty() && destination.is_empty() {
                continue;
            }
            if key.is_empty() || destination.is_empty() {
                return Err("Move rules need both a tag/label and a destination".to_owned());
            }
            move_rules.push(if row.kind_tag {
                MoveRule::tag_rule(&key, &destination)
            } else {
                MoveRule::label_rule(&key, &destination)
            });
        }
        let mut rss_feeds = Vec::with_capacity(self.feeds.len());
        for row in &self.feeds {
            let url = text(&row.url).trim().to_owned();
            if text(&row.name).trim().is_empty() && url.is_empty() {
                continue;
            }
            if url.is_empty() {
                return Err(format!("RSS feed '{}' needs a URL", text(&row.name).trim()));
            }
            rss_feeds.push(RssFeed {
                id: row.id.clone(),
                name: text(&row.name).trim().to_owned(),
                url,
                enabled: row.enabled,
            });
        }
        let mut rss_rules = Vec::with_capacity(self.rules.len());
        for row in &self.rules {
            if text(&row.name).trim().is_empty()
                && text(&row.must).trim().is_empty()
                && text(&row.must_not).trim().is_empty()
            {
                continue;
            }
            if text(&row.name).trim().is_empty() {
                return Err("RSS rules need a name".to_owned());
            }
            rss_rules.push(RssRule {
                id: row.id.clone(),
                name: text(&row.name).trim().to_owned(),
                enabled: row.enabled,
                feed_id: row.feed_id.clone(),
                must_contain: text(&row.must).trim().to_owned(),
                must_not_contain: text(&row.must_not).trim().to_owned(),
                regex_match: text(&row.regex).trim().to_owned(),
                season_range: text(&row.seasons).trim().to_owned(),
                episode_range: text(&row.episodes).trim().to_owned(),
                min_size_mb: int(&row.min_size, "Rule min size")?,
                max_size_mb: int(&row.max_size, "Rule max size")?,
                prefer_quality: text(&row.prefer_quality).trim().to_owned(),
                smart_episode: row.smart,
                label: text(&row.label).trim().to_owned(),
                tags: rtorrent_core::tags::normalise(text(&row.tags).split(',').map(str::to_owned)),
                save_path: text(&row.save_path).trim().to_owned(),
                start: row.start,
                top_of_queue: row.top,
                poll_minutes: int(&row.interval, "Rule poll interval")?,
            });
        }
        let mut bandwidth_rules = Vec::with_capacity(self.bandwidth.len());
        for row in &self.bandwidth {
            let key = text(&row.key).trim().to_owned();
            let down_kb = int(&row.down, "Rule download cap")?;
            let up_kb = int(&row.up, "Rule upload cap")?;
            let peers_max = int(&row.peers_max, "Rule max peers")?;
            let peers_min = int(&row.peers_min, "Rule min peers")?;
            let uploads_max = int(&row.uploads_max, "Rule upload slots")?;
            if key.is_empty()
                && down_kb == 0
                && up_kb == 0
                && peers_max == 0
                && peers_min == 0
                && uploads_max == 0
            {
                continue;
            }
            if key.is_empty() {
                return Err("Bandwidth rules need a tag or a label".to_owned());
            }
            let mut rule = if row.kind_tag {
                rtorrent_core::bandwidth::BandwidthRule::tag_rule(&row.id, &key, down_kb, up_kb)
            } else {
                rtorrent_core::bandwidth::BandwidthRule::label_rule(&row.id, &key, down_kb, up_kb)
            };
            rule.peers_max = peers_max;
            rule.peers_min = peers_min;
            rule.uploads_max = uploads_max;
            bandwidth_rules.push(rule);
        }
        let mut label_seed_goals = Vec::with_capacity(self.seed_overrides.len());
        for row in &self.seed_overrides {
            let label = text(&row.label).trim().to_owned();
            let ratio = float(&row.ratio, "Seed override ratio")?;
            let hours = float(&row.hours, "Seed override hours")?;
            if label.is_empty() && ratio == 0.0 && hours == 0.0 {
                continue;
            }
            if label.is_empty() {
                return Err("Seed overrides need a label".to_owned());
            }
            label_seed_goals.push(crate::settings::LabelSeedGoal {
                label,
                stop_ratio: ratio,
                seed_hours: hours,
            });
        }

        Ok(Settings {
            transport,
            poll_ms: poll_ms as u64,
            stall_window_s: stall_window_s as u64,
            default_save_path: text(&self.in_save_path).trim().to_owned(),
            show_add_dialog: self.draft.show_add_dialog,
            confirm_on_remove: self.draft.confirm_on_remove,
            down_limit_kb: int(&self.in_down_limit, "Download limit")?,
            up_limit_kb: int(&self.in_up_limit, "Upload limit")?,
            port_range: text(&self.in_port_range).trim().to_owned(),
            dht_enabled: self.draft.dht_enabled,
            watch_folder: self.draft.watch_folder.clone(),
            completion_notification_excluded_labels: self
                .draft
                .completion_notification_excluded_labels
                .clone(),
            torrent_throttles: self.draft.torrent_throttles.clone(),
            global_seed_goal: crate::settings::SeedGoal {
                stop_ratio: float(&self.in_seed_ratio, "Stop ratio")?,
                seed_hours: float(&self.in_seed_hours, "Seeding hours")?,
            },
            label_seed_goals,
            encryption: self.draft.encryption,
            pex_enabled: self.draft.pex_enabled,
            proxy_address: text(&self.in_proxy).trim().to_owned(),
            proxy_tracker_http: self.draft.proxy_tracker_http,
            bind_address: text(&self.in_bind).trim().to_owned(),
            local_address: text(&self.in_local).trim().to_owned(),
            max_peers: int(&self.in_max_peers, "Max peers")?,
            max_uploads_global: int(&self.in_max_uploads, "Upload slots")?,
            max_downloads_global: int(&self.in_max_downloads, "Download slots")?,
            max_active_downloads: int(&self.in_max_active, "Max active downloads")?,
            max_active_uploads: int(&self.in_max_up_active, "Max active uploads")?,
            max_active_torrents: int(&self.in_max_total, "Max active torrents")?,
            queue_slow_limit_kbs: int(&self.in_slow_limit, "Slow-torrent floor")?,
            label_defaults,
            watch_folders,
            incomplete_dir: text(&self.in_incomplete_dir).trim().to_owned(),
            move_rules,
            bandwidth_rules,
            collision_policy: self.draft.collision_policy,
            run_on_complete: text(&self.in_run_on_complete).trim().to_owned(),
            seed_goal_action: self.draft.seed_goal_action,
            turtle_down_kb: int(&self.in_turtle_down, "Turtle download limit")?,
            turtle_up_kb: int(&self.in_turtle_up, "Turtle upload limit")?,
            turtle_enabled: self.draft.turtle_enabled,
            turtle_schedule,
            schedule,
            connection_profiles: self.draft.connection_profiles.clone(),
            rss_feeds,
            rss_rules,
            rss_poll_minutes,
            mock: self.draft.mock,
            // View state rides through untouched: the dialog never edits it.
            filter: self.draft.filter.clone(),
            search: self.draft.search.clone(),
            sort_column: self.draft.sort_column.clone(),
            sort_descending: self.draft.sort_descending,
            sidebar_width: self.draft.sidebar_width,
            columns: self.draft.columns.clone(),
            active_tab: self.draft.active_tab.clone(),
        })
    }
}

/// Whole, non-negative numbers; empty means 0 (the daemon's unlimited).
fn parse_int(text: &str, field: &str) -> Result<i64, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    trimmed
        .parse::<i64>()
        .map_err(|_| format!("{field}: '{trimmed}' is not a whole number"))
        .and_then(|value| {
            if value < 0 {
                Err(format!("{field}: must be zero or greater"))
            } else {
                Ok(value)
            }
        })
}

/// Non-negative decimals; empty means 0 (off).
fn parse_float(text: &str, field: &str) -> Result<f64, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(0.0);
    }
    trimmed
        .parse::<f64>()
        .map_err(|_| format!("{field}: '{trimmed}' is not a number"))
        .and_then(|value| {
            if !value.is_finite() || value < 0.0 {
                Err(format!("{field}: must be zero or greater"))
            } else {
                Ok(value)
            }
        })
}

fn parse_port(text: &str) -> Result<u16, String> {
    parse_int(text, "TCP port").and_then(|port| {
        if (1..=65535).contains(&port) {
            Ok(port as u16)
        } else {
            Err("TCP port must be 1–65535".to_owned())
        }
    })
}

/// "HH:MM" to minutes since midnight, for the turtle window.
fn parse_hhmm(text: &str, field: &str) -> Result<i64, String> {
    let (hour, minute) = text
        .trim()
        .split_once(':')
        .ok_or_else(|| format!("{field}: use HH:MM, e.g. 23:00"))?;
    let hour: i64 = hour
        .trim()
        .parse()
        .map_err(|_| format!("{field}: '{hour}' is not an hour"))?;
    let minute: i64 = minute
        .trim()
        .parse()
        .map_err(|_| format!("{field}: '{minute}' is not a minute"))?;
    if !(0..24).contains(&hour) || !(0..60).contains(&minute) {
        return Err(format!("{field}: use HH:MM, e.g. 23:00"));
    }
    Ok(hour * 60 + minute)
}

fn mins_to_hhmm(mins: i64) -> String {
    format!("{:02}:{:02}", mins.div_euclid(60), mins.rem_euclid(60))
}

/// Wall-clock now in unix milliseconds, for temporary-override expiry.
fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

fn float_text(value: f64) -> String {
    if value == 0.0 {
        String::new()
    } else {
        value.to_string()
    }
}

fn sock_path(settings: &Settings) -> String {
    match &settings.transport {
        Transport::UnixSocket { path } => path.clone(),
        _ => String::new(),
    }
}

fn tcp_host(settings: &Settings) -> String {
    match &settings.transport {
        Transport::Tcp { host, .. } => host.clone(),
        _ => "127.0.0.1".to_owned(),
    }
}

fn tcp_port(settings: &Settings) -> String {
    match &settings.transport {
        Transport::Tcp { port, .. } => port.to_string(),
        _ => "5000".to_owned(),
    }
}

fn http_url(settings: &Settings) -> String {
    match &settings.transport {
        Transport::Http { url, .. } => url.clone(),
        _ => String::new(),
    }
}

fn http_user(settings: &Settings) -> String {
    match &settings.transport {
        Transport::Http { username, .. } => username.clone(),
        _ => String::new(),
    }
}

impl PreferencesDialog {
    /// Probe the transport as currently typed (G6-S2). Anonymous on purpose
    /// (see `Services::test_connection`); the result renders inline under
    /// the button rather than as a notice, so it never outlives the dialog.
    fn test_connection(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let transport = match self.transport_kind {
            TransportKind::Unix => Transport::UnixSocket {
                path: self.in_sock_path.read(cx).value().to_string(),
            },
            TransportKind::Tcp => Transport::Tcp {
                host: self.in_tcp_host.read(cx).value().to_string(),
                port: self
                    .in_tcp_port
                    .read(cx)
                    .value()
                    .to_string()
                    .trim()
                    .parse()
                    .unwrap_or(0),
            },
            TransportKind::Http => Transport::Http {
                url: self.in_http_url.read(cx).value().to_string(),
                username: self.in_http_user.read(cx).value().to_string(),
            },
        };
        self.test = TestState::Testing;
        cx.notify();
        let services = Arc::clone(&self.services);
        cx.spawn(async move |this, cx| {
            let probed = services.test_connection(transport).await;
            this.update(cx, |this, cx| {
                this.test = match probed {
                    Ok(version) => TestState::Ok(format!("rtorrent {version}")),
                    Err(error) => TestState::Err(error.to_string()),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for PreferencesDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette;
        let pane = self.pane;
        let mut nav = div().w(px(150.)).flex_none().flex().flex_col().gap(px(2.));
        for candidate in Pane::ALL {
            let label = candidate.label().to_owned();
            let selected = candidate == pane;
            nav = nav.child(
                div()
                    .id(SharedString::from(format!("prefs-nav-{label}")))
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .px(px(10.))
                    .rounded(px(4.))
                    .bg(theme::color(if selected {
                        palette.selected
                    } else {
                        palette.app
                    }))
                    .border_l_2()
                    .border_color(theme::color(if selected {
                        palette.accent_cyan
                    } else {
                        palette.app
                    }))
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::color(palette.selected)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pane = candidate;
                        cx.notify();
                    }))
                    .child(theme::text(
                        label,
                        11.,
                        theme::color(if selected {
                            palette.accent_cyan_bright
                        } else {
                            palette.text_body
                        }),
                    )),
            );
        }
        let content = match pane {
            Pane::Behavior => self.behavior_pane(cx),
            Pane::Downloads => self.downloads_pane(cx),
            Pane::Connection => self.connection_pane(cx),
            Pane::Speed => self.speed_pane(cx),
            Pane::BitTorrent => self.bittorrent_pane(cx),
            Pane::Network => self.network_pane(cx),
            Pane::Rss => self.rss_pane(cx),
            Pane::WebUi => self.webui_pane(),
            Pane::Advanced => self.advanced_pane(cx),
        };
        backdrop(&self.focus)
            .on_action(cx.listener(|this, _: &DismissOverlay, _, cx| this.cancel(cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.cancel(cx)),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.cancel(cx);
                }
            }))
            .child(
                panel("preferences-dialog", 660., palette)
                    .child(header(
                        "Preferences",
                        cx.listener(|this, _, _, cx| this.cancel(cx)),
                        palette,
                    ))
                    .child(
                        div()
                            .id("prefs-body")
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .gap(px(12.))
                            .px(px(16.))
                            .py(px(14.))
                            .overflow_y_scroll()
                            .child(nav)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(14.))
                                    .child(content),
                            ),
                    )
                    .child(
                        footer(palette)
                            .child(div().flex_1().min_w_0().child(match self.error.clone() {
                                Some(message) => error_line(&message, palette),
                                None => {
                                    note("Apply validates every field first.".to_owned(), palette)
                                }
                            }))
                            .child(
                                button(
                                    "prefs-cancel",
                                    "Cancel",
                                    ButtonKind::Secondary,
                                    true,
                                    palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                            )
                            .child(
                                button("prefs-apply", "Apply", ButtonKind::Primary, true, palette)
                                    .on_click(cx.listener(|this, _, _, cx| this.apply(cx))),
                            ),
                    ),
            )
    }
}

impl PreferencesDialog {
    /// G6-S4: confirms and notification exclusions.
    fn behavior_pane(&self, cx: &mut Context<Self>) -> Div {
        let palette = self.palette;
        let mut exclusions = div().flex().flex_col().gap(px(4.));
        if self.known_labels.is_empty() {
            exclusions =
                exclusions.child(note("No labels are currently known.".to_owned(), palette));
        }
        for label in self.known_labels.clone() {
            let excluded = self
                .draft
                .completion_notification_excluded_labels
                .iter()
                .any(|candidate| candidate == &label);
            let mark = label.clone();
            exclusions = exclusions.child(dynamic_checkbox(
                format!("prefs-exclude-{label}"),
                label,
                excluded,
                true,
                palette,
                cx.listener(move |this, _, _, cx| {
                    let labels = &mut this.draft.completion_notification_excluded_labels;
                    if labels.iter().any(|candidate| candidate == &mark) {
                        labels.retain(|candidate| candidate != &mark);
                    } else {
                        labels.push(mark.clone());
                    }
                    this.error = None;
                    cx.notify();
                }),
            ));
        }
        group("Behavior", palette)
            .child(checkbox(
                "prefs-show-add-dialog",
                "Show the add-torrent dialog (uncheck to add instantly with defaults)",
                self.draft.show_add_dialog,
                true,
                palette,
                cx.listener(|this, _, _, cx| {
                    this.draft.show_add_dialog = !this.draft.show_add_dialog;
                    this.error = None;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "prefs-confirm-remove",
                "Confirm before removing torrents",
                self.draft.confirm_on_remove,
                true,
                palette,
                cx.listener(|this, _, _, cx| {
                    this.draft.confirm_on_remove = !this.draft.confirm_on_remove;
                    this.error = None;
                    cx.notify();
                }),
            ))
            .child(form_field(
                "Exclude labels from completion notifications",
                exclusions,
                palette,
            ))
    }

    /// G6-S3: save path, incomplete dir, collision policy, move rules, label
    /// defaults, watch folders, run-on-complete.
    fn downloads_pane(&self, cx: &mut Context<Self>) -> Div {
        let palette = self.palette;
        let mut watch = div().flex().flex_col().gap(px(8.));
        if self.watch_folders.is_empty() {
            watch = watch.child(note("No watch folders.".to_owned(), palette));
        }
        for (index, row) in self.watch_folders.iter().enumerate() {
            watch = watch.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .p(px(8.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(theme::color(palette.border_strong))
                    .child(field_row("Folder", text_field(&row.path, palette), palette))
                    .child(field_row("Label", text_field(&row.label, palette), palette))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().flex_1().min_w_0().child(field_row(
                                "Save to",
                                text_field(&row.save_path, palette),
                                palette,
                            )))
                            .child(row_button(
                                format!("prefs-watch-remove-{index}"),
                                "Remove",
                                palette,
                                cx.listener(move |this, _, _, cx| {
                                    this.watch_folders.remove(index);
                                    this.error = None;
                                    cx.notify();
                                }),
                            )),
                    ),
            );
        }
        watch = watch.child(row_button(
            "prefs-watch-add".to_owned(),
            "Add watch folder",
            palette,
            cx.listener(|this, _, window, cx| {
                this.watch_folders.push(WatchRow {
                    path: input(cx, window, "folder to watch", ""),
                    label: input(cx, window, "(none)", ""),
                    save_path: input(cx, window, "(label / global default)", ""),
                });
                this.error = None;
                cx.notify();
            }),
        ));

        let mut defaults = div().flex().flex_col().gap(px(6.));
        if self.label_defaults.is_empty() {
            defaults = defaults.child(note("No per-label defaults.".to_owned(), palette));
        }
        for (index, row) in self.label_defaults.iter().enumerate() {
            defaults = defaults.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .w(px(140.))
                            .flex_none()
                            .child(text_field(&row.label, palette)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(text_field(&row.save_path, palette)),
                    )
                    .child(row_button(
                        format!("prefs-label-remove-{index}"),
                        "Remove",
                        palette,
                        cx.listener(move |this, _, _, cx| {
                            this.label_defaults.remove(index);
                            this.error = None;
                            cx.notify();
                        }),
                    )),
            );
        }
        defaults = defaults.child(row_button(
            "prefs-label-add".to_owned(),
            "Add label default",
            palette,
            cx.listener(|this, _, window, cx| {
                this.label_defaults.push(LabelRow {
                    label: input(cx, window, "label", ""),
                    save_path: input(cx, window, "save path", ""),
                });
                this.error = None;
                cx.notify();
            }),
        ));

        let mut rules = div().flex().flex_col().gap(px(6.));
        if self.move_rules.is_empty() {
            rules = rules.child(note(
                "No rules — completions move to their recorded save path (or stay put). First matching tag wins, then label.".to_owned(),
                palette,
            ));
        }
        for (index, row) in self.move_rules.iter().enumerate() {
            let kind_tag = row.kind_tag;
            rules = rules.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(option_chip(
                        format!("prefs-rule-kind-{index}"),
                        if kind_tag { "tag" } else { "label" },
                        true,
                        palette,
                        cx.listener(move |this, _, _, cx| {
                            if let Some(rule) = this.move_rules.get_mut(index) {
                                rule.kind_tag = !rule.kind_tag;
                            }
                            this.error = None;
                            cx.notify();
                        }),
                    ))
                    .child(
                        div()
                            .w(px(130.))
                            .flex_none()
                            .child(text_field(&row.key, palette)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(text_field(&row.destination, palette)),
                    )
                    .child(row_button(
                        format!("prefs-rule-remove-{index}"),
                        "Remove",
                        palette,
                        cx.listener(move |this, _, _, cx| {
                            this.move_rules.remove(index);
                            this.error = None;
                            cx.notify();
                        }),
                    )),
            );
        }
        rules = rules.child(row_button(
            "prefs-rule-add".to_owned(),
            "Add move rule",
            palette,
            cx.listener(|this, _, window, cx| {
                this.move_rules.push(MoveRow {
                    kind_tag: true,
                    key: input(cx, window, "tag or label", ""),
                    destination: input(cx, window, "destination", ""),
                });
                this.error = None;
                cx.notify();
            }),
        ));

        let collision = self.draft.collision_policy;
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                group("Saving Management", palette).child(field_row(
                    "Default save path",
                    text_field(&self.in_save_path, palette),
                    palette,
                )),
            )
            .child(
                group("Incomplete Torrents + Move on Complete", palette)
                    .child(field_row(
                        "Keep incomplete in",
                        text_field(&self.in_incomplete_dir, palette),
                        palette,
                    ))
                    .child(note(
                        "New downloads land here and move home on completion (recorded per torrent, crash-safe, local daemons only). Empty disables it.".to_owned(),
                        palette,
                    ))
                    .child(field_row(
                        "When taken",
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(option_chip(
                                "prefs-collision-error".to_owned(),
                                "Refuse the move",
                                collision == CollisionPolicy::Error,
                                palette,
                                cx.listener(|this, _, _, cx| {
                                    this.draft.collision_policy = CollisionPolicy::Error;
                                    this.error = None;
                                    cx.notify();
                                }),
                            ))
                            .child(option_chip(
                                "prefs-collision-rename".to_owned(),
                                "Rename with (1), (2), …",
                                collision == CollisionPolicy::AutoRename,
                                palette,
                                cx.listener(|this, _, _, cx| {
                                    this.draft.collision_policy = CollisionPolicy::AutoRename;
                                    this.error = None;
                                    cx.notify();
                                }),
                            )),
                        palette,
                    )),
            )
            .child(group("Move-on-Complete Rules", palette).child(rules))
            .child(
                group("On Completion", palette)
                    .child(field_row(
                        "Run command",
                        text_field(&self.in_run_on_complete, palette),
                        palette,
                    ))
                    .child(note(
                        "Runs on this machine, directly (no shell). Tokens: %N name · %F save path · %H hash.".to_owned(),
                        palette,
                    )),
            )
            .child(group("Watch Folders (auto-add; takes effect on restart)", palette).child(watch))
            .child(
                group("Per-label Default Save Paths", palette)
                    .child(defaults)
                    .child(note(
                        "Adding a torrent with a matching label pre-fills its save path from here.".to_owned(),
                        palette,
                    )),
            )
    }

    /// G6-S2: transport picker, test-connection, poll/stall.
    fn connection_pane(&self, cx: &mut Context<Self>) -> Div {
        let palette = self.palette;
        let kind = self.transport_kind;
        let mut kinds = div().flex().gap(px(6.));
        for (candidate, label) in [
            (TransportKind::Unix, "Unix socket"),
            (TransportKind::Tcp, "TCP"),
            (TransportKind::Http, "HTTP(S)"),
        ] {
            kinds = kinds.child(option_chip(
                format!(
                    "prefs-transport-{}",
                    label.to_lowercase().replace([' ', '(', ')'], "")
                ),
                label,
                kind == candidate,
                palette,
                cx.listener(move |this, _, _, cx| {
                    this.transport_kind = candidate;
                    this.test = TestState::Idle;
                    this.error = None;
                    cx.notify();
                }),
            ));
        }
        let transport_fields: Div = match kind {
            TransportKind::Unix => field_row(
                "Socket path",
                text_field(&self.in_sock_path, palette),
                palette,
            ),
            TransportKind::Tcp => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(field_row(
                    "Host",
                    text_field(&self.in_tcp_host, palette),
                    palette,
                ))
                .child(field_row(
                    "Port",
                    text_field(&self.in_tcp_port, palette),
                    palette,
                )),
            TransportKind::Http => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(field_row(
                    "URL",
                    text_field(&self.in_http_url, palette),
                    palette,
                ))
                .child(field_row(
                    "Username",
                    text_field(&self.in_http_user, palette),
                    palette,
                )),
        };
        let tcp_host = self.in_tcp_host.read(cx).value().to_string();
        let non_local_tcp = kind == TransportKind::Tcp
            && !matches!(tcp_host.trim(), "127.0.0.1" | "::1" | "localhost" | "");
        let test_note: Div = match self.test.clone() {
            TestState::Idle => note(String::new(), palette),
            TestState::Testing => note("Testing…".to_owned(), palette),
            TestState::Ok(version) => {
                theme::text(version, 10.5, theme::color(palette.accent_green_soft)).min_w_0()
            }
            TestState::Err(message) => error_line(&message, palette),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                group("rtorrent Connection", palette)
                    .child(div().flex().gap(px(6.)).child(kinds))
                    .child(transport_fields)
                    .when(non_local_tcp, |column| {
                        column.child(note(
                            "SCGI is unauthenticated: only use a TCP transport on a trusted network.".to_owned(),
                            palette,
                        ))
                    })
                    .child(
                        div().flex().items_center().gap(px(8.))
                            .child(row_button(
                                "prefs-test-connection".to_owned(),
                                "Test connection",
                                palette,
                                cx.listener(|this, _, window, cx| this.test_connection(window, cx)),
                            ))
                            .child(div().flex_1().min_w_0().child(test_note)),
                    ),
            )
            .child(
                group("Polling", palette)
                    .child(field_row(
                        "Poll interval (ms)",
                        text_field(&self.in_poll_ms, palette),
                        palette,
                    ))
                    .child(field_row(
                        "Stall window (s)",
                        text_field(&self.in_stall_window_s, palette),
                        palette,
                    )),
            )
    }
}

impl PreferencesDialog {
    /// G6-S2: global limits, connection caps, queue, turtle mode.
    fn speed_pane(&self, cx: &mut Context<Self>) -> Div {
        let palette = self.palette;
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                group("Speed Limits (KiB/s, 0 = unlimited)", palette)
                    .child(field_row(
                        "Download limit",
                        text_field(&self.in_down_limit, palette),
                        palette,
                    ))
                    .child(field_row(
                        "Upload limit",
                        text_field(&self.in_up_limit, palette),
                        palette,
                    )),
            )
            .child(
                group("Connection Limits (0 = default / unlimited)", palette)
                    .child(field_row(
                        "Max peers / torrent",
                        text_field(&self.in_max_peers, palette),
                        palette,
                    ))
                    .child(field_row(
                        "Upload slots (global)",
                        text_field(&self.in_max_up_active, palette),
                        palette,
                    ))
                    .child(field_row(
                        "Download slots (global)",
                        text_field(&self.in_max_downloads, palette),
                        palette,
                    )),
            )
            .child(
                group("Queue", palette)
                    .child(field_row(
                        "Max active downloads",
                        text_field(&self.in_max_active, palette),
                        palette,
                    ))
                    .child(field_row(
                        "Max active uploads",
                        text_field(&self.in_max_up_active, palette),
                        palette,
                    ))
                    .child(field_row(
                        "Max active torrents",
                        text_field(&self.in_max_total, palette),
                        palette,
                    ))
                    .child(field_row(
                        "Ignore slow below (KiB/s)",
                        text_field(&self.in_slow_limit, palette),
                        palette,
                    ))
                    .child(note(
                        "0 = no limit. Torrents slower than the floor are never held back and never counted; force-started torrents are exempt too.".to_owned(),
                        palette,
                    )),
            )
            .child(self.bandwidth_group(cx))
            .child(
                group("Turtle Mode (alternative limits, KiB/s)", palette)
                    .child(checkbox(
                        "prefs-turtle-manual",
                        "Turtle mode now",
                        self.draft.turtle_enabled,
                        true,
                        palette,
                        cx.listener(|this, _, _, cx| {
                            this.draft.turtle_enabled = !this.draft.turtle_enabled;
                            this.error = None;
                            cx.notify();
                        }),
                    ))
                    .child(field_row(
                        "Download limit",
                        text_field(&self.in_turtle_down, palette),
                        palette,
                    ))
                    .child(field_row(
                        "Upload limit",
                        text_field(&self.in_turtle_up, palette),
                        palette,
                    ))
                    .child(note(
                        "The manual toggle applies these rates whenever no pause window covers the clock.".to_owned(),
                        palette,
                    )),
            )
            .child(self.scheduler_group(cx))
    }

    /// Next-change preview line: best-effort parse of the grid as edited
    /// (unparseable rows simply drop out of the preview; Apply still
    /// validates strictly).
    fn sched_preview(&self, cx: &mut Context<Self>) -> Div {
        use rtorrent_core::schedule as sched;
        let palette = self.palette;
        let text = |input: &Entity<InputState>| input.read(cx).value().to_string();
        let mut windows = Vec::with_capacity(self.sched_windows.len());
        for row in &self.sched_windows {
            let (Ok(start), Ok(end)) = (
                parse_hhmm(&text(&row.start), "start"),
                parse_hhmm(&text(&row.end), "end"),
            ) else {
                continue;
            };
            if start == end {
                continue;
            }
            let (Ok(down), Ok(up)) = (
                parse_int(&text(&row.down), "down"),
                parse_int(&text(&row.up), "up"),
            ) else {
                continue;
            };
            windows.push(sched::SchedWindow {
                days: row.days.clone(),
                start_min: start,
                end_min: end,
                pause: row.pause,
                down_kb: down,
                up_kb: up,
            });
        }
        // Legacy fallback, mirroring the poller: an empty grid still shows
        // the imported turtle window.
        if windows.is_empty() {
            let legacy = &self.draft.turtle_schedule;
            windows = sched::import_legacy(&sched::LegacyWindow {
                enabled: legacy.enabled,
                start_min: legacy.start_min,
                end_min: legacy.end_min,
                days: legacy.days.clone(),
                down_kb: self.draft.turtle_down_kb,
                up_kb: self.draft.turtle_up_kb,
            });
        }
        let now_ms = now_millis();
        let over = match self.sched_override.mode {
            OverrideMode::Off => None,
            mode => {
                let minutes = parse_int(&text(&self.sched_override.minutes), "minutes").ok();
                let (Ok(down), Ok(up)) = (
                    parse_int(&text(&self.sched_override.down), "down"),
                    parse_int(&text(&self.sched_override.up), "up"),
                ) else {
                    return note("Override rates do not parse yet.".to_owned(), palette);
                };
                minutes.filter(|m| *m >= 1).map(|m| sched::TempOverride {
                    pause: mode == OverrideMode::Pause,
                    down_kb: down,
                    up_kb: up,
                    until_ms: now_ms + m.saturating_mul(60_000),
                })
            }
        };
        let manual = self.draft.turtle_enabled.then_some((
            text(&self.in_turtle_down).trim().parse().unwrap_or(0),
            text(&self.in_turtle_up).trim().parse().unwrap_or(0),
        ));
        let (weekday, minute) = crate::turtle::local_day_minute();
        match sched::next_change(&windows, over.as_ref(), manual, weekday, minute, now_ms) {
            Some(change) => {
                const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
                let to = match change.to {
                    sched::SchedKind::Open => "open",
                    sched::SchedKind::Limited => "limited",
                    sched::SchedKind::Paused => "paused",
                };
                let hours = change.in_minutes.div_euclid(60);
                let mins = change.in_minutes.rem_euclid(60);
                let away = if hours > 0 {
                    format!("{hours}h {mins}m")
                } else {
                    format!("{mins}m")
                };
                note(
                    format!(
                        "Next change: {} {:02}:{:02} → {} (in {}).",
                        DAYS[change.weekday as usize % 7],
                        change.minute.div_euclid(60),
                        change.minute.rem_euclid(60),
                        to,
                        away
                    ),
                    palette,
                )
            }
            None => note("No scheduled changes coming up.".to_owned(), palette),
        }
    }

    /// Weekly scheduler grid (V3-18 / QUE-05): windows with limit-vs-pause
    /// modes, a temporary override, and the next-change preview. An
    /// untouched legacy turtle window keeps working underneath until the
    /// grid gains its first window.
    fn scheduler_group(&self, cx: &mut Context<Self>) -> Div {
        let palette = self.palette;
        let legacy = &self.draft.turtle_schedule;
        let mut list = div().flex().flex_col().gap(px(8.));
        if self.sched_windows.is_empty() {
            if legacy.enabled {
                list = list.child(note(
                    format!(
                        "Showing the imported turtle window ({}–{}). Edit the grid to take over; importing disables the legacy window.",
                        mins_to_hhmm(legacy.start_min),
                        mins_to_hhmm(legacy.end_min),
                    ),
                    palette,
                ));
            } else {
                list = list.child(note(
                    "No windows — limits follow the manual toggle and the globals. First matching window wins; pause beats limits.".to_owned(),
                    palette,
                ));
            }
        }
        for (index, row) in self.sched_windows.iter().enumerate() {
            let pause = row.pause;
            let mut days = div().flex().gap(px(4.));
            for (day, name) in ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"]
                .into_iter()
                .enumerate()
            {
                let active = row.days.contains(&(day as u8));
                days = days.child(option_chip(
                    format!("prefs-sched-day-{index}-{day}"),
                    name,
                    active,
                    palette,
                    cx.listener(move |this, _, _, cx| {
                        if let Some(row) = this.sched_windows.get_mut(index) {
                            if row.days.contains(&(day as u8)) {
                                row.days.retain(|d| *d != day as u8);
                            } else {
                                row.days.push(day as u8);
                            }
                        }
                        this.error = None;
                        cx.notify();
                    }),
                ));
            }
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .p(px(8.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(theme::color(palette.border_strong))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(option_chip(
                                format!("prefs-sched-mode-{index}"),
                                if pause { "Pause" } else { "Limit" },
                                true,
                                palette,
                                cx.listener(move |this, _, _, cx| {
                                    if let Some(row) = this.sched_windows.get_mut(index) {
                                        row.pause = !row.pause;
                                    }
                                    this.error = None;
                                    cx.notify();
                                }),
                            ))
                            .child(div().flex_1())
                            .child(row_button(
                                format!("prefs-sched-remove-{index}"),
                                "Remove",
                                palette,
                                cx.listener(move |this, _, _, cx| {
                                    this.sched_windows.remove(index);
                                    this.error = None;
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(field_row("From", text_field(&row.start, palette), palette))
                            .child(theme::text(
                                "to".to_owned(),
                                10.5,
                                theme::color(palette.text_muted),
                            ))
                            .child(field_row("To", text_field(&row.end, palette), palette)),
                    )
                    .when(!pause, |card| {
                        card.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(field_row("Down", text_field(&row.down, palette), palette))
                                .child(field_row("Up", text_field(&row.up, palette), palette))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(note("KiB/s, 0 = unlimited.".to_owned(), palette)),
                                ),
                        )
                    })
                    .child(field_row("Days", days, palette)),
            );
        }
        list = list.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(row_button(
                    "prefs-sched-add".to_owned(),
                    "Add window",
                    palette,
                    cx.listener(|this, _, window, cx| {
                        this.sched_windows.push(SchedRow {
                            days: Vec::new(),
                            start: input(cx, window, "HH:MM", "22:00"),
                            end: input(cx, window, "HH:MM", "06:00"),
                            pause: false,
                            down: input(cx, window, "0 = ∞", ""),
                            up: input(cx, window, "0 = ∞", ""),
                        });
                        this.error = None;
                        cx.notify();
                    }),
                ))
                .child(div().flex_1())
                .child(row_button(
                    "prefs-sched-import".to_owned(),
                    "Import turtle window",
                    palette,
                    cx.listener(|this, _, window, cx| {
                        let legacy = this.draft.turtle_schedule.clone();
                        if legacy.enabled && legacy.start_min != legacy.end_min {
                            this.sched_windows.push(SchedRow {
                                days: legacy.days.clone(),
                                start: input(cx, window, "HH:MM", &mins_to_hhmm(legacy.start_min)),
                                end: input(cx, window, "HH:MM", &mins_to_hhmm(legacy.end_min)),
                                pause: false,
                                down: input(
                                    cx,
                                    window,
                                    "0 = ∞",
                                    &this.draft.turtle_down_kb.to_string(),
                                ),
                                up: input(
                                    cx,
                                    window,
                                    "0 = ∞",
                                    &this.draft.turtle_up_kb.to_string(),
                                ),
                            });
                            // The grid wins once non-empty; switch the legacy
                            // window off so the two cannot disagree.
                            this.draft.turtle_schedule.enabled = false;
                        }
                        this.error = None;
                        cx.notify();
                    }),
                )),
        );
        list = list.child(note(
            "No days selected = every day. An end time earlier than the start wraps past midnight. Wall-clock time, so daylight-saving shifts need no configuration.".to_owned(),
            palette,
        ));
        // Temporary override + next-change preview.
        let over = &self.sched_override;
        let mode = over.mode;
        list = list.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(theme::text(
                    "Override".to_owned(),
                    10.5,
                    theme::color(palette.text_muted),
                ))
                .child(option_chip(
                    "prefs-sched-over-off".to_owned(),
                    "Off",
                    mode == OverrideMode::Off,
                    palette,
                    cx.listener(|this, _, _, cx| {
                        this.sched_override.mode = OverrideMode::Off;
                        this.error = None;
                        cx.notify();
                    }),
                ))
                .child(option_chip(
                    "prefs-sched-over-pause".to_owned(),
                    "Pause",
                    mode == OverrideMode::Pause,
                    palette,
                    cx.listener(|this, _, _, cx| {
                        this.sched_override.mode = OverrideMode::Pause;
                        this.error = None;
                        cx.notify();
                    }),
                ))
                .child(option_chip(
                    "prefs-sched-over-limit".to_owned(),
                    "Limit",
                    mode == OverrideMode::Limit,
                    palette,
                    cx.listener(|this, _, _, cx| {
                        this.sched_override.mode = OverrideMode::Limit;
                        this.error = None;
                        cx.notify();
                    }),
                ))
                .child(
                    div()
                        .w(px(56.))
                        .flex_none()
                        .child(text_field(&over.minutes, palette)),
                )
                .child(theme::text(
                    "min".to_owned(),
                    10.5,
                    theme::color(palette.text_muted),
                )),
        );
        if mode == OverrideMode::Limit {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(field_row("Down", text_field(&over.down, palette), palette))
                    .child(field_row("Up", text_field(&over.up, palette), palette)),
            );
        }
        list = list.child(self.sched_preview(cx));
        group("Weekly Scheduler", palette).child(list)
    }

    /// Bandwidth rules group for the Speed pane (V3-18 / QUE-04).
    fn bandwidth_group(&self, cx: &mut Context<Self>) -> Div {
        let palette = self.palette;
        let mut rules = div().flex().flex_col().gap(px(8.));
        if self.bandwidth.is_empty() {
            rules = rules.child(note(
                "No rules — every torrent follows turtle/global. First matching tag wins, then label; hand-set limits always win over rules.".to_owned(),
                palette,
            ));
        }
        for (index, row) in self.bandwidth.iter().enumerate() {
            let kind_tag = row.kind_tag;
            rules = rules.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .p(px(8.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(theme::color(palette.border_strong))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(option_chip(
                                format!("prefs-bw-kind-{index}"),
                                if kind_tag { "tag" } else { "label" },
                                true,
                                palette,
                                cx.listener(move |this, _, _, cx| {
                                    if let Some(rule) = this.bandwidth.get_mut(index) {
                                        rule.kind_tag = !rule.kind_tag;
                                    }
                                    this.error = None;
                                    cx.notify();
                                }),
                            ))
                            .child(
                                div()
                                    .w(px(130.))
                                    .flex_none()
                                    .child(text_field(&row.key, palette)),
                            )
                            .child(div().flex_1())
                            .child(row_button(
                                format!("prefs-bw-remove-{index}"),
                                "Remove",
                                palette,
                                cx.listener(move |this, _, _, cx| {
                                    this.bandwidth.remove(index);
                                    this.error = None;
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().w(px(52.)).flex_none().child(theme::text(
                                "Down".to_owned(),
                                10.5,
                                theme::color(palette.text_muted),
                            )))
                            .child(
                                div()
                                    .w(px(70.))
                                    .flex_none()
                                    .child(text_field(&row.down, palette)),
                            )
                            .child(div().w(px(36.)).flex_none().child(theme::text(
                                "Up".to_owned(),
                                10.5,
                                theme::color(palette.text_muted),
                            )))
                            .child(
                                div()
                                    .w(px(70.))
                                    .flex_none()
                                    .child(text_field(&row.up, palette)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(note("KiB/s, 0 = unlimited.".to_owned(), palette)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().w(px(52.)).flex_none().child(theme::text(
                                "Peers".to_owned(),
                                10.5,
                                theme::color(palette.text_muted),
                            )))
                            .child(
                                div()
                                    .w(px(56.))
                                    .flex_none()
                                    .child(text_field(&row.peers_max, palette)),
                            )
                            .child(
                                div()
                                    .w(px(56.))
                                    .flex_none()
                                    .child(text_field(&row.peers_min, palette)),
                            )
                            .child(div().w(px(40.)).flex_none().child(theme::text(
                                "Slots".to_owned(),
                                10.5,
                                theme::color(palette.text_muted),
                            )))
                            .child(
                                div()
                                    .w(px(56.))
                                    .flex_none()
                                    .child(text_field(&row.uploads_max, palette)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(note("0 = daemon default.".to_owned(), palette)),
                            ),
                    ),
            );
        }
        rules = rules.child(row_button(
            "prefs-bw-add".to_owned(),
            "Add bandwidth rule",
            palette,
            cx.listener(|this, _, window, cx| {
                this.id_seq += 1;
                let id = format!("bw-{}", this.id_seq);
                this.bandwidth.push(BandwidthRow {
                    id,
                    kind_tag: true,
                    key: input(cx, window, "tag or label", ""),
                    down: input(cx, window, "0 = ∞", ""),
                    up: input(cx, window, "0 = ∞", ""),
                    peers_max: input(cx, window, "0", ""),
                    peers_min: input(cx, window, "0", ""),
                    uploads_max: input(cx, window, "0", ""),
                });
                this.error = None;
                cx.notify();
            }),
        ));
        group("Bandwidth Rules (KiB/s)", palette)
            .child(rules)
            .child(note(
                "Applied on the next poll; deleting a rule returns its torrents to global limits."
                    .to_owned(),
                palette,
            ))
    }

    /// G6-S3: port range, DHT, seed goals + label overrides.
    fn bittorrent_pane(&self, cx: &mut Context<Self>) -> Div {
        let palette = self.palette;
        let action = self.draft.seed_goal_action;
        let mut overrides = div().flex().flex_col().gap(px(6.));
        if self.seed_overrides.is_empty() {
            overrides = overrides.child(note("No per-label overrides.".to_owned(), palette));
        }
        for (index, row) in self.seed_overrides.iter().enumerate() {
            overrides = overrides.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .w(px(120.))
                            .flex_none()
                            .child(text_field(&row.label, palette)),
                    )
                    .child(
                        div()
                            .w(px(80.))
                            .flex_none()
                            .child(text_field(&row.ratio, palette)),
                    )
                    .child(
                        div()
                            .w(px(80.))
                            .flex_none()
                            .child(text_field(&row.hours, palette)),
                    )
                    .child(row_button(
                        format!("prefs-seed-remove-{index}"),
                        "Remove",
                        palette,
                        cx.listener(move |this, _, _, cx| {
                            this.seed_overrides.remove(index);
                            this.error = None;
                            cx.notify();
                        }),
                    )),
            );
        }
        overrides = overrides.child(row_button(
            "prefs-seed-add".to_owned(),
            "Add label override",
            palette,
            cx.listener(|this, _, window, cx| {
                this.seed_overrides.push(SeedRow {
                    label: input(cx, window, "label", ""),
                    ratio: input(cx, window, "0 = off", ""),
                    hours: input(cx, window, "0 = off", ""),
                });
                this.error = None;
                cx.notify();
            }),
        ));
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                group("BitTorrent", palette)
                    .child(field_row(
                        "Listen port range",
                        text_field(&self.in_port_range, palette),
                        palette,
                    ))
                    .child(checkbox(
                        "prefs-dht",
                        "Enable DHT (distributed hash table)",
                        self.draft.dht_enabled,
                        true,
                        palette,
                        cx.listener(|this, _, _, cx| {
                            this.draft.dht_enabled = !this.draft.dht_enabled;
                            this.error = None;
                            cx.notify();
                        }),
                    ))
                    .child(note(
                        "Port and DHT changes may require an rtorrent restart to take full effect."
                            .to_owned(),
                        palette,
                    )),
            )
            .child(
                group("Seeding limits", palette)
                    .child(field_row(
                        "When goal reached",
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(option_chip(
                                "prefs-goal-stop".to_owned(),
                                "Stop seeding",
                                action == SeedGoalAction::Stop,
                                palette,
                                cx.listener(|this, _, _, cx| {
                                    this.draft.seed_goal_action = SeedGoalAction::Stop;
                                    this.error = None;
                                    cx.notify();
                                }),
                            ))
                            .child(option_chip(
                                "prefs-goal-remove".to_owned(),
                                "Remove torrent",
                                action == SeedGoalAction::Remove,
                                palette,
                                cx.listener(|this, _, _, cx| {
                                    this.draft.seed_goal_action = SeedGoalAction::Remove;
                                    this.error = None;
                                    cx.notify();
                                }),
                            ))
                            .child(option_chip(
                                "prefs-goal-remove-data".to_owned(),
                                "Remove + data",
                                action == SeedGoalAction::RemoveData,
                                palette,
                                cx.listener(|this, _, _, cx| {
                                    this.draft.seed_goal_action = SeedGoalAction::RemoveData;
                                    this.error = None;
                                    cx.notify();
                                }),
                            )),
                        palette,
                    ))
                    .child(field_row(
                        "Stop at ratio",
                        text_field(&self.in_seed_ratio, palette),
                        palette,
                    ))
                    .child(field_row(
                        "…or after hours",
                        text_field(&self.in_seed_hours, palette),
                        palette,
                    ))
                    .child(note(
                        "Empty or 0 disables a rule. The first reached rule stops the torrent."
                            .to_owned(),
                        palette,
                    ))
                    .child(form_field(
                        "Label overrides (label · ratio · hours)",
                        overrides,
                        palette,
                    )),
            )
    }

    /// G6-S4: encryption, PEX, proxy, binding.
    fn network_pane(&self, cx: &mut Context<Self>) -> Div {
        let palette = self.palette;
        let encryption = self.draft.encryption;
        let mut modes = div().flex().gap(px(6.));
        for (mode, label) in [
            (EncryptionMode::Disabled, "Disabled"),
            (EncryptionMode::Allow, "Allow"),
            (EncryptionMode::Prefer, "Prefer"),
            (EncryptionMode::Require, "Require"),
        ] {
            modes = modes.child(option_chip(
                format!("prefs-encryption-{}", label.to_lowercase()),
                label,
                encryption == mode,
                palette,
                cx.listener(move |this, _, _, cx| {
                    this.draft.encryption = mode;
                    this.error = None;
                    cx.notify();
                }),
            ));
        }
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(group("Encryption", palette).child(field_row("Protocol encryption", modes, palette)))
            .child(
                group("Peer Exchange", palette).child(checkbox(
                    "prefs-pex",
                    "Enable peer exchange (PEX)",
                    self.draft.pex_enabled,
                    true,
                    palette,
                    cx.listener(|this, _, _, cx| {
                        this.draft.pex_enabled = !this.draft.pex_enabled;
                        this.error = None;
                        cx.notify();
                    }),
                )),
            )
            .child(
                group("Proxy", palette)
                    .child(field_row(
                        "HTTP proxy",
                        text_field(&self.in_proxy, palette),
                        palette,
                    ))
                    .child(checkbox(
                        "prefs-proxy-trackers",
                        "Route tracker requests through the proxy",
                        self.draft.proxy_tracker_http,
                        true,
                        palette,
                        cx.listener(|this, _, _, cx| {
                            this.draft.proxy_tracker_http = !this.draft.proxy_tracker_http;
                            this.error = None;
                            cx.notify();
                        }),
                    ))
                    .child(note(
                        "Only tracker announces go through the proxy — never peer traffic.".to_owned(),
                        palette,
                    )),
            )
            .child(
                group("Binding", palette)
                    .child(field_row(
                        "Bind address",
                        text_field(&self.in_bind, palette),
                        palette,
                    ))
                    .child(field_row(
                        "Local address",
                        text_field(&self.in_local, palette),
                        palette,
                    ))
                    .child(note(
                        "Empty means the daemon default. Bind to a VPN interface address to keep traffic on it.".to_owned(),
                        palette,
                    )),
            )
    }
}

impl PreferencesDialog {
    /// G6-S4: feeds, auto-download rules, poll interval.
    fn rss_pane(&self, cx: &mut Context<Self>) -> Div {
        let palette = self.palette;
        let mut feeds = div().flex().flex_col().gap(px(8.));
        if self.feeds.is_empty() {
            feeds = feeds.child(note("No feeds.".to_owned(), palette));
        }
        for (index, row) in self.feeds.iter().enumerate() {
            let enabled = row.enabled;
            feeds = feeds.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .p(px(8.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(theme::color(palette.border_strong))
                    .child(field_row("Name", text_field(&row.name, palette), palette))
                    .child(field_row("URL", text_field(&row.url, palette), palette))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(dynamic_checkbox(
                                format!("prefs-feed-enabled-{index}"),
                                "Enabled".to_owned(),
                                enabled,
                                true,
                                palette,
                                cx.listener(move |this, _, _, cx| {
                                    if let Some(feed) = this.feeds.get_mut(index) {
                                        feed.enabled = !feed.enabled;
                                    }
                                    this.error = None;
                                    cx.notify();
                                }),
                            ))
                            .child(div().flex_1())
                            .child(row_button(
                                format!("prefs-feed-remove-{index}"),
                                "Remove",
                                palette,
                                cx.listener(move |this, _, _, cx| {
                                    this.feeds.remove(index);
                                    this.error = None;
                                    cx.notify();
                                }),
                            )),
                    ),
            );
        }
        feeds = feeds.child(row_button(
            "prefs-feed-add".to_owned(),
            "Add feed",
            palette,
            cx.listener(|this, _, window, cx| {
                this.id_seq += 1;
                let id = format!("feed-{}", this.id_seq);
                this.feeds.push(FeedRow {
                    id,
                    name: input(cx, window, "feed name", ""),
                    url: input(cx, window, "https://…/feed", ""),
                    enabled: true,
                });
                this.error = None;
                cx.notify();
            }),
        ));

        let mut rules = div().flex().flex_col().gap(px(8.));
        if self.rules.is_empty() {
            rules = rules.child(note("No auto-download rules.".to_owned(), palette));
        }
        for (index, row) in self.rules.iter().enumerate() {
            let enabled = row.enabled;
            let feed_id = row.feed_id.clone();
            let mut feed_picker = div().flex().flex_wrap().gap(px(4.));
            let all = ("".to_owned(), "All feeds".to_owned());
            for (id, name) in std::iter::once(all).chain(
                self.feeds
                    .iter()
                    .map(|feed| (feed.id.clone(), feed.name.read(cx).value().to_string())),
            ) {
                let label = if name.trim().is_empty() {
                    "(unnamed)".to_owned()
                } else {
                    name
                };
                feed_picker = feed_picker.child(option_chip(
                    format!("prefs-rule-{index}-feed-{id}"),
                    &label,
                    feed_id == id,
                    palette,
                    cx.listener(move |this, _, _, cx| {
                        if let Some(rule) = this.rules.get_mut(index) {
                            rule.feed_id = id.clone();
                        }
                        this.error = None;
                        cx.notify();
                    }),
                ));
            }
            rules = rules.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .p(px(8.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(theme::color(palette.border_strong))
                    .child(field_row(
                        "Rule name",
                        text_field(&row.name, palette),
                        palette,
                    ))
                    .child(form_field("Feed", feed_picker, palette))
                    .child(field_row(
                        "Must contain",
                        text_field(&row.must, palette),
                        palette,
                    ))
                    .child(field_row(
                        "Must not contain",
                        text_field(&row.must_not, palette),
                        palette,
                    ))
                    .child(field_row(
                        "Regex",
                        text_field(&row.regex, palette),
                        palette,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().w(px(120.)).flex_none().child(theme::text(
                                "Seasons".to_owned(),
                                10.5,
                                theme::color(palette.text_muted),
                            )))
                            .child(div().w(px(110.)).flex_none().child(text_field(
                                &row.seasons,
                                palette,
                            )))
                            .child(theme::text(
                                "Episodes".to_owned(),
                                10.5,
                                theme::color(palette.text_muted),
                            ))
                            .child(div().w(px(110.)).flex_none().child(text_field(
                                &row.episodes,
                                palette,
                            ))),
                    )
                    .child(note(
                        "Seasons/episodes as 2, 1-3 or 1,4-6; blank means any. Titles without episode info never match a set filter.".to_owned(),
                        palette,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().w(px(120.)).flex_none().child(theme::text(
                                "Size MiB".to_owned(),
                                10.5,
                                theme::color(palette.text_muted),
                            )))
                            .child(div().w(px(70.)).flex_none().child(text_field(
                                &row.min_size,
                                palette,
                            )))
                            .child(theme::text(
                                "–".to_owned(),
                                10.5,
                                theme::color(palette.text_muted),
                            ))
                            .child(div().w(px(70.)).flex_none().child(text_field(
                                &row.max_size,
                                palette,
                            )))
                            .child(div().flex_1().min_w_0().child(note(
                                "0 = no bound; unknown feed sizes fail a set bound.".to_owned(),
                                palette,
                            ))),
                    )
                    .child(field_row(
                        "Prefer quality",
                        text_field(&row.prefer_quality, palette),
                        palette,
                    ))
                    .child(dynamic_checkbox(
                        format!("prefs-rule-smart-{index}"),
                        "Smart episode: keep only the best release per episode".to_owned(),
                        row.smart,
                        true,
                        palette,
                        cx.listener(move |this, _, _, cx| {
                            if let Some(rule) = this.rules.get_mut(index) {
                                rule.smart = !rule.smart;
                            }
                            this.error = None;
                            cx.notify();
                        }),
                    ))
                    .child(field_row("Label", text_field(&row.label, palette), palette))
                    .child(field_row(
                        "Tags",
                        text_field(&row.tags, palette),
                        palette,
                    ))
                    .child(field_row(
                        "Save to",
                        text_field(&row.save_path, palette),
                        palette,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(dynamic_checkbox(
                                format!("prefs-rule-start-{index}"),
                                "Start on add".to_owned(),
                                row.start,
                                true,
                                palette,
                                cx.listener(move |this, _, _, cx| {
                                    if let Some(rule) = this.rules.get_mut(index) {
                                        rule.start = !rule.start;
                                    }
                                    this.error = None;
                                    cx.notify();
                                }),
                            ))
                            .child(dynamic_checkbox(
                                format!("prefs-rule-top-{index}"),
                                "Top of queue".to_owned(),
                                row.top,
                                true,
                                palette,
                                cx.listener(move |this, _, _, cx| {
                                    if let Some(rule) = this.rules.get_mut(index) {
                                        rule.top = !rule.top;
                                    }
                                    this.error = None;
                                    cx.notify();
                                }),
                            ))
                            .child(div().w(px(90.)).flex_none().child(text_field(
                                &row.interval,
                                palette,
                            )))
                            .child(theme::text(
                                "min (0 = global)".to_owned(),
                                10.5,
                                theme::color(palette.text_muted),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(dynamic_checkbox(
                                format!("prefs-rule-enabled-{index}"),
                                "Enabled".to_owned(),
                                enabled,
                                true,
                                palette,
                                cx.listener(move |this, _, _, cx| {
                                    if let Some(rule) = this.rules.get_mut(index) {
                                        rule.enabled = !rule.enabled;
                                    }
                                    this.error = None;
                                    cx.notify();
                                }),
                            ))
                            .child(div().flex_1())
                            .child(row_button(
                                format!("prefs-rule-test-{index}"),
                                if self.rss_testing { "Testing…" } else { "Test" },
                                palette,
                                cx.listener(move |this, _, _, cx| {
                                    this.test_rule(index, cx);
                                }),
                            ))
                            .child(row_button(
                                format!("prefs-rule-remove-{index}"),
                                "Remove",
                                palette,
                                cx.listener(move |this, _, _, cx| {
                                    this.rules.remove(index);
                                    this.error = None;
                                    cx.notify();
                                }),
                            )),
                    ),
            );
            // The last test report, shown under the rule it belongs to.
            if let Some((test_id, result)) = &self.rss_test {
                if test_id == &row.id {
                    let mut report = div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .p(px(8.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(theme::color(palette.border_strong));
                    match result {
                        Err(err) => {
                            report = report.child(error_line(err, palette));
                        }
                        Ok(test) if test.rows.is_empty() => {
                            report = report
                                .child(note("No items in the rule's feeds.".to_owned(), palette));
                        }
                        Ok(test) => {
                            for item in test.rows.iter().take(12) {
                                let mark = if item.matched { "✓" } else { "✗" };
                                let colour = if item.matched {
                                    palette.accent_green_soft
                                } else {
                                    palette.text_dim
                                };
                                let mut line = format!("{mark} {}", item.title);
                                if !item.matched && !item.reason.is_empty() {
                                    line.push_str(&format!(" — {}", item.reason));
                                }
                                report =
                                    report.child(theme::text(line, 10.5, theme::color(colour)));
                            }
                            if test.rows.len() > 12 {
                                report = report.child(note(
                                    format!("…and {} more", test.rows.len() - 12),
                                    palette,
                                ));
                            }
                        }
                    }
                    rules = rules.child(report);
                }
            }
        }
        rules = rules.child(row_button(
            "prefs-rule-add".to_owned(),
            "Add rule",
            palette,
            cx.listener(|this, _, window, cx| {
                this.id_seq += 1;
                let id = format!("rule-{}", this.id_seq);
                this.rules.push(RuleRow {
                    id,
                    name: input(cx, window, "rule name", ""),
                    feed_id: String::new(),
                    must: input(cx, window, "must contain", ""),
                    must_not: input(cx, window, "must not contain", ""),
                    regex: input(cx, window, "optional regex", ""),
                    seasons: input(cx, window, "e.g. 1-3", ""),
                    episodes: input(cx, window, "e.g. 4-12", ""),
                    min_size: input(cx, window, "MiB, 0 = off", ""),
                    max_size: input(cx, window, "MiB, 0 = off", ""),
                    prefer_quality: input(cx, window, "e.g. 1080p", ""),
                    smart: false,
                    label: input(cx, window, "(none)", ""),
                    tags: input(cx, window, "comma-separated", ""),
                    save_path: input(cx, window, "(label / global default)", ""),
                    start: true,
                    top: false,
                    interval: input(cx, window, "0 = global", ""),
                    enabled: true,
                });
                this.error = None;
                cx.notify();
            }),
        ));

        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                group("RSS", palette)
                    .child(field_row(
                        "Poll interval (min)",
                        text_field(&self.in_rss_poll, palette),
                        palette,
                    ))
                    .child(note(
                        "0 disables background polling. Matching items auto-add deduped against a persisted seen-set.".to_owned(),
                        palette,
                    ))
                    .child(self.seen_row(cx)),
            )
            .child(group("Feeds", palette).child(feeds))
            .child(group("Auto-download Rules", palette).child(rules))
    }

    /// Seen-set row: count plus Clear (V3-23). Export travels through the
    /// desktop shell, which has a file picker; here Clear is the escape
    /// hatch after fixing a broken rule.
    fn seen_row(&self, cx: &mut Context<Self>) -> Div {
        let palette = self.palette;
        let count = self.model.read(cx).rss_seen_count();
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(theme::text(
                format!("{count} remembered downloads"),
                10.5,
                theme::color(palette.text_muted),
            ))
            .child(div().flex_1())
            .child(row_button(
                "prefs-rss-clear-seen".to_owned(),
                "Clear seen-set",
                palette,
                cx.listener(|this, _, _, cx| {
                    this.model.update(cx, |model, cx| {
                        model.rss_clear_seen(cx);
                    });
                }),
            ))
    }

    /// G6-S5: the Web UI stays a documented stub — hosting is toggled from
    /// the menu, and server identity lives in its TOML, not in Settings.
    fn webui_pane(&self) -> Div {
        let palette = self.palette;
        group("Web UI", palette).child(note(
            "The browser UI is served from inside this app and toggled from the menu — there is nothing to configure here. Server identity (name, password, bind address) lives in rtorrent-web.toml for hosted installs.".to_owned(),
            palette,
        ))
    }

    /// G6-S5: mock-mode toggle for offline development.
    fn advanced_pane(&self, cx: &mut Context<Self>) -> Div {
        let palette = self.palette;
        group("Advanced", palette)
            .child(checkbox(
                "prefs-mock",
                "Mock mode (fixture torrents, no daemon)",
                self.draft.mock,
                true,
                palette,
                cx.listener(|this, _, _, cx| {
                    this.draft.mock = !this.draft.mock;
                    this.error = None;
                    cx.notify();
                }),
            ))
            .child(note(
                "Mock mode restarts the connection on the fixture backend. Diagnostics beyond this live in the Log pane.".to_owned(),
                palette,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_accept_empty_as_zero_and_reject_negatives() {
        assert_eq!(parse_int("", "Limit"), Ok(0));
        assert_eq!(parse_int("  ", "Limit"), Ok(0));
        assert_eq!(parse_int("512", "Limit"), Ok(512));
        assert_eq!(
            parse_int("-1", "Limit"),
            Err("Limit: must be zero or greater".to_owned())
        );
        assert!(parse_int("1.5", "Limit").is_err());
        assert!(parse_int("abc", "Limit").is_err());
    }

    #[test]
    fn floats_accept_empty_as_off_and_reject_negatives() {
        assert_eq!(parse_float("", "Ratio"), Ok(0.0));
        assert_eq!(parse_float("2.5", "Ratio"), Ok(2.5));
        assert!(parse_float("-0.5", "Ratio").is_err());
        assert!(parse_float("nan", "Ratio").is_err());
    }

    #[test]
    fn ports_stay_inside_1_to_65535() {
        assert_eq!(parse_port("5000"), Ok(5000));
        assert!(parse_port("0").is_err());
        assert!(parse_port("65536").is_err());
        assert!(parse_port("").is_err());
    }

    #[test]
    fn hhmm_parses_a_window_and_rejects_clock_nonsense() {
        assert_eq!(parse_hhmm("23:00", "Start"), Ok(1380));
        assert_eq!(parse_hhmm("00:00", "Start"), Ok(0));
        assert_eq!(parse_hhmm("9:05", "Start"), Ok(545));
        assert!(parse_hhmm("24:00", "Start").is_err());
        assert!(parse_hhmm("12:60", "Start").is_err());
        assert!(parse_hhmm("noon", "Start").is_err());
        assert_eq!(mins_to_hhmm(0), "00:00");
        assert_eq!(mins_to_hhmm(1380), "23:00");
    }

    #[test]
    fn transport_kind_follows_the_live_document() {
        let mut settings = Settings::default();
        assert_eq!(TransportKind::of(&settings.transport), TransportKind::Unix);
        settings.transport = Transport::Tcp {
            host: "10.0.0.5".into(),
            port: 5000,
        };
        assert_eq!(TransportKind::of(&settings.transport), TransportKind::Tcp);
        settings.transport = Transport::Http {
            url: "https://box/RPC2".into(),
            username: String::new(),
        };
        assert_eq!(TransportKind::of(&settings.transport), TransportKind::Http);
    }
}
