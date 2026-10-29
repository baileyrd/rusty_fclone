# ADR-0037: Hash small files in disk order

- Status: Reverted 2026-10-29 (see "Outcome" below)
- Date: 2026-10-28
- Related: ADR-0001 (staged hashing), ADR-0016 (hash cache), ADR-0035
  (bounded-memory traversal)

## Context

Candidates reach hashing sorted by `(size, file_id)`, one size group at a
time. On the 22M-file NTFS drive, each small-file size group is spread
thinly across the whole file table. So every file costs a random seek for
its `stat` (the cache check), and another for its open and read. That holds
even on a cache hit. Measured there, the scan managed about 60-100 files/s
with the disk busy and the CPU idle, which works out to days for the whole
tree.

## Decision

- Small-file size groups (at or below `small_file_threshold`, all of which
  come before the first large one) are taken in batches of up to 2^18
  distinct files. Each batch is hashed in `file_id` order across its sizes,
  then each size is grouped by hash as before. On NTFS the file id is the
  file-table record number, and on ext4 it is the inode. Either tracks
  where the file's metadata (and, for tiny NTFS files, its data) lives.
- Within a batch, files are hashed 256 at a time, as rayon tasks, and the
  cache is committed after each chunk. This keeps the I/O in flight inside
  a small region of the disk and keeps what an interrupt can lose the same
  as before (ADR-0016).
- Large files keep the per-size-group path (partial hash first). Their
  cost is reading content rather than finding it, and there are far fewer
  of them.

## Consequences

- On a 20k-file test tree of interleaved small sizes, the median distance
  between consecutive opens fell from 2,559 to 65 inode ranks, about the
  width of one 256-file chunk. On the 22M-file drive a 2^18 batch puts
  consecutive files about 85 file-table records apart. That is a short
  forward hop instead of a full random seek. The speedup on the real drive
  is to be measured there.
- Results are unchanged; only the order of reads differs. Small-file
  groups are emitted per batch rather than per size.
- A batch holds about 40 MB of paths, on top of ADR-0035's bounds. A single
  size group larger than a batch is still held whole, as before.

## Amendment (2026-10-29): path order, not file-id order

Measured on the 22M-file drive, file-id order made no difference. v0.4.2
and v0.4.3 both managed about 10k read syscalls a minute, with the disk
seek-bound and the CPU idle. Each file's dominant cost, even on a cache
hit, is the `stat`. That resolves the path through each folder's index,
and those index blocks are scattered across the volume whatever order the
file-table records are visited in.

So a batch is now hashed in path order instead. Files that share a folder
are checked back to back, which means one read of that folder's index serves
all of them. On NTFS, records created together in one folder also tend to
sit together in the file table. Everything else in this decision stands:
the batch size, the 256-file chunks, and the large-file path. The file id
is no longer carried past hardlink collapse.

## Outcome (2026-10-29): reverted

Both orderings were measured on the 22M-file drive, against v0.4.2's
size-by-size hashing, at the same point in the hashing phase:

- v0.4.2 (size groups hashed concurrently): about 12.7k groups and 21-byte
  files 41 minutes into hashing.
- v0.4.3 (file-id order): 11-byte files after about 88 minutes, at about
  37-67 groups a minute.
- v0.4.4 (path order): 6.3k groups and 11-byte files after 60 minutes,
  using a quarter of v0.4.3's read syscalls.

Each file costs a metadata read whatever the order, so the order buys
little. What did cost something is the batch: it is hashed in 256-file
chunks one after another, while v0.4.2 hashed several size groups at once
and kept more metadata reads queued on the disk. The batching is removed
and hashing is back to v0.4.2's per-size-group path. The index-driven walk
(ADR-0036) stays.
