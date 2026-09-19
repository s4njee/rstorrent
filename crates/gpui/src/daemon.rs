//! Starting a local rtorrent daemon on demand.
//!
//! A port of `src-tauri/src/daemon_start.rs`, minus the Tauri resource
//! plumbing. The app is a *client* for an rtorrent daemon, but the macOS build
//! also ships its own so a fresh install needs no `brew install rtorrent`:
//! `tools/build-rtorrent-macos.sh` stages the runtime, `build.rs` copies it
//! beside the executable, and `tools/bundle-gpui-macos.sh` puts it in the
//! `.app`'s `Contents/Resources`. When it is there we launch it; otherwise we
//! fall back to whatever is on `PATH`.
//!
//! Starting mirrors `start-rtorrent.sh` — create the session directory, clear
//! stale lock/socket files, and launch rtorrent inside a detached tmux session
//! (falling back to a direct spawn when tmux is absent).
//!
//! On Windows the daemon lives inside a WSL VM; [`crate::daemon_wsl`] imports
//! and starts the bundled one there, and the functions below delegate to it.

#[cfg(not(target_os = "windows"))]
use std::path::{Path, PathBuf};

use rtorrent_core::types::Transport;

/// The rtorrent/libtorrent tag staged by `tools/build-rtorrent-macos.sh`.
///
/// Keep this in step with the script's default `RTORRENT_VERSION`; it labels
/// the generated config and the log line, and never gates behavior.
pub const BUNDLED_RTORRENT_VERSION: &str = "0.15.7";

/// Where the staged runtime sits inside a `.app`.
///
/// The executable is at `Contents/MacOS/`, so this is one level up and into
/// `Resources` — the same layout `tauri.conf.json` produces for the Tauri
/// shell, which is what lets one staging directory serve both.
#[cfg(not(target_os = "windows"))]
const BUNDLE_RUNTIME: &str = "../Resources/binaries/rtorrent/rtorrent";

/// Where the runtime sits next to a bare executable, which is where a cargo
/// build puts it (`build.rs` copies the staged directory into the target dir).
#[cfg(not(target_os = "windows"))]
const ADJACENT_RUNTIME: &str = "binaries/rtorrent/rtorrent";

/// Start a local rtorrent, preferring the runtime this build ships.
///
/// Returns a message for the status line saying what happened and whether the
/// daemon came from the bundle.
///
/// # Errors
///
/// Returns a user-facing message when no rtorrent can be found, or when the
/// launch itself fails.
#[cfg(not(target_os = "windows"))]
pub fn start(transport: &Transport) -> Result<String, String> {
    // A daemon on another host is not ours to launch, and the starter config
    // below would write a remote endpoint into a local `~/.rtorrent.rc`. Same
    // guard, and the same wording, as the Tauri command.
    if !crate::settings::is_localhost(transport) {
        return Err("start is only available for a local daemon".to_owned());
    }

    let (bin, bundled) = rtorrent_bin()?;

    // A bundled daemon should come up reachable: rtorrent will not open SCGI
    // from its built-in defaults, so a machine that has never had rtorrent
    // gets a minimal config. An existing `~/.rtorrent.rc` is never touched.
    if bundled {
        ensure_config(transport)?;
    }

    let home = home_dir();
    let session_dir = home.join(".rtorrent/session");
    let socket_path = socket_path(transport, &home);
    let socket = Path::new(&socket_path);

    std::fs::create_dir_all(&session_dir)
        .map_err(|error| format!("could not create {}: {error}", session_dir.display()))?;

    if is_tmux_session_running() {
        return Ok("rtorrent is already running (tmux session 'rtorrent')".to_owned());
    }
    if is_process_running() {
        return Ok("rtorrent is already running".to_owned());
    }

    // A daemon that died without cleaning up leaves a lock and a socket behind
    // that stop the next one from starting.
    let lock = session_dir.join("rtorrent.lock");
    if lock.exists() {
        let _ = std::fs::remove_file(&lock);
    }
    if socket.exists() {
        let _ = std::fs::remove_file(socket);
    }

    // The daemon's own stderr, which is the one stream it is safe to take: its
    // stdout is the tmux pane it draws its interface on.
    let log = home.join(".rtorrent/rtorrent.err");
    launch(&bin, &log)?;

    // rtorrent reads its config and opens the socket within milliseconds, and a
    // key it does not know kills it immediately after — leaving the socket
    // behind. So give it a beat to settle or die before looking: a socket is not
    // proof of life, and claiming "started" for a daemon that is already gone is
    // worse than failing, because the poller contradicts it a second later.
    std::thread::sleep(SETTLE);
    if !is_alive() {
        return Err(exited_message(&log));
    }

    let origin = if bundled { "bundled" } else { "system" };
    if matches!(transport, Transport::UnixSocket { .. }) {
        // Waiting for the socket lets the caller's first poll succeed rather
        // than failing once while the daemon finishes coming up.
        for _ in 0..SOCKET_ATTEMPTS {
            if socket.exists() {
                return Ok(format!("rtorrent started ({origin}, socket {socket_path})"));
            }
            std::thread::sleep(SOCKET_POLL);
        }
        return Ok(format!(
            "rtorrent launched ({origin}) — waiting for it to create the socket…"
        ));
    }
    Ok(format!("rtorrent started ({origin})"))
}

