//! Password hashing for the single web-login credential.
//!
//! WE1 uses this only for the `hash-password` subcommand; the login flow and
//! session store land in WE5. argon2id with per-hash random salts; the PHC
//! string (which embeds the salt and parameters) is what goes in the config.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

/// Session lifetime, refreshed on each validated request (sliding expiry).
const SESSION_TTL: Duration = Duration::from_secs(30 * 24 * 3600);
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
///
/// Exercised by tests now; the login flow that calls it in anger lands in WE5.
#[allow(dead_code)]
pub fn verify_password(hash: &str, password: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// In-memory session store: opaque token → sliding expiry. (Sessions live for
/// the server process; a restart asks users to log in again.)
#[derive(Default)]
pub struct Sessions {
    inner: Mutex<HashMap<String, Instant>>,
}

impl Sessions {
    /// Mint a fresh 128-bit session token.
    pub fn create(&self) -> String {
        let mut buf = [0u8; 16];
        OsRng.fill_bytes(&mut buf);
        let token = hex(&buf);
        self.inner
            .lock()
            .unwrap()
            .insert(token.clone(), Instant::now() + SESSION_TTL);
        token
    }

    /// True if the token is live; refreshes its expiry (sliding window).
    pub fn validate(&self, token: &str) -> bool {
        let mut map = self.inner.lock().unwrap();
        match map.get(token) {
            Some(&exp) if exp > Instant::now() => {
                map.insert(token.to_string(), Instant::now() + SESSION_TTL);
                true
            }
            Some(_) => {
                map.remove(token);
                false
            }
            None => false,
        }
    }

    pub fn revoke(&self, token: &str) {
        self.inner.lock().unwrap().remove(token);
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
    fn rate_limiter_caps_attempts_per_ip() {
        let r = RateLimiter::default();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        // RATE_MAX allowed, then blocked.
        for _ in 0..RATE_MAX {
            assert!(r.allow(ip));
        }
        assert!(!r.allow(ip));
        // A different IP is unaffected.
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
