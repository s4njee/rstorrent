//! [`ScgiClient`] — the real rtorrent backend.
//!
//! Implements [`RtorrentApi`] by translating each call into rtorrent XML-RPC
//! methods sent over SCGI ([`super::scgi`]). Batched mutations use
//! `system.multicall` to keep the number of round-trips down. Field-command
//! ordering for `d.multicall2` is defined once in [`LIST_COMMANDS`] so the
//! response columns and [`RawTorrent`] stay in sync.

use async_trait::async_trait;

use super::scgi;
use super::xmlrpc::Value;
use super::{LoadOptions, RawGlobal, RawTorrent, Result, RtorrentApi, RtorrentError};
use crate::ipc::{FileNode, PeerRow, Transport, TrackerRow};

/// The per-download commands fetched by `d.multicall2`, in column order. The
/// indices here must match the `row[i]` reads in [`row_to_raw`].
const LIST_COMMANDS: &[&str] = &[
    "d.hash=",             // 0
    "d.name=",             // 1
    "d.size_bytes=",       // 2
    "d.bytes_done=",       // 3
    "d.complete=",         // 4
    "d.is_active=",        // 5
    "d.is_open=",          // 6
    "d.hashing=",          // 7
    "d.message=",          // 8
    "d.down.rate=",        // 9
    "d.up.rate=",          // 10
    "d.ratio=",            // 11
    "d.custom1=",          // 12
    "d.directory=",        // 13
    "d.base_path=",        // 14
    "d.peers_complete=",   // 15
    "d.peers_accounted=",  // 16
    "d.peers_connected=",  // 17
    "d.priority=",         // 18
    "d.is_private=",       // 19
];

/// rtorrent client that talks to a live daemon over SCGI.
pub struct ScgiClient {
    transport: Transport,
}

impl ScgiClient {
    pub fn new(transport: Transport) -> Self {
        Self { transport }
    }

    /// Single XML-RPC call.
    async fn call(&self, method: &str, params: &[Value]) -> Result<Value> {
        scgi::call(&self.transport, method, params).await
    }

    /// `system.multicall`: run several methods in one round-trip. Returns one
    /// result (or fault) per input call, in order.
    async fn multicall(&self, calls: &[(&str, Vec<Value>)]) -> Result<Vec<Result<Value>>> {
        let arr = Value::Array(
            calls
                .iter()
                .map(|(m, p)| {
                    Value::Struct(vec![
                        ("methodName".into(), Value::Str((*m).to_string())),
                        ("params".into(), Value::Array(p.clone())),
                    ])
                })
                .collect(),
        );
        let resp = self.call("system.multicall", &[arr]).await?;
        let items = resp
            .as_array()
            .ok_or_else(|| RtorrentError::Unexpected("system.multicall did not return an array".into()))?;
        Ok(items
            .iter()
            .map(|it| {
                // Per the spec, a success is wrapped in a one-element array and a
                // failure is a {faultCode, faultString} struct.
                if let Some(code) = it.get("faultCode").and_then(Value::as_i64) {
                    let message = it
                        .get("faultString")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    Err(RtorrentError::Fault { code, message })
                } else if let Some(a) = it.as_array() {
                    Ok(a.first().cloned().unwrap_or(Value::Str(String::new())))
                } else {
                    Ok(it.clone())
                }
            })
            .collect())
    }

    /// Run the same single-argument command over a batch of hashes.
    async fn batch_hashes(&self, method: &str, hashes: &[String]) -> Result<()> {
        if hashes.is_empty() {
            return Ok(());
        }
        let calls: Vec<(&str, Vec<Value>)> = hashes
            .iter()
            .map(|h| (method, vec![Value::Str(h.clone())]))
            .collect();
        // Surface the first fault so the caller can log it.
        for r in self.multicall(&calls).await? {
            r?;
        }
        Ok(())
    }
}

/// Build the command list a load call applies to the new download.
fn load_commands(opts: &LoadOptions) -> Vec<String> {
    let mut cmds = vec![format!("d.directory.set={}", opts.directory)];
    if !opts.label.is_empty() {
        cmds.push(format!("d.custom1.set={}", opts.label));
    }
    if opts.top_of_queue {
        cmds.push("d.priority.set=3".to_string());
    }
    cmds
}

