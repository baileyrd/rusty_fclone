//! Incremental full-file-hash cache via `redb` (ADR-0016).
//!
//! Opt-in (`ScanOptions::cache_path`): when a file's `(size, mtime)` match a
//! cached entry, its full hash is reused instead of re-reading and
//! re-hashing the file. Only the full-hash stage is cached — the
//! partial-hash pruning stage is cheap enough (small sampled ranges) that
//! caching it wouldn't meaningfully help, and caching only one stage keeps
//! invalidation trivial: a cache hit is valid exactly when the file's size
//! and mtime are unchanged, regardless of any other scan option.

use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use redb::{Database, ReadableDatabase, TableDefinition};

const TABLE: TableDefinition<&str, [u8; ENTRY_LEN]> = TableDefinition::new("full_hash_cache_v1");
const ENTRY_LEN: usize = 8 + 8 + 4 + 16; // size, mtime_secs, mtime_nanos, hash

/// One cached `(size, mtime, full_hash)` record for a single path.
#[derive(Clone, Copy)]
struct CacheEntry {
    size: u64,
    mtime_secs: u64,
    mtime_nanos: u32,
    hash: u128,
}

impl CacheEntry {
    fn encode(self) -> [u8; ENTRY_LEN] {
        let mut buf = [0u8; ENTRY_LEN];
        buf[0..8].copy_from_slice(&self.size.to_le_bytes());
        buf[8..16].copy_from_slice(&self.mtime_secs.to_le_bytes());
        buf[16..20].copy_from_slice(&self.mtime_nanos.to_le_bytes());
        buf[20..36].copy_from_slice(&self.hash.to_le_bytes());
        buf
    }

    fn decode(buf: [u8; ENTRY_LEN]) -> Self {
        Self {
            size: u64::from_le_bytes(buf[0..8].try_into().unwrap()),
            mtime_secs: u64::from_le_bytes(buf[8..16].try_into().unwrap()),
            mtime_nanos: u32::from_le_bytes(buf[16..20].try_into().unwrap()),
            hash: u128::from_le_bytes(buf[20..36].try_into().unwrap()),
        }
    }
}

/// A `(size, modified-time)` snapshot cheap enough to take per-file at
/// cache-lookup time without threading it through the rest of the
/// pipeline's data model.
pub(crate) type FileStat = (u64, SystemTime);

pub(crate) fn stat(path: &Path) -> Option<FileStat> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    Some((metadata.len(), modified))
}

pub(crate) struct HashCache {
    db: Database,
}

impl HashCache {
    /// Opens (creating if necessary) a cache database at `path`. Ensures
    /// the table exists so a lookup against a brand-new database is a
    /// clean miss rather than an error.
    pub(crate) fn open(path: &Path) -> Result<Self, redb::Error> {
        let db = Database::create(path)?;
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(TABLE)?;
        }
        write_txn.commit()?;
        Ok(Self { db })
    }

    /// Returns the cached full hash for `path` if present and its stored
    /// `(size, mtime)` still match `stat` -- `None` on any miss (never
    /// seen before, invalidated by a size/mtime change, or a read error).
    pub(crate) fn get(&self, path: &Path, stat: FileStat) -> Option<u128> {
        let key = path.to_str()?;
        let (secs, nanos) = split_mtime(stat.1);
        let read_txn = self.db.begin_read().ok()?;
        let table = read_txn.open_table(TABLE).ok()?;
        let guard = table.get(key).ok()??;
        let entry = CacheEntry::decode(guard.value());
        if entry.size == stat.0 && entry.mtime_secs == secs && entry.mtime_nanos == nanos {
            Some(entry.hash)
        } else {
            None
        }
    }

    /// Persists every `(path, stat, hash)` in one write transaction.
    /// Best-effort: a write failure is logged and otherwise ignored,
    /// matching this crate's error-tolerance stance (ADR-0004) -- a cache
    /// that fails to persist should degrade to "no speedup next time,"
    /// never to a failed scan.
    pub(crate) fn put_batch(&self, entries: &[(Arc<Path>, FileStat, u128)]) {
        if entries.is_empty() {
            return;
        }
        let result = (|| -> Result<(), redb::Error> {
            let write_txn = self.db.begin_write()?;
            {
                let mut table = write_txn.open_table(TABLE)?;
                for (path, stat, hash) in entries {
                    let Some(key) = path.to_str() else {
                        continue;
                    };
                    let (secs, nanos) = split_mtime(stat.1);
                    let entry = CacheEntry {
                        size: stat.0,
                        mtime_secs: secs,
                        mtime_nanos: nanos,
                        hash: *hash,
                    };
                    table.insert(key, entry.encode())?;
                }
            }
            write_txn.commit()?;
            Ok(())
        })();
        if let Err(err) = result {
            tracing::warn!(error = %err, "failed to persist hash cache entries");
        }
    }
}

