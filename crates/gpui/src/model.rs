//! The torrent list as the views see it.
//!
//! `TorrentsModel` is the GPUI-side owner of the snapshot stream: it holds
//! every DTO, the connection state and the globals, polls the daemon on the
//! configured cadence (with backoff while disconnected), and prunes selection
//! to existing torrents. Views read it directly; every mutation goes through
//! [`Services`], which is followed by an immediate re-poll so the UI reflects
//! what the daemon actually did rather than what was asked for.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{Context, Global};
use rtorrent_core::types::{
    AddOptions, ConnPhase, ConnState, DetailPayload, DetailTab, GlobalStats, LogLevel, Status,
    TorrentDto,
};

use crate::app_log::AppLog;
use crate::detail_panes::{self, Pane};
use crate::rate_history::RateHistory;
use crate::services::{PeerVerb, QueueDirection, ServiceTask, Services};
use crate::settings::{Settings, SettingsStore};
use crate::table_state::{self, Filter, Selection, Sort};
use crate::web_host::{self, WebHost};

/// Backoff schedule (seconds) after consecutive fast-poll failures.
const BACKOFF: [u64; 4] = [1, 2, 5, 10];
/// Max new tracker hosts resolved per tick, to avoid a burst on first load.
const TRACKERS_PER_TICK: usize = 5;
/// File lists indexed per batch, and how many ticks apart (V3-12). One
/// `f.multicall` per torrent is the cost, so ten seconds apart keeps it off the
/// fast poll while a large library still fills within minutes.
const FILE_INDEX_BATCH: usize = 5;
const FILE_INDEX_EVERY_TICKS: u64 = 10;
/// The detail poll's cadence: slower than the list, because a pane's data is
/// bigger, changes more slowly, and nothing else waits on it.
const DETAIL_MS: u64 = 2_000;

pub struct TorrentsModel {
    services: Arc<Services>,
    settings_store: SettingsStore,
    torrents: Vec<TorrentDto>,
    globals: GlobalStats,
    connection: ConnState,
    filter: Filter,
    search: String,
    sort: Sort,
    selection: Selection,
    failures: usize,
    polling: bool,
    detail_polling: bool,
    /// The pane the panel is showing, and the thing it polls for.
    active_tab: Pane,
    /// What the detail poll is watching: the focused torrent and the pane's
    /// daemon tab. `None` when nothing is selected, or the pane has no payload
    /// to fetch.
    detail_watch: Option<(String, DetailTab)>,
    /// The last detail payload that arrived for the current watch. Its own field
    /// on purpose: a pane's payload must never re-render the table.
    detail: Option<DetailPayload>,
    /// Per-torrent rate samples, taken as the list is polled.
    rates: RateHistory,
    /// The app's own log, for the Log pane.
    log: AppLog,
    /// Poll-side policy memory (seed-goal records, completion flags, pushed
    /// limits). One per model, shared with the policy continuation the way the
    /// backend `Arc` is shared with daemon calls.
    policy_state: Arc<std::sync::Mutex<crate::policy::PolicyState>>,
    /// Lazily built file-path index for library search (V3-12). Shared the same
    /// way the policy state is: one instance per model, filled by a bounded
    /// batch each poll.
    file_index: Arc<std::sync::Mutex<rtorrent_core::file_index::FileIndex>>,
    /// Successful poll ticks, for the file index's slower cadence.
    ticks: u64,
    /// The web UI, while the app is serving it.
    web: WebHost,
    /// Move-on-complete journal + live tasks (V3-14), shared with the policy
    /// continuation the way the backend `Arc` is shared with daemon calls.
    moves: Arc<std::sync::Mutex<rtorrent_core::mover::MoveStore>>,
    /// Last-seen journal states by op id, for logging terminal transitions.
    known_moves: std::collections::HashMap<String, (rtorrent_core::complete::MoveState, String)>,
    /// The status notice currently shows move progress (so a finished move
    /// clears it without eating anyone else's notice).
    move_notice: bool,
    /// The move journal was recovered once after startup.
    moves_resumed: bool,
    /// RSS auto-download runner state (V3-23). Taken by the slow loop each
    /// pass and put back, so only one pass ever runs at a time.
    rss: Option<crate::rss::Runner>,
    rss_polling: bool,
    /// The last thing the user should know about: an action that failed, or a
    /// settings save that could not be written. The status bar shows this until
    /// the next successful action clears it.
    notice: Option<String>,
}

impl TorrentsModel {
    pub fn new(services: Arc<Services>, settings_store: SettingsStore) -> Self {
        let settings = services.settings();
        let rss = crate::rss::Runner::new(Arc::clone(&services), &settings_store);
        let filter = parse_filter(&settings.filter);
        let sort = parse_sort(&settings.sort_column, settings.sort_descending);
        // The journal lives beside the settings file, like the Tauri shell's.
        let journal_path = settings_store
            .path()
            .parent()
            .map(|dir| dir.join("move-journal.json"))
            .unwrap_or_else(|| std::path::PathBuf::from("move-journal.json"));
        let moves = Arc::new(std::sync::Mutex::new(
            rtorrent_core::mover::MoveStore::load(journal_path),
        ));
        // A pane this build does not have (or a hand-edited file) falls back
        // rather than leaving the panel blank.
        let active_tab = Pane::parse(&settings.active_tab).unwrap_or(detail_panes::DEFAULT_PANE);
        Self {
            services,
            settings_store,
            torrents: Vec::new(),
            globals: rtorrent_core::snapshot::empty_globals(),
            connection: ConnState {
                phase: ConnPhase::Connecting,
                endpoint: String::new(),
                daemon_version: None,
                error: None,
                retry_in_seconds: None,
            },
            filter,
            search: settings.search.clone(),
            sort,
            selection: Selection::default(),
            failures: 0,
            polling: false,
            detail_polling: false,
            active_tab,
            detail_watch: None,
            detail: None,
            rates: RateHistory::default(),
            log: AppLog::default(),
            policy_state: Arc::new(std::sync::Mutex::new(crate::policy::PolicyState::default())),
            file_index: Arc::new(std::sync::Mutex::new(
                rtorrent_core::file_index::FileIndex::default(),
            )),
            ticks: 0,
            web: WebHost::default(),
            moves,
            known_moves: std::collections::HashMap::new(),
            move_notice: false,
            moves_resumed: false,
            rss: Some(rss),
            rss_polling: false,
            notice: None,
        }
    }

    /// Start the ~1s poll loop and the slower detail loop. Safe to call once;
    /// further calls are no-ops.
    pub fn start_polling(&mut self, cx: &mut Context<Self>) {
        if self.polling {
            return;
        }
        self.polling = true;
        self.poll_once(cx);
        self.autostart_daemon(cx);
        self.start_detail_polling(cx);
        self.start_rss_polling(cx);
    }

    /// The RSS loop: a slow pass on GPUI's executor that hands the work to
    /// the services runtime (plain HTTPS needs a reactor) and puts the
    /// runner back when the pass lands, so passes never overlap.
    fn start_rss_polling(&mut self, cx: &mut Context<Self>) {
        if self.rss_polling {
            return;
        }
        self.rss_polling = true;
        self.rss_once(cx);
    }

