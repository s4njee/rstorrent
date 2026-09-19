//! Password hashing and the persistent session store.
//!
//! argon2id with per-hash random salts; the PHC string (which embeds the salt
//! and parameters) is what goes in the config.
//!
//! Sessions (WEB-05) survive a server restart: tokens are stored **hashed**
//! (SHA-256) with their expiry in a small JSON file beside the config, so a leak
//! of the file is not a set of live logins. The file is the rotatable secret
//! material — [`Sessions::revoke_all`] clears it, which is the "sign out
//! everywhere" control.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Session lifetime, refreshed on each validated request (sliding expiry).
const SESSION_TTL: Duration = Duration::from_secs(30 * 24 * 3600);
/// Persist sliding-expiry refreshes at most this often, so a 1 s poll does not
/// write the store every second.
const PERSIST_EVERY: Duration = Duration::from_secs(300);
/// The on-disk store format version.
const STORE_VERSION: u32 = 1;
/// Login rate-limit window and cap, per client IP.
const RATE_WINDOW: Duration = Duration::from_secs(60);
const RATE_MAX: u32 = 5;
/// The session cookie name.
pub const COOKIE: &str = "rstorrent_session";

/// Hash a plaintext password into an argon2id PHC string for `[auth].password_hash`.
pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow!("hashing failed: {e}"))
}

/// Verify a plaintext password against a stored PHC hash. A malformed stored
/// hash, or a mismatch, is `false` — never an error the caller must branch on.
pub fn verify_password(hash: &str, password: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// The persisted store shape: hashed token → expiry (Unix milliseconds).
#[derive(Serialize, Deserialize)]
struct StoredSessions {
    version: u32,
    #[serde(default)]
    tokens: HashMap<String, i64>,
}

#[derive(Default)]
struct Inner {
    tokens: HashMap<String, i64>,
    last_persist: Option<Instant>,
}

/// Session store: raw token minted to the browser, its SHA-256 kept here with an
/// expiry. `path` is `None` for an in-memory store (tests, the desktop host).
#[derive(Default)]
pub struct Sessions {
    inner: Mutex<Inner>,
    path: Option<PathBuf>,
}

impl Sessions {
    /// A store persisted to `path`, loading any live sessions from a previous run.
    #[must_use]
    pub fn with_store(path: PathBuf) -> Self {
        let stored: StoredSessions = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(StoredSessions {
                version: STORE_VERSION,
                tokens: HashMap::new(),
            });
        let now = now_ms();
        let tokens = stored
            .tokens
            .into_iter()
            .filter(|(_, expiry)| *expiry > now)
            .collect();
        Self {
            inner: Mutex::new(Inner {
                tokens,
                last_persist: None,
            }),
            path: Some(path),
        }
    }

    /// Mint a fresh 256-bit session token and persist it immediately.
    pub fn create(&self) -> String {
        let mut buf = [0u8; 32];
        OsRng.fill_bytes(&mut buf);
        let token = hex(&buf);
        let mut inner = self.inner.lock().unwrap();
        inner.tokens.insert(hash_token(&token), now_ms() + ttl_ms());
        self.persist(&mut inner);
        token
    }

    /// True if the token is live; refreshes its expiry (sliding window).
    pub fn validate(&self, token: &str) -> bool {
        let key = hash_token(token);
        let now = now_ms();
        let mut inner = self.inner.lock().unwrap();
        match inner.tokens.get(&key).copied() {
            Some(expiry) if expiry > now => {
                inner.tokens.insert(key, now + ttl_ms());
                let due = inner
                    .last_persist
                    .is_none_or(|at| at.elapsed() >= PERSIST_EVERY);
                if due {
                    self.persist(&mut inner);
                }
                true
            }
            Some(_) => {
                inner.tokens.remove(&key);
                self.persist(&mut inner);
                false
            }
            None => false,
        }
    }

    /// Revoke one session (sign out this browser).
    pub fn revoke(&self, token: &str) {
        let mut inner = self.inner.lock().unwrap();
        if inner.tokens.remove(&hash_token(token)).is_some() {
            self.persist(&mut inner);
        }
    }

    /// Revoke every session — the "sign out everywhere" control.
    pub fn revoke_all(&self) {
        let mut inner = self.inner.lock().unwrap();
        if !inner.tokens.is_empty() {
            inner.tokens.clear();
            self.persist(&mut inner);
        }
    }

    /// How many sessions are live (diagnostics/tests).
    #[must_use]
    pub fn active(&self) -> usize {
        self.inner.lock().unwrap().tokens.len()
    }

    /// Write the store, if it has a path. Called with the lock held; the file is
    /// small and writes are throttled, so the brief block is acceptable.
    fn persist(&self, inner: &mut Inner) {
        inner.last_persist = Some(Instant::now());
        let Some(path) = &self.path else {
            return;
        };
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let stored = StoredSessions {
            version: STORE_VERSION,
            tokens: inner.tokens.clone(),
        };
        if let Ok(text) = serde_json::to_string_pretty(&stored) {
            let temporary = path.with_extension("json.tmp");
            if std::fs::write(&temporary, text).is_ok() {
                let _ = std::fs::rename(&temporary, path);
            }
        }
    }
}

/// Per-IP fixed-window rate limiter for the login endpoint.
#[derive(Default)]
pub struct RateLimiter {
    inner: Mutex<HashMap<IpAddr, (u32, Instant)>>,
}

impl RateLimiter {
    /// Record an attempt; `false` once the window cap is exceeded.
    pub fn allow(&self, ip: IpAddr) -> bool {
        let mut map = self.inner.lock().unwrap();
        let now = Instant::now();
        let entry = map.entry(ip).or_insert((0, now));
        if now.duration_since(entry.1) > RATE_WINDOW {
            *entry = (0, now);
        }
        entry.0 += 1;
        entry.0 <= RATE_MAX
    }
}

/// SHA-256 of a token, hex — what the store keeps.
fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex(&hasher.finalize())
}

