//! The native service bridge used by the GPUI shell.
//!
//! GPUI's executor is deliberately not assumed to run the app's futures. The
//! rtorrent client, on the other hand, needs Tokio for SCGI/HTTP I/O.
//! [`Services`] owns one small Tokio runtime and turns each daemon operation
//! into a sendable future. A GPUI model hands that future to `cx.spawn` and
//! remains free of runtime handles.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use rtorrent_core::rtorrent::{
    make_backend, LoadOptions, RawGlobal, RawStats, RawTorrent, RtorrentApi, RtorrentError,
};
use rtorrent_core::types::{
    AddOptions, ConnPhase, ConnState, DaemonHealth, FileNode, GlobalStats, PeerRow, PieceInfo,
    TorrentDto, TrackerRow, Transport,
};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::Notify;

use crate::settings::{endpoint_label, Settings};

/// The default logical name for the single native window (unused today, kept
/// for future multi-window session routing).
pub const GPUI_WINDOW: &str = "gpui";

/// A future that can be awaited by a GPUI background task.
pub type ServiceTask<T> = Pin<Box<dyn Future<Output = Result<T, RtorrentError>> + Send + 'static>>;

/// A small Tokio-backed facade over the rtorrent backend plus the fast-poll
/// loop the Tauri shell runs in `src-tauri/src/poller.rs`.
///
/// The backend is swappable at runtime (when the transport or mock flag
/// changes), behind an `RwLock<Arc<…>>`: clone the `Arc` out under a short
/// lock and drop the guard before awaiting, so no std lock is held across an
/// `.await`.
pub struct Services {
    runtime: Option<Runtime>,
    backend: RwLock<Arc<dyn RtorrentApi>>,
    conn: RwLock<ConnState>,
    settings: RwLock<Settings>,
    tracker_cache: std::sync::Mutex<std::collections::HashMap<String, String>>,
    /// Notified to request an immediate extra poll (after a user action).
    /// Shared so detached jobs (session import) can wake the poll loop.
    pub repoll: Arc<Notify>,
    /// Shared session-import job state (V3-22): the dialog polls this while
    /// open, and a reopened dialog rejoins a running job instead of forking
    /// a second one. The journal on disk is the crash-safe half.
    session_import: Arc<Mutex<SessionImportState>>,
    /// Mirrors into the core import driver's cancel flag.
    session_import_cancel: Arc<AtomicBool>,
}

/// Progress of the detached session-import job (V3-22 / LIB-09).
#[derive(Clone, Debug, Default)]
pub struct SessionImportState {
    pub running: bool,
    pub done: bool,
    /// Hash currently being added/rechecked, for the dialog's status line.
    pub current: String,
    pub added: usize,
    pub resumed: usize,
    pub skipped: usize,
    pub failed: Vec<String>,
    /// Tail of per-torrent log lines (bounded, newest last).
    pub log: Vec<String>,
}

