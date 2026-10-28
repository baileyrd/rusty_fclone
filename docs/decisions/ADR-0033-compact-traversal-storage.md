# ADR-0033: Compact traversal storage; full paths only for shared sizes

- Status: Accepted (storage bounded by ADR-0035)
- Date: 2026-10-27
- Related: ADR-0004 and ADR-0011 (deferred prefix-compressed path storage
  "without benchmark evidence"), ADR-0012 (fused traversal + hardlink
  collapse into one `HashMap` fold), ADR-0016 (redb hash cache)

## Context

A real scan of a ~1.3M-file tree was OOM-killed mid-traversal, at a
slightly different file count each run. Until traversal finishes, every
file lives in `by_file_id: HashMap<FileId, (u64, Vec<Arc<Path>>)>`: one
hash-map slot, one `Vec` allocation, and one full-path `Arc<Path>` per
file. Measured on a synthetic 400k-file tree (peak RSS, Linux):

| | old | this ADR |
|---|---|---|
| traversal storage per file | ~575 B | ~75 B |
| peak RSS, mostly-unique sizes | 302 MB | 102 MB |
| wall time | 2.5 s | 1.0 s |

At 1.3M files that is ~800 MB for the file list alone, before hashing.

## Decision

- `traversal::Candidate` stores `parent: Arc<Path>` (jwalk already shares
  one per directory) plus `name: Box<OsStr>`, not a full path.
  `Candidate::path()` joins on demand.
- `pipeline::run_scan` collects candidates into a flat `Vec<Candidate>`,
  then `size_groups` sorts by `(size, file_id)` in place and walks the
  runs: sizes held by one distinct file are dropped, hardlink aliases are
  collapsed, and a full `Arc<Path>` is built only for the survivors. This
  replaces ADR-0012's per-file `HashMap` fold; the redundant-pass concern
  there doesn't apply, since the one extra pass is an in-place sort of
  small fixed-size records rather than a second map build.
- On Unix, `FileId` comes from the metadata traversal already fetched
  (`dev`, `ino`), not a second `stat` via `file_id::get_file_id`. That
  halves `stat` calls per file, which matters most on FUSE mounts (ntfs-3g).
- The redb cache's in-memory page cache is capped at 64 MiB instead of
  redb's 1 GiB default.

## Consequences

- Memory during traversal scales with directory count plus file-name
  bytes, not full-path bytes. Files of a shared size still get a full
  path, as before, since hashing needs one.
- No public API change; `ScanEvent`/`DuplicateGroup` are unchanged.
- Hashing still waits for traversal to finish (ADR-0012), so very large
  trees still hold O(files) in memory. A 16.9M-file tree needed spilling
  to disk; see ADR-0035.