/// Extract a hostname from a tracker URL (`udp://host:port/announce` → `host`).
fn host_of(url: &str) -> String {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    let host = after_scheme
        .split(['/', ':'])
        .next()
        .unwrap_or(after_scheme);
    host.to_string()
}

/// Map one `d.multicall2` row (a positional array) into a [`RawTorrent`].
fn row_to_raw(row: &[Value]) -> RawTorrent {
    let s = |i: usize| row.get(i).and_then(Value::as_str).unwrap_or("").to_string();
    let n = |i: usize| row.get(i).and_then(Value::as_i64).unwrap_or(0);
    let b = |i: usize| row.get(i).and_then(Value::as_bool).unwrap_or(false);
    RawTorrent {
        hash: s(0).to_uppercase(),
        name: s(1),
        size_bytes: n(2),
        bytes_done: n(3),
        complete: b(4),
        is_active: b(5),
        is_open: b(6),
        hashing: n(7) > 0,
        message: s(8),
        down_rate: n(9),
        up_rate: n(10),
        ratio_permille: n(11),
        label: s(12),
        directory: s(13),
        base_path: s(14),
        peers_complete: n(15),
        peers_accounted: n(16),
        peers_connected: n(17),
        priority: n(18),
        is_private: b(19),
    }
}

#[async_trait]
impl RtorrentApi for ScgiClient {
    async fn client_version(&self) -> Result<String> {
        Ok(self
            .call("system.client_version", &[])
            .await?
            .as_str()
            .unwrap_or("")
            .to_string())
    }

    async fn list_snapshot(&self) -> Result<Vec<RawTorrent>> {
        let mut params = vec![Value::Str(String::new()), Value::Str("main".into())];
        params.extend(LIST_COMMANDS.iter().map(|c| Value::Str((*c).into())));
        let resp = self.call("d.multicall2", &params).await?;
        let rows = resp
            .as_array()
            .ok_or_else(|| RtorrentError::Unexpected("d.multicall2 did not return an array".into()))?;
        Ok(rows
            .iter()
            .filter_map(Value::as_array)
            .map(row_to_raw)
            .collect())
    }

    async fn global_stats(&self) -> Result<RawGlobal> {
        let results = self
            .multicall(&[
                ("throttle.global_down.rate", vec![]),
                ("throttle.global_down.max_rate", vec![]),
                ("throttle.global_up.rate", vec![]),
                ("throttle.global_up.max_rate", vec![]),
            ])
            .await?;
        let at = |i: usize| {
            results
                .get(i)
                .and_then(|r| r.as_ref().ok())
                .and_then(Value::as_i64)
                .unwrap_or(0)
        };
        // DHT node count is best-effort: swallow errors and unknown shapes.
        let dht_nodes = match self.call("dht.statistics", &[]).await {
            Ok(v) => v
                .get("nodes")
                .or_else(|| v.get("active"))
                .and_then(Value::as_i64)
                .unwrap_or(0),
            Err(_) => 0,
        };
        Ok(RawGlobal {
            down_rate: at(0),
            down_rate_limit: at(1),
            up_rate: at(2),
            up_rate_limit: at(3),
            dht_nodes,
        })
    }

    async fn primary_tracker(&self, hash: &str) -> Result<String> {
        let resp = self
            .call(
                "t.multicall",
                &[Value::Str(hash.into()), Value::Str(String::new()), Value::Str("t.url=".into())],
            )
            .await?;
        let url = resp
            .as_array()
            .and_then(|rows| rows.first())
            .and_then(Value::as_array)
            .and_then(|row| row.first())
            .and_then(Value::as_str)
            .unwrap_or("");
        Ok(host_of(url))
    }

