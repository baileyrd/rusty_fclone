//! Import full-file hashes from an existing `fclones --cache` database, so
//! a tree fclones already scanned doesn't need re-hashing here (ADR-0019).
//!
//! fclones' cache is a `sled` database keyed by `(device, inode, chunk_pos,
//! chunk_len)` rather than by path, so this only helps for a file whose
//! platform file-id fclones can also resolve (Unix `st_dev`/`st_ino` via
//! the `file-id` crate's [`FileId::Inode`] variant -- Windows is out of
//! scope, see the ADR). A cache entry is fclones' *full-file* hash exactly
//! when its key's `chunk_pos` is `0` and `chunk_len` equals the file's
//! length -- fclones uses the same key shape for its prefix/suffix sample
//! hashes, which use a different `chunk_pos`/`chunk_len` and must not be
//! mistaken for a full hash.
//!
//! Only fclones' `xxhash3` algorithm (`--hash-fn xxhash`) is byte-for-byte
//! interchangeable with this crate's own xxh3-128 full hash: fclones'
//! `HashFn::Xxhash` hashes with the same `xxhash_rust::xxh3` crate this
//! project already depends on. Every other fclones hash function (its
//! default `metro`, `blake3`, `sha256`, ...) computes a different digest
//! entirely and is never looked at here.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use file_id::{get_file_id, FileId};
use serde::{Deserialize, Serialize};

/// The tree fclones opens for its xxhash3 cache with no `--transform`.
/// Mirrors `fclones::cache::HashCache::open`'s
/// `format!("hash_db:{:?}:{}", algorithm, transform.unwrap_or("<none>"))`
/// for `algorithm = HashFn::Xxhash` (its derived `Debug` prints the bare
/// variant name) and no transform.
const XXHASH_TREE_NAME: &str = "hash_db:Xxhash:<none>";

/// Mirrors fclones' `cache::Key` (`src/cache.rs`) field-for-field, so
/// `bincode`'s positional encoding lines up byte-for-byte with what
/// fclones itself wrote. `FileId`/`FilePos`/`FileLen` are single-field
/// newtypes there, which serde/bincode serialize transparently as their
/// inner integer -- so plain `u64`s here encode identically.
#[derive(Serialize, Deserialize)]
struct FclonesKey {
    device: u64,
    inode: u64,
    chunk_pos: u64,
    chunk_len: u64,
}

/// Mirrors fclones' `cache::CachedFileInfo`. `hash` mirrors fclones'
/// `FileHash`, which serializes as a *hex string of its little-endian
/// bytes* (`serializer.collect_str(hex::encode(&self.0))`) -- not a
/// standard big-endian hex formatting of the integer.
#[derive(Serialize, Deserialize)]
struct FclonesCachedFileInfo {
    modified_timestamp_ms: u64,
    file_len: u64,
    data_len: u64,
    hash: String,
}

/// Read access to an existing fclones hash-cache database, for importing
/// full-file hashes it already computed.
pub(crate) struct FclonesImportCache {
    /// `None` when the database opened but has no xxhash3 tree (fclones
    /// was never run with `--hash-fn xxhash` against it, or the database
    /// is otherwise empty of anything usable here) -- every lookup is then
    /// a clean, cheap miss rather than an error.
    tree: Option<sled::Tree>,
}

impl FclonesImportCache {
    /// Opens an fclones cache database directory (e.g. `~/.cache/fclones`)
    /// for lookups. Returns `None` (after logging a warning) if the path
    /// doesn't exist or isn't a valid sled database -- the scan proceeds
    /// without import, matching this crate's cache-failure tolerance
    /// (ADR-0004, ADR-0016). Unlike `--cache`, this path is meant to
    /// already exist (it's *importing from* fclones, not creating a new
    /// database), so -- unlike `sled::open`'s own create-if-missing
    /// behavior -- a missing directory is treated as a (loudly logged)
    /// error rather than silently created as an empty database a typo'd
    /// path would otherwise produce with zero indication anything was
    /// wrong.
    pub(crate) fn open(dir: &Path) -> Option<Self> {
        if !dir.exists() {
            tracing::warn!(
                path = %dir.display(),
                "fclones cache path does not exist, continuing without import"
            );
            return None;
        }
        match sled::open(dir) {
            Ok(db) => {
                let tree = db.open_tree(XXHASH_TREE_NAME).ok();
                Some(FclonesImportCache { tree })
            }
            Err(err) => {
                tracing::warn!(
                    path = %dir.display(),
                    error = %err,
                    "failed to open fclones cache for import, continuing without it"
                );
                None
            }
        }
    }

