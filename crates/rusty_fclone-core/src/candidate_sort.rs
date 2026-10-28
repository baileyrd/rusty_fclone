//! Sorting traversal candidates by `(size, file_id)` in bounded memory
//! (ADR-0035).
//!
//! Candidates are buffered up to a fixed count; each full buffer is sorted
//! and spilled to an anonymous temp file as a "run", and [`finish`] merges
//! the runs (plus whatever is still buffered) back into one sorted stream.
//! A tree smaller than one buffer never touches the disk.
//!
//! [`finish`]: CandidateSorter::finish

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};

use file_id::FileId;

use crate::traversal::Candidate;

/// Candidates buffered before a spill: ~100-150 bytes each with its name,
/// so roughly 100-150 MB of buffer however large the tree is.
pub(crate) const DEFAULT_BUFFER: usize = 1 << 20;

/// A candidate read back in sorted order, with its full path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sorted {
    pub size: u64,
    pub file_id: FileId,
    pub path: PathBuf,
}

/// Collects candidates, spilling sorted runs to `spill_dir` as the buffer
/// fills.
pub(crate) struct CandidateSorter {
    buffer: Vec<Candidate>,
    limit: usize,
    spill_dir: PathBuf,
    runs: Vec<File>,
}

impl CandidateSorter {
    /// A sorter that spills every `limit` candidates into `spill_dir`.
    pub(crate) fn new(spill_dir: PathBuf, limit: usize) -> Self {
        Self {
            buffer: Vec::new(),
            limit: limit.max(1),
            spill_dir,
            runs: Vec::new(),
        }
    }

    /// Adds `candidate`, spilling the buffer once it's full. On a spill
    /// error the candidates stay buffered (the scan carries on, in memory)
    /// and the error is returned for the caller to report.
    pub(crate) fn push(&mut self, candidate: Candidate) -> io::Result<()> {
        self.buffer.push(candidate);
        if self.buffer.len() < self.limit {
            return Ok(());
        }
        sort(&mut self.buffer);
        let run = write_run(&self.spill_dir, &self.buffer)?;
        self.runs.push(run);
        self.buffer.clear();
        Ok(())
    }

    /// Every candidate pushed, in `(size, file_id)` order.
    pub(crate) fn finish(mut self) -> io::Result<Merge> {
        sort(&mut self.buffer);
        let mut sources: Vec<Source> = Vec::with_capacity(self.runs.len() + 1);
        for mut run in self.runs {
            run.rewind()?;
            sources.push(Source::Run(BufReader::new(run)));
        }
        sources.push(Source::Buffer(self.buffer.into_iter()));
        Merge::new(sources)
    }
}

fn sort(candidates: &mut [Candidate]) {
    candidates.sort_unstable_by_key(|c| (c.size, c.file_id));
}

/// Writes `sorted` to a fresh anonymous temp file. The file has no name on
/// Unix (deleted on Windows when closed), so nothing is left behind even if
/// the process is killed.
fn write_run(dir: &Path, sorted: &[Candidate]) -> io::Result<File> {
    let mut writer = BufWriter::new(tempfile::tempfile_in(dir)?);
    for candidate in sorted {
        write_record(&mut writer, candidate)?;
    }
    writer.into_inner().map_err(io::IntoInnerError::into_error)
}

/// One input to the merge: a spilled run, or the final in-memory buffer.
enum Source {
    Run(BufReader<File>),
    Buffer(std::vec::IntoIter<Candidate>),
}

impl Source {
    fn next(&mut self) -> io::Result<Option<Sorted>> {
        match self {
            Self::Run(reader) => read_record(reader),
            Self::Buffer(iter) => Ok(iter.next().map(|c| Sorted {
                size: c.size,
                file_id: c.file_id,
                path: c.path(),
            })),
        }
    }
}