/// Whether opening the app should bring up the bundled daemon itself.
///
/// True only when this build ships a runtime, the connection is a local SCGI
/// endpoint (never an HTTP front end, which is someone else's server), nothing
/// answers on it, and no rtorrent is already running — so a daemon the user
/// manages, or one still coming up, is never second-guessed. Blocking (it
/// probes the endpoint and shells out), so call it off the UI thread.
#[cfg(not(target_os = "windows"))]
#[must_use]
pub fn should_autostart(transport: &Transport) -> bool {
    let local_scgi = match transport {
        Transport::UnixSocket { .. } => true,
        Transport::Tcp { .. } => crate::settings::is_localhost(transport),
        Transport::Http { .. } => false,
    };
    local_scgi && bundled_rtorrent().is_some() && !is_reachable(transport) && !is_alive()
}

/// On Windows the bundled daemon runs in WSL; see [`crate::daemon_wsl`].
#[cfg(target_os = "windows")]
#[must_use]
pub fn should_autostart(transport: &Transport) -> bool {
    crate::daemon_wsl::should_autostart(transport)
}

/// Whether something accepts connections on the SCGI endpoint.
pub(crate) fn is_reachable(transport: &Transport) -> bool {
    use std::net::ToSocketAddrs;
    match transport {
        #[cfg(unix)]
        Transport::UnixSocket { path } => std::os::unix::net::UnixStream::connect(path).is_ok(),
        // Windows cannot reach a unix socket inside the WSL VM.
        #[cfg(not(unix))]
        Transport::UnixSocket { .. } => false,
        Transport::Tcp { host, port } => (host.as_str(), *port)
            .to_socket_addrs()
            .into_iter()
            .flatten()
            .any(|addr| {
                std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300))
                    .is_ok()
            }),
        Transport::Http { .. } => true,
    }
}

/// How long to let a freshly launched daemon settle before believing in it.
///
/// The window in which a bad config file kills rtorrent is the first few
/// milliseconds of its startup, so this only has to outlast that.
#[cfg(not(target_os = "windows"))]
const SETTLE: std::time::Duration = std::time::Duration::from_millis(500);

/// Whether the daemon is still there: its tmux session, or the process itself
/// when tmux was not available.
#[cfg(not(target_os = "windows"))]
fn is_alive() -> bool {
    is_tmux_session_running() || is_process_running()
}

/// What to say about a daemon that did not survive its own startup.
///
/// rtorrent reports a fatal config error on *its* stdout — the tmux pane it owns
/// and draws its interface on — so the app cannot capture it without taking that
/// interface away from whoever attaches. Anything it does write to stderr (a
/// dyld failure, for instance) is in the log, so that is used when it is there.
#[cfg(not(target_os = "windows"))]
fn exited_message(log: &Path) -> String {
    match last_line(log) {
        Some(reason) => format!("rtorrent exited immediately — {reason}"),
        None => "rtorrent exited immediately and said nothing on stderr — run \
                 `rtorrent` in a terminal to see why (see docs/rtorrent-setup.md)"
            .to_owned(),
    }
}

/// The last non-empty line of a file, when there is one.
#[cfg(not(target_os = "windows"))]
fn last_line(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let line = text.lines().rev().find(|line| !line.trim().is_empty())?;
    Some(line.trim().to_owned()).filter(|line| !line.is_empty())
}

/// Start the bundled rtorrent inside WSL; see [`crate::daemon_wsl`].
///
/// # Errors
///
/// Returns a user-facing message when WSL, the runtime or the launch fails.
#[cfg(target_os = "windows")]
pub fn start(transport: &Transport) -> Result<String, String> {
    crate::daemon_wsl::start(transport)
}

