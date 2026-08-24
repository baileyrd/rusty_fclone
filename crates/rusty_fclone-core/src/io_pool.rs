use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use crossbeam_channel::{bounded, Sender};
use xxhash_rust::xxh3::Xxh3;

/// Chunk size used to stream a file through hashing or byte-comparison
/// without ever buffering the whole thing (ADR-0002 addendum: this closes
/// the "full-file hashing buffers the whole file in memory" gap noted when
/// that ADR was written).
const STREAM_CHUNK_SIZE: usize = 1024 * 1024;

/// A fixed-size pool of blocking I/O worker threads, kept deliberately
/// separate from the CPU-bound rayon pool used for hashing (ADR-0002).
///
/// Oversubscribing this pool relative to core count lets it keep more read
/// requests in flight than there are CPUs, hiding per-request latency
/// instead of serializing on it.
pub(crate) struct IoPool {
    tx: Option<Sender<ReadJob>>,
    workers: Vec<JoinHandle<()>>,
}

enum ReadJob {
    Ranges {
        path: PathBuf,
        ranges: Vec<(u64, usize)>,
        reply: Sender<io::Result<Vec<u8>>>,
    },
    HashFull {
        path: PathBuf,
        reply: Sender<io::Result<u128>>,
    },
    FilesEqual {
        a: PathBuf,
        b: PathBuf,
        reply: Sender<io::Result<bool>>,
    },
}

impl IoPool {
    pub(crate) fn new(threads: usize) -> Self {
        let threads = threads.max(1);
        let (tx, rx) = bounded::<ReadJob>(threads * 4);
        let workers = (0..threads)
            .map(|_| {
                let rx = rx.clone();
                std::thread::spawn(move || {
                    for job in rx.iter() {
                        match job {
                            ReadJob::Ranges {
                                path,
                                ranges,
                                reply,
                            } => {
                                let _ = reply.send(read_ranges(&path, &ranges));
                            }
                            ReadJob::HashFull { path, reply } => {
                                let _ = reply.send(hash_full_file(&path));
                            }
                            ReadJob::FilesEqual { a, b, reply } => {
                                let _ = reply.send(files_equal(&a, &b));
                            }
                        }
                    }
                })
            })
            .collect();
        Self {
            tx: Some(tx),
            workers,
        }
    }

    /// Reads and concatenates each `(offset, length)` range from `path`, in
    /// the order given. Used for the partial-hash stage's head/mid/tail
    /// samples.
    pub(crate) fn read_ranges(
        &self,
        path: &Path,
        ranges: Vec<(u64, usize)>,
    ) -> io::Result<Vec<u8>> {
        let (reply_tx, reply_rx) = bounded(1);
        self.tx
            .as_ref()
            .expect("io pool not yet shut down")
            .send(ReadJob::Ranges {
                path: path.to_path_buf(),
                ranges,
                reply: reply_tx,
            })
            .expect("io pool workers alive");
        reply_rx.recv().expect("io worker replies")
    }

    /// Hashes the entire contents of `path` with xxh3-128, streaming in
    /// fixed-size chunks rather than buffering the whole file. Used for the
    /// full-hash stage.
    pub(crate) fn hash_full_file(&self, path: &Path) -> io::Result<u128> {
        let (reply_tx, reply_rx) = bounded(1);
        self.tx
            .as_ref()
            .expect("io pool not yet shut down")
            .send(ReadJob::HashFull {
                path: path.to_path_buf(),
                reply: reply_tx,
            })
            .expect("io pool workers alive");
        reply_rx.recv().expect("io worker replies")
    }

    /// Byte-compares `a` and `b` in full, streaming in fixed-size chunks
    /// and short-circuiting on the first mismatch, rather than buffering
    /// both files. Used by `--verify`.
    pub(crate) fn files_equal(&self, a: &Path, b: &Path) -> io::Result<bool> {
        let (reply_tx, reply_rx) = bounded(1);
        self.tx
            .as_ref()
            .expect("io pool not yet shut down")
            .send(ReadJob::FilesEqual {
                a: a.to_path_buf(),
                b: b.to_path_buf(),
                reply: reply_tx,
            })
            .expect("io pool workers alive");
        reply_rx.recv().expect("io worker replies")
    }
}