impl Services {
    /// Create services from settings. Runtime construction is synchronous but
    /// does no network or filesystem work.
    ///
    /// # Errors
    ///
    /// Returns the runtime's I/O error when Tokio cannot create its worker
    /// threads or drivers.
    pub fn new(settings: Settings) -> std::io::Result<Self> {
        let runtime = Builder::new_multi_thread()
            .enable_io()
            .enable_time()
            .thread_name("rstorrent-gpui")
            .build()?;
        let backend = make_backend(settings.transport.clone(), settings.mock);
        let conn = ConnState {
            phase: ConnPhase::Connecting,
            endpoint: endpoint_label(&settings.transport),
            daemon_version: None,
            error: None,
            retry_in_seconds: None,
        };
        Ok(Self {
            runtime: Some(runtime),
            backend: RwLock::new(Arc::from(backend)),
            conn: RwLock::new(conn),
            settings: RwLock::new(settings),
            tracker_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
            repoll: Arc::new(Notify::new()),
            session_import: Arc::new(Mutex::new(SessionImportState::default())),
            session_import_cancel: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Deterministic design fixture, with the same torrents as the Tauri
    /// shell's mock backend. Used by `--demo` and the handoff fixture.
    ///
    /// # Errors
    ///
    /// Returns the runtime construction error.
    pub fn mock() -> std::io::Result<Self> {
        Self::new(Settings {
            mock: true,
            ..Settings::default()
        })
    }

    /// Clone the current backend `Arc` (drop the guard before awaiting on it).
    #[must_use]
    pub fn backend(&self) -> Arc<dyn RtorrentApi> {
        self.backend.read().unwrap().clone()
    }

    /// Snapshot of the current settings.
    #[must_use]
    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    /// Latest connection state, mirrored into every snapshot.
    #[must_use]
    pub fn conn(&self) -> ConnState {
        self.conn.read().unwrap().clone()
    }

    /// Current tracker host for a hash, if the slow path has resolved it.
    pub fn tracker_host(&self, hash: &str) -> String {
        self.tracker_cache
            .lock()
            .unwrap()
            .get(hash)
            .cloned()
            .unwrap_or_default()
    }

    /// Replace settings and rebuild the backend when the transport or mock
    /// flag changed. Every setting is live by the next snapshot.
    pub fn update_settings(&self, next: Settings) -> Settings {
        let rebuild = {
            let current = self.settings.read().unwrap();
            current.transport != next.transport || current.mock != next.mock
        };
        *self.settings.write().unwrap() = next.clone();
        if rebuild {
            let backend = make_backend(next.transport.clone(), next.mock);
            *self.backend.write().unwrap() = Arc::from(backend);
            *self.conn.write().unwrap() = ConnState {
                phase: ConnPhase::Connecting,
                endpoint: endpoint_label(&next.transport),
                daemon_version: None,
                error: None,
                retry_in_seconds: None,
            };
            self.tracker_cache.lock().unwrap().clear();
        }
        self.repoll.notify_one();
        next
    }

    /// One fast-poll tick: list + globals. Returns `Err` when the daemon is
    /// unreachable; the model reports the disconnected state and backs off.
    pub fn tick(&self) -> ServiceTask<(Vec<RawTorrent>, RawGlobal)> {
        let backend = self.backend();
        self.submit(async move {
            let torrents = backend.list_snapshot().await?;
            let globals = backend.global_stats().await?;
            Ok((torrents, globals))
        })
    }

    /// The torrent list.
    pub fn list_snapshot(&self) -> ServiceTask<Vec<RawTorrent>> {
        let backend = self.backend();
        self.submit(async move { backend.list_snapshot().await })
    }

    /// Global rates, limits and DHT node count.
    pub fn global_stats(&self) -> ServiceTask<RawGlobal> {
        let backend = self.backend();
        self.submit(async move { backend.global_stats().await })
    }

    /// The daemon version string.
    pub fn client_version(&self) -> ServiceTask<String> {
        let backend = self.backend();
        self.submit(async move { backend.client_version().await })
    }

    /// Probe a transport without adopting it (Preferences → Test connection).
    /// Anonymous on purpose: passwords live in the OS keychain, which this
    /// shell does not manage yet, so a passworded HTTP daemon reports what an
    /// anonymous probe sees.
    pub fn test_connection(&self, transport: Transport) -> ServiceTask<String> {
        self.submit(async move {
            let backend = make_backend(transport, false);
            backend.client_version().await
        })
    }

    /// Push daemon-affecting preferences after Apply: listen port range, DHT
    /// flag, then the network prefs (encryption, PEX, proxy, bind, caps).
    /// Stops at the first refusal and reports it, so the dialog can name the
    /// field that did not take.
    pub fn push_daemon_config(&self, settings: Settings) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move {
            backend.set_port_range(&settings.port_range).await?;
            backend.set_dht(settings.dht_enabled).await?;
            crate::network_prefs::apply(backend.as_ref(), &settings).await;
            Ok(())
        })
    }

    /// True when the saved transport differs from the live one in a way that
    /// needs a daemon push (port range, DHT, or any network-pref key).
    #[must_use]
    pub fn daemon_config_changed(old: &Settings, next: &Settings) -> bool {
        old.port_range != next.port_range
            || old.dht_enabled != next.dht_enabled
            || old.encryption != next.encryption
            || old.pex_enabled != next.pex_enabled
            || old.proxy_address != next.proxy_address
            || old.proxy_tracker_http != next.proxy_tracker_http
            || old.bind_address != next.bind_address
            || old.local_address != next.local_address
            || old.max_peers != next.max_peers
            || old.max_uploads_global != next.max_uploads_global
            || old.max_downloads_global != next.max_downloads_global
    }

    /// Primary tracker host for a hash (the slow poll), cached per hash.
    pub fn primary_tracker(&self, hash: String) -> ServiceTask<String> {
        let backend = self.backend();
        self.submit(async move { backend.primary_tracker(&hash).await })
    }

    /// Assemble DTOs from a raw tick. Tracker hosts come from the cache;
    /// named-throttle limits come from settings.
    pub fn to_dtos(&self, raw: &[RawTorrent]) -> Vec<TorrentDto> {
        let settings = self.settings();
        let cache = self.tracker_cache.lock().unwrap();
        raw.iter()
            .map(|t| {
                let limits = settings
                    .torrent_throttles
                    .iter()
                    .find(|definition| definition.name == t.throttle_name)
                    .map(|definition| (definition.down_kb, definition.up_kb));
                rtorrent_core::rtorrent::derive::to_dto(
                    t,
                    cache.get(&t.hash).map_or("", String::as_str),
                    limits,
                )
            })
            .collect()
    }

    /// Remember resolved tracker hosts.
    pub fn remember_trackers(&self, resolved: Vec<(String, String)>) {
        let mut cache = self.tracker_cache.lock().unwrap();
        for (hash, host) in resolved {
            cache.entry(hash).or_insert(host);
        }
    }

    /// Assemble globals from raw counters plus turtle state.
    pub fn to_globals(&self, raw: &RawGlobal, turtle_active: bool) -> GlobalStats {
        rtorrent_core::snapshot::to_globals(raw, None, None, turtle_active)
    }

    /// Set the connection state (the poll loop owns this).
    #[allow(dead_code)]
    pub fn set_conn(&self, next: ConnState) {
        *self.conn.write().unwrap() = next;
    }

    /// Ask the daemon for an immediate action. Each method clones the backend
    /// `Arc` and submits one future, so GPUI handlers stay one-liners.
    pub fn start(&self, hashes: Vec<String>) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move { backend.start(&hashes).await })
    }

    pub fn stop(&self, hashes: Vec<String>) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move { backend.stop(&hashes).await })
    }

    pub fn recheck(&self, hashes: Vec<String>) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move { backend.recheck(&hashes).await })
    }

    pub fn force_reannounce(&self, hashes: Vec<String>) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move { backend.force_reannounce(&hashes).await })
    }

    pub fn set_label(&self, hashes: Vec<String>, label: String) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move { backend.set_label(&hashes, &label).await })
    }

    /// Replace the tags on each hash (V3-10); an empty list clears them.
    pub fn set_tags(&self, hashes: Vec<String>, tags: Vec<String>) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move { backend.set_tags(&hashes, &tags).await })
    }

    /// The runtime the daemon calls run on.
    //
    // For a host that needs a Tokio context of its own rather than a second
    // runtime beside this one — the web UI server is the only one (`web_host`),
    // because GPUI's executor has no reactor to hang it on.
    #[must_use]
    pub fn runtime(&self) -> tokio::runtime::Handle {
        self.runtime
            .as_ref()
            .expect("the service runtime lives as long as the app")
            .handle()
            .clone()
    }

    /// True when the daemon's files are on this machine.
    #[must_use]
    pub fn is_local(&self) -> bool {
        crate::settings::is_localhost(&self.settings().transport)
    }

    /// Remove torrents, optionally moving their data to the Trash first.
    ///
    /// The base paths are read *before* the erase, because afterwards rtorrent
    /// no longer knows them. Data is only touched for a local daemon, and only
    /// ever through the Trash — never a hard delete.
    pub fn remove(&self, hashes: Vec<String>, delete_data: bool) -> ServiceTask<()> {
        let backend = self.backend();
        let local = self.is_local();
        self.submit(async move {
            let mut paths = Vec::new();
            if delete_data && local {
                for hash in &hashes {
                    if let Ok(path) = backend.base_path(hash).await {
                        if !path.is_empty() {
                            paths.push(path);
                        }
                    }
                }
            }
            backend.erase(&hashes).await?;
            for path in paths {
                if let Err(error) = crate::localfs::trash(&path) {
                    eprintln!("rstorrent: could not trash {path}: {error}");
                }
            }
            Ok(())
        })
    }

    /// Reorder torrents in the queue (QUE-02): swap whole (priority,
    /// queue_pos) pairs with the neighbour, so a torrent takes exactly the
    /// rank above/below it, or pin the daemon's max/min band with an extreme
    /// sequence value. rtorrent has no true positions — bands, never "#" slots.
    pub fn queue_move(&self, hashes: Vec<String>, direction: QueueDirection) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move {
            let rows = backend.list_snapshot().await?;
            let dir = match direction {
                QueueDirection::Top => rtorrent_core::queue::MoveDir::Top,
                QueueDirection::Up => rtorrent_core::queue::MoveDir::Up,
                QueueDirection::Down => rtorrent_core::queue::MoveDir::Down,
                QueueDirection::Bottom => rtorrent_core::queue::MoveDir::Bottom,
            };
            for reorder in rtorrent_core::queue::plan_reorder(&rows, &hashes, dir) {
                backend
                    .set_priority(&reorder.hash, reorder.priority)
                    .await?;
                backend
                    .set_custom_metadata(
                        &reorder.hash,
                        &[(
                            rtorrent_core::queue::POS_KEY,
                            &rtorrent_core::queue::format_pos(reorder.pos),
                        )],
                    )
                    .await?;
            }
            Ok(())
        })
    }

    /// Toggle force-start (QUE-01) on the selection: mixed selections switch
    /// on, uniformly forced selections switch off. Forced torrents are exempt
    /// from the client queue scheduler.
    pub fn toggle_force_start(&self, hashes: Vec<String>) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move {
            let rows = backend.list_snapshot().await?;
            let value = rtorrent_core::queue::force_toggle_value(&rows, &hashes);
            for hash in &hashes {
                backend
                    .set_custom_metadata(hash, &[(rtorrent_core::queue::FORCE_KEY, value)])
                    .await?;
            }
            Ok(())
        })
    }

    /// Test one RSS rule against its feeds (V3-23): the first few items of
    /// each in-scope feed with the verdict and the first failing clause, so
    /// the preferences preview explains every miss. Capped — previews show
    /// the head of a feed, never all of it.
    pub fn rss_test(
        &self,
        rule: rtorrent_core::rss::Rule,
        feeds: Vec<rtorrent_core::rss::Feed>,
    ) -> ServiceTask<crate::rss::TestReport> {
        self.submit(async move {
            use rtorrent_core::rss::{explain, fetch, Verdict};
            let mut rows = Vec::new();
            for feed in feeds
                .iter()
                .filter(|f| f.enabled && (rule.feed_id.is_empty() || rule.feed_id == f.id))
                .take(3)
            {
                let items = fetch(&feed.url).await.map_err(|e| {
                    rtorrent_core::rtorrent::RtorrentError::Unexpected(format!(
                        "{} failed: {e}",
                        feed.name
                    ))
                })?;
                for item in items.into_iter().take(20) {
                    let (matched, reason) = match explain(&rule, &item) {
                        Verdict::Match { .. } => (true, String::new()),
                        Verdict::NoMatch { clauses } => (
                            false,
                            clauses
                                .into_iter()
                                .find(|c| !c.passed)
                                .map(|c| format!("{}: {}", c.label, c.detail))
                                .unwrap_or_default(),
                        ),
                        Verdict::RuleError(err) => (false, err),
                    };
                    rows.push(crate::rss::TestRow {
                        feed: feed.name.clone(),
                        title: item.title,
                        matched,
                        reason,
                    });
                    if rows.len() >= 30 {
                        break;
                    }
                }
                if rows.len() >= 30 {
                    break;
                }
            }
            Ok(crate::rss::TestReport { rows })
        })
    }

    /// Build the portable session manifest text (V3-22 / LIB-09): snapshot,
    /// per-torrent trackers, best-available sources, global limits. Never
    /// credentials. The dialog saves the text to the user's picked path.
    pub fn export_session_text(&self) -> ServiceTask<String> {
        let backend = self.backend();
        let settings = self.settings();
        self.submit(async move {
            let rows = backend.list_snapshot().await?;
            let mut trackers = std::collections::HashMap::new();
            let mut sources = std::collections::HashMap::new();
            for row in &rows {
                if let Ok(list) = backend.trackers(&row.hash).await {
                    trackers.insert(row.hash.clone(), list.into_iter().map(|t| t.url).collect());
                }
                if let Some(source) = session_source_for(&row.source_path) {
                    sources.insert(row.hash.clone(), source);
                }
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or_default();
            Ok(rtorrent_core::session::export_manifest(
                &rows,
                &trackers,
                &sources,
                settings.down_limit_kb,
                settings.up_limit_kb,
                now,
                env!("CARGO_PKG_VERSION"),
            )
            .to_json())
        })
    }

    /// Dry-run validation + restore preview from manifest text (V3-22).
    pub fn validate_session(
        &self,
        manifest_text: String,
        selected: Option<std::collections::HashSet<String>>,
        remaps: Vec<(String, String)>,
    ) -> ServiceTask<rtorrent_core::session::ValidationDto> {
        let backend = self.backend();
        self.submit(async move {
            let manifest = rtorrent_core::session::Manifest::parse(&manifest_text)
                .map_err(RtorrentError::Unexpected)?;
            let existing: std::collections::HashSet<String> = backend
                .list_snapshot()
                .await?
                .into_iter()
                .map(|t| t.hash)
                .collect();
            Ok(rtorrent_core::session::preview(
                &manifest,
                &existing,
                selected.as_ref(),
                &remaps,
            ))
        })
    }

    /// Read-only discovery of another client's resume data (V3-22 / LIB-10).
    /// `dir` is the `BT_backup` folder for qBittorrent, or the config dir
    /// (or its `resume/` subdir) for Transmission.
    pub fn scan_foreign(
        &self,
        client: String,
        dir: String,
    ) -> ServiceTask<rtorrent_core::foreign::ScanReport> {
        self.submit(async move {
            let client: rtorrent_core::foreign::ForeignClient =
                client.parse().map_err(RtorrentError::Unexpected)?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or_default();
            Ok(rtorrent_core::foreign::scan(
                client,
                std::path::Path::new(&dir),
                now,
            ))
        })
    }

    /// Snapshot of the shared session-import job state (V3-22).
    #[must_use]
    pub fn session_import_state(&self) -> SessionImportState {
        self.session_import.lock().unwrap().clone()
    }

    /// Cancel a running session import after the current torrent's step.
    pub fn cancel_session_import(&self) {
        self.session_import_cancel.store(true, Ordering::Relaxed);
    }

    /// Start the detached, journaled session import (V3-22 / LIB-09).
    /// Fails when a job is already running; the dialog polls
    /// [`Services::session_import_state`] and rejoins after reopen.
    ///
    /// # Errors
    ///
    /// Fails when the manifest does not parse or another import is running.
    pub fn start_session_import(
        &self,
        journal_path: std::path::PathBuf,
        manifest_text: String,
        selected: Option<std::collections::HashSet<String>>,
        remaps: Vec<(String, String)>,
        resume: bool,
    ) -> Result<(), String> {
        use rtorrent_core::session;
        {
            let mut state = self.session_import.lock().unwrap();
            if state.running {
                return Err("an import is already running".into());
            }
            *state = SessionImportState {
                running: true,
                ..Default::default()
            };
        }
        let manifest = session::Manifest::parse(&manifest_text)?;
        let backend = self.backend();
        let shared = Arc::clone(&self.session_import);
        let cancel_flag = Arc::clone(&self.session_import_cancel);
        let repoll = self.repoll.clone();
        cancel_flag.store(false, Ordering::Relaxed);
        let runtime = self
            .runtime
            .as_ref()
            .expect("service runtime is alive")
            .handle()
            .clone();
        runtime.spawn(async move {
            let outcome = run_session_import_job(SessionImportJob {
                backend: backend.as_ref(),
                manifest: &manifest,
                selected,
                remaps,
                resume,
                journal_path: &journal_path,
                shared: &shared,
                cancel_flag: &cancel_flag,
            })
            .await;
            let mut state = shared.lock().unwrap();
            state.running = false;
            state.done = true;
            match outcome {
                Ok(summary) => {
                    state.added = summary.added;
                    state.resumed = summary.resumed;
                    state.skipped = summary.skipped;
                    state.failed = summary.failed.clone();
                    push_import_line(
                        &mut state,
                        format!(
                            "import finished: {} added, {} resumed, {} skipped, {} failed",
                            summary.added,
                            summary.resumed,
                            summary.skipped,
                            summary.failed.len()
                        ),
                    );
                }
                Err(e) => {
                    state.failed = vec![e.clone()];
                    push_import_line(&mut state, format!("import failed: {e}"));
                }
            }
            repoll.notify_one();
        });
        Ok(())
    }

    /// Apply a per-torrent rate limit through the app-owned named-throttle pool.
    ///
    /// Returns the definition the caller must persist when a slot was newly
    /// defined or redefined, so settings stay the source of truth for names the
    /// daemon forgets across restarts.
    ///
    /// # Errors
    ///
    /// Fails when the daemon rejects the assignment, or when the torrent does
    /// not end up on the throttle we asked for.
    pub fn set_torrent_limits(
        &self,
        hashes: Vec<String>,
        down_kb: i64,
        up_kb: i64,
    ) -> ServiceTask<Option<crate::settings::NamedThrottle>> {
        let backend = self.backend();
        let definitions = self.settings().torrent_throttles;
        self.submit(async move {
            if hashes.is_empty() {
                return Ok(None);
            }
            if down_kb < 0 || up_kb < 0 {
                return Err(RtorrentError::Unexpected(
                    "rate limits must be zero or greater".to_owned(),
                ));
            }
            if down_kb == 0 && up_kb == 0 {
                backend.assign_throttle(&hashes, None).await?;
                let assignment = backend.torrent_throttle_name(&hashes[0]).await?;
                if assignment.is_empty() {
                    // Hand-cleared limits opt out of bandwidth rules too.
                    for hash in &hashes {
                        let _ = backend
                            .set_custom_metadata(hash, &[(rtorrent_core::bandwidth::RULE_KEY, "")])
                            .await;
                    }
                    return Ok(None);
                }
                return Err(RtorrentError::Unexpected(format!(
                    "torrent still uses throttle {assignment}"
                )));
            }

            let rows = backend.list_snapshot().await?;
            let active: std::collections::HashSet<String> = rows
                .iter()
                .map(|torrent| torrent.throttle_name.clone())
                .filter(|name| !name.is_empty())
                .collect();
            let (definition, changed) =
                crate::throttles::allocate(&definitions, &active, down_kb, up_kb)
                    .map_err(|message| RtorrentError::Unexpected(message.to_owned()))?;

            backend
                .define_named_throttle(&definition.name, down_kb, up_kb)
                .await?;
            backend
                .assign_throttle(&hashes, Some(&definition.name))
                .await?;
            let assignment = backend.torrent_throttle_name(&hashes[0]).await?;
            if assignment != definition.name {
                return Err(RtorrentError::Unexpected(format!(
                    "torrent uses throttle '{assignment}' after assignment"
                )));
            }
            // A hand-set limit is a torrent override (V3-18 precedence):
            // drop any bandwidth-rule marker so the engine keeps off.
            for hash in &hashes {
                let _ = backend
                    .set_custom_metadata(hash, &[(rtorrent_core::bandwidth::RULE_KEY, "")])
                    .await;
            }
            Ok(changed.then_some(definition))
        })
    }

    /// Relocate a torrent's download directory, optionally moving its data.
    ///
    /// rtorrent requires the torrent to be closed for `d.directory.set`, so an
    /// active torrent is stopped first and restarted after — and restarted
    /// again if the move fails, so a failure never leaves it paused.
    pub fn set_location(&self, hash: String, path: String, move_data: bool) -> ServiceTask<()> {
        let backend = self.backend();
        let local = self.is_local();
        self.submit(async move {
            let path = crate::localfs::to_daemon_path(&path).map_err(RtorrentError::Unsupported)?;
            let should_move = move_data && local;

            let rows = backend.list_snapshot().await?;
            let torrent = rows
                .iter()
                .find(|torrent| torrent.hash.eq_ignore_ascii_case(&hash));
            let was_active = torrent.is_some_and(|torrent| torrent.is_active);
            let old_base_path = torrent.map_or(String::new(), |torrent| torrent.base_path.clone());

            let one = std::slice::from_ref(&hash);
            if was_active {
                backend.stop(one).await?;
            }

            if should_move && !old_base_path.is_empty() {
                if let Err(message) = crate::localfs::move_torrent_data(&old_base_path, &path) {
                    if was_active {
                        let _ = backend.start(one).await;
                    }
                    return Err(RtorrentError::Unexpected(message));
                }
            }

            backend.set_directory(&hash, &path).await?;
            // A manual move wins over automation: drop any recorded final_dir
            // so a later completion does not drag the torrent back (V3-14).
            let _ = backend
                .set_custom_metadata(&hash, &[(rtorrent_core::complete::FINAL_DIR_KEY, "")])
                .await;
            if was_active {
                backend.start(one).await?;
            }
            Ok(())
        })
    }

    pub fn set_super_seeding(&self, hash: String, enabled: bool) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move { backend.set_super_seeding(&hash, enabled).await })
    }

    pub fn set_connection_limits(
        &self,
        hash: String,
        peers_max: i64,
        peers_min: i64,
        uploads_max: i64,
    ) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move {
            backend
                .set_connection_limits(&hash, peers_max, peers_min, uploads_max)
                .await?;
            // Hand-set caps opt out of bandwidth rules (V3-18 precedence).
            let _ = backend
                .set_custom_metadata(&hash, &[(rtorrent_core::bandwidth::RULE_KEY, "")])
                .await;
            Ok(())
        })
    }

    pub fn set_file_priority(&self, hash: String, index: usize, priority: i64) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move { backend.set_file_priority(&hash, index, priority).await })
    }

    pub fn add_tracker(&self, hash: String, url: String) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move { backend.add_tracker(&hash, &url).await })
    }

    pub fn remove_tracker(&self, hash: String, index: usize) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move { backend.remove_tracker(&hash, index).await })
    }

    pub fn set_tracker_enabled(
        &self,
        hash: String,
        index: usize,
        enabled: bool,
    ) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move { backend.set_tracker_enabled(&hash, index, enabled).await })
    }

    /// One of the three Peers-tab verbs: ban, snub or disconnect.
    pub fn peer_action(&self, hash: String, peer_id: String, verb: PeerVerb) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move {
            match verb {
                PeerVerb::Ban => backend.ban_peer(&hash, &peer_id).await,
                PeerVerb::Snub => backend.snub_peer(&hash, &peer_id).await,
                PeerVerb::Disconnect => backend.disconnect_peer(&hash, &peer_id).await,
            }
        })
    }

    /// Add a `.torrent` file's bytes with the caller's options.
    ///
    /// When an incomplete dir is configured (V3-14) the torrent loads there
    /// and its chosen path is recorded in `d.custom=final_dir`. The metadata
    /// write is best-effort — the add itself already succeeded — matching the
    /// Tauri shell, which logs the failure and continues.
    pub fn add_raw(&self, bytes: Vec<u8>, options: AddOptions) -> ServiceTask<()> {
        let backend = self.backend();
        let settings = self.settings();
        self.submit(async move {
            let final_hash = rtorrent_core::torrent_file::read_metadata_bytes(&bytes)
                .ok()
                .map(|meta| meta.info_hash);
            let mut opts = load_options(&options)?;
            let incomplete = crate::localfs::to_daemon_path(&settings.incomplete_dir)
                .map_err(RtorrentError::Unsupported)?;
            let routed =
                rtorrent_core::complete::route_new_download(&incomplete, &opts.directory);
            opts.directory = routed.0;
            let final_dir = routed.1;
            backend.load_raw(bytes, opts).await?;
            if let (Some(hash), Some(final_dir)) = (final_hash, final_dir) {
                let _ = backend
                    .set_custom_metadata(
                        &hash,
                        &[(rtorrent_core::complete::FINAL_DIR_KEY, final_dir.as_str())],
                    )
                    .await;
            }
            Ok(())
        })
    }

    /// Add a magnet URI or torrent URL with the caller's options.
    ///
    /// Incomplete-dir routing (V3-14) applies as in [`Self::add_raw`]; the
    /// final dir is recorded only when the magnet carries a parseable hex
    /// info-hash, otherwise the completion falls back to the move rules.
    pub fn add_magnet(&self, uri: String, options: AddOptions) -> ServiceTask<()> {
        let backend = self.backend();
        let settings = self.settings();
        self.submit(async move {
            let mut opts = load_options(&options)?;
            let incomplete = crate::localfs::to_daemon_path(&settings.incomplete_dir)
                .map_err(RtorrentError::Unsupported)?;
            let routed =
                rtorrent_core::complete::route_new_download(&incomplete, &opts.directory);
            opts.directory = routed.0;
            let final_dir = routed.1;
            backend.load_magnet(&uri, opts).await?;
            if let (Some(hash), Some(final_dir)) =
                (rtorrent_core::rtorrent::magnet_hash(&uri), final_dir)
            {
                let _ = backend
                    .set_custom_metadata(
                        &hash,
                        &[(rtorrent_core::complete::FINAL_DIR_KEY, final_dir.as_str())],
                    )
                    .await;
            }
            Ok(())
        })
    }

    /// Build the magnet link for a torrent: `xt` plus its display name.
    ///
    /// The info-hash alone is not enough to join a private swarm, which is why
    /// the context menu disables this for private torrents rather than handing
    /// out a link that cannot work.
    pub fn magnet_uri(&self, hash: String) -> ServiceTask<String> {
        let backend = self.backend();
        self.submit(async move {
            let rows = backend.list_snapshot().await?;
            let name = rows
                .iter()
                .find(|torrent| torrent.hash.eq_ignore_ascii_case(&hash))
                .map_or(String::new(), |torrent| torrent.name.clone());
            Ok(format!(
                "magnet:?xt=urn:btih:{hash}&dn={}",
                urlencode(&name)
            ))
        })
    }

    /// Reveal a torrent's data in the platform file manager (local daemons only).
    pub fn open_destination(&self, hash: String) -> ServiceTask<()> {
        let backend = self.backend();
        let local = self.is_local();
        self.submit(async move {
            if !local {
                return Err(RtorrentError::Unsupported(
                    "open destination is only available for a local daemon".to_owned(),
                ));
            }
            let path = backend.base_path(&hash).await?;
            if path.is_empty() {
                return Err(RtorrentError::Unsupported("no path on disk yet".to_owned()));
            }
            crate::localfs::reveal(&path).map_err(RtorrentError::Unsupported)
        })
    }

    // --- Detail-tab reads (the 2 s poll) ------------------------------------

    /// The Trackers tab's rows.
    pub fn trackers(&self, hash: String) -> ServiceTask<Vec<TrackerRow>> {
        let backend = self.backend();
        self.submit(async move { backend.trackers(&hash).await })
    }

    /// The Peers tab's rows.
    pub fn peers(&self, hash: String) -> ServiceTask<Vec<PeerRow>> {
        let backend = self.backend();
        self.submit(async move { backend.peers(&hash).await })
    }

    /// The Content tab's file tree.
    pub fn files(&self, hash: String) -> ServiceTask<Vec<FileNode>> {
        let backend = self.backend();
        self.submit(async move { backend.files(&hash).await })
    }

    /// The General tab's pieces bar and availability map.
    pub fn pieces(&self, hash: String) -> ServiceTask<PieceInfo> {
        let backend = self.backend();
        self.submit(async move { backend.pieces(&hash).await })
    }

    /// The daemon's own counters, for the Statistics dialog.
    pub fn statistics(&self) -> ServiceTask<RawStats> {
        let backend = self.backend();
        self.submit(async move { backend.statistics().await })
    }

    /// The daemon's health, for the Statistics dialog's Daemon tab.
    pub fn daemon_health(&self) -> ServiceTask<DaemonHealth> {
        let backend = self.backend();
        self.submit(async move { backend.daemon_health().await })
    }

    pub fn save_session(&self) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move { backend.save_session().await })
    }

    pub fn shutdown(&self) -> ServiceTask<()> {
        let backend = self.backend();
        self.submit(async move { backend.shutdown().await })
    }

    pub fn retry_now(&self) {
        self.repoll.notify_one();
    }

    pub(crate) fn submit<T, F>(&self, operation: F) -> ServiceTask<T>
    where
        T: Send + 'static,
        F: Future<Output = Result<T, RtorrentError>> + Send + 'static,
    {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.runtime
            .as_ref()
            .expect("service runtime is alive")
            .spawn(async move {
                let _ = sender.send(operation.await);
            });
        Box::pin(async move {
            receiver.await.map_err(|_| {
                RtorrentError::Unreachable("native service runtime stopped".to_owned())
            })?
        })
    }
}

