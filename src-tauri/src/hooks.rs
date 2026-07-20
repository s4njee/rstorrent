//! Client-side completion hook (C13).
//!
//! When configured, a user-supplied command is run on *this* machine each time a
//! torrent completes. It is run directly — not through a shell — so there is no
//! command injection to worry about: the template is split on whitespace into a
//! program and arguments, and the `%N`/`%F`/`%H` tokens are substituted per
//! token, so a value containing spaces stays a single argument. (A user who
//! needs pipes or redirects points the hook at a script.)
//!
//! This is deliberately *not* rtorrent's `execute.*` / `method.set_key` — those
//! run arbitrary commands on the daemon and are a stated non-goal. This runs
//! only what the local user has typed into Preferences.

use std::process::Command;

/// Substitute the hook tokens in one template token.
fn substitute(token: &str, name: &str, path: &str, hash: &str) -> String {
    token
        .replace("%N", name)
        .replace("%F", path)
        .replace("%H", hash)
}

/// Split `template` into `(program, args)` with the tokens substituted, or
/// `None` if the template is blank.
fn build(template: &str, name: &str, path: &str, hash: &str) -> Option<(String, Vec<String>)> {
    let mut tokens = template.split_whitespace();
    let program = substitute(tokens.next()?, name, path, hash);
    let args = tokens.map(|t| substitute(t, name, path, hash)).collect();
    Some((program, args))
}

/// Run the completion hook for one torrent, if `template` is non-empty.
///
/// Spawns on a detached thread that waits on the child, so the poller never
/// blocks and no zombie is left behind. Returns the resolved program name (for
/// logging) when a hook was launched.
pub fn run_on_complete(template: &str, name: &str, path: &str, hash: &str) -> Option<String> {
    let (program, args) = build(template, name, path, hash)?;
    let launched = program.clone();
    // `status()` waits for and reaps the child; do it off-thread.
    let _ = std::thread::Builder::new()
        .name("completion-hook".to_string())
        .spawn(move || {
            let _ = Command::new(&program).args(&args).status();
        });
    Some(launched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_substituted_per_argument() {
        let (program, args) = build("notify %N %F %H", "My File", "/srv/My File", "ABC").unwrap();
        assert_eq!(program, "notify");
        // A value with spaces stays one argument (no shell splitting).
        assert_eq!(args, vec!["My File", "/srv/My File", "ABC"]);
    }

    #[test]
    fn tokens_embedded_in_a_token_are_replaced() {
        let (program, args) = build("/bin/log --msg=done:%H", "n", "p", "DEADBEEF").unwrap();
        assert_eq!(program, "/bin/log");
        assert_eq!(args, vec!["--msg=done:DEADBEEF"]);
    }

    #[test]
    fn blank_template_builds_nothing() {
        assert!(build("   ", "n", "p", "h").is_none());
        assert!(build("", "n", "p", "h").is_none());
    }
}
