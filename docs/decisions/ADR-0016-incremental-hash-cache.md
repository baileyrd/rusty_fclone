# ADR-0016: Incremental full-hash cache via `redb`

- Status: Accepted
- Date: 2026-08-24
- Related: ADR-0001 (staged-hashing pipeline this caches into), ADR-0004
  (error-tolerance contract this follows for cache failures)

## Context

Re-scanning a tree that hasn't changed since the last run re-reads and
re-hashes every file from scratch every time, even though the expensive
part — the full-file read and xxh3 hash — produces the same result if the
file's content hasn't changed. A cheap, well-known way to detect "content
probably unchanged" without re-reading the file is to compare `(size,
mtime)` against a value recorded on a previous run: if either differs, the
file must be re-read; if both match, the file is *extremely* likely
unchanged (the same heuristic `make`, `rsync --checksum`'s faster sibling,
and most build-cache tools use), and mirror-caching the full-file hash
directly for a repeatedly-scanned tree (backups, media libraries, download
folders) skips the dominant cost of a re-scan.

## Decision

- **Embedded key-value store, not a new service or a general-purpose
  database**: `redb`, a pure-Rust embedded KV store with no C toolchain
  dependency (fits this project's existing dependency profile — jwalk,
  rayon, xxhash-rust are all pure Rust) and no separate server process to
  run or configure. A relational store (SQLite) would be genuine overkill
  for a single `(path) -> (size, mtime, hash)` lookup table with no query
  needs beyond exact-key get/put.
- **Opt-in via `--cache <path>` / `ScanOptions::cache_path: Option<PathBuf>`**,
  off by default — matches every other CLI flag in this project
  (`--verify`, `--follow-symlinks`, etc.), and caching means writing a file
  to disk, which shouldn't happen without being asked.
- **Cache only the full hash, not the partial hash**: the partial-hash
  stage only reads small sampled ranges (`partial_hash_sample_size`,
  default 16 KiB total) — cheap enough that caching it wouldn't
  meaningfully help, and skipping it entirely (see below) is a bigger win
  anyway. Caching exactly one value also keeps invalidation trivial: a hit
  is valid exactly when `(size, mtime)` match, independent of every other
  scan option (`--verify`, `--partial-hash-sample-size`, etc. don't affect
  what a file's full hash *is*).
- **A cache hit skips both partial-hash *and* full-hash for that file**:
  `process_size_group`'s full-hash stage checks the cache first; on a hit,
  the cached hash is used directly (no I/O at all for that file this
  scan). This is safe regardless of whether the file was small enough to
  skip partial-hashing or not — the cache check happens once, at the point
  where a real full hash would otherwise be computed.
- **`(size, mtime)` re-stat happens at cache-lookup time, not threaded
  through the pipeline's data model**: `Candidate`/`FileGroup` don't carry
  mtime — adding it would mean widening those types (already carefully
  tuned in ADR-0011/ADR-0012) for a value only the cache path needs. One
  extra `fs::metadata` call per file when `--cache` is set is negligible
  next to a full-file read, and exactly zero cost when caching is off
  (the check is skipped entirely, not just its result discarded).
- **Writes are batched per size-group, not per-file**: `redb` write
  transactions are exclusive, so opening/committing one per file inside a
  `rayon::into_par_iter()` closure would serialize hashing threads on a
  lock. Cache-miss results are instead collected as plain data during the
  parallel full-hash stage, then written in one transaction after
  `.collect()` — one write transaction per size-group rather than per scan
  (keeps groups independently streamable) or per file (would serialize).
- **A cache-open or cache-write failure degrades gracefully, never aborts
  the scan**: matches ADR-0004's error-tolerance stance. An unreadable
  cache path logs a warning and the scan proceeds with caching disabled
  for that run; a write failure logs a warning and that size-group's
  results simply won't be cached for next time.
- **Key is the path string** (not the platform file-id): the cache exists
  to speed up re-scanning the *same tree*, where the same logical file is
  found at the same path across runs. A renamed file is a correctly-handled
  cache miss (nothing wrong happens, just no speedup for that one file) —
  keying by file-id instead would need extra plumbing for no benefit this
  use case cares about.

## Consequences

- New dependency: `redb`, `rusty_fclone-core` only.
- `ScanOptions` gains a `cache_path: Option<PathBuf>` field; the CLI gains
  `--cache <path>`.
- Manually smoke-tested end-to-end: a cold run against two 5 MB duplicate
  files logs zero cache hits (both freshly hashed and cached); an
  immediately following warm run logs exactly two `full-hash cache hit`
  trace events (one per file) and still reports the correct duplicate
  group. A dedicated pipeline test (`a_changed_file_is_not_served_a_stale_cached_hash`)
  confirms a content change (and therefore mtime change) between two
  cached scans is never served a stale hash.
- Benchmark verification of the cache-off path was inconclusive in this
  environment: `cargo bench`'s comparison against its saved baseline
  swung between "+144% regressed" and "-6.8% improved" across consecutive
  runs of *identical* code, which is far too large a swing to reflect a
  real effect and instead reflects this sandboxed container's variable
  background load (consistent with ADR-0008's already-noted caveat about
  this environment's benchmark noise). Structurally, the cache-off path
  is unaffected: `cache: Option<&HashCache>` is `None` when `--cache` is
  omitted, so every cache-related branch (`cache.and_then(...)`,
  `if let (Some(cache), Some(stat)) = ...`, the post-collect write) is a
  cheap `None` check that short-circuits before doing any extra work —
  not benchmarked cleanly here, but not structurally in question either.
- No cache invalidation beyond `(size, mtime)`: a file whose content
  changes without its mtime changing (a contrived/adversarial case, or a
  filesystem that doesn't update mtime reliably) would be served a stale
  hash. This is the same trust model `make` and most incremental build
  tools already accept; not treated as a gap worth a stronger (and slower)
  content-based invalidation scheme for v1.
- Multiple concurrent `rusty-fclone --cache <path>` processes against the
  same cache file are not a design goal here — `redb` itself handles
  concurrent access safely (one writer, multiple readers), so this
  wouldn't corrupt the cache, but no attempt was made to verify or
  optimize for that scenario.