    async fn trackers(&self, hash: &str) -> Result<Vec<TrackerRow>> {
        let resp = self
            .call(
                "t.multicall",
                &[
                    Value::Str(hash.into()),
                    Value::Str(String::new()),
                    Value::Str("t.url=".into()),
                    Value::Str("t.is_usable=".into()),
                    Value::Str("t.scrape_complete=".into()),
                    Value::Str("t.scrape_incomplete=".into()),
                ],
            )
            .await?;
        let rows = resp.as_array().unwrap_or(&[]);
        Ok(rows
            .iter()
            .filter_map(Value::as_array)
            .map(|r| {
                let usable = r.get(1).and_then(Value::as_bool).unwrap_or(false);
                TrackerRow {
                    url: r.first().and_then(Value::as_str).unwrap_or("").to_string(),
                    status: if usable { "working".into() } else { "error".into() },
                    seeds: r.get(2).and_then(Value::as_i64).unwrap_or(0),
                    leeches: r.get(3).and_then(Value::as_i64).unwrap_or(0),
                    last_announce: String::new(),
                }
            })
            .collect())
    }

    async fn peers(&self, hash: &str) -> Result<Vec<PeerRow>> {
        let resp = self
            .call(
                "p.multicall",
                &[
                    Value::Str(hash.into()),
                    Value::Str(String::new()),
                    Value::Str("p.address=".into()),
                    Value::Str("p.client_version=".into()),
                    Value::Str("p.completed_percent=".into()),
                    Value::Str("p.down_rate=".into()),
                    Value::Str("p.up_rate=".into()),
                    Value::Str("p.is_encrypted=".into()),
                    Value::Str("p.is_incoming=".into()),
                ],
            )
            .await?;
        let rows = resp.as_array().unwrap_or(&[]);
        Ok(rows
            .iter()
            .filter_map(Value::as_array)
            .map(|r| {
                let mut flags = String::new();
                if r.get(5).and_then(Value::as_bool).unwrap_or(false) {
                    flags.push('E');
                }
                if r.get(6).and_then(Value::as_bool).unwrap_or(false) {
                    flags.push('I');
                }
                PeerRow {
                    address: r.first().and_then(Value::as_str).unwrap_or("").to_string(),
                    client: r.get(1).and_then(Value::as_str).unwrap_or("").to_string(),
                    progress: r.get(2).and_then(Value::as_i64).unwrap_or(0) as f64,
                    down_rate: r.get(3).and_then(Value::as_i64).unwrap_or(0),
                    up_rate: r.get(4).and_then(Value::as_i64).unwrap_or(0),
                    flags,
                }
            })
            .collect())
    }

    async fn files(&self, hash: &str) -> Result<Vec<FileNode>> {
        let resp = self
            .call(
                "f.multicall",
                &[
                    Value::Str(hash.into()),
                    Value::Str(String::new()),
                    Value::Str("f.path=".into()),
                    Value::Str("f.size_bytes=".into()),
                    Value::Str("f.priority=".into()),
                    Value::Str("f.completed_chunks=".into()),
                    Value::Str("f.size_chunks=".into()),
                ],
            )
            .await?;
        let rows = resp.as_array().unwrap_or(&[]);
        Ok(rows
            .iter()
            .filter_map(Value::as_array)
            .map(|r| {
                let done = r.get(3).and_then(Value::as_i64).unwrap_or(0);
                let total = r.get(4).and_then(Value::as_i64).unwrap_or(0).max(1);
                FileNode {
                    path: r.first().and_then(Value::as_str).unwrap_or("").to_string(),
                    size: r.get(1).and_then(Value::as_i64).unwrap_or(0),
                    priority: r.get(2).and_then(Value::as_i64).unwrap_or(1),
                    progress: done as f64 / total as f64 * 100.0,
                    is_dir: false,
                }
            })
            .collect())
    }

    async fn start(&self, hashes: &[String]) -> Result<()> {
        self.batch_hashes("d.start", hashes).await
    }

    async fn stop(&self, hashes: &[String]) -> Result<()> {
        self.batch_hashes("d.stop", hashes).await
    }

    async fn recheck(&self, hashes: &[String]) -> Result<()> {
        self.batch_hashes("d.check_hash", hashes).await
    }

    async fn erase(&self, hashes: &[String]) -> Result<()> {
        self.batch_hashes("d.erase", hashes).await
    }

    async fn load_raw(&self, bytes: Vec<u8>, opts: LoadOptions) -> Result<()> {
        let method = if opts.start { "load.raw_start" } else { "load.raw" };
        let mut params = vec![Value::Str(String::new()), Value::Bytes(bytes)];
        params.extend(load_commands(&opts).into_iter().map(Value::Str));
        self.call(method, &params).await.map(|_| ())
    }

