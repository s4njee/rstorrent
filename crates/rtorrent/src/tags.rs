//! Torrent tags (V3-10): a many-to-many, client-managed label alongside the
//! ruTorrent label.
//!
//! The label is the *category* (`d.custom1`, one per torrent, ruTorrent's
//! convention). Tags are a *set* per torrent, stored in rtorrent's keyed
//! `d.custom` namespace under the key `tags`, so they follow the torrent across
//! daemon restarts and across every client that talks to the same daemon — which
//! is what makes them sync between profiles and the web console for free.
//!
//! The wire form is a single comma-separated string (`d.custom` holds one
//! string). Names cannot contain a comma, so the parser and the writer agree on
//! the separator; everything else is preserved.

/// The `d.custom` key tags live under.
pub const CUSTOM_KEY: &str = "tags";

/// Longest single tag accepted; anything longer is truncated rather than
/// rejected, so a pasted name never fails a bulk edit.
const MAX_TAG_LEN: usize = 64;

/// Normalise a user-entered tag list: trim, drop empties, drop the separator,
/// cap the length, and de-duplicate case-insensitively (the first spelling wins,
/// so "Linux" then "linux" keeps "Linux").
#[must_use]
pub fn normalise<I, S>(tags: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out: Vec<String> = Vec::new();
    for raw in tags {
        let cleaned: String = raw.as_ref().chars().filter(|c| *c != ',').collect();
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            continue;
        }
        let cleaned: String = cleaned.chars().take(MAX_TAG_LEN).collect();
        let cleaned = cleaned.trim_end().to_string();
        if cleaned.is_empty() {
            continue;
        }
        let key = cleaned.to_lowercase();
        if out.iter().any(|existing| existing.to_lowercase() == key) {
            continue;
        }
        out.push(cleaned);
    }
    out
}

/// The wire form stored in `d.custom=tags`.
#[must_use]
pub fn encode(tags: &[String]) -> String {
    tags.join(",")
}

/// Parse the wire form back into a normalised list.
#[must_use]
pub fn parse(raw: &str) -> Vec<String> {
    normalise(raw.split(','))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_trims_drops_empties_and_the_separator() {
        let tags = normalise([" linux ", "", "  ", "a,b", "c"]);
        assert_eq!(tags, vec!["linux", "ab", "c"]);
    }

    #[test]
    fn normalise_dedupes_case_insensitively_keeping_the_first_spelling() {
        let tags = normalise(["Linux", "linux", "LINUX"]);
        assert_eq!(tags, vec!["Linux"]);
    }

    #[test]
    fn normalise_caps_a_single_tag() {
        let long = "x".repeat(200);
        assert_eq!(
            normalise([long]).first().map(String::len),
            Some(MAX_TAG_LEN)
        );
    }

    #[test]
    fn encode_parse_round_trips() {
        let tags = normalise(["iso", "linux", "archive"]);
        assert_eq!(encode(&tags), "iso,linux,archive");
        assert_eq!(parse("iso,linux,archive"), tags);
        assert!(parse("").is_empty());
        // A trailing separator (an append bug elsewhere) is not an empty tag.
        assert_eq!(parse("iso,"), vec!["iso"]);
    }
}
