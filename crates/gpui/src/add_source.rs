//! What may be handed to the daemon as a torrent source.
//!
//! A port of the validation and name-picking the Tauri add-magnet dialog keeps
//! beside its JSX. Pure, because "is this line loadable?" is a rule worth
//! testing rather than a side effect of a text field.

/// Whether a line is something rtorrent can load: a magnet carrying a btih hash,
/// or an `http(s)` URL.
///
/// Deliberately loose about everything else — rtorrent is the authority on what
/// it will accept, and refusing a valid but unusual URI is worse than passing it
/// on and reporting what the daemon said.
#[must_use]
pub fn is_valid(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() {
        return false;
    }
    if is_magnet(text) {
        return magnet_hash(text).is_some();
    }
    let lower = text.to_ascii_lowercase();
    ["http://", "https://"].iter().any(|scheme| {
        lower
            .strip_prefix(scheme)
            .is_some_and(|rest| !rest.trim().is_empty())
    })
}

/// A magnet's info-hash, if it carries one.
#[must_use]
pub fn magnet_hash(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let needle = "xt=urn:btih:";
    let start = lower.find(needle)? + needle.len();
    let hash: String = text[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    (!hash.is_empty()).then_some(hash)
}

/// A magnet's display name, from its `dn` parameter, percent-decoded.
///
/// `None` when the magnet carries none, which is common: the daemon then names
/// the torrent itself once it has the metadata.
#[must_use]
pub fn display_name(uri: &str) -> Option<String> {
    let query = uri.split_once('?').map_or(uri, |(_, query)| query);
    for field in query.split('&') {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        if key == "dn" {
            let name = percent_decode(value);
            if !name.trim().is_empty() {
                return Some(name);
            }
        }
    }
    None
}

fn is_magnet(text: &str) -> bool {
    text.to_ascii_lowercase().starts_with("magnet:?")
}

/// Undo `%XX` escaping and the `+` a query uses for a space.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let pair = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
                match pair.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    None => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAGNET: &str =
        "magnet:?xt=urn:btih:c12fe1c06bba254a9dc9f519b335aa7c1367a88a&dn=Debian+13";

    #[test]
    fn a_magnet_with_a_hash_is_loadable() {
        assert!(is_valid(MAGNET));
        assert!(is_valid("magnet:?xt=urn:btih:ABC123"));
        // Case does not matter in the scheme.
        assert!(is_valid("MAGNET:?XT=URN:BTIH:abc123"));
    }

    #[test]
    fn a_magnet_without_a_hash_is_not() {
        assert!(!is_valid("magnet:?dn=something"));
        assert!(!is_valid("magnet:?xt=urn:sha1:abc"));
        assert!(!is_valid("magnet:?"));
    }

    #[test]
    fn an_http_url_is_loadable_and_anything_else_is_not() {
        assert!(is_valid("https://example.com/a.torrent"));
        assert!(is_valid("http://example.com/a.torrent"));
        assert!(is_valid("HTTPS://example.com/a.torrent"));
        // A scheme with nothing after it is not a URL.
        assert!(!is_valid("https://"));
        assert!(!is_valid("ftp://example.com/a.torrent"));
        assert!(!is_valid("example.com/a.torrent"));
        assert!(!is_valid(""));
        assert!(!is_valid("   "));
    }

    #[test]
    fn whitespace_around_a_source_is_ignored() {
        assert!(is_valid(&format!("  {MAGNET}\n")));
    }

    #[test]
    fn the_hash_is_read_off_the_xt_parameter() {
        assert_eq!(
            magnet_hash(MAGNET),
            Some("c12fe1c06bba254a9dc9f519b335aa7c1367a88a".to_owned())
        );
        assert_eq!(magnet_hash("magnet:?dn=nope"), None);
    }

    #[test]
    fn a_magnets_name_comes_from_its_display_name() {
        assert_eq!(display_name(MAGNET), Some("Debian 13".to_owned()));
        // `%20` and `+` both mean a space.
        assert_eq!(
            display_name("magnet:?dn=Debian%2013.iso"),
            Some("Debian 13.iso".to_owned())
        );
        // The name is not always first.
        assert_eq!(
            display_name("magnet:?xt=urn:btih:abc&dn=Later"),
            Some("Later".to_owned())
        );
    }

    #[test]
    fn a_magnet_without_a_name_has_none() {
        assert_eq!(display_name("magnet:?xt=urn:btih:abc"), None);
        assert_eq!(display_name("magnet:?dn="), None);
        assert_eq!(display_name("magnet:?dn=%20"), None);
    }

    #[test]
    fn a_broken_escape_is_left_alone_rather_than_dropped() {
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
        // Multibyte characters survive a decode.
        assert_eq!(percent_decode("caf%C3%A9"), "café");
    }
}
