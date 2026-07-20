//! `.rtorrent.rc` network tuning for a 1 Gbps link.
//!
//! rtorrent reads `.rtorrent.rc` only at startup, so the tuner does two things:
//! it writes a managed, delimited block into the file (so the settings survive a
//! daemon restart), and — separately, in the command layer — pushes the same
//! values over XML-RPC so most of them take effect on the running daemon without
//! a restart (see [`PROFILE`] and `commands::apply_tuning`).
//!
//! The file write only makes sense for a *local* daemon (macOS/Linux directly,
//! or the WSL VM on Windows). A remote daemon's config file isn't ours to reach,
//! so the caller gates the write on [`crate::settings::is_localhost`] and applies
//! the profile live-only in that case.
//!
//! Values here are the "aggressive" profile: sized for a machine (or WSL VM)
//! with 8 GB+ of RAM pushing sustained gigabit. The one value that can bite is
//! `pieces.memory.max` (rtorrent's disk cache) — 4 GiB assumes the headroom is
//! there.

/// One tuning directive.
///
/// `file_value` is the human-readable form written into `.rtorrent.rc` (e.g.
/// `4G`); `live_value` is the same quantity in bytes/count, for the XML-RPC
/// setter, which wants an integer rather than a `4G`-style suffix string.
pub struct Tune {
    pub method: &'static str,
    pub file_value: &'static str,
    pub live_value: i64,
}

/// Markers delimiting the block the tuner owns. Re-running rewrites everything
/// between them; anything outside is left untouched.
pub const BLOCK_START: &str = "# >>> rstorrent: 1 Gbps tuning >>>";
pub const BLOCK_END: &str = "# <<< rstorrent: 1 Gbps tuning <<<";

const MIB: i64 = 1024 * 1024;
const GIB: i64 = 1024 * MIB;

/// The aggressive 1 Gbps profile.
pub const PROFILE: &[Tune] = &[
    // Upload/download slots: plenty of concurrent transfers to saturate the pipe.
    Tune {
        method: "throttle.max_uploads.set",
        file_value: "50",
        live_value: 50,
    },
    Tune {
        method: "throttle.max_uploads.global.set",
        file_value: "500",
        live_value: 500,
    },
    Tune {
        method: "throttle.max_downloads.set",
        file_value: "50",
        live_value: 50,
    },
    Tune {
        method: "throttle.max_downloads.global.set",
        file_value: "500",
        live_value: 500,
    },
    // Peer counts: more peers per torrent means more parallel throughput.
    Tune {
        method: "throttle.min_peers.normal.set",
        file_value: "50",
        live_value: 50,
    },
    Tune {
        method: "throttle.max_peers.normal.set",
        file_value: "500",
        live_value: 500,
    },
    Tune {
        method: "throttle.min_peers.seed.set",
        file_value: "30",
        live_value: 30,
    },
    Tune {
        method: "throttle.max_peers.seed.set",
        file_value: "500",
        live_value: 500,
    },
    // File and socket ceilings (keep within the daemon's ulimit -n).
    Tune {
        method: "network.http.max_open.set",
        file_value: "128",
        live_value: 128,
    },
    Tune {
        method: "network.max_open_files.set",
        file_value: "1024",
        live_value: 1024,
    },
    Tune {
        method: "network.max_open_sockets.set",
        file_value: "3000",
        live_value: 3000,
    },
    // Big socket buffers for a high bandwidth-delay product.
    Tune {
        method: "network.receive_buffer.size.set",
        file_value: "8M",
        live_value: 8 * MIB,
    },
    Tune {
        method: "network.send_buffer.size.set",
        file_value: "16M",
        live_value: 16 * MIB,
    },
    Tune {
        method: "network.xmlrpc.size_limit.set",
        file_value: "8M",
        live_value: 8 * MIB,
    },
    // Disk cache. The one to watch on a low-RAM box.
    Tune {
        method: "pieces.memory.max.set",
        file_value: "4G",
        live_value: 4 * GIB,
    },
];

/// The `(method, live_value)` pairs the command layer applies over XML-RPC.
pub fn live_calls() -> Vec<(&'static str, i64)> {
    PROFILE.iter().map(|t| (t.method, t.live_value)).collect()
}