    async fn load_magnet(&self, uri: &str, opts: LoadOptions) -> Result<()> {
        let method = if opts.start { "load.start" } else { "load.normal" };
        let mut params = vec![Value::Str(String::new()), Value::Str(uri.into())];
        params.extend(load_commands(&opts).into_iter().map(Value::Str));
        self.call(method, &params).await.map(|_| ())
    }

    async fn set_label(&self, hashes: &[String], label: &str) -> Result<()> {
        let calls: Vec<(&str, Vec<Value>)> = hashes
            .iter()
            .map(|h| {
                (
                    "d.custom1.set",
                    vec![Value::Str(h.clone()), Value::Str(label.into())],
                )
            })
            .collect();
        for r in self.multicall(&calls).await? {
            r?;
        }
        Ok(())
    }

    async fn set_directory(&self, hash: &str, path: &str) -> Result<()> {
        self.call(
            "d.directory.set",
            &[Value::Str(hash.into()), Value::Str(path.into())],
        )
        .await
        .map(|_| ())
    }

    async fn set_priority(&self, hash: &str, priority: i64) -> Result<()> {
        self.call(
            "d.priority.set",
            &[Value::Str(hash.into()), Value::Int(priority)],
        )
        .await
        .map(|_| ())
    }

    async fn set_file_priority(&self, hash: &str, index: usize, priority: i64) -> Result<()> {
        // f.* commands target `HASH:fINDEX`.
        let target = format!("{hash}:f{index}");
        self.call("f.priority.set", &[Value::Str(target), Value::Int(priority)])
            .await
            .map(|_| ())
    }

    async fn base_path(&self, hash: &str) -> Result<String> {
        Ok(self
            .call("d.base_path", &[Value::Str(hash.into())])
            .await?
            .as_str()
            .unwrap_or("")
            .to_string())
    }

