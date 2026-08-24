use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use crossbeam_channel::{bounded, Sender};

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
    Full {
        path: PathBuf,
        reply: Sender<io::Result<Vec<u8>>>,
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
                            ReadJob::Full { path, reply } => {
                                let _ = reply.send(std::fs::read(&path));
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

    /// Reads the entire contents of `path`. Used for the full-hash stage.
    pub(crate) fn read_full(&self, path: &Path) -> io::Result<Vec<u8>> {
        let (reply_tx, reply_rx) = bounded(1);
        self.tx
            .as_ref()
            .expect("io pool not yet shut down")
            .send(ReadJob::Full {
                path: path.to_path_buf(),
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
    fn reads_full_file() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        let pool = IoPool::new(2);
        let bytes = pool.read_full(file.path()).unwrap();
        assert_eq!(bytes, b"hello world");
    }
}