fn split_mtime(t: SystemTime) -> (u64, u32) {
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => (d.as_secs(), d.subsec_nanos()),
        Err(_) => (0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_entry_round_trips_through_encoding() {
        let entry = CacheEntry {
            size: 12345,
            mtime_secs: 1_700_000_000,
            mtime_nanos: 123_456_789,
            hash: 0xdead_beef_cafe_babe_1234_5678_9abc_def0,
        };
        let decoded = CacheEntry::decode(entry.encode());
        assert_eq!(decoded.size, entry.size);
        assert_eq!(decoded.mtime_secs, entry.mtime_secs);
        assert_eq!(decoded.mtime_nanos, entry.mtime_nanos);
        assert_eq!(decoded.hash, entry.hash);
    }

    #[test]
    fn miss_on_a_brand_new_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = HashCache::open(&dir.path().join("cache.redb")).unwrap();
        let stat = (10, SystemTime::now());
        assert_eq!(cache.get(Path::new("/some/file.txt"), stat), None);
    }

    #[test]
    fn hit_when_size_and_mtime_match_a_stored_entry() {
        let dir = tempfile::tempdir().unwrap();
        let cache = HashCache::open(&dir.path().join("cache.redb")).unwrap();
        let path: Arc<Path> = Arc::from(Path::new("/some/file.txt"));
        let stat = (10, SystemTime::now());
        cache.put_batch(&[(path.clone(), stat, 42)]);

        assert_eq!(cache.get(&path, stat), Some(42));
    }

    #[test]
    fn miss_when_size_differs_from_the_stored_entry() {
        let dir = tempfile::tempdir().unwrap();
        let cache = HashCache::open(&dir.path().join("cache.redb")).unwrap();
        let path: Arc<Path> = Arc::from(Path::new("/some/file.txt"));
        let mtime = SystemTime::now();
        cache.put_batch(&[(path.clone(), (10, mtime), 42)]);

        assert_eq!(cache.get(&path, (11, mtime)), None);
    }

    #[test]
    fn miss_when_mtime_differs_from_the_stored_entry() {
        let dir = tempfile::tempdir().unwrap();
        let cache = HashCache::open(&dir.path().join("cache.redb")).unwrap();
        let path: Arc<Path> = Arc::from(Path::new("/some/file.txt"));
        let original_mtime = SystemTime::now();
        cache.put_batch(&[(path.clone(), (10, original_mtime), 42)]);

        let changed_mtime = original_mtime + std::time::Duration::from_secs(1);
        assert_eq!(cache.get(&path, (10, changed_mtime)), None);
    }

    #[test]
    fn reopening_an_existing_cache_file_preserves_its_entries() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("cache.redb");
        let path: Arc<Path> = Arc::from(Path::new("/some/file.txt"));
        let stat = (10, SystemTime::now());
        {
            let cache = HashCache::open(&cache_path).unwrap();
            cache.put_batch(&[(path.clone(), stat, 99)]);
        }

        let reopened = HashCache::open(&cache_path).unwrap();
        assert_eq!(reopened.get(&path, stat), Some(99));
    }

    #[test]
    fn put_batch_overwrites_a_previous_entry_for_the_same_path() {
        let dir = tempfile::tempdir().unwrap();
        let cache = HashCache::open(&dir.path().join("cache.redb")).unwrap();
        let path: Arc<Path> = Arc::from(Path::new("/some/file.txt"));
        let stat_a = (10, SystemTime::now());
        let stat_b = (20, stat_a.1 + std::time::Duration::from_secs(1));
        cache.put_batch(&[(path.clone(), stat_a, 1)]);
        cache.put_batch(&[(path.clone(), stat_b, 2)]);

        assert_eq!(
            cache.get(&path, stat_a),
            None,
            "the old stat must no longer match"
        );
        assert_eq!(cache.get(&path, stat_b), Some(2));
    }
}