    /// Looks up `path`'s full-content hash in the fclones cache. Returns
    /// `None` on any miss: not present, a different hash function was
    /// used, the file's platform id can't be resolved (non-Unix), or every
    /// candidate entry tried is stale (`size`/`modified_ms` no longer
    /// match what fclones recorded -- the same staleness check fclones'
    /// own `HashCache::get` performs).
    ///
    /// fclones only ever caches a `chunk_len == size` entry (a *bona fide*
    /// full-content hash, from its `group_by_contents` stage) for a file
    /// at or above its own prefix-sample length. A smaller file instead
    /// gets only a prefix-hash entry keyed by that *unclamped* prefix
    /// length -- fclones requests that many bytes regardless of the
    /// file's actual size, and the read simply stops at EOF, so the
    /// resulting hash *is* the full-content hash even though the key
    /// doesn't say `chunk_len == size`. Since the prefix length used for
    /// any given cached entry isn't recorded anywhere we can read, this
    /// also tries fclones' two documented default prefix lengths (4 KiB
    /// for SSDs, 16 KiB for HDDs/unknown -- see `device.rs`) whenever the
    /// file is small enough that one of them would have covered it. A run
    /// with an explicit non-default `--max-prefix-size` won't be found
    /// this way; that's a missed opportunity, never a wrong hash, since
    /// every candidate is still gated by the same size/mtime check below.
    pub(crate) fn lookup_full_hash(
        &self,
        path: &Path,
        size: u64,
        modified_ms: u64,
    ) -> Option<u128> {
        const DEFAULT_PREFIX_LENS: [u64; 2] = [4 * 1024, 16 * 1024];

        let tree = self.tree.as_ref()?;
        let FileId::Inode {
            device_id,
            inode_number,
        } = get_file_id(path).ok()?
        else {
            return None; // Windows / an id kind fclones doesn't use this way
        };

        let candidate_chunk_lens = std::iter::once(size).chain(
            DEFAULT_PREFIX_LENS
                .into_iter()
                .filter(|&prefix_len| size <= prefix_len && prefix_len != size),
        );

        for chunk_len in candidate_chunk_lens {
            let key = FclonesKey {
                device: device_id,
                inode: inode_number,
                chunk_pos: 0,
                chunk_len,
            };
            let Ok(key_bytes) = bincode::serialize(&key) else {
                continue;
            };
            let Ok(Some(value_bytes)) = tree.get(key_bytes) else {
                continue;
            };
            let Ok(info) = bincode::deserialize::<FclonesCachedFileInfo>(&value_bytes) else {
                continue;
            };
            if info.file_len != size || info.modified_timestamp_ms != modified_ms {
                continue; // stale: file changed since fclones cached it
            }
            if let Some(hash) = decode_le_hash(&info.hash) {
                return Some(hash);
            }
        }
        None
    }
}