/// K-way merge of sorted sources. Yields an error at most once, then ends.
pub(crate) struct Merge {
    sources: Vec<Source>,
    heap: BinaryHeap<Reverse<Head>>,
    failed: bool,
}

/// The next record of one source, ordered by its sort key alone.
struct Head {
    key: (u64, FileId),
    source: usize,
    record: Sorted,
}

impl PartialEq for Head {
    fn eq(&self, other: &Self) -> bool {
        (self.key, self.source) == (other.key, other.source)
    }
}

impl Eq for Head {}

impl PartialOrd for Head {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Head {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.key, self.source).cmp(&(other.key, other.source))
    }
}

impl Merge {
    fn new(sources: Vec<Source>) -> io::Result<Self> {
        let mut merge = Self {
            heap: BinaryHeap::with_capacity(sources.len()),
            sources,
            failed: false,
        };
        for source in 0..merge.sources.len() {
            merge.refill(source)?;
        }
        Ok(merge)
    }

    fn refill(&mut self, source: usize) -> io::Result<()> {
        if let Some(record) = self.sources[source].next()? {
            let key = (record.size, record.file_id);
            self.heap.push(Reverse(Head {
                key,
                source,
                record,
            }));
        }
        Ok(())
    }
}

impl Iterator for Merge {
    type Item = io::Result<Sorted>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        let Reverse(head) = self.heap.pop()?;
        if let Err(err) = self.refill(head.source) {
            self.failed = true;
            return Some(Err(err));
        }
        Some(Ok(head.record))
    }
}

// Record layout, little-endian: size u64, file id (tag u8 + fields),
// path length u32, path bytes. Only ever read back by this same process.

fn write_record(out: &mut impl Write, candidate: &Candidate) -> io::Result<()> {
    out.write_all(&candidate.size.to_le_bytes())?;
    write_file_id(out, &candidate.file_id)?;
    let path = path_bytes(&candidate.path());
    let len = u32::try_from(path.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path too long to spill"))?;
    out.write_all(&len.to_le_bytes())?;
    out.write_all(&path)
}

/// The next record, or `None` at a clean end of input.
fn read_record(input: &mut impl Read) -> io::Result<Option<Sorted>> {
    let mut size = [0; 8];
    match input.read_exact(&mut size) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err),
    }
    let file_id = read_file_id(input)?;
    let mut path = vec![0; read_u32(input)? as usize];
    input.read_exact(&mut path)?;
    Ok(Some(Sorted {
        size: u64::from_le_bytes(size),
        file_id,
        path: path_from_bytes(path)?,
    }))
}

fn write_file_id(out: &mut impl Write, id: &FileId) -> io::Result<()> {
    match *id {
        FileId::Inode {
            device_id,
            inode_number,
        } => {
            out.write_all(&[0])?;
            out.write_all(&device_id.to_le_bytes())?;
            out.write_all(&inode_number.to_le_bytes())
        }
        FileId::LowRes {
            volume_serial_number,
            file_index,
        } => {
            out.write_all(&[1])?;
            out.write_all(&volume_serial_number.to_le_bytes())?;
            out.write_all(&file_index.to_le_bytes())
        }
        FileId::HighRes {
            volume_serial_number,
            file_id,
        } => {
            out.write_all(&[2])?;
            out.write_all(&volume_serial_number.to_le_bytes())?;
            out.write_all(&file_id.to_le_bytes())
        }
    }
}

fn read_file_id(input: &mut impl Read) -> io::Result<FileId> {
    let mut tag = [0; 1];
    input.read_exact(&mut tag)?;
    match tag[0] {
        0 => Ok(FileId::new_inode(read_u64(input)?, read_u64(input)?)),
        1 => Ok(FileId::new_low_res(read_u32(input)?, read_u64(input)?)),
        2 => {
            let volume = read_u64(input)?;
            let mut file_id = [0; 16];
            input.read_exact(&mut file_id)?;
            Ok(FileId::new_high_res(volume, u128::from_le_bytes(file_id)))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "corrupt spill record",
        )),
    }
}