impl Drop for Services {
    fn drop(&mut self) {
        // `Services` is normally dropped by GPUI, but tests may release it
        // while another async runtime is on the current thread.
        // `Runtime::drop` would panic in that situation; background shutdown
        // still stops the worker threads without blocking the caller.
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

/// Manual queue-reorder direction (QUE-02), mirroring the toolbar verbs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueDirection {
    Top,
    Up,
    Down,
    Bottom,
}

impl QueueDirection {
    /// Past-tense verb for the app log.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Top => "moved to the top",
            Self::Up => "moved up",
            Self::Down => "moved down",
            Self::Bottom => "moved to the bottom",
        }
    }
}

/// One of the Peers tab's row actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerVerb {
    Ban,
    Snub,
    Disconnect,
}

impl PeerVerb {
    /// The past-tense verb for the app log.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ban => "banned",
            Self::Snub => "snubbed",
            Self::Disconnect => "disconnected",
        }
    }
}

/// Best-effort re-addable source for one torrent (V3-22 export): an embedded
/// `.torrent` when its file is readable, else the recorded magnet/URL/path.
fn session_source_for(source_path: &str) -> Option<rtorrent_core::session::ManifestSource> {
    use rtorrent_core::session::ManifestSource;
    let trimmed = source_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("magnet:")
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
    {
        return Some(ManifestSource::Magnet(trimmed.to_owned()));
    }
    match std::fs::read(trimmed) {
        Ok(bytes) => Some(ManifestSource::TorrentBase64(
            rtorrent_core::session::encode_base64(&bytes),
        )),
        Err(_) => Some(ManifestSource::TorrentPath(trimmed.to_owned())),
    }
}