/// How long to wait for the daemon to open its socket, and how often to look.
#[cfg(not(target_os = "windows"))]
const SOCKET_ATTEMPTS: usize = 15;
#[cfg(not(target_os = "windows"))]
const SOCKET_POLL: std::time::Duration = std::time::Duration::from_millis(200);

/// Launch `bin`, detached, in a tmux session when tmux is available.
///
/// tmux keeps the daemon alive after this process exits and gives the user a
/// way back into it; a plain spawn is the fallback for machines without it.
///
/// `log` takes the daemon's stderr. Its stdout is deliberately left alone: that
/// is the interface rtorrent draws, and someone attaching to the session should
/// find it there rather than an empty pane.
#[cfg(not(target_os = "windows"))]
fn launch(bin: &Path, log: &Path) -> Result<(), String> {
    let bin = bin.to_string_lossy().into_owned();

    if let Some(tmux) = tmux_bin() {
        // rtorrent wants more descriptors than the macOS default for a large
        // session; the quoting keeps a path with an apostrophe working.
        let shell_command = format!(
            "ulimit -n 4096 2>/dev/null || true; TERM=xterm-256color {} 2> {}",
            shell_quote(&bin),
            shell_quote(&log.to_string_lossy()),
        );
        let status = std::process::Command::new(tmux)
            .args(["new-session", "-d", "-s", "rtorrent", &shell_command])
            .status()
            .map_err(|error| format!("could not start tmux: {error}"))?;
        if !status.success() {
            return Err(format!("tmux failed to start rtorrent (exit {status})"));
        }
        return Ok(());
    }

    // A missing log must not stop the daemon from starting.
    let stderr = std::fs::File::create(log)
        .map_or_else(|_| std::process::Stdio::null(), std::process::Stdio::from);
    std::process::Command::new(&bin)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(stderr)
        .spawn()
        .map_err(|error| format!("could not start rtorrent ({bin}): {error}"))?;
    Ok(())
}

/// Wrap a value for a shell, escaping an embedded apostrophe.
#[cfg(not(target_os = "windows"))]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Resolve the rtorrent to launch, preferring the one this build ships.
///
/// Returns the path and whether it came from the bundle, which decides whether
/// we may write a starter config.
///
/// # Errors
///
/// Returns a user-facing message when neither the bundle nor `PATH` has one.
#[cfg(not(target_os = "windows"))]
pub fn rtorrent_bin() -> Result<(PathBuf, bool), String> {
    if let Some(path) = bundled_rtorrent() {
        return Ok((path, true));
    }
    find_rtorrent_bin().map(|path| (path, false))
}

/// The runtime this build ships, if it is there.
///
/// `RSTORRENT_RTORRENT_BIN` overrides everything, so a developer can point the
/// app at any build (and the QA checklist can exercise the fallback).
#[cfg(not(target_os = "windows"))]
pub fn bundled_rtorrent() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("RSTORRENT_RTORRENT_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    bundled_rtorrent_in(std::env::current_exe().ok()?.parent()?)
}

/// The bundle lookup for a given executable directory, split out so it can be
/// tested against a fixture tree rather than this build's own layout.
#[cfg(not(target_os = "windows"))]
pub fn bundled_rtorrent_in(exe_dir: &Path) -> Option<PathBuf> {
    [BUNDLE_RUNTIME, ADJACENT_RUNTIME]
        .iter()
        .map(|relative| resolve(exe_dir, relative))
        .find(|candidate| candidate.is_file())
}

/// Join a layout-relative path onto the executable's directory, applying any
/// `..` as it goes.
///
/// Lexical rather than `canonicalize`: the path is handed to a spawn and shown
/// in the status line, and neither should carry `Contents/MacOS/../Resources`.
#[cfg(not(target_os = "windows"))]
fn resolve(exe_dir: &Path, relative: &str) -> PathBuf {
    let mut path = exe_dir.to_path_buf();
    for part in Path::new(relative).components() {
        match part {
            std::path::Component::ParentDir => {
                path.pop();
            }
            std::path::Component::Normal(name) => path.push(name),
            _ => {}
        }
    }
    path
}

