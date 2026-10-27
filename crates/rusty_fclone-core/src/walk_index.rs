//! Persistent per-directory walk index (ADR-0034).
//!
//! Lives in the same redb file as the hash cache (`ScanOptions::cache_path`).
//! Each fully-stat-ed directory is recorded as its own mtime plus the
//! `(name, size, file id)` of every regular file in it, committed in batches
//! while the walk is still running. A later walk -- a re-run, or the retry
//! of a killed one -- that finds a directory's mtime unchanged reuses the
//! record instead of `stat`-ing each file again: adding, removing, or
//! renaming an entry always bumps the directory's mtime, so its listing is
//! known to be the same.
//!
//! What a record can't see is a file rewritten in place, which changes the
//! file's size/mtime but not its directory's. Such a file is grouped under
//! its old size until a `rescan_all` walk. That can hide a duplicate, never
//! invent one: every file that reaches hashing is re-`stat`-ed and read live.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use file_id::FileId;
use redb::{Database, ReadableDatabase, TableDefinition};

const TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("walk_index_v1");

/// Directories committed per write transaction -- also the most a kill can
/// lose, besides whatever arrived in the last `FLUSH_INTERVAL`.
const BATCH_DIRS: usize = 512;
/// Upper bound on how long a finished directory waits uncommitted.
const FLUSH_INTERVAL: Duration = Duration::from_secs(2);
/// A directory modified this recently might still change within the same
/// mtime tick after we listed it ("racy git" problem), so it isn't recorded.
const RACY_WINDOW: Duration = Duration::from_secs(2);

/// What traversal needs about one regular file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileStat {
    pub size: u64,
    pub file_id: FileId,
}

/// Recorded files of one directory, keyed by name.
pub(crate) type DirFiles = HashMap<String, FileStat>;

/// Read side of the index, shared by jwalk's worker threads.
///
/// Holds the database weakly: jwalk can drop its callback (and this with
/// it) a little after the walk ends, and a strong reference there would
/// keep the redb file locked against the next scan in the same process.
pub(crate) struct WalkIndex {
    db: Weak<Database>,
    reuse: bool,
    tx: Sender<WriterMsg>,
}

/// Owns the background thread that commits recorded directories.
pub(crate) struct IndexWriter {
    tx: Sender<WriterMsg>,
    join: JoinHandle<()>,
}

enum WriterMsg {
    Record(String, Vec<u8>),
    /// Commit what's queued and exit. Explicit rather than waiting for every
    /// sender to drop, since jwalk owns the clones held by its callback.
    Finish,
}

impl WalkIndex {
    /// Opens the index in `db` and starts its writer thread. With `reuse`
    /// false (`rescan_all`), lookups always miss but directories are still
    /// recorded, refreshing the whole index.
    pub(crate) fn open(db: Arc<Database>, reuse: bool) -> Result<(Self, IndexWriter), redb::Error> {
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(TABLE)?;
        }
        write_txn.commit()?;

        let (tx, rx) = unbounded();
        let weak_db = Arc::downgrade(&db);
        let join = std::thread::spawn(move || write_loop(&db, &rx));
        let writer = IndexWriter {
            tx: tx.clone(),
            join,
        };
        let index = Self {
            db: weak_db,
            reuse,
            tx,
        };
        Ok((index, writer))
    }

    /// The recorded files of `dir`, if it was recorded with exactly `mtime`.
    pub(crate) fn lookup(&self, dir: &Path, mtime: SystemTime) -> Option<DirFiles> {
        if !self.reuse {
            return None;
        }
        let key = dir.to_str()?;
        let read_txn = self.db.upgrade()?.begin_read().ok()?;
        let table = read_txn.open_table(TABLE).ok()?;
        let guard = table.get(key).ok()??;
        let (recorded_mtime, files) = decode(guard.value())?;
        (recorded_mtime == split_mtime(mtime)?).then_some(files)
    }

    /// Queues `dir`'s complete file list for the writer. Skipped when the
    /// directory can't be keyed or encoded, or was modified too recently to
    /// trust its mtime.
    pub(crate) fn record(&self, dir: &Path, mtime: SystemTime, files: &DirFiles) {
        let racy = SystemTime::now()
            .duration_since(mtime)
            .map_or(true, |age| age < RACY_WINDOW);
        if racy {
            return;
        }
        let (Some(key), Some(value)) = (dir.to_str(), encode(mtime, files)) else {
            return;
        };
        let _ = self.tx.send(WriterMsg::Record(key.to_owned(), value));
    }
}

