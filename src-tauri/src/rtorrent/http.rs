//! XML-RPC over HTTP(S) — the remote-daemon transport (B9).
//!
//! Where SCGI needs netstring framing, this is plain XML-RPC as originally
//! specified: POST the `methodCall` document with `Content-Type: text/xml` and
//! read the `methodResponse` back. That's what an nginx `scgi_pass` front end
//! (the standard ruTorrent/seedbox setup) expects.
//!
//! Auth is HTTP Basic, which is what those front ends use. The credential is
//! only as safe as the channel, so callers should prefer `https://` — see
//! `is_insecure_credentialed` for the check the UI surfaces.

use std::time::Duration;

use reqwest::Client;

use super::xmlrpc::{self, Value};
use super::{Result, RtorrentError};

const TIMEOUT: Duration = Duration::from_secs(10);

/// Encode a method call, POST it, and parse the XML-RPC response.
pub async fn call(
    url: &str,
    username: &str,
    password: Option<&str>,
    method: &str,
    params: &[Value],
) -> Result<Value> {
    let body = xmlrpc::method_call(method, params);

    let client = Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| RtorrentError::Protocol(format!("http client: {e}")))?;

    let mut request = client
        .post(url)
        .header("Content-Type", "text/xml")
        .body(body);
    if !username.is_empty() {
        request = request.basic_auth(username, password);
    }

    let response = request.send().await.map_err(|e| {
        // A connect/DNS/TLS failure is "can't reach it"; a timeout is its own
        // case so the UI can say so rather than blaming the address.
        if e.is_timeout() {
            RtorrentError::Timeout
        } else {
            RtorrentError::Unreachable(format!("{url}: {e}"))
        }
    })?;

    let status = response.status();
    if !status.is_success() {
        // 401/403 are the overwhelmingly common misconfiguration here, so name
        // them rather than dumping a raw status line.
        return Err(match status.as_u16() {
            // Blaming the credentials is wrong when we never had a password to
            // send: the likely cause is a denied/empty Keychain entry.
            401 | 403 if !username.is_empty() && password.is_none() => {
                RtorrentError::Unreachable(format!(
                    "{url}: authentication failed ({status}) — no saved password was \
                     available for '{username}'; re-enter it in Preferences"
                ))
            }
            401 | 403 => RtorrentError::Unreachable(format!(
                "{url}: authentication failed ({status}) — check the username and password"
            )),
            404 => RtorrentError::Unreachable(format!(
                "{url}: not found (404) — is this the RPC path, e.g. /RPC2?"
            )),
            _ => RtorrentError::Protocol(format!("{url}: HTTP {status}")),
        });
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| RtorrentError::Protocol(format!("reading response: {e}")))?;
    xmlrpc::parse_response(&bytes)
}

/// Would this endpoint send credentials in the clear? True for a
/// password-bearing `http://` URL to anywhere but the local machine.
///
/// Basic auth is base64, not encryption, so plain HTTP to a remote host hands
/// the password to anything on the path. Localhost is exempt: it never leaves
/// the machine, and it's how an nginx bridge is commonly tested.
pub fn is_insecure_credentialed(url: &str, username: &str) -> bool {
    if username.is_empty() {
        return false;
    }
    let lower = url.trim().to_lowercase();
    if !lower.starts_with("http://") {
        return false;
    }
    !host_is_local(&lower)
}

/// Is this URL's host the local machine?
///
/// This gates delete-data and reveal-in-Finder, so it parses rather than
/// pattern-matches: userinfo must not be mistaken for the host
/// (`http://localhost@evil.example` is *remote*), and a bracketed IPv6 literal
/// must not be split on its own colons.
pub fn host_is_local(url: &str) -> bool {
    let after_scheme = url.trim().split("://").nth(1).unwrap_or(url.trim());
    // The authority ends at the first '/', and any userinfo within it ends at
    // the last '@' — bound the search to the authority so a '@' in the path
    // can't shift it.
    let authority_end = after_scheme.find('/').unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    let authority = match authority.rfind('@') {
        Some(at) => &authority[at + 1..],
        None => authority,
    };

    let host = if let Some(rest) = authority.strip_prefix('[') {
        // Bracketed IPv6: the host is what's inside the brackets.
        rest.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or(authority)
    };

    matches!(
        host.to_lowercase().as_str(),
        "127.0.0.1" | "::1" | "localhost" | "0.0.0.0"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_local_hosts() {
        assert!(host_is_local("http://127.0.0.1:8080/RPC2"));
        assert!(host_is_local("http://localhost/RPC2"));
        assert!(host_is_local("https://localhost:443/RPC2"));
        assert!(host_is_local("http://[::1]:8080/RPC2"));
        assert!(!host_is_local("https://seedbox.example.com/RPC2"));
        // A userinfo section must not be mistaken for the host.
        assert!(!host_is_local("http://localhost@evil.example/RPC2"));
        assert!(host_is_local("http://user:pw@127.0.0.1:8080/RPC2"));
        // A '@' in the path must not shift where the host is read from.
        assert!(!host_is_local("https://evil.example/RPC2@localhost"));
    }

    #[test]
    fn flags_credentials_over_plain_http_to_a_remote_host() {
        assert!(is_insecure_credentialed("http://box.example/RPC2", "alice"));
    }

    #[test]
    fn allows_https_and_anonymous_and_localhost() {
        // TLS protects the credential.
        assert!(!is_insecure_credentialed("https://box.example/RPC2", "alice"));
        // No credential to leak.
        assert!(!is_insecure_credentialed("http://box.example/RPC2", ""));
        // Never leaves the machine (and is how a local bridge is tested).
        assert!(!is_insecure_credentialed("http://127.0.0.1:8080/RPC2", "alice"));
    }
}