/// Append a line to the shared import log, keeping the last 50.
fn push_import_line(state: &mut SessionImportState, line: String) {
    state.log.push(line);
    while state.log.len() > 50 {
        state.log.remove(0);
    }
}

/// Execute a journaled session import on the service runtime (V3-22). The
/// shared state carries progress for the dialog; the journal on disk carries
/// crash-safety. Returns the run summary for the final status line.
struct SessionImportJob<'a> {
    backend: &'a dyn RtorrentApi,
    manifest: &'a rtorrent_core::session::Manifest,
    selected: Option<std::collections::HashSet<String>>,
    remaps: Vec<(String, String)>,
    resume: bool,
    journal_path: &'a std::path::Path,
    shared: &'a Arc<Mutex<SessionImportState>>,
    cancel_flag: &'a Arc<AtomicBool>,
}

async fn run_session_import_job(
    job: SessionImportJob<'_>,
) -> Result<rtorrent_core::session::ImportSummary, String> {
    use rtorrent_core::session::{self, ImportJournal, RestoreAction};
    let SessionImportJob {
        backend,
        manifest,
        selected,
        remaps,
        resume,
        journal_path,
        shared,
        cancel_flag,
    } = job;
    let mut journal = session::load_journal(journal_path);
    let manifest_id = ImportJournal::manifest_id(manifest);
    if !resume || journal.manifest_id != manifest_id {
        journal = ImportJournal {
            manifest_id: manifest_id.clone(),
            total: manifest.torrents.len(),
            started_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or_default(),
            ..Default::default()
        };
    }
    let existing: std::collections::HashSet<String> = backend
        .list_snapshot()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|t| t.hash)
        .collect();
    let planned = session::plan_restore(manifest, &existing, selected.as_ref(), &remaps);
    let mut skipped = 0usize;
    let mut items = Vec::new();
    for item in planned {
        match item.action {
            RestoreAction::Add => items.push(session::PlannedRestore {
                torrent: item.torrent,
                dst_dir: item.dst_dir,
            }),
            _ => skipped += 1,
        }
    }
    let log_shared = Arc::clone(shared);
    let log: session::ImportLog = Arc::new(move |level, message, hash| {
        let mut state = log_shared.lock().unwrap();
        if let Some(hash) = hash {
            state.current = hash;
        }
        let _ = level;
        push_import_line(&mut state, message);
    });
    // Mirror the shared cancel flag into the plain flag the driver polls, and
    // stop mirroring once the run ends so the watcher task exits.
    let done = Arc::new(AtomicBool::new(false));
    let done_mirror = Arc::clone(&done);
    let cancel_mirror = Arc::new(AtomicBool::new(false));
    let cancel_driver = Arc::clone(&cancel_mirror);
    let cancel_shared = Arc::clone(cancel_flag);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if done_mirror.load(Ordering::Relaxed) || cancel_shared.load(Ordering::Relaxed) {
                if cancel_shared.load(Ordering::Relaxed) {
                    cancel_mirror.store(true, Ordering::Relaxed);
                }
                break;
            }
        }
    });
    let save_path = journal_path.to_path_buf();
    let save = move |journal: &ImportJournal| {
        let _ = session::save_journal(&save_path, journal);
    };
    let now_ms = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_default()
    };
    let mut summary = session::run_import(
        backend,
        &items,
        &mut journal,
        &save,
        &log,
        &cancel_driver,
        now_ms,
    )
    .await;
    done.store(true, Ordering::Relaxed);
    summary.skipped = skipped;
    if summary.failed.is_empty() && !cancel_driver.load(Ordering::Relaxed) {
        let _ = std::fs::remove_file(journal_path);
    }
    Ok(summary)
}

