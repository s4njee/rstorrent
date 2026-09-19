//! Decoding the piece map for the Pieces pane.
//!
//! rtorrent reports the bitfield and the availability map as hex strings, one
//! bit (or one peer count) per chunk. A torrent can have hundreds of thousands
//! of chunks and the stripe has a few hundred columns, so both are downsampled
//! here — the only place that happens.
//!
//! A port of `src/utils/bitfield.ts`.

/// Decode a hex string into bytes, two characters per byte.
///
/// Both the bitfield and the availability map use this encoding; only the
/// meaning of a byte differs. A trailing half-byte is ignored and a non-hex
/// character reads as zero, so a garbled buffer degrades to "nothing here"
/// rather than an error the UI would have to render.
#[must_use]
pub fn bytes_from_hex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| (digit(pair[0]) << 4) | digit(pair[1]))
        .collect()
}

fn digit(byte: u8) -> u8 {
    char::from(byte).to_digit(16).map_or(0, |value| value as u8)
}

/// Whether piece `index` is present. Bits are MSB-first within each byte, which
/// is how rtorrent packs them.
#[must_use]
pub fn has_piece(bytes: &[u8], index: usize) -> bool {
    bytes
        .get(index / 8)
        .is_some_and(|byte| byte & (0x80 >> (index % 8)) != 0)
}

/// The completed fraction of each stripe column.
///
/// Each column covers a slice of the piece range, so a few hundred columns can
/// represent any number of pieces. Empty when there is nothing to divide up.
#[must_use]
pub fn bucket_fractions(bytes: &[u8], total_pieces: i64, buckets: usize) -> Vec<f64> {
    ranges(total_pieces, buckets)
        .into_iter()
        .map(|(start, end)| {
            if end <= start {
                return 0.0;
            }
            let present = (start..end)
                .filter(|index| has_piece(bytes, *index))
                .count();
            present as f64 / (end - start) as f64
        })
        .collect()
}

/// The mean peer count of each column, and the peak mean.
///
/// The map is normalised against the peak rather than against 255, so a swarm
/// where the best-covered stretch has two peers still reads as fully covered.
#[must_use]
pub fn bucket_averages(counts: &[u8], total_pieces: i64, buckets: usize) -> (Vec<f64>, f64) {
    let mut peak: f64 = 0.0;
    let averages: Vec<f64> = ranges(total_pieces, buckets)
        .into_iter()
        .map(|(start, end)| {
            if end <= start {
                return 0.0;
            }
            let sum: f64 = (start..end)
                .map(|index| f64::from(counts.get(index).copied().unwrap_or(0)))
                .sum();
            let mean = sum / (end - start) as f64;
            peak = peak.max(mean);
            mean
        })
        .collect();
    (averages, peak)
}

/// The swarm's "distributed copies": the least-available chunk's peer count plus
/// the fraction of chunks that beat it.
///
/// Below 1 means at least one stretch exists on no connected peer — the signal
/// that a download could stall if those peers leave.
#[must_use]
pub fn distributed_copies(counts: &[u8], total_pieces: i64) -> f64 {
    let available = usize::try_from(total_pieces).unwrap_or(0).min(counts.len());
    if available == 0 {
        return 0.0;
    }
    let slice = &counts[..available];
    let floor = slice.iter().copied().min().unwrap_or(0);
    let above = slice.iter().filter(|count| **count > floor).count();
    f64::from(floor) + above as f64 / available as f64
}