    async fn set_throttles(&self, down_kb: i64, up_kb: i64) -> Result<()> {
        for r in self
            .multicall(&[
                (
                    "throttle.global_down.max_rate.set_kb",
                    vec![Value::Str(String::new()), Value::Int(down_kb)],
                ),
                (
                    "throttle.global_up.max_rate.set_kb",
                    vec![Value::Str(String::new()), Value::Int(up_kb)],
                ),
            ])
            .await?
        {
            r?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_of_strips_scheme_port_and_path() {
        assert_eq!(host_of("udp://tracker.example.org:6969/announce"), "tracker.example.org");
        assert_eq!(host_of("https://bttracker.debian.org/announce"), "bttracker.debian.org");
        assert_eq!(host_of("torrent.ubuntu.com"), "torrent.ubuntu.com");
    }

    #[test]
    fn row_to_raw_maps_columns() {
        let row = vec![
            Value::Str("abcd".into()), // hash → uppercased
            Value::Str("ubuntu.iso".into()),
            Value::Int(1000),
            Value::Int(500),
            Value::Int(0), // complete
            Value::Int(1), // active
            Value::Int(1), // open
            Value::Int(0), // hashing
            Value::Str(String::new()),
            Value::Int(42),  // down rate
            Value::Int(7),   // up rate
            Value::Int(1900),
            Value::Str("linux-iso".into()),
            Value::Str("/srv".into()),
            Value::Str("/srv/ubuntu.iso".into()),
            Value::Int(3),
            Value::Int(9),
            Value::Int(12),
            Value::Int(2),
            Value::Int(0),
        ];
        let t = row_to_raw(&row);
        assert_eq!(t.hash, "ABCD");
        assert_eq!(t.name, "ubuntu.iso");
        assert_eq!(t.down_rate, 42);
        assert_eq!(t.label, "linux-iso");
        assert!(t.is_active && t.is_open && !t.complete);
    }

    #[test]
    fn load_commands_include_directory_and_label() {
        let opts = LoadOptions {
            directory: "/srv/dl".into(),
            label: "iso".into(),
            start: true,
            top_of_queue: true,
            unselected_indexes: vec![],
        };
        let cmds = load_commands(&opts);
        assert_eq!(cmds[0], "d.directory.set=/srv/dl");
        assert!(cmds.contains(&"d.custom1.set=iso".to_string()));
        assert!(cmds.contains(&"d.priority.set=3".to_string()));
    }
}

/// Live integration tests against a real rtorrent daemon.
///
/// These are `#[ignore]`d so the normal `cargo test` run stays hermetic. To run
/// them, start rtorrent with an SCGI socket and point the env var at it:
///
/// ```sh
/// RSTORRENT_TEST_SOCKET=~/.rtorrent/rpc.socket \
///   cargo test --lib live -- --ignored --nocapture
/// ```
#[cfg(test)]
mod live {
    use super::*;
    use crate::ipc::Transport;
    use crate::rtorrent::derive;

    fn client() -> Option<ScgiClient> {
        std::env::var("RSTORRENT_TEST_SOCKET")
            .ok()
            .map(|path| ScgiClient::new(Transport::UnixSocket { path }))
    }

    #[tokio::test]
    #[ignore]
    async fn live_read_paths() {
        let Some(c) = client() else {
            eprintln!("skip: set RSTORRENT_TEST_SOCKET");
            return;
        };
        let version = c.client_version().await.expect("client_version");
        println!("rtorrent version = {version}");
        assert!(!version.is_empty());

        let globals = c.global_stats().await.expect("global_stats");
        println!("globals = {globals:?}");

        let list = c.list_snapshot().await.expect("list_snapshot");
        println!("torrents = {}", list.len());
        for t in &list {
            println!(
                "  {} {:>6.1}% {:?} {}",
                t.hash,
                derive::percent(t),
                derive::status(t),
                t.name
            );
        }
    }

    /// Load a real `.torrent` (the E8 add-file path), verify it appears with the
    /// info-hash our parser computes, exercise stop/start, then erase it. Set
    /// `RSTORRENT_TEST_TORRENT` to a `.torrent` file whose data is already on disk
    /// (so no network is needed — it will hash-check and idle).
    #[tokio::test]
    #[ignore]
    async fn live_add_actions_erase() {
        let Some(c) = client() else {
            eprintln!("skip: set RSTORRENT_TEST_SOCKET");
            return;
        };
        let Some(path) = std::env::var("RSTORRENT_TEST_TORRENT").ok() else {
            eprintln!("skip: set RSTORRENT_TEST_TORRENT");
            return;
        };

        let bytes = std::fs::read(&path).expect("read .torrent");
        // Validate our metadata parser against the same file.
        let meta = crate::torrent_file::read_metadata(&path).expect("parse metadata");
        let hash = meta.info_hash.clone();
        println!("parsed: {} ({} bytes, hash {})", meta.name, meta.size, hash);

        let opts = LoadOptions {
            directory: dirs_download(),
            label: "rstorrent-test".into(),
            start: false,
            top_of_queue: false,
            unselected_indexes: vec![],
        };
        c.load_raw(bytes, opts).await.expect("load_raw");

        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        let list = c.list_snapshot().await.expect("list after add");
        let row = list.iter().find(|t| t.hash.eq_ignore_ascii_case(&hash));
        println!("added present = {} (list size {})", row.is_some(), list.len());
        assert!(row.is_some(), "loaded torrent should appear");
        // Our parser's hash must match what rtorrent reports.
        assert_eq!(row.unwrap().hash, hash);
        assert_eq!(row.unwrap().label, "rstorrent-test");

        // Exercise start then stop.
        c.start(std::slice::from_ref(&hash)).await.expect("start");
        c.stop(std::slice::from_ref(&hash)).await.expect("stop");

        // Clean up.
        c.erase(std::slice::from_ref(&hash)).await.expect("erase");
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        let list = c.list_snapshot().await.expect("list after erase");
        assert!(
            !list.iter().any(|t| t.hash.eq_ignore_ascii_case(&hash)),
            "erased torrent should be gone"
        );
        println!("erase confirmed, list size {}", list.len());
    }

    fn dirs_download() -> String {
        std::env::var("HOME")
            .map(|h| format!("{h}/Downloads/rstorrent-test"))
            .unwrap_or_else(|_| "/tmp".into())
    }
}