    fn rss_once(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let work = this
                .update(cx, |this: &mut Self, _| {
                    this.rss.take().map(|runner| {
                        (
                            Arc::clone(&this.services),
                            this.settings_store.clone(),
                            runner,
                        )
                    })
                })
                .ok()
                .flatten();
            if let Some((services, settings_store, mut runner)) = work {
                let outcome = services
                    .runtime()
                    .spawn(async move {
                        let logs = runner.tick().await;
                        (runner, logs)
                    })
                    .await;
                // A panicking pass must not lose the runner: rebuild it.
                let (runner, logs) = match outcome {
                    Ok(done) => done,
                    Err(_) => (
                        crate::rss::Runner::new(Arc::clone(&services), &settings_store),
                        Vec::new(),
                    ),
                };
                this.update(cx, |this: &mut Self, cx| {
                    for line in logs {
                        this.log.push(line.level, line.message, line.hash);
                    }
                    this.rss = Some(runner);
                    cx.notify();
                })
                .ok();
            }
            cx.background_executor()
                .timer(crate::rss::tick_interval())
                .await;
            this.update(cx, |this, cx| this.rss_once(cx)).ok();
        })
        .detach();
    }

    /// The panel's own poll: whatever pane is in view, on its own cadence, so a
    /// larger payload never touches the list's tick.
    fn start_detail_polling(&mut self, cx: &mut Context<Self>) {
        if self.detail_polling {
            return;
        }
        self.detail_polling = true;
        self.refresh_detail(cx);
        self.poll_detail_once(cx);
    }

    fn poll_detail_once(&mut self, cx: &mut Context<Self>) {
        let services = Arc::clone(&self.services);
        let watch = self.detail_watch.clone();
        cx.spawn(async move |this, cx| {
            if let Some((hash, tab)) = watch {
                let payload = fetch_detail(&services, hash, tab).await;
                if let Some(payload) = payload {
                    this.update(cx, |this, cx| this.apply_detail(payload, cx))
                        .ok();
                }
            }
            // The detail cadence, or an early wake when something asked for a
            // refresh: an action's re-poll, a settings change, a new watch.
            //
            // GPUI's own timer, not `tokio::time::sleep`: this runs on GPUI's
            // executor, which has no Tokio reactor behind it to hang a sleep on.
            // `repoll` is a plain notification, so it is safe to await here.
            let cadence = cx
                .background_executor()
                .timer(Duration::from_millis(DETAIL_MS));
            let woken = services.repoll.notified();
            futures::pin_mut!(cadence);
            futures::pin_mut!(woken);
            futures::future::select(cadence, woken).await;
            this.update(cx, |this, cx| this.poll_detail_once(cx)).ok();
        })
        .detach();
    }

    /// Keep a payload only if it still answers what the panel is showing: a
    /// reply that arrived after the selection moved on must not overwrite it.
    fn apply_detail(&mut self, payload: DetailPayload, cx: &mut Context<Self>) {
        let current = self
            .detail_watch
            .as_ref()
            .is_some_and(|(hash, tab)| *hash == payload.hash && *tab == payload.tab);
        if current {
            self.detail = Some(payload);
            cx.notify();
        }
    }

    /// Point the detail poll at what the panel shows, dropping any payload that
    /// belonged to something else, and wake it now.
    fn refresh_detail(&mut self, cx: &mut Context<Self>) {
        let watch = detail_panes::focused_hash(&self.selection)
            .map(|hash| (hash, self.active_tab.daemon_tab()));
        if watch != self.detail_watch {
            // A different subject or pane: the payload in hand describes neither.
            self.detail = None;
            self.detail_watch = watch;
        }
        self.services.retry_now();
        cx.notify();
    }

    fn poll_once(&mut self, cx: &mut Context<Self>) {
        let services = Arc::clone(&self.services);
        let task = services.list_snapshot();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let globals = services.global_stats().await;
            let version = if this
                .update(cx, |this, _| this.failures > 0)
                .unwrap_or(false)
            {
                services.client_version().await.ok()
            } else {
                None
            };
            this.update(cx, |this, cx| {
                this.apply_tick(result, globals.ok(), version, cx);
                let delay = this.next_delay_ms();
                cx.notify();
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(delay))
                        .await;
                    this.update(cx, |this, cx| this.poll_once(cx)).ok();
                })
                .detach();
            })
            .ok();
        })
        .detach();
    }

    fn apply_tick(
        &mut self,
        result: Result<
            Vec<rtorrent_core::rtorrent::RawTorrent>,
            rtorrent_core::rtorrent::RtorrentError,
        >,
        globals: Option<rtorrent_core::rtorrent::RawGlobal>,
        version: Option<String>,
        cx: &mut Context<Self>,
    ) {
        match (result, globals) {
            (Ok(raw), Some(globals)) => {
                let was_connected = self.connection.phase == ConnPhase::Connected;
                self.failures = 0;
                let settings = self.services.settings();
                if !was_connected {
                    self.connection = ConnState {
                        phase: ConnPhase::Connected,
                        endpoint: crate::settings::endpoint_label(&settings.transport),
                        daemon_version: version,
                        error: None,
                        retry_in_seconds: None,
                    };
                    self.notice = None;
                    let where_to = self.connection.endpoint.clone();
                    let version = self
                        .connection
                        .daemon_version
                        .clone()
                        .unwrap_or_else(|| "unknown version".to_owned());
                    self.log.push(
                        LogLevel::Info,
                        format!("connected to rtorrent {version} at {where_to}"),
                        None,
                    );
                    // A new session is judged fresh: goal memory and completion
                    // flags start over, and the pushed-limits memory clears so
                    // the reconnect replays throttles and network prefs (the
                    // daemon forgets its runtime config on restart).
                    self.policy_state.lock().unwrap().on_reconnect(&settings);
                    // The old daemon's file lists must not answer a search
                    // against this one (V3-12): index from scratch.
                    if let Ok(mut guard) = self.file_index.lock() {
                        guard.clear();
                    }
                }
                // Resolve tracker hosts for hashes we haven't seen yet.
                let unknown: Vec<String> = raw
                    .iter()
                    .map(|torrent| torrent.hash.clone())
                    .filter(|hash| self.services.tracker_host(hash).is_empty())
                    .take(TRACKERS_PER_TICK)
                    .collect();
                if !unknown.is_empty() {
                    let services = Arc::clone(&self.services);
                    cx.spawn(async move |this, cx| {
                        let mut resolved = Vec::new();
                        for hash in unknown {
                            if let Ok(host) = services.primary_tracker(hash.clone()).await {
                                resolved.push((hash, host));
                            }
                        }
                        this.update(cx, |this: &mut Self, cx| {
                            this.services.remember_trackers(resolved);
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                }
                // Turtle state is poll output, not a setting echo.
                let turtle_active = crate::policy::schedule_state(&settings).turtle_active();
                self.globals = self.services.to_globals(&globals, turtle_active);
                // One rate sample per torrent per tick: the Transfer pane's chart
                // has nothing else to draw from.
                self.rates.record(&self.torrents);
                // Free space on the save volume, for the status bar. Asked here —
                // on a successful tick only — so a stall or disconnect never
                // shows a stale number, and a remote daemon never shows one at
                // all (its files are not on this machine). `statvfs` is a
                // syscall, not daemon I/O: it runs on GPUI's background
                // executor, not the service runtime's blocking pool.
                let local = crate::settings::is_localhost(&settings.transport);
                let save_path = settings.default_save_path.clone();
                let disk = cx.background_executor().spawn(async move {
                    local
                        .then(|| crate::localfs::free_space(&save_path))
                        .flatten()
                });
                // Policy owns one `PolicyState` per model, behind the same kind
                // of short lock the services use for the backend: cloned out of
                // the borrow before anything awaits.
                let services = Arc::clone(&self.services);
                let policy_state = Arc::clone(&self.policy_state);
                let moves = Arc::clone(&self.moves);
                self.torrents = self.services.to_dtos(&raw);
                let existing: HashSet<String> =
                    self.torrents.iter().map(|t| t.hash.clone()).collect();
                table_state::prune(&mut self.selection, &existing);
                // Move-journal recovery runs once, against the first real
                // snapshot (V3-14).
                if !self.moves_resumed {
                    self.resume_moves_once(raw.clone(), cx);
                }
                // Library search index (V3-12): keep it to live torrents, and
                // fill a bounded batch on a slower cadence so the extra
                // `f.multicall` never rides the fast poll.
                self.ticks = self.ticks.wrapping_add(1);
                let live = existing.clone();
                let index = Arc::clone(&self.file_index);
                if let Ok(mut guard) = index.lock() {
                    guard.retain_live(&live);
                }
                if self.ticks == 1 || self.ticks.is_multiple_of(FILE_INDEX_EVERY_TICKS) {
                    let hashes: Vec<String> =
                        raw.iter().map(|torrent| torrent.hash.clone()).collect();
                    let needed: Vec<String> = index
                        .lock()
                        .map(|guard| {
                            guard
                                .needs(&hashes, FILE_INDEX_BATCH)
                                .into_iter()
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default();
                    if !needed.is_empty() {
                        let services = Arc::clone(&self.services);
                        cx.spawn(async move |this, cx| {
                            for hash in needed {
                                if let Ok(files) = services.files(hash.clone()).await {
                                    let paths: Vec<String> =
                                        files.into_iter().map(|file| file.path).collect();
                                    if let Ok(mut guard) = index.lock() {
                                        guard.record(&hash, paths);
                                    }
                                }
                            }
                            // Re-filter with the new filenames on the next frame.
                            this.update(cx, |_this, cx| cx.notify()).ok();
                        })
                        .detach();
                    }
                }
                cx.notify();
                // Finish the tick from the same continuation: fold in the disk
                // answer, then run policy against the rows above.
                cx.spawn(async move |this, cx| {
                    let free_space = disk.await;
                    this.update(cx, |this, cx| {
                        this.globals.free_space = free_space;
                        cx.notify();
                    })
                    .ok();
                    let log = {
                        // `run_tick` borrows `&mut PolicyState`, which cannot
                        // cross the runtime boundary; take the state out, run,
                        // and put it back. A tick that races another tick (only
                        // possible if a previous policy pass is still running,
                        // which means a slow daemon) simply runs with a fresh
                        // state — goal memory is keyed to survive one skip.
                        let taken = policy_state
                            .lock()
                            .map(|mut guard| std::mem::take(&mut *guard))
                            .unwrap_or_default();
                        let result = services
                            .runtime()
                            .spawn(async move {
                                let mut owned = taken;
                                let log = crate::policy::run_tick(
                                    &services, &settings, &raw, &mut owned, &moves,
                                )
                                .await;
                                (owned, log)
                            })
                            .await;
                        match result {
                            Ok((owned, log)) => {
                                if let Ok(mut guard) = policy_state.lock() {
                                    *guard = owned;
                                }
                                log
                            }
                            Err(_) => Default::default(),
                        }
                    };
                    this.update(cx, |this, cx| this.apply_policy_log(log, cx))
                        .ok();
                })
                .detach();
                return;
            }
            _ => {
                self.failures += 1;
                let delay = BACKOFF[(self.failures - 1).min(BACKOFF.len() - 1)];
                let settings = self.services.settings();
                // Logged once per outage, not once per failed poll.
                let was_connected = self.connection.phase != ConnPhase::Disconnected;
                self.connection = ConnState {
                    phase: ConnPhase::Disconnected,
                    endpoint: crate::settings::endpoint_label(&settings.transport),
                    daemon_version: None,
                    error: Some("rtorrent unreachable".to_owned()),
                    retry_in_seconds: Some(delay as i64),
                };
                if was_connected {
                    self.log.push(
                        LogLevel::Warn,
                        format!("lost rtorrent — retrying every {delay}s"),
                        None,
                    );
                }
                self.torrents.clear();
                cx.notify();
            }
        }
    }

    /// Turn one policy pass into log lines and user-visible side effects.
    ///
    /// Completions notify (a desktop notification where the platform has one,
    /// else a log line the Log pane shows); everything policy did or failed to
    /// do lands in the log with the torrent attributed where there is one.
    fn apply_policy_log(&mut self, log: crate::policy::PolicyLog, cx: &mut Context<Self>) {
        use crate::policy::{Completion, PolicyLine};
        for completion in &log.completions {
            let Completion {
                hash,
                name,
                size_bytes,
                ..
            } = completion;
            crate::notifications::post_completion(name, *size_bytes);
            self.log.push(
                LogLevel::Info,
                format!("download complete · {}", crate::format::bytes(*size_bytes)),
                Some(hash.clone()),
            );
        }
        for line in &log.lines {
            match line {
                PolicyLine::Hooked { program, hash } => self.log.push(
                    LogLevel::Info,
                    format!("run-on-complete: launched {program}"),
                    Some(hash.clone()),
                ),
                PolicyLine::SeedStopped { message, hash } => self.log.push(
                    LogLevel::Info,
                    format!("{message} — stopped"),
                    Some(hash.clone()),
                ),
                PolicyLine::SeedRemoved { message, hash } => {
                    self.log
                        .push(LogLevel::Info, message.clone(), Some(hash.clone()))
                }
                PolicyLine::SeedFailed { message } => {
                    self.log.push(LogLevel::Error, message.clone(), None);
                }
                PolicyLine::QueueStopped { count } => self.log.push(
                    LogLevel::Info,
                    format!("queued {count} torrent(s) over the active limit"),
                    None,
                ),
                PolicyLine::QueueStarted { count } => self.log.push(
                    LogLevel::Info,
                    format!("started {count} queued torrent(s)"),
                    None,
                ),
                PolicyLine::SchedPaused { count } => self.log.push(
                    LogLevel::Info,
                    format!("scheduler paused {count} torrent(s) for the pause window"),
                    None,
                ),
                PolicyLine::SchedResumed { count } => self.log.push(
                    LogLevel::Info,
                    format!("scheduler released {count} torrent(s) at the window end"),
                    None,
                ),
                PolicyLine::BandwidthAdopted { message, hash } => {
                    self.log
                        .push(LogLevel::Info, message.clone(), Some(hash.clone()))
                }
                PolicyLine::BandwidthReleased { hash } => self.log.push(
                    LogLevel::Info,
                    "bandwidth rule released to global limits".to_owned(),
                    Some(hash.clone()),
                ),
                PolicyLine::BandwidthFailed { message } => {
                    self.log.push(LogLevel::Error, message.clone(), None);
                }
                PolicyLine::Trashed { path } => match crate::localfs::trash(path) {
                    Ok(()) => {
                        self.log
                            .push(LogLevel::Info, format!("moved to Trash: {path}"), None)
                    }
                    Err(error) => self.log.push(
                        LogLevel::Warn,
                        format!("could not trash {path}: {error}"),
                        None,
                    ),
                },
            }
        }
        if !log.completions.is_empty() || !log.lines.is_empty() {
            cx.notify();
        }
        self.apply_move_statuses(log.moves, cx);
    }

    /// Diff the move snapshot against the last tick: terminal transitions
    /// become log lines, and a running move owns the status notice.
    ///
    /// Detached move tasks cannot touch the model, so this diff is the one
    /// place their outcomes surface.
    fn apply_move_statuses(
        &mut self,
        statuses: Vec<rtorrent_core::mover::MoveStatus>,
        cx: &mut Context<Self>,
    ) {
        use rtorrent_core::complete::MoveState;
        for status in &statuses {
            let changed = self
                .known_moves
                .get(&status.id)
                .map_or(true, |(state, _)| *state != status.state);
            if changed {
                match status.state {
                    MoveState::Done => self.log.push(
                        LogLevel::Info,
                        format!("moved {} to {}", status.name, status.dst),
                        Some(status.hash.clone()),
                    ),
                    MoveState::Failed => self.log.push(
                        LogLevel::Error,
                        format!("could not move {}: {}", status.name, status.error),
                        Some(status.hash.clone()),
                    ),
                    MoveState::Cancelled => self.log.push(
                        LogLevel::Info,
                        format!("move of {} cancelled — resumed in place", status.name),
                        Some(status.hash.clone()),
                    ),
                    MoveState::Pending | MoveState::InProgress => {}
                }
                self.known_moves.insert(
                    status.id.clone(),
                    (status.state.clone(), status.error.clone()),
                );
            }
        }
        let live: std::collections::HashSet<String> =
            statuses.iter().map(|s| s.id.clone()).collect();
        self.known_moves.retain(|id, _| live.contains(id));

        if let Some(active) = statuses.iter().find(|s| {
            matches!(
                s.state,
                rtorrent_core::complete::MoveState::Pending
                    | rtorrent_core::complete::MoveState::InProgress
            )
        }) {
            if self.notice.is_none() || self.move_notice {
                let pct = if active.total_bytes > 0 {
                    (u128::from(active.done_bytes.min(active.total_bytes)) * 100
                        / u128::from(active.total_bytes)) as u64
                } else {
                    0
                };
                self.notice = Some(format!("moving {} → {} · {pct}%", active.name, active.dst));
                self.move_notice = true;
                cx.notify();
            }
        } else if self.move_notice {
            self.notice = None;
            self.move_notice = false;
            cx.notify();
        }
    }

    /// Hashes with a live (pending/in-progress) move, for the context menu.
    #[must_use]
    pub fn active_move_hashes(&self) -> std::collections::HashSet<String> {
        self.moves
            .lock()
            .map(|guard| {
                guard
                    .snapshot()
                    .into_iter()
                    .filter(|s| {
                        matches!(
                            s.state,
                            rtorrent_core::complete::MoveState::Pending
                                | rtorrent_core::complete::MoveState::InProgress
                        )
                    })
                    .map(|s| s.hash)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Hashes with a retryable (failed/cancelled) move, for the context menu.
    #[must_use]
    pub fn retryable_move_hashes(&self) -> std::collections::HashSet<String> {
        self.moves
            .lock()
            .map(|guard| {
                guard
                    .snapshot()
                    .into_iter()
                    .filter(|s| {
                        matches!(
                            s.state,
                            rtorrent_core::complete::MoveState::Failed
                                | rtorrent_core::complete::MoveState::Cancelled
                        )
                    })
                    .map(|s| s.hash)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Cancel the single selection's live move, if any.
    pub fn cancel_move_for_selection(&mut self, cx: &mut Context<Self>) {
        let Some(hash) = self.single_selection().map(|t| t.hash.clone()) else {
            return;
        };
        if self.moves.lock().is_ok_and(|guard| guard.cancel(&hash)) {
            self.log.push(
                LogLevel::Info,
                format!("cancelling move of {}", hash),
                Some(hash),
            );
            cx.notify();
        }
    }

    /// Drop the single selection's failed/cancelled entry so the next tick
    /// re-plans from daemon truth.
    pub fn retry_move_for_selection(&mut self, cx: &mut Context<Self>) {
        let Some(hash) = self.single_selection().map(|t| t.hash.clone()) else {
            return;
        };
        let retried = self.moves.lock().is_ok_and(|guard| {
            let mut guard = guard;
            let retried = guard.retry(&hash);
            if retried {
                let _ = guard.save();
            }
            retried
        });
        if retried {
            self.log.push(
                LogLevel::Info,
                format!("retrying move of {hash}"),
                Some(hash),
            );
            self.services.retry_now();
            cx.notify();
        }
    }

    /// Recover the move journal once, after the first successful tick.
    fn resume_moves_once(
        &mut self,
        raw: Vec<rtorrent_core::rtorrent::RawTorrent>,
        cx: &mut Context<Self>,
    ) {
        if self.moves_resumed {
            return;
        }
        self.moves_resumed = true;
        if self
            .moves
            .lock()
            .is_ok_and(|guard| guard.journal().resumable().is_empty())
        {
            return;
        }
        let services = Arc::clone(&self.services);
        let moves = Arc::clone(&self.moves);
        cx.spawn(async move |this, cx| {
            let live: std::collections::HashSet<String> =
                raw.iter().map(|t| t.hash.clone()).collect();
            let backend = services.backend();
            let on_event: Arc<rtorrent_core::mover::MoveEventFn> = Arc::new(|_| {});
            rtorrent_core::mover::resume_moves(
                backend.as_ref(),
                &moves,
                &live,
                &crate::localfs::exists,
                crate::policy::unix_millis(),
                on_event,
            )
            .await;
            this.update(cx, |this: &mut Self, cx| {
                this.log
                    .push(LogLevel::Info, "move journal recovered", None);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn next_delay_ms(&self) -> u64 {
        if self.failures > 0 {
            BACKOFF[(self.failures - 1).min(BACKOFF.len() - 1)] * 1000
        } else {
            self.services.settings().poll_ms.max(250)
        }
    }

    #[must_use]
    pub fn torrents(&self) -> &[TorrentDto] {
        &self.torrents
    }

    /// One torrent by hash, for the views that act on a single row.
    #[must_use]
    pub fn torrent(&self, hash: &str) -> Option<&TorrentDto> {
        self.torrents.iter().find(|torrent| torrent.hash == hash)
    }

    #[must_use]
    pub fn globals(&self) -> &GlobalStats {
        &self.globals
    }

    #[must_use]
    pub fn connection(&self) -> &ConnState {
        &self.connection
    }

    #[must_use]
    pub fn filter(&self) -> &Filter {
        &self.filter
    }

    #[must_use]
    pub fn search(&self) -> &str {
        &self.search
    }

    #[must_use]
    pub fn sort(&self) -> Sort {
        self.sort
    }

    #[must_use]
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    #[must_use]
    pub fn services(&self) -> &Arc<Services> {
        &self.services
    }

    /// The shared file-path index for library search (V3-12), so the table can
    /// match a torrent's contained filenames.
    #[must_use]
    pub fn file_index(&self) -> Arc<std::sync::Mutex<rtorrent_core::file_index::FileIndex>> {
        Arc::clone(&self.file_index)
    }

    /// Where the persisted all-time transfer counters live, so the Statistics
    /// dialog folds into the same file the shell and the Tauri app use.
    #[must_use]
    pub fn stats_path(&self) -> std::path::PathBuf {
        self.settings_store.stats_path()
    }

    /// Where the session-import journal lives (V3-22), beside the settings
    /// file so either native shell can resume the other's import.
    #[must_use]
    pub fn import_journal_path(&self) -> std::path::PathBuf {
        self.settings_store.import_journal_path()
    }

    /// Log a finished session import like every other mutation (V3-22).
    pub fn note_session_import(
        &mut self,
        added: usize,
        skipped: usize,
        failed: &[String],
        cx: &mut Context<Self>,
    ) {
        if failed.is_empty() {
            self.notice = None;
            self.log.push(
                LogLevel::Info,
                format!("import finished: {added} added, {skipped} skipped"),
                None,
            );
        } else {
            let message = format!("import finished: {added} added, {} failed", failed.len());
            self.notice = Some(message.clone());
            self.log.push(LogLevel::Error, message, None);
            for failure in failed.iter().take(5) {
                self.log.push(LogLevel::Error, failure.clone(), None);
            }
        }
        cx.notify();
        self.services.retry_now();
    }

    /// The pane the panel is showing.
    #[must_use]
    pub fn active_tab(&self) -> Pane {
        self.active_tab
    }

    /// The torrent the panel describes: the anchor of the selection while it is
    /// still selected, else what is left. `None` with nothing selected.
    #[must_use]
    pub fn focused_torrent(&self) -> Option<&TorrentDto> {
        let hash = detail_panes::focused_hash(&self.selection)?;
        self.torrent(&hash)
    }

    /// The last payload for the current watch, if it has arrived.
    #[must_use]
    pub fn detail(&self) -> Option<&DetailPayload> {
        self.detail.as_ref()
    }

    /// The rate samples this session has taken, for the Transfer pane's chart.
    #[must_use]
    pub fn rates(&self) -> &RateHistory {
        &self.rates
    }

    /// What the app has done this session, for the Log pane.
    #[must_use]
    pub fn log(&self) -> &AppLog {
        &self.log
    }

    /// Start the web UI, or stop it when it is already serving.
    ///
    /// The server runs on the daemon's own runtime — GPUI's executor has no
    /// reactor to hang an axum server on — and reads the daemon this app is
    /// connected to, so the browser sees the same torrents without a config
    /// file. Starting puts the link on the clipboard, which is the fastest way
    /// to get it into a browser.
    pub fn toggle_web_ui(&mut self, cx: &mut Context<Self>) {
        if let Some(server) = self.web.take() {
            self.notice = Some("stopping the web UI…".to_owned());
            cx.notify();
            cx.spawn(async move |this, cx| {
                let outcome = server.stop().await;
                this.update(cx, |this: &mut Self, cx| {
                    match outcome {
                        Ok(()) => {
                            this.log.push(LogLevel::Info, "stopped the web UI", None);
                            this.notice = Some("web UI stopped".to_owned());
                        }
                        Err(error) => {
                            let message = format!("the web UI did not stop cleanly: {error}");
                            this.log.push(LogLevel::Error, message.clone(), None);
                            this.notice = Some(message);
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
            return;
        }

        let settings = self.services.settings();
        let runtime = self.services.runtime();
        self.notice = Some("starting the web UI…".to_owned());
        cx.notify();

        cx.spawn(async move |this, cx| {
            let started = runtime
                .spawn(async move { web_host::start(settings).await })
                .await;
            this.update(cx, |this: &mut Self, cx| {
                let message = match started {
                    Ok(Ok((server, url))) => {
                        this.web.adopt(server);
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(url.clone()));
                        format!("web UI at {url} — link copied")
                    }
                    Ok(Err(error)) => error,
                    Err(join) => format!("the web UI task failed: {join}"),
                };
                // One line either way: the URL when it is up, the reason when it
                // is not.
                let level = if this.web.is_running() {
                    LogLevel::Info
                } else {
                    LogLevel::Error
                };
                this.log.push(level, message.clone(), None);
                this.notice = Some(message);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Record a torrent this app created: the log line and the status notice.
    ///
    /// The dialog shows the info-hash itself; this is so the Log pane holds it
    /// afterwards, and so the table re-polls to pick the new torrent up.
    pub fn note_creation(&mut self, message: &str, hash: Option<String>, cx: &mut Context<Self>) {
        self.log.push(LogLevel::Info, message.to_owned(), hash);
        self.notice = Some(message.to_owned());
        self.services.retry_now();
        cx.notify();
    }

    /// Switch the panel's pane, persist the choice, and start polling it.
    pub fn set_active_tab(&mut self, pane: Pane, cx: &mut Context<Self>) {
        if self.active_tab == pane {
            return;
        }
        self.active_tab = pane;
        let key = pane.key().to_owned();
        self.update_settings(|settings| settings.active_tab = key, cx);
        self.refresh_detail(cx);
    }

    /// RSS seen-set size for the preferences row (V3-23).
    #[must_use]
    pub fn rss_seen_count(&self) -> usize {
        self.rss.as_ref().map_or(0, crate::rss::Runner::seen_count)
    }

    /// Drop every remembered RSS id; the next tick re-matches live items.
    pub fn rss_clear_seen(&mut self, cx: &mut Context<Self>) {
        if let Some(runner) = self.rss.as_mut() {
            runner.clear_seen();
            self.log
                .push(LogLevel::Info, "cleared the RSS seen-set".to_owned(), None);
        }
        cx.notify();
    }

    /// The last action failure or settings problem, if any.
    #[must_use]
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn clear_notice(&mut self, cx: &mut Context<Self>) {
        if self.notice.take().is_some() {
            cx.notify();
        }
    }

    /// Report something the user should know but that is not a daemon failure —
    /// a surface that is not ported yet, say.
    pub fn set_notice(&mut self, message: &str, cx: &mut Context<Self>) {
        self.notice = Some(message.to_owned());
        cx.notify();
    }

    /// Rows in display order (hashes), for selection arithmetic.
    #[must_use]
    pub fn order(&self) -> Vec<String> {
        table_state::visible(&self.torrents, &self.filter, &self.search, self.sort)
            .iter()
            .map(|t| t.hash.clone())
            .collect()
    }

    /// The selected torrents, in display order.
    #[must_use]
    pub fn selected(&self) -> Vec<&TorrentDto> {
        let selection = &self.selection.hashes;
        table_state::visible(&self.torrents, &self.filter, &self.search, self.sort)
            .into_iter()
            .filter(|torrent| selection.contains(&torrent.hash))
            .collect()
    }

    /// The selected hashes, in display order so batch actions are stable.
    #[must_use]
    pub fn selected_hashes(&self) -> Vec<String> {
        self.selected().iter().map(|t| t.hash.clone()).collect()
    }

    #[must_use]
    pub fn has_selection(&self) -> bool {
        !self.selection.hashes.is_empty()
    }

    /// Distinct labels in use, for the "Set label" submenu.
    #[must_use]
    pub fn labels(&self) -> Vec<String> {
        let mut labels: Vec<String> = self
            .torrents
            .iter()
            .map(|torrent| torrent.label.clone())
            .filter(|label| !label.is_empty())
            .collect();
        labels.sort();
        labels.dedup();
        labels
    }

    /// Distinct tags in use, for the tag editor's chips (V3-10).
    #[must_use]
    pub fn tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self
            .torrents
            .iter()
            .flat_map(|torrent| torrent.tags.iter().cloned())
            .collect();
        tags.sort();
        tags.dedup();
        tags
    }

    /// The union of tags on the current selection, sorted — what the tag editor
    /// shows as the starting point.
    #[must_use]
    pub fn selected_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self
            .selected()
            .iter()
            .flat_map(|torrent| torrent.tags.iter().cloned())
            .collect();
        tags.sort();
        tags.dedup();
        tags
    }

    /// True when every selected torrent is stopped — what Resume/Pause and the
    /// Space shortcut switch on.
    #[must_use]
    pub fn selection_is_stopped(&self) -> bool {
        let selected = self.selected();
        !selected.is_empty()
            && selected
                .iter()
                .all(|torrent| matches!(torrent.status, Status::Paused))
    }

    /// The single selected torrent, when exactly one row is selected.
    #[must_use]
    pub fn single_selection(&self) -> Option<&TorrentDto> {
        let hashes = self.selection.hashes.len();
        if hashes != 1 {
            return None;
        }
        self.selected().first().copied()
    }

    pub fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        self.filter = filter;
        cx.notify();
    }

    pub fn set_search(&mut self, search: String, cx: &mut Context<Self>) {
        self.search = search;
        cx.notify();
    }

    pub fn set_sort(&mut self, sort: Sort, cx: &mut Context<Self>) {
        self.sort = sort;
        cx.notify();
    }

    pub fn click(&mut self, hash: &str, modifiers: table_state::Modifiers, cx: &mut Context<Self>) {
        let order = self.order();
        let selection = table_state::click(&self.selection, &order, hash, modifiers);
        self.set_selection(selection, cx);
    }

    /// Replace the selection.
    ///
    /// The one writer, so the panel's subject — and therefore what it polls —
    /// follows every path a selection can change by.
    pub fn set_selection(&mut self, selection: table_state::Selection, cx: &mut Context<Self>) {
        self.selection = selection;
        self.refresh_detail(cx);
    }

    /// Select every visible row (⌘A).
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        let order = self.order();
        let selection = Selection {
            anchor: order.first().cloned(),
            hashes: order.into_iter().collect(),
        };
        self.set_selection(selection, cx);
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.set_selection(Selection::default(), cx);
    }

    pub fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let order = self.order();
        let moved = table_state::step(&self.selection, &order, delta);
        self.set_selection(moved.selection, cx);
    }

    /// Persist a settings change and hand it to the services that act on it.
    ///
    /// One writer for the whole file: the shell's view state and the model's
    /// daemon-affecting settings both come through here, so a save can never
    /// race another save.
    pub fn update_settings(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        let mut settings = self.services.settings();
        change(&mut settings);
        self.services.update_settings(settings.clone());
        if let Err(error) = self.settings_store.save(&settings) {
            self.notice = Some(format!("could not save settings: {error}"));
        }
        cx.notify();
    }

    /// Persist the Preferences dialog's validated draft (G6-S1).
    ///
    /// Transport/mock changes rebuild the backend through the single settings
    /// writer; daemon-affecting keys (port range, DHT, network prefs) are
    /// pushed without waiting for a reconnect, and the notice reports what
    /// the daemon accepted. Everything else is live by the next poll tick.
    pub fn apply_preferences(&mut self, next: Settings, cx: &mut Context<Self>) {
        let old = self.services.settings();
        self.update_settings(|settings| *settings = next.clone(), cx);
        // Reprofiled bandwidth rules must re-adopt at the new rates (V3-18):
        // clear their markers so the next tick adopts fresh.
        let changed =
            rtorrent_core::bandwidth::changed_rule_ids(&old.bandwidth_rules, &next.bandwidth_rules);
        if !changed.is_empty() {
            let services = Arc::clone(&self.services);
            cx.spawn(async move |this, cx| {
                let backend = services.backend();
                if let Ok(rows) = backend.list_snapshot().await {
                    for t in rows
                        .iter()
                        .filter(|t| changed.iter().any(|id| id == &t.throttle_rule))
                    {
                        let _ = backend
                            .set_custom_metadata(
                                &t.hash,
                                &[(rtorrent_core::bandwidth::RULE_KEY, "")],
                            )
                            .await;
                    }
                }
                this.update(cx, |this: &mut Self, cx| {
                    this.log
                        .push(LogLevel::Info, "bandwidth rules updated".to_owned(), None);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        if !Services::daemon_config_changed(&old, &next) {
            self.notice = Some("preferences saved".to_owned());
            self.services.retry_now();
            cx.notify();
            return;
        }
        let services = Arc::clone(&self.services);
        cx.spawn(async move |this, cx| {
            let pushed = services.push_daemon_config(next).await;
            this.update(cx, |this: &mut Self, cx| {
                this.notice = Some(match pushed {
                    Ok(()) => "preferences saved — daemon updated".to_owned(),
                    Err(error) => {
                        format!("preferences saved, but the daemon refused a value: {error}")
                    }
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
        self.services.retry_now();
    }

    /// Flip the manual turtle-mode toggle and persist it like every other
    /// settings change. The next poll tick recomputes the effective limits
    /// and pushes them (see `policy::run_tick`), so this takes effect
    /// within one poll interval.
    pub fn toggle_turtle(&mut self, cx: &mut Context<Self>) {
        self.update_settings(
            |settings| {
                settings.turtle_enabled = !settings.turtle_enabled;
            },
            cx,
        );
        let enabled = self.services.settings().turtle_enabled;
        self.notice = Some(if enabled {
            "turtle mode on".to_owned()
        } else {
            "turtle mode off".to_owned()
        });
        self.services.retry_now();
        cx.notify();
    }

    // --- Selection verbs ----------------------------------------------------

    pub fn start_selection(&mut self, cx: &mut Context<Self>) {
        self.dispatch_for_selection("started", |services, hashes| services.start(hashes), cx);
    }

    pub fn stop_selection(&mut self, cx: &mut Context<Self>) {
        self.dispatch_for_selection("stopped", |services, hashes| services.stop(hashes), cx);
    }

    pub fn recheck_selection(&mut self, cx: &mut Context<Self>) {
        self.dispatch_for_selection("rechecked", |services, hashes| services.recheck(hashes), cx);
    }

    pub fn reannounce_selection(&mut self, cx: &mut Context<Self>) {
        self.dispatch_for_selection(
            "reannounced",
            |services, hashes| services.force_reannounce(hashes),
            cx,
        );
    }

    /// Remove the selection, confirming first when the setting asks for it.
    pub fn remove_selection(&mut self, delete_data: bool, cx: &mut Context<Self>) {
        self.dispatch_for_selection(
            if delete_data {
                "removed, with data,"
            } else {
                "removed"
            },
            move |services, hashes| services.remove(hashes, delete_data),
            cx,
        );
    }

    pub fn queue_move(&mut self, direction: QueueDirection, cx: &mut Context<Self>) {
        self.dispatch_for_selection(
            direction.as_str(),
            move |services, hashes| services.queue_move(hashes, direction),
            cx,
        );
    }

    /// Toggle force-start on the selection (QUE-01).
    pub fn toggle_force_start(&mut self, cx: &mut Context<Self>) {
        self.dispatch_for_selection(
            "toggled force-start on",
            |services, hashes| services.toggle_force_start(hashes),
            cx,
        );
    }

    pub fn set_label(&mut self, label: String, cx: &mut Context<Self>) {
        self.dispatch_for_selection(
            "set label on",
            |services, hashes| services.set_label(hashes, label),
            cx,
        );
    }

    /// Replace the tags on the selection (V3-10); an empty list clears them.
    pub fn set_tags(&mut self, tags: Vec<String>, cx: &mut Context<Self>) {
        self.dispatch_for_selection(
            "set tags on",
            move |services, hashes| services.set_tags(hashes, tags),
            cx,
        );
    }

    /// Select one torrent and clear the filter/search, so a duplicate can be
    /// shown from the add dialog (V3-13).
    pub fn focus_torrent(&mut self, hash: String, cx: &mut Context<Self>) {
        self.selection = table_state::Selection {
            hashes: [hash.clone()].into_iter().collect(),
            anchor: Some(hash),
        };
        self.filter = table_state::Filter::All;
        self.search.clear();
        self.update_settings(
            |settings| {
                settings.filter.clear();
                settings.search.clear();
            },
            cx,
        );
        cx.notify();
    }

    /// Add the given announce URLs to an existing torrent, for the duplicate
    /// dialog's "merge trackers" (V3-13).
    pub fn merge_trackers(&mut self, hash: String, urls: Vec<String>, cx: &mut Context<Self>) {
        if urls.is_empty() {
            return;
        }
        let count = urls.len();
        self.dispatch(
            format!("merged {count} tracker(s) into an existing torrent"),
            Some(hash.clone()),
            move |services| {
                let calls: Vec<ServiceTask<()>> = urls
                    .into_iter()
                    .map(|url| services.add_tracker(hash.clone(), url))
                    .collect();
                Box::pin(async move {
                    for call in calls {
                        call.await?;
                    }
                    Ok(())
                })
            },
            cx,
        );
    }

    /// Add tags to, and remove tags from, every selected torrent. The new list
    /// differs per torrent (they start with different tags), so the selection is
    /// grouped by the list it should end up with and written in a few calls.
    pub fn edit_tags(&mut self, add: Vec<String>, remove: Vec<String>, cx: &mut Context<Self>) {
        let hashes = self.selected_hashes();
        if hashes.is_empty() {
            return;
        }
        let groups = table_state::tag_edits(&self.torrents, &hashes, &add, &remove);
        if groups.is_empty() {
            return;
        }
        let count = hashes.len();
        let noun = if count == 1 { "torrent" } else { "torrents" };
        self.dispatch(
            format!("updated tags on {count} {noun}"),
            None,
            move |services| {
                let calls: Vec<ServiceTask<()>> = groups
                    .iter()
                    .map(|(hashes, tags)| services.set_tags(hashes.clone(), tags.clone()))
                    .collect();
                Box::pin(async move {
                    for call in calls {
                        call.await?;
                    }
                    Ok(())
                })
            },
            cx,
        );
    }

    /// Apply a per-torrent rate limit, persisting any newly defined pool slot.
    pub fn set_torrent_limits(&mut self, down_kb: i64, up_kb: i64, cx: &mut Context<Self>) {
        let hashes = self.selected_hashes();
        if hashes.is_empty() {
            return;
        }
        let services = Arc::clone(&self.services);
        let task = services.set_torrent_limits(hashes, down_kb, up_kb);
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |this: &mut Self, cx| {
                match outcome {
                    Ok(Some(definition)) => {
                        let mut settings = this.services.settings();
                        settings
                            .torrent_throttles
                            .retain(|existing| existing.name != definition.name);
                        settings.torrent_throttles.push(definition);
                        this.services.update_settings(settings.clone());
                        if let Err(error) = this.settings_store.save(&settings) {
                            this.notice = Some(format!("could not save the rate limit: {error}"));
                        }
                    }
                    Ok(None) => this.notice = None,
                    Err(error) => this.notice = Some(error.to_string()),
                }
                cx.notify();
            })
            .ok();
            services.retry_now();
        })
        .detach();
    }

    pub fn set_location(&mut self, path: String, move_data: bool, cx: &mut Context<Self>) {
        let Some(hash) = self.single_selection().map(|torrent| torrent.hash.clone()) else {
            return;
        };
        self.dispatch(
            if move_data {
                "moved the data for"
            } else {
                "set the location of"
            },
            Some(hash.clone()),
            move |services| services.set_location(hash, path, move_data),
            cx,
        );
    }

    pub fn set_super_seeding(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let Some(hash) = self.single_selection().map(|torrent| torrent.hash.clone()) else {
            return;
        };
        self.dispatch(
            if enabled {
                "enabled super-seeding"
            } else {
                "disabled super-seeding"
            },
            Some(hash.clone()),
            move |services| services.set_super_seeding(hash, enabled),
            cx,
        );
    }

    /// Apply the per-torrent connection caps. They are addressed by one hash,
    /// so this only applies to a single selection.
    pub fn set_connection_limits(
        &mut self,
        peers_max: i64,
        peers_min: i64,
        uploads_max: i64,
        cx: &mut Context<Self>,
    ) {
        let Some(hash) = self.single_selection().map(|torrent| torrent.hash.clone()) else {
            return;
        };
        self.dispatch(
            "set the connection limits",
            Some(hash.clone()),
            move |services| services.set_connection_limits(hash, peers_max, peers_min, uploads_max),
            cx,
        );
    }

    /// Copy the single selection's magnet link to the clipboard.
    pub fn copy_magnet(&mut self, cx: &mut Context<Self>) {
        let Some(hash) = self.single_selection().map(|torrent| torrent.hash.clone()) else {
            return;
        };
        let services = Arc::clone(&self.services);
        let task = services.magnet_uri(hash);
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |this: &mut Self, cx| {
                match outcome {
                    Ok(uri) => {
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(uri));
                        this.notice = Some("magnet link copied".to_owned());
                    }
                    Err(error) => this.notice = Some(error.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn open_destination(&mut self, cx: &mut Context<Self>) {
        let Some(hash) = self.single_selection().map(|torrent| torrent.hash.clone()) else {
            return;
        };
        self.dispatch(
            "opened the destination",
            Some(hash.clone()),
            move |services| services.open_destination(hash),
            cx,
        );
    }

    pub fn save_session(&mut self, cx: &mut Context<Self>) {
        self.dispatch(
            "saved the session",
            None,
            |services| services.save_session(),
            cx,
        );
    }

    /// Start a local rtorrent daemon, preferring the runtime this build ships.
    ///
    /// The launch blocks — it spawns the daemon and then waits for its socket —
    /// so it runs on the background executor rather than the UI thread. The
    /// outcome becomes the status notice, and the poll loop is nudged either
    /// way, so a successful start replaces the disconnected card with the table
    /// without the user having to retry.
    pub fn start_daemon(&mut self, cx: &mut Context<Self>) {
        let transport = self.services.settings().transport;
        let services = Arc::clone(&self.services);
        self.notice = Some("starting rtorrent…".to_owned());
        cx.notify();

        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { crate::daemon::start(&transport) })
                .await;
            this.update(cx, |this: &mut Self, cx| {
                // Either way the message is for the status bar: a start says
                // which runtime it used, a failure says why it could not.
                this.notice = Some(match outcome {
                    Ok(message) | Err(message) => message,
                });
                cx.notify();
            })
            .ok();
            services.retry_now();
        })
        .detach();
    }

    /// On launch, bring up the bundled daemon when nothing is serving the
    /// configured local endpoint — so a fresh install opens onto a working
    /// client rather than the disconnected card. The checks and the start both
    /// block, so they run on the background executor; when a daemon is already
    /// there (or the connection is remote, or this is mock mode) nothing
    /// happens and nothing is said.
    fn autostart_daemon(&mut self, cx: &mut Context<Self>) {
        let settings = self.services.settings();
        if settings.mock {
            return;
        }
        let transport = settings.transport;
        let services = Arc::clone(&self.services);

        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    crate::daemon::should_autostart(&transport)
                        .then(|| crate::daemon::start(&transport))
                })
                .await;
            let Some(outcome) = outcome else {
                return;
            };
            this.update(cx, |this: &mut Self, cx| {
                this.notice = Some(match outcome {
                    Ok(message) | Err(message) => message,
                });
                cx.notify();
            })
            .ok();
            services.retry_now();
        })
        .detach();
    }

    pub fn shutdown(&mut self, cx: &mut Context<Self>) {
        self.dispatch(
            "shut the daemon down",
            None,
            |services| services.shutdown(),
            cx,
        );
    }

    pub fn peer_action(
        &mut self,
        hash: String,
        peer_id: String,
        verb: PeerVerb,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(
            format!("{} a peer", verb.as_str()),
            Some(hash.clone()),
            move |services| services.peer_action(hash, peer_id, verb),
            cx,
        );
    }

    pub fn set_file_priority(
        &mut self,
        hash: String,
        index: usize,
        priority: i64,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(
            "set a file priority",
            Some(hash.clone()),
            move |services| services.set_file_priority(hash, index, priority),
            cx,
        );
    }

    pub fn add_tracker(&mut self, hash: String, url: String, cx: &mut Context<Self>) {
        self.dispatch(
            "added a tracker",
            Some(hash.clone()),
            move |services| services.add_tracker(hash, url),
            cx,
        );
    }

    /// Add a `.torrent` from its bytes, with the dialog's options.
    pub fn add_raw(&mut self, bytes: Vec<u8>, options: AddOptions, cx: &mut Context<Self>) {
        self.dispatch(
            "added a torrent",
            None,
            move |services| services.add_raw(bytes, options),
            cx,
        );
    }

    /// Add a magnet URI or torrent URL, with the dialog's options.
    pub fn add_magnet(&mut self, uri: String, options: AddOptions, cx: &mut Context<Self>) {
        self.dispatch(
            "added a magnet",
            None,
            move |services| services.add_magnet(uri, options),
            cx,
        );
    }

    pub fn remove_tracker(&mut self, hash: String, index: usize, cx: &mut Context<Self>) {
        self.dispatch(
            "removed a tracker",
            Some(hash.clone()),
            move |services| services.remove_tracker(hash, index),
            cx,
        );
    }

    pub fn set_tracker_enabled(
        &mut self,
        hash: String,
        index: usize,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        self.dispatch(
            if enabled {
                "enabled a tracker"
            } else {
                "disabled a tracker"
            },
            Some(hash.clone()),
            move |services| services.set_tracker_enabled(hash, index, enabled),
            cx,
        );
    }

    /// Announce one torrent to its trackers now.
    pub fn reannounce(&mut self, hash: String, cx: &mut Context<Self>) {
        self.dispatch(
            "reannounced",
            Some(hash.clone()),
            move |services| services.force_reannounce(vec![hash]),
            cx,
        );
    }

    /// Run a mutation over the current selection, then re-poll.
    ///
    /// The log line counts the rows, so the Log pane can say what the action did
    /// rather than only that something happened.
    fn dispatch_for_selection<F>(
        &mut self,
        verb: &'static str,
        operation: F,
        cx: &mut Context<Self>,
    ) where
        F: FnOnce(&Services, Vec<String>) -> ServiceTask<()> + 'static,
    {
        let hashes = self.selected_hashes();
        if hashes.is_empty() {
            return;
        }
        let count = hashes.len();
        let noun = if count == 1 { "torrent" } else { "torrents" };
        // Attributed only when there is one torrent to attribute it to.
        let about = if count == 1 {
            hashes.first().cloned()
        } else {
            None
        };
        self.dispatch(
            format!("{verb} {count} {noun}"),
            about,
            move |services| operation(services, hashes),
            cx,
        );
    }

    /// Run one mutation, report a failure, and re-poll immediately.
    ///
    /// The daemon is the truth: nothing is applied optimistically, so an action
    /// that the daemon rejects leaves the rows as they were. `label` is what the
    /// app log calls the action.
    fn dispatch<F, T>(
        &mut self,
        label: impl Into<String>,
        about: Option<String>,
        operation: F,
        cx: &mut Context<Self>,
    ) where
        F: FnOnce(&Services) -> ServiceTask<T> + 'static,
        T: Send + 'static,
    {
        let services = Arc::clone(&self.services);
        let task = operation(&self.services);
        let label = label.into();
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |this: &mut Self, cx| {
                match outcome {
                    Ok(_) => {
                        this.notice = None;
                        this.log.push(LogLevel::Info, label, about.clone());
                    }
                    Err(error) => {
                        let message = error.to_string();
                        this.notice = Some(message.clone());
                        this.log
                            .push(LogLevel::Error, format!("{label}: {message}"), about);
                    }
                }
                cx.notify();
            })
            .ok();
            services.retry_now();
        })
        .detach();
    }

    pub fn retry_now(&self) {
        self.services.retry_now();
    }
}

/// Fetch one pane's payload from the daemon.
///
/// The pane→tab mapping is [`Pane::daemon_tab`]; this is the reverse, and the
/// only place a `DetailPayload` is built. `Speed` and `Log` are not the daemon's
/// to answer — the chart comes from the snapshots the list already carries, and
/// the log is the app's own — so they fetch nothing and the panel still renders.
///
/// A failed fetch yields `None` rather than an empty payload: the pane then
/// keeps its last good data instead of flashing "no file data" at every hiccup.
async fn fetch_detail(services: &Services, hash: String, tab: DetailTab) -> Option<DetailPayload> {
    let mut payload = DetailPayload {
        hash: hash.clone(),
        tab,
        trackers: None,
        peers: None,
        files: None,
        pieces: None,
    };
    match tab {
        DetailTab::Content => payload.files = Some(services.files(hash).await.ok()?),
        DetailTab::Peers => payload.peers = Some(services.peers(hash).await.ok()?),
        DetailTab::Trackers => payload.trackers = Some(services.trackers(hash).await.ok()?),
        DetailTab::General => payload.pieces = Some(services.pieces(hash).await.ok()?),
        DetailTab::Speed | DetailTab::Log => return None,
    }
    Some(payload)
}

/// The app's shared models.
pub struct AppModels {
    pub torrents: gpui_kit::Entity<TorrentsModel>,
}

impl Global for AppModels {}

fn parse_filter(raw: &str) -> Filter {
    let (kind, value) = raw.split_once(':').unwrap_or(("", raw));
    match kind {
        "status" => match value {
            "downloading" => Filter::Status(table_state::StatusKey::Downloading),
            "seeding" => Filter::Status(table_state::StatusKey::Seeding),
            "completed" => Filter::Status(table_state::StatusKey::Completed),
            "paused" => Filter::Status(table_state::StatusKey::Paused),
            "stalled" => Filter::Status(table_state::StatusKey::Stalled),
            "checking" => Filter::Status(table_state::StatusKey::Checking),
            "error" => Filter::Status(table_state::StatusKey::Error),
            _ => Filter::All,
        },
        "label" if !value.is_empty() => Filter::Label(value.to_owned()),
        "tag" if !value.is_empty() => Filter::Tag(value.to_owned()),
        "tracker" if !value.is_empty() => Filter::Tracker(value.to_owned()),
        "view" if !value.is_empty() => Filter::View(value.to_owned()),
        "error" if !value.is_empty() => Filter::ErrorKind(value.to_owned()),
        _ => Filter::All,
    }
}

fn parse_sort(column: &str, descending: bool) -> Sort {
    use crate::table_state::SortColumn;
    let column = match column {
        "size" => SortColumn::Size,
        "percent" => SortColumn::Percent,
        "status" => SortColumn::Status,
        "downRate" => SortColumn::DownRate,
        "upRate" => SortColumn::UpRate,
        "etaSeconds" => SortColumn::EtaSeconds,
        "ratio" => SortColumn::Ratio,
        "startedAt" => SortColumn::StartedAt,
        "finishedAt" => SortColumn::FinishedAt,
        _ => SortColumn::Name,
    };
    Sort {
        column,
        ascending: !descending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::columns::ColumnState;

    fn model() -> TorrentsModel {
        let settings_store = SettingsStore::new(std::env::temp_dir().join(format!(
            "rstorrent-gpui-model-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )));
        let settings = Settings {
            mock: true,
            ..Settings::default()
        };
        let services = Arc::new(Services::new(settings).expect("runtime"));
        TorrentsModel::new(services, settings_store)
    }

    #[test]
    fn filter_and_sort_round_trip_through_settings_strings() {
        assert_eq!(parse_filter(""), Filter::All);
        assert_eq!(
            parse_filter("status:paused"),
            Filter::Status(table_state::StatusKey::Paused)
        );
        assert_eq!(parse_filter("label:video"), Filter::Label("video".into()));
        assert_eq!(parse_filter("garbage"), Filter::All);

        let sort = parse_sort("size", true);
        assert_eq!(sort.column, table_state::SortColumn::Size);
        assert!(!sort.ascending);
        let sort = parse_sort("nonsense", false);
        assert_eq!(sort.column, table_state::SortColumn::Name);
        assert!(sort.ascending);
    }

    #[test]
    fn a_fresh_model_has_no_selection_and_no_notice() {
        let model = model();
        assert!(!model.has_selection());
        assert_eq!(model.notice(), None);
        assert_eq!(model.labels(), Vec::<String>::new());
        assert!(!model.selection_is_stopped());
        assert!(model.single_selection().is_none());
    }

    /// The table's default column state and the model's default sort must agree
    /// with what the settings file starts as, or a first run would paint a
    /// different table from a restarted run.
    #[test]
    fn defaults_agree_with_the_settings_file() {
        let settings = Settings::default();
        assert_eq!(
            parse_sort(&settings.sort_column, settings.sort_descending).column,
            table_state::SortColumn::Name
        );
        assert_eq!(
            ColumnState::from_prefs(&settings.columns),
            ColumnState::default()
        );
    }
}
