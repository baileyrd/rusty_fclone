use xxhash_rust::xxh3::Xxh3;

/// Hashes the concatenation of `chunks` with xxh3-128 (ADR-0001).
///
/// Taking multiple chunks (rather than one pre-concatenated buffer) lets
/// callers hand over the head/middle/tail samples of the partial-hash stage
/// without an extra allocation to join them first.
pub(crate) fn hash_chunks(chunks: &[&[u8]]) -> u128 {
    let mut hasher = Xxh3::new();
    for chunk in chunks {
        hasher.update(chunk);
    }
    hasher.digest128()
}

/// Byte ranges to sample for the partial-hash stage: head, middle, and tail.
///
/// Sampling three points (rather than just a prefix) catches files that
/// share an identical header/metadata block but differ in their body — a
/// prefix-only sample would let those through to a full hash unnecessarily
/// (ADR-0001).
pub(crate) fn sample_ranges(size: u64, sample_size: u64) -> Vec<(u64, usize)> {
    let sample_size = sample_size.max(1);
    let sample = sample_size.min(size) as usize;
    let head = (0u64, sample);
    let mid_offset = (size / 2).saturating_sub(sample_size / 2);
    let mid = (mid_offset, sample);
    let tail_offset = size.saturating_sub(sample_size);
    let tail = (tail_offset, sample);
    vec![head, mid, tail]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_chunks_is_order_sensitive_and_deterministic() {
        let a = hash_chunks(&[b"hello", b"world"]);
        let b = hash_chunks(&[b"hello", b"world"]);
        let c = hash_chunks(&[b"world", b"hello"]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn sample_ranges_cover_head_middle_tail() {
        let ranges = sample_ranges(1_000_000, 1024);
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0], (0, 1024));
        assert_eq!(ranges[2], (1_000_000 - 1024, 1024));
    }

    #[test]
    fn sample_ranges_clamp_to_small_files() {
        let ranges = sample_ranges(10, 1024);
        for (_, len) in ranges {
            assert!(len <= 10);
        }
    }
}
