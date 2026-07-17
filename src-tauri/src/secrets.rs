//! Keychain storage for remote-daemon credentials (B9).
//!
//! `settings.json` is plaintext on disk, so an HTTP transport's password never
//! goes in it. Instead it lives in the macOS Keychain, keyed by the endpoint it
//! belongs to, and is read only when a client is constructed.
//!
//! Every operation is best-effort: the Keychain can legitimately fail (the user
//! denies access, the item is missing, the entry is unreadable after a rebuild
//! changes the app signature). A missing password must degrade to "no
//! credentials" rather than take the app down, so these return Option/bool
//! rather than propagating errors.

use keyring::Entry;

const SERVICE: &str = "rstorrent";

/// Keychain account key. Both parts matter: the same seedbox can hold
/// credentials for more than one user, and one user can have several boxes.
fn account(url: &str, username: &str) -> String {
    format!("{username}@{url}")
}

fn entry(url: &str, username: &str) -> Option<Entry> {
    Entry::new(SERVICE, &account(url, username)).ok()
}

/// Store (or replace) the password for an endpoint. Returns whether it stuck.
pub fn set_password(url: &str, username: &str, password: &str) -> bool {
    match entry(url, username) {
        Some(e) => e.set_password(password).is_ok(),
        None => false,
    }
}

/// Read the password for an endpoint, if one was saved.
pub fn get_password(url: &str, username: &str) -> Option<String> {
    entry(url, username)?.get_password().ok()
}

/// Is a password saved for this endpoint? Lets the UI show a saved-state hint
/// without ever reading the secret back into the webview.
pub fn has_password(url: &str, username: &str) -> bool {
    get_password(url, username).is_some()
}

/// Forget an endpoint's password. Absent entries count as success.
pub fn clear_password(url: &str, username: &str) -> bool {
    match entry(url, username) {
        Some(e) => match e.delete_credential() {
            Ok(()) => true,
            Err(keyring::Error::NoEntry) => true,
            Err(_) => false,
        },
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_key_distinguishes_user_and_endpoint() {
        assert_eq!(
            account("https://box.example/RPC2", "alice"),
            "alice@https://box.example/RPC2"
        );
        // Same user, different boxes must not collide.
        assert_ne!(
            account("https://a.example/RPC2", "alice"),
            account("https://b.example/RPC2", "alice")
        );
        // Same box, different users must not collide.
        assert_ne!(
            account("https://a.example/RPC2", "alice"),
            account("https://a.example/RPC2", "bob")
        );
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Round-trip a password through the real Keychain. Ignored by default:
    /// it touches the user's login keychain and can prompt for access.
    #[test]
    #[ignore]
    fn live_keychain_roundtrip() {
        let url = "http://127.0.0.1:8099/RPC2";
        let user = "keychain-test-user";
        assert!(set_password(url, user, "s3cret"), "set failed");
        assert_eq!(get_password(url, user).as_deref(), Some("s3cret"));
        assert!(has_password(url, user));
        assert!(clear_password(url, user), "clear failed");
        assert!(!has_password(url, user), "password survived clear");
        // Clearing an absent entry is not an error.
        assert!(clear_password(url, user));
    }

    /// The app must read the credential the running instance will actually use.
    #[test]
    #[ignore]
    fn live_reads_seeded_credential() {
        let Some(url) = std::env::var("RSTORRENT_TEST_HTTP_URL").ok() else {
            eprintln!("skip: set RSTORRENT_TEST_HTTP_URL");
            return;
        };
        let user = std::env::var("RSTORRENT_TEST_HTTP_USER").unwrap_or_default();
        println!("keychain lookup {user}@{url} -> {:?}", get_password(&url, &user).map(|_| "<found>"));
        assert!(has_password(&url, &user), "no keychain entry for {user}@{url}");
    }
}
