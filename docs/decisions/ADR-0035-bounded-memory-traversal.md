# ADR-0035: Bounded-memory traversal: spill sorted candidates, bounded walk queue

- Status: Accepted
- Date: 2026-10-28
- Related: ADR-0012 (traversal before hashing), ADR-0033 (compact
  traversal storage), ADR-0034 (durable walk index)

## Context

ADR-0033 cut traversal storage to ~75 B per file but kept it O(files): a
~16.9M-file, ~20 TB tree was still OOM-killed mid-scan. Measured on a
synthetic 4M-file tree (peak RSS, `--cache`), two things grew with the tree:

- The candidate list, then a full `Arc<Path>` for every file in a shared
  size, all built before hashing began.
- jwalk's own result queue. It is ordered and unbounded, so when its
  workers outrun the consuming loop (as on a re-run, where the walk index
  makes listing nearly free) it buffers most of the tree's `DirEntry`s:
  ~430 MB of the 4M-file re-run's 700 MB peak.

## Decision

- `candidate_sort::CandidateSorter` buffers up to 2^20 candidates (about
  100-150 MB). When the buffer fills, it sorts it by `(size, file_id)` and
  writes it to an anonymous temp file. After the walk, a k-way merge streams
  every candidate back in sorted order. A tree under one buffer never
  touches the disk.
- Runs go next to the `--cache` file when there is one, so a tmpfs `/tmp`
  doesn't hold them in RAM, and to the system temp directory otherwise.
  `tempfile::tempfile_in` gives unnamed files on Unix, so a killed run
  leaves nothing behind. This moves `tempfile` from dev-dependencies to
  dependencies.
- Size groups are built lazily from the merged stream and fed to hashing
  through `par_bridge`. Only the groups being hashed hold full paths.
- jwalk workers build candidates inside `process_read_dir` and send them
  over a bounded channel of 2^14 entries, then drop the files from the
  listing. jwalk's ordered queue then carries only directories, and a slow
  consumer stalls the workers instead of growing memory. Order doesn't
  matter, since candidates are sorted afterwards.
- The walk index's writer queue (ADR-0034) is bounded at 2,048 directories
  for the same reason.
- With `-v`, the scan logs how many directories the walk index reused and
  how many it stat-ed, so a resumed run is visible.

## Consequences

- On a 4M-file tree, peak RSS fell from 736 MB to 248 MB on a fresh
  `--cache` run and from 732 MB to 208 MB on a re-run. Peak no longer
  grows with file count, except for the largest single size group and the
  folder-dedup and plan paths, which collect duplicate groups by design.
- Disk use while sorting is about 30 B plus the path length per file,
  roughly 2 GB for 16.9M files. If a spill fails (for example, the disk is
  full), the scan logs a warning and keeps candidates in memory, as before.

## Addendum (2026-10-28): hashing progress

On the 22M-file drive the walk finished and the scan then sat on its last
`scanning...` line, at near-zero CPU, while it read small files one seek
at a time; nothing printed until a whole size group was confirmed. Hashing
now emits `ScanEvent::HashProgress` (file reads and bytes read, from
`IoPool` counters) about once a second until `Finished`. Empty files are
grouped without being read, since they're identical by definition.