fn read_u32(input: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(input: &mut impl Read) -> io::Result<u64> {
    let mut bytes = [0; 8];
    input.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn path_from_bytes(bytes: Vec<u8>) -> io::Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(windows)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(windows)]
fn path_from_bytes(bytes: Vec<u8>) -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    if bytes.len() % 2 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "corrupt spill record",
        ));
    }
    let wide: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    Ok(PathBuf::from(OsString::from_wide(&wide)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::sync::Arc;

    fn candidate(name: &str, size: u64, ino: u64) -> Candidate {
        Candidate {
            parent: Arc::from(Path::new("/root")),
            name: Box::from(OsStr::new(name)),
            size,
            file_id: FileId::new_inode(1, ino),
        }
    }

    fn sorted_all(limit: usize, candidates: Vec<Candidate>) -> (usize, Vec<Sorted>) {
        let dir = tempfile::tempdir().unwrap();
        let mut sorter = CandidateSorter::new(dir.path().to_path_buf(), limit);
        for c in candidates {
            sorter.push(c).unwrap();
        }
        let runs = sorter.runs.len();
        let out = sorter.finish().unwrap().collect::<io::Result<_>>().unwrap();
        (runs, out)
    }

    #[test]
    fn merges_spilled_runs_with_the_buffer_in_sort_order() {
        let candidates: Vec<Candidate> = (0..50u64)
            .map(|i| candidate(&format!("f{i}"), (i * 37) % 11, i))
            .collect();
        let (runs, out) = sorted_all(7, candidates);

        assert_eq!(runs, 7);
        assert_eq!(out.len(), 50);
        let keys: Vec<(u64, FileId)> = out.iter().map(|s| (s.size, s.file_id)).collect();
        let mut expected = keys.clone();
        expected.sort_unstable();
        assert_eq!(keys, expected);
        let f3 = out
            .iter()
            .find(|s| s.file_id == FileId::new_inode(1, 3))
            .unwrap();
        assert_eq!(f3.path, Path::new("/root/f3"));
        assert_eq!(f3.size, (3 * 37) % 11);
    }

    #[test]
    fn small_input_never_spills() {
        let (runs, out) = sorted_all(10, vec![candidate("b", 2, 2), candidate("a", 1, 1)]);
        assert_eq!(runs, 0);
        assert_eq!(out[0].path, Path::new("/root/a"));
        assert_eq!(out[1].path, Path::new("/root/b"));
    }

    #[test]
    fn empty_input_yields_nothing() {
        let (runs, out) = sorted_all(4, Vec::new());
        assert_eq!((runs, out.len()), (0, 0));
    }

    #[test]
    fn every_file_id_kind_round_trips() {
        let ids = [
            FileId::new_inode(u64::MAX, 7),
            FileId::new_low_res(9, u64::MAX),
            FileId::new_high_res(3, u128::MAX - 1),
        ];
        for id in ids {
            let mut bytes = Vec::new();
            write_file_id(&mut bytes, &id).unwrap();
            assert_eq!(read_file_id(&mut bytes.as_slice()).unwrap(), id);
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_names_survive_a_spill() {
        use std::os::unix::ffi::OsStrExt;
        let name = OsStr::from_bytes(b"caf\xe9");
        let c = Candidate {
            name: Box::from(name),
            ..candidate("x", 5, 1)
        };
        let (runs, out) = sorted_all(1, vec![c]);
        assert_eq!(runs, 1);
        assert_eq!(out[0].path, Path::new("/root").join(name));
    }

    #[test]
    fn truncated_run_is_an_error_not_a_silent_end() {
        let mut bytes = Vec::new();
        write_record(&mut bytes, &candidate("a", 1, 1)).unwrap();
        bytes.truncate(bytes.len() - 1);
        assert!(read_record(&mut bytes.as_slice()).is_err());
    }
}
