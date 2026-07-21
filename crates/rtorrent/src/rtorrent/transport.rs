//! Transport dispatch: one `call` for every way of reaching a daemon.
//!
//! rtorrent speaks the same XML-RPC either way; only the envelope differs —
//! SCGI netstrings over a socket ([`super::scgi`]) for a local daemon, or an
//! HTTP POST ([`super::http`]) for a remote one behind nginx. Keeping the choice
//! here means [`super::client::RpcClient`] never branches on it.

use super::xmlrpc::Value;
use super::{http, scgi, Result};
use crate::types::Transport;

/// Perform one XML-RPC call over whichever transport is configured.
///
/// `password` is only consulted for HTTP; it comes from the Keychain rather
/// than the settings file (see `crate::secrets`).
pub async fn call(
    transport: &Transport,
    password: Option<&str>,
    method: &str,
    params: &[Value],
) -> Result<Value> {
    match transport {
        Transport::UnixSocket { .. } | Transport::Tcp { .. } => {
            scgi::call(transport, method, params).await
        }
        Transport::Http { url, username } => {
            http::call(url, username, password, method, params).await
        }
    }
}