impl IndexWriter {
    /// Commits every directory queued so far and stops the writer.
    pub(crate) fn finish(self) {
        let _ = self.tx.send(WriterMsg::Finish);
        if self.join.join().is_err() {
            tracing::warn!("walk index writer thread panicked");
        }
    }
}

fn write_loop(db: &Database, rx: &Receiver<WriterMsg>) {
    let mut batch: Vec<(String, Vec<u8>)> = Vec::new();
    let mut last_commit = Instant::now();
    loop {
        let msg = rx.recv_timeout(FLUSH_INTERVAL);
        let done = matches!(
            msg,
            Ok(WriterMsg::Finish) | Err(RecvTimeoutError::Disconnected)
        );
        if let Ok(WriterMsg::Record(key, value)) = msg {
            batch.push((key, value));
        }
        if done || batch.len() >= BATCH_DIRS || last_commit.elapsed() >= FLUSH_INTERVAL {
            commit(db, &mut batch);
            last_commit = Instant::now();
        }
        if done {
            return;
        }
    }
}

/// Best-effort, like the hash cache (ADR-0016): a failed commit costs the
/// next walk some `stat`s, never this scan.
fn commit(db: &Database, batch: &mut Vec<(String, Vec<u8>)>) {
    if batch.is_empty() {
        return;
    }
    let result = (|| -> Result<(), redb::Error> {
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(TABLE)?;
            for (key, value) in batch.iter() {
                table.insert(key.as_str(), value.as_slice())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    })();
    if let Err(err) = result {
        tracing::warn!(error = %err, dirs = batch.len(), "failed to write walk index batch");
    }
    batch.clear();
}

fn split_mtime(mtime: SystemTime) -> Option<(u64, u32)> {
    let since_epoch = mtime.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some((since_epoch.as_secs(), since_epoch.subsec_nanos()))
}

/// Layout: mtime secs (u64), nanos (u32), file count (u32), then per file:
/// name length (u32), UTF-8 name, size (u64), device (u64), inode (u64).
/// Only `FileId::Inode` is encodable (Unix); anything else isn't recorded.
fn encode(mtime: SystemTime, files: &DirFiles) -> Option<Vec<u8>> {
    let (secs, nanos) = split_mtime(mtime)?;
    let mut buf = Vec::with_capacity(16 + files.len() * 48);
    buf.extend_from_slice(&secs.to_le_bytes());
    buf.extend_from_slice(&nanos.to_le_bytes());
    buf.extend_from_slice(&u32::try_from(files.len()).ok()?.to_le_bytes());
    for (name, stat) in files {
        let FileId::Inode {
            device_id,
            inode_number,
        } = stat.file_id
        else {
            return None;
        };
        buf.extend_from_slice(&u32::try_from(name.len()).ok()?.to_le_bytes());
        buf.extend_from_slice(name.as_bytes());
        buf.extend_from_slice(&stat.size.to_le_bytes());
        buf.extend_from_slice(&device_id.to_le_bytes());
        buf.extend_from_slice(&inode_number.to_le_bytes());
    }
    Some(buf)
}

fn decode(mut buf: &[u8]) -> Option<((u64, u32), DirFiles)> {
    let secs = take_u64(&mut buf)?;
    let nanos = take_u32(&mut buf)?;
    let count = take_u32(&mut buf)? as usize;
    let mut files = HashMap::with_capacity(count.min(buf.len()));
    for _ in 0..count {
        let name_len = take_u32(&mut buf)? as usize;
        let name = std::str::from_utf8(take(&mut buf, name_len)?)
            .ok()?
            .to_owned();
        let size = take_u64(&mut buf)?;
        let file_id = FileId::new_inode(take_u64(&mut buf)?, take_u64(&mut buf)?);
        files.insert(name, FileStat { size, file_id });
    }
    buf.is_empty().then_some(((secs, nanos), files))
}

fn take<'a>(buf: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    if buf.len() < n {
        return None;
    }
    let (head, rest) = buf.split_at(n);
    *buf = rest;
    Some(head)
}