/// Render the managed block (markers included, no trailing newline).
pub fn render_block() -> String {
    let mut s = String::new();
    s.push_str(BLOCK_START);
    s.push('\n');
    s.push_str("# Managed by rstorrent (Tune for 1 Gbps). Re-running the tuner rewrites\n");
    s.push_str("# everything between these markers, so edits inside will be lost.\n");
    for t in PROFILE {
        s.push_str(t.method);
        s.push_str(" = ");
        s.push_str(t.file_value);
        s.push('\n');
    }
    s.push_str(BLOCK_END);
    s
}

/// Splice `block` into `existing` `.rtorrent.rc` contents: replace a prior
/// managed block in place, or append one (separated by a blank line) if none is
/// present. Idempotent — running it twice yields the same file.
pub fn splice_block(existing: &str, block: &str) -> String {
    if let (Some(start), Some(end_marker)) = (existing.find(BLOCK_START), existing.find(BLOCK_END))
    {
        if start < end_marker {
            // Extend the replaced span to the end of the end-marker's line.
            let after = end_marker + BLOCK_END.len();
            let line_end = existing[after..]
                .find('\n')
                .map(|n| after + n + 1)
                .unwrap_or(existing.len());
            let mut out = String::with_capacity(existing.len());
            out.push_str(&existing[..start]);
            out.push_str(block);
            out.push('\n');
            out.push_str(&existing[line_end..]);
            return out;
        }
    }
    let mut out = existing.to_string();
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(block);
    out.push('\n');
    out
}

/// A short, non-blocking description of where the block will be written, for the
/// preview dialog. Deliberately avoids probing WSL (which can spawn a process).
pub fn display_path() -> String {
    #[cfg(not(target_os = "windows"))]
    {
        crate::settings::home_dir()
            .join(".rtorrent.rc")
            .display()
            .to_string()
    }
    #[cfg(target_os = "windows")]
    {
        "~/.rtorrent.rc  (inside WSL)".to_string()
    }
}

/// Write the managed block into the local daemon's `.rtorrent.rc`, returning the
/// path written. Callers must only invoke this for a local daemon.
///
/// May shell out to WSL on Windows, so run it on the blocking pool.
pub fn write_block() -> Result<String, String> {
    let block = render_block();
    #[cfg(not(target_os = "windows"))]
    {
        let path = crate::settings::home_dir().join(".rtorrent.rc");
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let next = splice_block(&existing, &block);
        std::fs::write(&path, next)
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        Ok(path.display().to_string())
    }
    #[cfg(target_os = "windows")]
    {
        // The daemon lives in the WSL VM; its rc file is `$HOME/.rtorrent.rc`
        // there, read and written through wsl.exe.
        let existing = crate::wsl::read_home_file(".rtorrent.rc")
            .ok_or("could not reach WSL to read ~/.rtorrent.rc")?;
        let next = splice_block(&existing, &block);
        crate::wsl::write_home_file(".rtorrent.rc", &next)?;
        let shown = crate::wsl::distro()
            .map(|d| format!("{}/.rtorrent.rc", d.home.trim_end_matches('/')))
            .unwrap_or_else(|| "~/.rtorrent.rc (WSL)".to_string());
        Ok(shown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_is_delimited_and_covers_the_profile() {
        let block = render_block();
        assert!(block.starts_with(BLOCK_START));
        assert!(block.ends_with(BLOCK_END));
        assert!(block.contains("pieces.memory.max.set = 4G"));
        assert!(block.contains("network.send_buffer.size.set = 16M"));
    }

    #[test]
    fn append_when_no_block_present() {
        let existing = "session.path.set = /home/you/.session\n";
        let out = splice_block(existing, &render_block());
        assert!(out.starts_with(existing));
        assert!(out.contains(BLOCK_START));
        assert!(out.contains(BLOCK_END));
    }

    #[test]
    fn splice_is_idempotent_and_replaces_in_place() {
        let base = "network.port_range.set = 6881-6899\n";
        let once = splice_block(base, &render_block());
        let twice = splice_block(&once, &render_block());
        assert_eq!(once, twice, "re-running must not stack blocks");
        // The pre-existing directive outside the block is preserved.
        assert!(twice.contains("network.port_range.set = 6881-6899"));
        assert_eq!(twice.matches(BLOCK_START).count(), 1);
    }

    #[test]
    fn content_after_the_block_survives_a_rewrite() {
        let base = format!("{}\nold = 1\n{}\nafter = 2\n", BLOCK_START, BLOCK_END);
        let out = splice_block(&base, &render_block());
        assert!(out.contains("after = 2"));
        assert!(!out.contains("old = 1"));
    }

    #[test]
    fn live_values_are_positive() {
        assert!(live_calls().iter().all(|(_, v)| *v > 0));
    }
}