/// A one-line description of the rtorrent [`start`] would launch.
///
/// Bundling is invisible when it works — the daemon simply starts — so this is
/// how a build answers "which binary did Start use?", for the QA checklist and
/// for anyone debugging why a fresh install fell back to a system daemon.
#[must_use]
pub fn describe() -> String {
    #[cfg(not(target_os = "windows"))]
    {
        describe_resolution(rtorrent_bin())
    }
    #[cfg(target_os = "windows")]
    {
        crate::daemon_wsl::describe()
    }
}

/// The line [`describe`] prints for a resolution. Split out so all three shapes
/// can be tested without depending on this machine's `PATH`.
#[cfg(not(target_os = "windows"))]
fn describe_resolution(resolved: Result<(PathBuf, bool), String>) -> String {
    match resolved {
        Ok((path, bundled)) => format!(
            "rtorrent: {} ({})",
            path.display(),
            if bundled { "bundled" } else { "system" }
        ),
        Err(error) => format!("rtorrent: none — {error}"),
    }
}

/// The socket the app will connect to, so a start can wait for it.
#[cfg(not(target_os = "windows"))]
#[must_use]
pub fn socket_path(transport: &Transport, home: &Path) -> String {
    match transport {
        Transport::UnixSocket { path } => path.clone(),
        _ => home
            .join(".rtorrent/rpc.socket")
            .to_string_lossy()
            .into_owned(),
    }
}

#[cfg(not(target_os = "windows"))]
fn is_tmux_session_running() -> bool {
    tmux_bin().is_some_and(|tmux| {
        command_succeeds(&tmux.to_string_lossy(), &["has-session", "-t", "rtorrent"])
    })
}

/// tmux from `PATH`, or where a package manager puts it. An app opened from
/// Finder gets launchd's bare `PATH`, which has no Homebrew directory in it.
#[cfg(not(target_os = "windows"))]
fn tmux_bin() -> Option<PathBuf> {
    std::env::var_os("PATH")
        .and_then(|paths| on_path(&paths, "tmux"))
        .or_else(|| {
            [
                "/opt/homebrew/bin/tmux",
                "/usr/local/bin/tmux",
                "/opt/local/bin/tmux",
            ]
            .iter()
            .map(PathBuf::from)
            .find(|path| path.is_file())
        })
}

#[cfg(not(target_os = "windows"))]
fn is_process_running() -> bool {
    command_succeeds("pgrep", &["-x", "rtorrent"])
}