/// Converts a [`SystemTime`] to the same millisecond-since-epoch
/// resolution fclones stores (`Duration::as_millis`), for comparison
/// against a `CachedFileInfo.modified_timestamp_ms`.
pub(crate) fn to_millis(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Decodes fclones' hex-encoded hash string back into a `u128`, reversing
/// `FileHash::from(u128)`'s `write_u128::<LittleEndian>` -- i.e. the hex
/// string is the *little-endian* bytes of the value, not its standard
/// big-endian hex representation.
fn decode_le_hash(hex: &str) -> Option<u128> {
    if hex.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for i in 0..16 {
        bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(u128::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;

    /// Writes one entry into a fresh fclones-shaped sled cache exactly as
    /// fclones' own `HashCache::put`/`typed_sled` would, for a real file's
    /// real (device, inode) -- this is what makes the round-trip test
    /// below a genuine test of the on-disk format, not just of this
    /// module talking to itself.
    fn seed_fclones_cache(
        cache_dir: &Path,
        file: &Path,
        chunk_len: u64,
        size: u64,
        modified_ms: u64,
        hash: u128,
    ) {
        let db = sled::open(cache_dir).unwrap();
        let tree = db.open_tree(XXHASH_TREE_NAME).unwrap();
        let FileId::Inode {
            device_id,
            inode_number,
        } = get_file_id(file).unwrap()
        else {
            panic!("test only runs where file-id resolves to Inode");
        };
        let key = FclonesKey {
            device: device_id,
            inode: inode_number,
            chunk_pos: 0,
            chunk_len,
        };
        let value = FclonesCachedFileInfo {
            modified_timestamp_ms: modified_ms,
            file_len: size,
            data_len: chunk_len,
            hash: hex::encode(hash.to_le_bytes()),
        };
        tree.insert(
            bincode::serialize(&key).unwrap(),
            bincode::serialize(&value).unwrap(),
        )
        .unwrap();
        tree.flush().unwrap();
    }

    /// Tiny hex-encode helper so the test doesn't need its own dependency
    /// just to build a fixture -- mirrors `decode_le_hash` in reverse.
    mod hex {
        pub(super) fn encode(bytes: [u8; 16]) -> String {
            bytes.iter().map(|b| format!("{b:02x}")).collect()
        }
    }

    #[test]
    fn decode_le_hash_matches_a_real_fclones_report_value() {
        // Captured from a real `fclones group --cache --hash-fn xxhash`
        // run in development against a 30-byte file, cross-checked against
        // `xxhash_rust::xxh3::xxh3_128` computed independently over the
        // same bytes -- see ADR-0019.
        let hash = decode_le_hash("db8881b16c4f743a75c09b4fec87d411".get(0..32).unwrap());
        assert_eq!(hash, Some(23700399710133407773276464628843514075u128));
    }

    #[test]
    fn open_returns_none_and_creates_nothing_for_a_nonexistent_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");

        assert!(FclonesImportCache::open(&missing).is_none());
        assert!(
            !missing.exists(),
            "a typo'd path must not be silently created"
        );
    }

    #[test]
    fn miss_when_the_cache_directory_has_no_xxhash_tree() {
        let dir = tempfile::tempdir().unwrap();
        // A valid, empty sled database -- e.g. fclones was only ever run
        // with its default (Metro) hash function against this path.
        sled::open(dir.path()).unwrap();

        let cache = FclonesImportCache::open(dir.path()).unwrap();
        let file = dir.path().join("does-not-matter.txt");
        fs::write(&file, b"content").unwrap();
        assert_eq!(cache.lookup_full_hash(&file, 7, 0), None);
    }

    #[test]
    fn hit_for_a_real_file_with_a_matching_seeded_entry() {
        // A file at/above fclones' prefix length gets a real
        // chunk_len == size full-content entry.
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("fclones-cache");
        let file = dir.path().join("file.txt");
        fs::write(&file, b"hello world duplicate content\n").unwrap();
        let metadata = fs::metadata(&file).unwrap();
        let size = metadata.len();
        let modified_ms = to_millis(metadata.modified().unwrap());

        seed_fclones_cache(
            &cache_dir,
            &file,
            size,
            size,
            modified_ms,
            0x1122_3344_5566_7788_99aa_bbcc_ddee_ff00,
        );

        let cache = FclonesImportCache::open(&cache_dir).unwrap();
        assert_eq!(
            cache.lookup_full_hash(&file, size, modified_ms),
            Some(0x1122_3344_5566_7788_99aa_bbcc_ddee_ff00)
        );
    }

    #[test]
    fn hit_for_a_small_file_only_keyed_by_fclones_default_prefix_length() {
        // Matches fclones' real, verified behavior (see the ADR and this
        // module's docs): a file smaller than its prefix length is never
        // given a chunk_len == size entry -- only a chunk_len == 16384 (or
        // 4096) entry, because fclones requests that many bytes
        // regardless of the file's actual size and the read just stops at
        // EOF. Captured directly from a real `fclones group --cache
        // --hash-fn xxhash` run against a 30-byte file.
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("fclones-cache");
        let file = dir.path().join("file.txt");
        fs::write(&file, b"hello world duplicate content\n").unwrap(); // 30 bytes
        let metadata = fs::metadata(&file).unwrap();
        let size = metadata.len();
        assert!(size < 16 * 1024);
        let modified_ms = to_millis(metadata.modified().unwrap());

        seed_fclones_cache(
            &cache_dir,
            &file,
            16 * 1024,
            size,
            modified_ms,
            0x1122_3344_5566_7788_99aa_bbcc_ddee_ff00,
        );

        let cache = FclonesImportCache::open(&cache_dir).unwrap();
        assert_eq!(
            cache.lookup_full_hash(&file, size, modified_ms),
            Some(0x1122_3344_5566_7788_99aa_bbcc_ddee_ff00)
        );
    }

    #[test]
    fn miss_for_a_small_file_keyed_by_a_non_default_prefix_length() {
        // An explicit --max-prefix-size override that isn't one of
        // fclones' two documented defaults can't be recovered -- a real
        // limitation (see this module's docs), not a wrong result: it
        // still falls through to a clean miss, never a mismatched hash.
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("fclones-cache");
        let file = dir.path().join("file.txt");
        fs::write(&file, b"hello world duplicate content\n").unwrap();
        let metadata = fs::metadata(&file).unwrap();
        let size = metadata.len();
        let modified_ms = to_millis(metadata.modified().unwrap());

        seed_fclones_cache(&cache_dir, &file, 64 * 1024, size, modified_ms, 42);

        let cache = FclonesImportCache::open(&cache_dir).unwrap();
        assert_eq!(cache.lookup_full_hash(&file, size, modified_ms), None);
    }

    #[test]
    fn miss_when_the_files_size_no_longer_matches_the_seeded_entry() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("fclones-cache");
        let file = dir.path().join("file.txt");
        fs::write(&file, b"hello world duplicate content\n").unwrap();
        let metadata = fs::metadata(&file).unwrap();
        let modified_ms = to_millis(metadata.modified().unwrap());

        seed_fclones_cache(
            &cache_dir,
            &file,
            metadata.len(),
            metadata.len(),
            modified_ms,
            42,
        );

        let cache = FclonesImportCache::open(&cache_dir).unwrap();
        assert_eq!(
            cache.lookup_full_hash(&file, metadata.len() + 1, modified_ms),
            None
        );
    }

    #[test]
    fn miss_when_the_files_mtime_no_longer_matches_the_seeded_entry() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("fclones-cache");
        let file = dir.path().join("file.txt");
        fs::write(&file, b"hello world duplicate content\n").unwrap();
        let metadata = fs::metadata(&file).unwrap();
        let modified_ms = to_millis(metadata.modified().unwrap());

        seed_fclones_cache(
            &cache_dir,
            &file,
            metadata.len(),
            metadata.len(),
            modified_ms,
            42,
        );

        let cache = FclonesImportCache::open(&cache_dir).unwrap();
        assert_eq!(
            cache.lookup_full_hash(&file, metadata.len(), modified_ms + 1000),
            None
        );
    }

    #[test]
    fn miss_when_the_seeded_entry_is_a_partial_hash_not_a_full_one() {
        // A prefix-sample entry (chunk_pos = 0 but chunk_len < file_len)
        // must never be mistaken for the full-file hash.
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("fclones-cache");
        let file = dir.path().join("file.txt");
        fs::write(&file, b"hello world duplicate content\n").unwrap();
        let metadata = fs::metadata(&file).unwrap();
        let modified_ms = to_millis(metadata.modified().unwrap());

        let db = sled::open(&cache_dir).unwrap();
        let tree = db.open_tree(XXHASH_TREE_NAME).unwrap();
        let FileId::Inode {
            device_id,
            inode_number,
        } = get_file_id(&file).unwrap()
        else {
            panic!("test only runs where file-id resolves to Inode");
        };
        let key = FclonesKey {
            device: device_id,
            inode: inode_number,
            chunk_pos: 0,
            chunk_len: 8, // a prefix sample, not the full 30-byte file
        };
        let value = FclonesCachedFileInfo {
            modified_timestamp_ms: modified_ms,
            file_len: metadata.len(),
            data_len: 8,
            hash: hex::encode(42u128.to_le_bytes()),
        };
        tree.insert(
            bincode::serialize(&key).unwrap(),
            bincode::serialize(&value).unwrap(),
        )
        .unwrap();
        tree.flush().unwrap();
        drop(tree);
        drop(db); // release sled's exclusive file lock before reopening

        let cache = FclonesImportCache::open(&cache_dir).unwrap();
        assert_eq!(
            cache.lookup_full_hash(&file, metadata.len(), modified_ms),
            None
        );
    }

    #[test]
    fn to_millis_matches_expected_duration_since_epoch() {
        let t = UNIX_EPOCH + Duration::from_millis(1_700_000_000_123);
        assert_eq!(to_millis(t), 1_700_000_000_123);
    }
}