fn take_u32(buf: &mut &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(take(buf, 4)?.try_into().ok()?))
}

fn take_u64(buf: &mut &[u8]) -> Option<u64> {
    Some(u64::from_le_bytes(take(buf, 8)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> DirFiles {
        HashMap::from([
            (
                "a.jpg".to_owned(),
                FileStat {
                    size: 10,
                    file_id: FileId::new_inode(1, 100),
                },
            ),
            (
                "b.jpg".to_owned(),
                FileStat {
                    size: 20,
                    file_id: FileId::new_inode(1, 101),
                },
            ),
        ])
    }

    fn old_mtime() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::new(1_700_000_000, 123)
    }

    fn open_index(dir: &Path, reuse: bool) -> (WalkIndex, IndexWriter) {
        let db = Arc::new(Database::create(dir.join("index.redb")).unwrap());
        WalkIndex::open(db, reuse).unwrap()
    }

    #[test]
    fn encode_decode_round_trip() {
        let buf = encode(old_mtime(), &files()).unwrap();
        let (mtime, decoded) = decode(&buf).unwrap();
        assert_eq!(mtime, (1_700_000_000, 123));
        assert_eq!(decoded, files());
    }

    #[test]
    fn decode_rejects_truncated_and_trailing_bytes() {
        let buf = encode(old_mtime(), &files()).unwrap();
        assert!(decode(&buf[..buf.len() - 1]).is_none());
        let mut longer = buf.clone();
        longer.push(0);
        assert!(decode(&longer).is_none());
    }

    #[test]
    fn recorded_dir_is_found_only_with_the_same_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Path::new("/data/photos");
        let (index, writer) = open_index(tmp.path(), true);
        index.record(dir, old_mtime(), &files());
        let db = index.db.upgrade().unwrap();
        drop(index);
        writer.finish();

        let (index, writer) = WalkIndex::open(db, true).unwrap();
        assert_eq!(index.lookup(dir, old_mtime()), Some(files()));
        assert_eq!(
            index.lookup(dir, old_mtime() + Duration::from_nanos(1)),
            None
        );
        assert_eq!(index.lookup(Path::new("/data/other"), old_mtime()), None);
        drop(index);
        writer.finish();
    }

    #[test]
    fn rescan_all_never_reuses_a_record() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Path::new("/data/photos");
        let (index, writer) = open_index(tmp.path(), true);
        index.record(dir, old_mtime(), &files());
        let db = index.db.upgrade().unwrap();
        drop(index);
        writer.finish();

        let (index, writer) = WalkIndex::open(db, false).unwrap();
        assert_eq!(index.lookup(dir, old_mtime()), None);
        drop(index);
        writer.finish();
    }

    #[test]
    fn recently_modified_dir_is_not_recorded() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Path::new("/data/photos");
        let (index, writer) = open_index(tmp.path(), true);
        let now = SystemTime::now();
        index.record(dir, now, &files());
        let db = index.db.upgrade().unwrap();
        drop(index);
        writer.finish();

        let (index, writer) = WalkIndex::open(db, true).unwrap();
        assert_eq!(index.lookup(dir, now), None);
        drop(index);
        writer.finish();
    }
}