/// The piece range each column covers. A range may be empty when there are more
/// columns than pieces; the callers render that as nothing.
fn ranges(total_pieces: i64, buckets: usize) -> Vec<(usize, usize)> {
    let Ok(total) = usize::try_from(total_pieces) else {
        return Vec::new();
    };
    if total == 0 || buckets == 0 {
        return Vec::new();
    }
    (0..buckets)
        .map(|bucket| {
            let start = bucket * total / buckets;
            let end = (bucket + 1) * total / buckets;
            (start, end.max(start))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first four pieces of a byte, MSB-first: 0b1100_0000.
    const FIRST_TWO: &str = "c0";

    #[test]
    fn hex_decodes_two_characters_at_a_time() {
        assert_eq!(bytes_from_hex("c0"), vec![0xc0]);
        assert_eq!(bytes_from_hex("00ff10"), vec![0x00, 0xff, 0x10]);
        // A trailing half-byte is not a byte.
        assert_eq!(bytes_from_hex("c0f"), vec![0xc0]);
        // Nothing at all is the legitimate "no map yet" case.
        assert!(bytes_from_hex("").is_empty());
    }

    #[test]
    fn a_garbled_buffer_reads_as_absent_rather_than_failing() {
        assert_eq!(bytes_from_hex("zz"), vec![0]);
        assert!(!has_piece(&bytes_from_hex("zz"), 0));
    }

    #[test]
    fn pieces_are_msb_first_within_a_byte() {
        let bytes = bytes_from_hex(FIRST_TWO);
        assert!(has_piece(&bytes, 0));
        assert!(has_piece(&bytes, 1));
        assert!(!has_piece(&bytes, 2));
        assert!(!has_piece(&bytes, 7));
        // Past the end of the buffer there is nothing to have.
        assert!(!has_piece(&bytes, 8));
    }

    #[test]
    fn a_full_bitfield_is_every_column_full() {
        let bytes = bytes_from_hex("ffff");
        let fractions = bucket_fractions(&bytes, 16, 4);
        assert_eq!(fractions, vec![1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn an_empty_bitfield_is_every_column_empty() {
        let fractions = bucket_fractions(&bytes_from_hex("0000"), 16, 4);
        assert_eq!(fractions, vec![0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_column_reports_the_fraction_of_its_slice() {
        // Bits 0..8 of "80" = piece 0 only, so the first of two columns is
        // half covered and the second is bare.
        let fractions = bucket_fractions(&bytes_from_hex("8000"), 16, 2);
        assert_eq!(fractions, vec![0.125, 0.0]);
    }

    #[test]
    fn there_are_no_columns_without_pieces_or_buckets() {
        assert!(bucket_fractions(&bytes_from_hex("ff"), 0, 4).is_empty());
        assert!(bucket_fractions(&bytes_from_hex("ff"), 8, 0).is_empty());
    }

    #[test]
    fn more_columns_than_pieces_leaves_the_extra_ones_empty() {
        // Two pieces across four columns: two columns carry a piece each and
        // two have nothing to describe.
        let fractions = bucket_fractions(&bytes_from_hex("c0"), 2, 4);
        assert_eq!(fractions, vec![0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn availability_averages_normalise_against_the_peak() {
        // Two columns over four chunks: means 4 and 1.
        let (averages, peak) = bucket_averages(&[4, 4, 0, 1], 4, 2);
        assert_eq!(averages, vec![4.0, 0.5]);
        assert_eq!(peak, 4.0);
    }

    #[test]
    fn a_short_availability_buffer_counts_as_no_peers() {
        // Only the first chunk was decoded: the rest read as absent, not as a
        // panic or a wrong average.
        let (averages, peak) = bucket_averages(&[2], 4, 2);
        assert_eq!(averages, vec![1.0, 0.0]);
        assert_eq!(peak, 1.0);
    }

    #[test]
    fn distributed_copies_measures_the_worst_covered_chunk() {
        // Every chunk on 2 peers: two whole copies.
        assert_eq!(distributed_copies(&[2, 2, 2, 2], 4), 2.0);
        // One chunk on nobody: the floor is 0, with three quarters above it.
        assert_eq!(distributed_copies(&[0, 1, 1, 1], 4), 0.75);
        // No data at all is not a copy.
        assert_eq!(distributed_copies(&[], 4), 0.0);
        assert_eq!(distributed_copies(&[1, 1], 0), 0.0);
    }

    #[test]
    fn distributed_copies_ignores_chunks_beyond_the_buffer() {
        assert_eq!(distributed_copies(&[3, 3], 2), 3.0);
    }
}