/// Project the caller's add options onto what the client layer needs.
///
/// The save path may have come from this machine's folder picker, so it is
/// normalized into the daemon's namespace here — the one place every add
/// (and create-torrent's seeding) passes through. A no-op off Windows.
fn load_options(options: &AddOptions) -> Result<LoadOptions, RtorrentError> {
    Ok(LoadOptions {
        directory: crate::localfs::to_daemon_path(&options.save_path)
            .map_err(RtorrentError::Unsupported)?,
        label: options.label.clone(),
        start: options.start,
        top_of_queue: options.top_of_queue,
        unselected_indexes: options.unselected_indexes.clone(),
    })
}

/// Percent-encode a magnet's display name, matching the Tauri command's
/// `urlencode`: unreserved characters pass through, everything else becomes
/// `%XX`, and spaces become `+` through the same rule (the magnet's `dn` is a
/// query parameter).
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtorrent_core::types::Transport;

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn mock_service_lists_without_a_daemon() {
        let services = Services::mock().expect("runtime");
        let torrents = services.list_snapshot().await.expect("listing");
        assert!(!torrents.is_empty());
        let dtos = services.to_dtos(&torrents);
        assert_eq!(dtos.len(), torrents.len());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn adding_a_magnet_carries_the_dialogs_options() {
        let services = Services::mock().expect("runtime");
        let before = services.list_snapshot().await.expect("listing").len();

        let options = crate::add_dialogs::add_options(
            "/downloads/iso",
            "iso",
            false,
            true,
            false,
            Vec::new(),
        );
        services
            .add_magnet("magnet:?xt=urn:btih:c12fe1c&dn=Debian".to_owned(), options)
            .await
            .expect("the mock accepts a magnet");

        let after = services.list_snapshot().await.expect("listing");
        assert_eq!(after.len(), before + 1);
        let added = after
            .iter()
            .find(|torrent| torrent.name == "Debian")
            .expect("the added torrent");
        // The whole form travelled: the daemon knows where it goes, what it is
        // labelled and that it was not started.
        assert_eq!(added.label, "iso");
        assert_eq!(added.directory, "/downloads/iso");
        assert!(!added.is_active);
        // Top of the queue is rtorrent's priority 3.
        assert_eq!(added.priority, 3);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn adding_a_torrent_files_bytes_reaches_the_daemon() {
        let services = Services::mock().expect("runtime");
        let before = services.list_snapshot().await.expect("listing").len();

        let options = crate::add_dialogs::add_options("", "", true, false, true, vec![1]);
        services
            .add_raw(vec![0x64, 0x38, 0x3a], options)
            .await
            .expect("the mock accepts raw bytes");

        let after = services.list_snapshot().await.expect("listing");
        assert_eq!(after.len(), before + 1);
        let added = after
            .iter()
            .find(|torrent| torrent.name == "added-from-file.iso")
            .expect("the added torrent");
        assert!(added.is_active);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn update_settings_rebuilds_backend_on_transport_change() {
        let services = Services::mock().expect("runtime");
        let mut next = services.settings();
        next.mock = false;
        next.transport = Transport::Tcp {
            host: "127.0.0.1".into(),
            port: 5000,
        };
        services.update_settings(next);
        assert_eq!(services.conn().phase, ConnPhase::Connecting);
    }

    #[test]
    fn daemon_config_changed_fires_only_on_daemon_keys() {
        let old = Settings::default();
        // View state and automation never need a daemon push.
        let mut same = old.clone();
        same.filter = "status:downloading".into();
        same.down_limit_kb = 100;
        same.turtle_enabled = true;
        assert!(!Services::daemon_config_changed(&old, &same));

        let mut port = old.clone();
        port.port_range = "7000-7010".into();
        assert!(Services::daemon_config_changed(&old, &port));

        let mut dht = old.clone();
        dht.dht_enabled = !dht.dht_enabled;
        assert!(Services::daemon_config_changed(&old, &dht));

        let mut proxy = old.clone();
        proxy.proxy_address = "127.0.0.1:8080".into();
        assert!(Services::daemon_config_changed(&old, &proxy));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn push_daemon_config_succeeds_against_the_mock() {
        let services = Services::mock().expect("runtime");
        let mut next = services.settings();
        next.port_range = "7000-7010".into();
        next.dht_enabled = true;
        services
            .push_daemon_config(next)
            .await
            .expect("the mock accepts every directive");
    }
}
