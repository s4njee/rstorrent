//! Push the Network-pane preferences (v1.6: D7–D9, D11) to the daemon.
//!
//! These are rtorrent global knobs the app owns: protocol encryption/PEX, an
//! HTTP proxy for tracker announces, listen/bind addresses, and global peer /
//! slot caps. They're applied both when Preferences is saved and whenever the
//! poller (re)connects, so they survive a daemon restart — rtorrent forgets
//! runtime-set config, and several of these have no getter to read back.
//!
//! Everything is best-effort: an older build that rejects one directive must not
//! stop the rest, and a daemon that's momentarily unreachable just gets them on
//! the next connect.

use crate::ipc::Settings;
use crate::rtorrent::RtorrentApi;

/// The string-valued directives for these settings (encryption, proxy, bind).
fn string_directives(s: &Settings) -> Vec<(&'static str, &str)> {
    // Unchecked (or empty) clears any proxy previously set.
    let proxy = if s.proxy_tracker_http {
        s.proxy_address.trim()
    } else {
        ""
    };
    let mut out: Vec<(&'static str, &str)> = vec![
        ("protocol.encryption.set", s.encryption.flags()),
        ("network.http.proxy_address.set", proxy),
    ];
    // Bind/local addresses rebind the listen socket, so only push them when set
    // — clobbering the default on every save would needlessly drop connections.
    let bind = s.bind_address.trim();
    if !bind.is_empty() {
        out.push(("network.bind_address.set", bind));
    }
    let local = s.local_address.trim();
    if !local.is_empty() {
        out.push(("network.local_address.set", local));
    }
    out
}

/// The integer-valued directives for these settings (PEX, global caps).
fn int_directives(s: &Settings) -> Vec<(&'static str, i64)> {
    let mut out: Vec<(&'static str, i64)> = vec![("protocol.pex.set", i64::from(s.pex_enabled))];
    // 0 means "leave the daemon default" for the peer cap (0 peers would be
    // nonsensical); the global slot caps take 0 as unlimited, which is fine.
    if s.max_peers > 0 {
        out.push(("throttle.max_peers.normal.set", s.max_peers));
        out.push(("throttle.max_peers.seed.set", s.max_peers));
    }
    if s.max_uploads_global > 0 {
        out.push(("throttle.max_uploads.global.set", s.max_uploads_global));
    }
    if s.max_downloads_global > 0 {
        out.push(("throttle.max_downloads.global.set", s.max_downloads_global));
    }
    out
}

/// Apply the network-preference directives to `backend`, best-effort.
pub async fn apply(backend: &dyn RtorrentApi, s: &Settings) {
    let _ = backend.apply_config_str(&string_directives(s)).await;
    let _ = backend.apply_config(&int_directives(s)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::EncryptionMode;

    #[test]
    fn encryption_presets_map_to_flag_lists() {
        assert_eq!(EncryptionMode::Disabled.flags(), "none");
        assert!(EncryptionMode::Require.flags().contains("require"));
        assert!(EncryptionMode::Prefer.flags().contains("enable_retry"));
    }

    fn proxy_value(s: &Settings) -> Option<String> {
        string_directives(s)
            .iter()
            .find(|(k, _)| *k == "network.http.proxy_address.set")
            .map(|(_, v)| v.to_string())
    }

    #[test]
    fn proxy_only_pushed_when_enabled() {
        // Checkbox off → proxy cleared even though an address is present.
        let mut s = Settings {
            proxy_address: "127.0.0.1:8080".into(),
            proxy_tracker_http: false,
            ..Default::default()
        };
        assert_eq!(proxy_value(&s).as_deref(), Some(""));
        s.proxy_tracker_http = true;
        assert_eq!(proxy_value(&s).as_deref(), Some("127.0.0.1:8080"));
    }

    #[test]
    fn bind_and_local_pushed_only_when_set() {
        let unset = Settings::default();
        assert!(string_directives(&unset)
            .iter()
            .all(|(k, _)| *k != "network.bind_address.set"));
        let bound = Settings {
            bind_address: "10.0.0.2".into(),
            ..Default::default()
        };
        assert!(string_directives(&bound)
            .iter()
            .any(|(k, v)| *k == "network.bind_address.set" && *v == "10.0.0.2"));
    }

    #[test]
    fn zero_caps_are_left_at_default() {
        // Only PEX is always present; no throttle caps when all are zero.
        let zero = Settings {
            max_peers: 0,
            max_uploads_global: 0,
            max_downloads_global: 0,
            ..Default::default()
        };
        let ints = int_directives(&zero);
        assert!(ints.iter().all(|(k, _)| !k.starts_with("throttle.")));
        assert!(ints.iter().any(|(k, _)| *k == "protocol.pex.set"));

        let capped = Settings {
            max_peers: 300,
            max_uploads_global: 50,
            ..Default::default()
        };
        let ints = int_directives(&capped);
        assert!(ints
            .iter()
            .any(|(k, v)| *k == "throttle.max_peers.normal.set" && *v == 300));
        assert!(ints
            .iter()
            .any(|(k, v)| *k == "throttle.max_uploads.global.set" && *v == 50));
    }
}
