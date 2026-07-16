//! Allocation for rstorrent's small app-owned named-throttle pool.
//!
//! rtorrent stores a throttle name on each download but forgets the name's
//! rate definition when the daemon restarts. Definitions therefore live in app
//! settings and are replayed on each connection. Slots actively assigned to a
//! torrent are never repurposed; an inactive stale slot can be recycled.

use std::collections::HashSet;

use crate::ipc::NamedThrottle;

pub const POOL_SIZE: usize = 8;
const PREFIX: &str = "rstorrent_";

/// Choose a definition for a requested rate pair. The boolean is true when the
/// selected slot is new or must be redefined and persisted.
pub fn allocate(
    definitions: &[NamedThrottle],
    active_names: &HashSet<String>,
    down_kb: i64,
    up_kb: i64,
) -> Result<(NamedThrottle, bool), &'static str> {
    if let Some(existing) = definitions
        .iter()
        .find(|item| item.down_kb == down_kb && item.up_kb == up_kb)
    {
        return Ok((existing.clone(), false));
    }

    for slot in 1..=POOL_SIZE {
        let name = format!("{PREFIX}{slot}");
        if !definitions.iter().any(|item| item.name == name) {
            return Ok((NamedThrottle { name, down_kb, up_kb }, true));
        }
    }

    if let Some(stale) = definitions
        .iter()
        .find(|item| !active_names.contains(&item.name))
    {
        return Ok((
            NamedThrottle {
                name: stale.name.clone(),
                down_kb,
                up_kb,
            },
            true,
        ));
    }

    Err("all per-torrent rate-limit slots are currently in use")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition(slot: usize, rate: i64) -> NamedThrottle {
        NamedThrottle {
            name: format!("{PREFIX}{slot}"),
            down_kb: rate,
            up_kb: rate,
        }
    }

    #[test]
    fn reuses_matching_definition() {
        let definitions = vec![definition(1, 512)];
        let (chosen, changed) = allocate(&definitions, &HashSet::new(), 512, 512).unwrap();
        assert_eq!(chosen, definitions[0]);
        assert!(!changed);
    }

    #[test]
    fn allocates_first_free_slot() {
        let definitions = vec![definition(1, 512), definition(3, 2048)];
        let (chosen, changed) = allocate(&definitions, &HashSet::new(), 1024, 0).unwrap();
        assert_eq!(chosen.name, "rstorrent_2");
        assert_eq!((chosen.down_kb, chosen.up_kb), (1024, 0));
        assert!(changed);
    }

    #[test]
    fn only_recycles_an_inactive_full_pool_slot() {
        let definitions: Vec<_> = (1..=POOL_SIZE)
            .map(|slot| definition(slot, slot as i64))
            .collect();
        let active: HashSet<_> = definitions
            .iter()
            .skip(1)
            .map(|item| item.name.clone())
            .collect();
        let (chosen, changed) = allocate(&definitions, &active, 999, 111).unwrap();
        assert_eq!(chosen.name, "rstorrent_1");
        assert!(changed);

        let all_active = definitions
            .iter()
            .map(|item| item.name.clone())
            .collect();
        assert!(allocate(&definitions, &all_active, 999, 111).is_err());
    }
}