/// Whether a command runs and exits zero, with its output discarded.
#[cfg(not(target_os = "windows"))]
fn command_succeeds(command: &str, args: &[&str]) -> bool {
    std::process::Command::new(command)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// The starter `.rtorrent.rc` for `transport`, rooted at `home`.
///
/// `None` when there is nothing useful to write: a remote/HTTP daemon has no
/// local SCGI endpoint for us to open.
#[cfg(not(target_os = "windows"))]
pub fn render_config(transport: &Transport, home: &Path) -> Option<String> {
    // The bundled rtorrent is pinned at 0.15.7, which spells the listen range
    // `network.port_range.set` (0.16.20 renamed it). Leave the range out
    // entirely — the app sets it over XML-RPC once connected, where it can pick
    // the name the running daemon actually knows.
    let scgi = match transport {
        Transport::UnixSocket { path } => format!("network.scgi.open_local = {path}"),
        Transport::Tcp { host, port } => format!("network.scgi.open_port = {host}:{port}"),
        Transport::Http { .. } => return None,
    };

    Some(format!(
        "# Written by rstorrent for its bundled rtorrent {version}.\n\
         # Delete this file to manage the daemon yourself.\n\
         \n\
         directory.default.set = {downloads}\n\
         session.path.set      = {session}\n\
         {scgi}\n",
        version = BUNDLED_RTORRENT_VERSION,
        downloads = home.join("Downloads").display(),
        session = home.join(".rtorrent/session").display(),
    ))
}

/// Write a minimal `~/.rtorrent.rc` when the user has never had one, so the
/// bundled daemon opens the SCGI endpoint the app connects to. An existing file
/// is left exactly as it is.
#[cfg(not(target_os = "windows"))]
fn ensure_config(transport: &Transport) -> Result<(), String> {
    let home = home_dir();
    let path = home.join(".rtorrent.rc");
    if path.exists() {
        return Ok(());
    }
    let Some(body) = render_config(transport, &home) else {
        return Ok(());
    };

    let _ = std::fs::create_dir_all(home.join(".rtorrent/session"));
    let _ = std::fs::create_dir_all(home.join("Downloads"));

    std::fs::write(&path, body)
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

/// The first `rtorrent` on `PATH`, or one of the places a package manager puts
/// it without touching the shell's environment.
///
/// # Errors
///
/// Returns a user-facing message when there is no system rtorrent either.
#[cfg(not(target_os = "windows"))]
pub fn find_rtorrent_bin() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("PATH").and_then(|paths| on_path(&paths, "rtorrent")) {
        return Ok(path);
    }
    for candidate in [
        "/opt/homebrew/bin/rtorrent",
        "/usr/local/bin/rtorrent",
        "/usr/bin/rtorrent",
        "/opt/local/bin/rtorrent",
    ] {
        let path = Path::new(candidate);
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
    }
    Err(
        "rtorrent executable not found — this build ships one on macOS, or \
         install it with 'brew install rtorrent'"
            .to_owned(),
    )
}

/// The first executable `name` on a `PATH`-style list. Split out from
/// [`find_rtorrent_bin`] so the search can be tested without a real `PATH`.
#[cfg(not(target_os = "windows"))]
pub fn on_path(paths: &std::ffi::OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(paths)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// The user's home directory, matching `src-tauri/src/settings.rs`.
#[cfg(not(target_os = "windows"))]
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rstorrent-gpui-daemon-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn unix_socket_config_opens_the_socket() {
        let transport = Transport::UnixSocket {
            path: "/Users/you/.rtorrent/rpc.socket".into(),
        };
        let body = render_config(&transport, Path::new("/Users/you")).expect("config");
        assert!(body.contains("network.scgi.open_local = /Users/you/.rtorrent/rpc.socket"));
        assert!(body.contains("session.path.set      = /Users/you/.rtorrent/session"));
        assert!(body.contains("directory.default.set = /Users/you/Downloads"));
        // The listen range is deliberately absent: the pre-0.16.20 name is only
        // correct for the bundled 0.15.7 and the renamed one for newer daemons,
        // so the app sets it over XML-RPC where it can tell which is which.
        assert!(!body.contains("port.range"));
    }

    #[test]
    fn tcp_config_opens_the_port() {
        let transport = Transport::Tcp {
            host: "127.0.0.1".into(),
            port: 5000,
        };
        let body = render_config(&transport, Path::new("/home/you")).expect("config");
        assert!(body.contains("network.scgi.open_port = 127.0.0.1:5000"));
    }

    #[test]
    fn http_transport_has_no_local_config() {
        let transport = Transport::Http {
            url: "https://box.example/RPC2".into(),
            username: "alice".into(),
        };
        assert!(render_config(&transport, Path::new("/home/you")).is_none());
    }

    #[test]
    fn the_config_names_the_bundled_version() {
        let body = render_config(
            &Transport::UnixSocket {
                path: "/tmp/rpc.socket".into(),
            },
            Path::new("/Users/you"),
        )
        .expect("config");
        assert!(body.contains(BUNDLED_RTORRENT_VERSION));
    }

    #[test]
    fn a_bundle_finds_the_runtime_beside_its_resources() {
        // Contents/MacOS/rstorrent-gpui + Contents/Resources/binaries/rtorrent/
        let app = scratch_dir("bundle");
        let macos = app.join("Contents/MacOS");
        let runtime = app.join("Contents/Resources/binaries/rtorrent");
        fs::create_dir_all(&macos).unwrap();
        fs::create_dir_all(&runtime).unwrap();
        fs::write(runtime.join("rtorrent"), b"#!/bin/sh\n").unwrap();

        assert_eq!(bundled_rtorrent_in(&macos), Some(runtime.join("rtorrent")));
        let _ = fs::remove_dir_all(&app);
    }

    #[test]
    fn a_cargo_build_finds_the_runtime_next_to_the_executable() {
        let target = scratch_dir("target");
        let runtime = target.join("binaries/rtorrent");
        fs::create_dir_all(&runtime).unwrap();
        fs::write(runtime.join("rtorrent"), b"#!/bin/sh\n").unwrap();

        assert_eq!(bundled_rtorrent_in(&target), Some(runtime.join("rtorrent")));
        let _ = fs::remove_dir_all(&target);
    }

    #[test]
    fn an_unstaged_build_has_no_bundled_runtime() {
        let dir = scratch_dir("unstaged");
        assert_eq!(bundled_rtorrent_in(&dir), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_named_like_the_binary_is_not_the_binary() {
        // The staging directory keeps a README even when the runtime is absent,
        // so the lookup must test for a file, not for the directory's presence.
        let dir = scratch_dir("readme-only");
        let runtime = dir.join("binaries/rtorrent");
        fs::create_dir_all(&runtime).unwrap();
        fs::write(runtime.join("README.md"), b"not a daemon").unwrap();

        assert_eq!(bundled_rtorrent_in(&dir), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_search_skips_entries_without_the_binary() {
        let dir = scratch_dir("path");
        let bin = dir.join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("rtorrent"), b"#!/bin/sh\n").unwrap();

        let search = std::env::join_paths([dir.join("empty"), bin.clone()]).unwrap();
        assert_eq!(on_path(&search, "rtorrent"), Some(bin.join("rtorrent")));
        // Nothing to find in a list of empty or unpopulated entries.
        let empty = std::env::join_paths([dir.join("empty")]).unwrap();
        assert_eq!(on_path(&empty, "rtorrent"), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_remote_daemon_cannot_be_started_locally() {
        // Both transports are refused before anything is resolved or spawned,
        // which is what makes this safe to call from a test.
        for transport in [
            Transport::Http {
                url: "https://seedbox.example/RPC2".into(),
                username: String::new(),
            },
            Transport::Tcp {
                host: "10.0.0.5".into(),
                port: 5000,
            },
        ] {
            let error = start(&transport).expect_err("a remote daemon is refused");
            assert!(error.contains("local"), "{error}");
        }
    }

    #[test]
    fn a_death_that_wrote_to_stderr_is_quoted_back() {
        let dir = scratch_dir("exited");
        let log = dir.join("rtorrent.err");
        fs::write(
            &log,
            "dyld: Library not loaded: @executable_path/libcurl.4.dylib\n",
        )
        .unwrap();

        let message = exited_message(&log);
        assert!(
            message.starts_with("rtorrent exited immediately — "),
            "{message}"
        );
        assert!(message.contains("libcurl.4.dylib"), "{message}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_silent_death_points_at_the_daemons_own_output() {
        // rtorrent reports a config error on *its* stdout, which the launcher
        // deliberately leaves to the tmux pane, so the message has to say where
        // to look instead of pretending to know why.
        let dir = scratch_dir("silent");
        let log = dir.join("rtorrent.err");
        for content in ["", "\n   \n"] {
            fs::write(&log, content).unwrap();
            assert!(exited_message(&log).contains("terminal"), "{content:?}");
        }
        // A log that was never written at all is the same case.
        assert!(exited_message(&dir.join("missing.err")).contains("terminal"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn shell_quoting_survives_an_apostrophe() {
        assert_eq!(
            shell_quote("/Users/you/.rtorrent/rpc.socket"),
            "'/Users/you/.rtorrent/rpc.socket'"
        );
        // The one character that could break out of the quotes.
        assert_eq!(shell_quote("/Users/o'brien"), r"'/Users/o'\''brien'");
    }

    #[test]
    fn describe_names_the_bundled_runtime() {
        let described = describe_resolution(Ok((
            PathBuf::from(
                "/Applications/rstorrent.app/Contents/Resources/binaries/rtorrent/rtorrent",
            ),
            true,
        )));
        assert_eq!(
            described,
            "rtorrent: /Applications/rstorrent.app/Contents/Resources/binaries/rtorrent/rtorrent (bundled)"
        );
    }

    #[test]
    fn describe_names_a_system_runtime() {
        assert_eq!(
            describe_resolution(Ok((PathBuf::from("/opt/homebrew/bin/rtorrent"), false))),
            "rtorrent: /opt/homebrew/bin/rtorrent (system)"
        );
    }

    #[test]
    fn describe_reports_when_there_is_no_runtime() {
        let described = describe_resolution(Err("rtorrent executable not found".to_owned()));
        assert_eq!(described, "rtorrent: none — rtorrent executable not found");
    }

    #[test]
    fn the_socket_to_wait_for_follows_the_transport() {
        let home = Path::new("/Users/you");
        assert_eq!(
            socket_path(
                &Transport::UnixSocket {
                    path: "/tmp/custom.socket".into()
                },
                home
            ),
            "/tmp/custom.socket"
        );
        // A TCP daemon still writes its socket where the default config says.
        assert_eq!(
            socket_path(
                &Transport::Tcp {
                    host: "127.0.0.1".into(),
                    port: 5000
                },
                home
            ),
            "/Users/you/.rtorrent/rpc.socket"
        );
    }
}