/// Milliseconds since the Unix epoch.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

fn ttl_ms() -> i64 {
    i64::try_from(SESSION_TTL.as_millis()).unwrap_or(i64::MAX)
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        std::env::temp_dir()
            .join(format!("rstorrent-web-auth-{}-{stamp}", std::process::id()))
            .join(name)
    }

    #[test]
    fn sessions_create_validate_revoke() {
        let s = Sessions::default();
        let token = s.create();
        assert!(s.validate(&token));
        assert!(!s.validate("not-a-token"));
        s.revoke(&token);
        assert!(!s.validate(&token));
    }

    #[test]
    fn sessions_survive_a_restart_through_the_store() {
        let path = scratch("sessions.json");
        let token = {
            let first = Sessions::with_store(path.clone());
            first.create()
        };
        // A fresh process loading the same file still accepts the token.
        let second = Sessions::with_store(path.clone());
        assert!(second.validate(&token));
        assert_eq!(second.active(), 1);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn revoke_all_clears_every_session_and_persists_it() {
        let path = scratch("sessions.json");
        let first = Sessions::with_store(path.clone());
        let a = first.create();
        let b = first.create();
        assert_eq!(first.active(), 2);
        first.revoke_all();
        assert_eq!(first.active(), 0);
        assert!(!first.validate(&a) && !first.validate(&b));

        // The clear is durable: a restart does not revive them.
        let second = Sessions::with_store(path.clone());
        assert_eq!(second.active(), 0);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn an_expired_stored_token_is_dropped_on_load() {
        let path = scratch("sessions.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let expired = format!(
            r#"{{"version":1,"tokens":{{"deadbeef":{}}}}}"#,
            now_ms() - 1000
        );
        std::fs::write(&path, expired).unwrap();
        let sessions = Sessions::with_store(path.clone());
        assert_eq!(sessions.active(), 0, "an expired token must not load");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_store_holds_hashes_not_tokens() {
        let path = scratch("sessions.json");
        let sessions = Sessions::with_store(path.clone());
        let token = sessions.create();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains(&token), "the raw token must not be on disk");
        assert!(text.contains(&hash_token(&token)));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn rate_limiter_caps_attempts_per_ip() {
        let r = RateLimiter::default();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        for _ in 0..RATE_MAX {
            assert!(r.allow(ip));
        }
        assert!(!r.allow(ip));
        assert!(r.allow("10.0.0.2".parse().unwrap()));
    }

    #[test]
    fn hash_then_verify_round_trips() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(verify_password(&hash, "correct horse battery staple"));
        assert!(!verify_password(&hash, "Tr0ub4dor&3"));
    }

    #[test]
    fn each_hash_uses_a_fresh_salt() {
        let a = hash_password("same").unwrap();
        let b = hash_password("same").unwrap();
        assert_ne!(
            a, b,
            "salts must differ so equal passwords hash differently"
        );
        assert!(verify_password(&a, "same") && verify_password(&b, "same"));
    }

    #[test]
    fn a_garbage_hash_is_a_clean_false() {
        assert!(!verify_password("not-a-phc-string", "whatever"));
    }
}