impl Drop for IoPool {
    fn drop(&mut self) {
        // Drop the sender first so workers' `rx.iter()` loops see the
        // channel close and exit, then join them for a clean shutdown.
        self.tx.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn read_ranges(path: &Path, ranges: &[(u64, usize)]) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut buf = Vec::new();
    for &(offset, len) in ranges {
        file.seek(SeekFrom::Start(offset))?;
        let mut chunk = vec![0u8; len];
        let read = read_up_to(&mut file, &mut chunk)?;
        chunk.truncate(read);
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

fn hash_full_file(path: &Path) -> io::Result<u128> {
    let mut file = File::open(path)?;
    let mut hasher = Xxh3::new();
    let mut chunk = vec![0u8; STREAM_CHUNK_SIZE];
    loop {
        let read = read_up_to(&mut file, &mut chunk)?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
    }
    Ok(hasher.digest128())
}

fn files_equal(a: &Path, b: &Path) -> io::Result<bool> {
    let mut file_a = File::open(a)?;
    let mut file_b = File::open(b)?;
    let mut chunk_a = vec![0u8; STREAM_CHUNK_SIZE];
    let mut chunk_b = vec![0u8; STREAM_CHUNK_SIZE];
    loop {
        let read_a = read_up_to(&mut file_a, &mut chunk_a)?;
        let read_b = read_up_to(&mut file_b, &mut chunk_b)?;
        if read_a != read_b {
            return Ok(false); // different lengths
        }
        if read_a == 0 {
            return Ok(true); // both hit EOF at the same point, never differed
        }
        if chunk_a[..read_a] != chunk_b[..read_b] {
            return Ok(false);
        }
    }
}

/// Reads up to `buf.len()` bytes, returning `0` only at true EOF (loops
/// until either the buffer fills or the source is exhausted, since a single
/// `Read::read` call is allowed to return a short read before EOF).
fn read_up_to(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match file.read(&mut buf[total..])? {
            0 => break,
            n => total += n,
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_ranges_in_order() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"0123456789").unwrap();
        let pool = IoPool::new(2);
        let bytes = pool.read_ranges(file.path(), vec![(0, 3), (7, 3)]).unwrap();
        assert_eq!(bytes, b"012789");
    }

    #[test]
    fn hashes_full_file_matching_hash_chunks() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        let pool = IoPool::new(2);
        let streamed = pool.hash_full_file(file.path()).unwrap();
        let expected = crate::hash::hash_chunks(&[b"hello world"]);
        assert_eq!(streamed, expected);
    }

    #[test]
    fn hashes_a_file_larger_than_the_stream_chunk_size() {
        let content = vec![0x5Au8; STREAM_CHUNK_SIZE * 2 + 137];
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&content).unwrap();
        let pool = IoPool::new(2);
        let streamed = pool.hash_full_file(file.path()).unwrap();
        let expected = crate::hash::hash_chunks(&[&content]);
        assert_eq!(streamed, expected);
    }

    #[test]
    fn files_equal_true_for_identical_content() {
        let mut a = tempfile::NamedTempFile::new().unwrap();
        let mut b = tempfile::NamedTempFile::new().unwrap();
        a.write_all(b"same content").unwrap();
        b.write_all(b"same content").unwrap();
        let pool = IoPool::new(2);
        assert!(pool.files_equal(a.path(), b.path()).unwrap());
    }

    #[test]
    fn files_equal_false_for_different_content_same_length() {
        let mut a = tempfile::NamedTempFile::new().unwrap();
        let mut b = tempfile::NamedTempFile::new().unwrap();
        a.write_all(b"aaaaa").unwrap();
        b.write_all(b"aaaab").unwrap();
        let pool = IoPool::new(2);
        assert!(!pool.files_equal(a.path(), b.path()).unwrap());
    }

    #[test]
    fn files_equal_false_for_different_lengths() {
        let mut a = tempfile::NamedTempFile::new().unwrap();
        let mut b = tempfile::NamedTempFile::new().unwrap();
        a.write_all(b"short").unwrap();
        b.write_all(b"a bit longer").unwrap();
        let pool = IoPool::new(2);
        assert!(!pool.files_equal(a.path(), b.path()).unwrap());
    }

    #[test]
    fn files_equal_across_the_stream_chunk_boundary() {
        let mut a_content = vec![1u8; STREAM_CHUNK_SIZE + 10];
        let mut b_content = a_content.clone();
        // Differ only in the very last byte, past the first chunk boundary.
        a_content[STREAM_CHUNK_SIZE + 9] = 1;
        b_content[STREAM_CHUNK_SIZE + 9] = 2;
        let mut a = tempfile::NamedTempFile::new().unwrap();
        let mut b = tempfile::NamedTempFile::new().unwrap();
        a.write_all(&a_content).unwrap();
        b.write_all(&b_content).unwrap();
        let pool = IoPool::new(2);
        assert!(!pool.files_equal(a.path(), b.path()).unwrap());
    }
}
