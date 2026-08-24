# ADR-0011: `Arc<Path>` for internal path storage

- Status: Accepted
- Date: 2026-08-24
- Related: ADR-0004 (engine API and data model, which flagged this)

## Context

ADR-0004 noted that v1's detection pipeline stores paths as owned
`PathBuf`s throughout its grouping stages
(`HashMap<u64, Vec<(PathBuf, Vec<PathBuf>)>>` and friends), and deferred
the more ambitious fix — prefix-compressed path storage, as fclones does —
until benchmark evidence showed it was the actual bottleneck on a
real multi-million-file tree. That evidence doesn't exist yet, and
building a compact trie-based path store is real complexity this project's
"don't add complexity speculatively" principle says to avoid without it.

Separately from prefix-compression, though, the pipeline clones the same
path repeatedly as a file moves through its stages: collapsed into a
hardlink-alias group, grouped by size, filtered through the partial-hash
stage, filtered again through the full-hash stage, and finally sorted into
a `DuplicateGroup`. Each of those `.clone()`s on a `PathBuf` is a fresh
heap allocation and byte copy of the whole path string. That's a real,
low-risk, non-speculative cost to cut — independent of whether
prefix-compression is ever worth doing.

## Decision

Represent every path carried through the detection pipeline as `Arc<Path>`
instead of `PathBuf`:

- `traversal::Candidate.path`
- `pipeline::FileGroup` (`(Arc<Path>, Vec<Arc<Path>>)`)
- `error::FileError.path`
- `model::DuplicateGroup.paths` (the public, on-the-wire type)

`Arc<Path>::clone()` is a refcount increment, not an allocation — every one
of the clone points above becomes effectively free. `ScanOptions`'s
`scan(root: impl Into<PathBuf>, ...)` entry point and `ScanError::InvalidRoot`
are deliberately left as plain `PathBuf`: they're each a single one-off
value (the caller's root argument), not something duplicated per file
across the pipeline, so there's nothing to gain and converting them would
only cost API ergonomics for embedders passing in a `String`/`&str`/`PathBuf`.

The `action` module's own types (`FileAction.path`, `ActionPlan.kept`,
`ApplyReport.succeeded`) are also left as `PathBuf`: they're built once per
confirmed duplicate group (bounded by group size, not tree size), the same
scale ADR-0009 already operates at, so there's no meaningful win from
threading `Arc<Path>` further into that module — only a wider diff. `plan()`
converts at the seam with `.to_path_buf()`.

## Consequences

- No new dependencies — `Arc<Path>` is entirely `std`.
- Public API change: `DuplicateGroup.paths` and `FileError.path` are now
  `Arc<Path>` rather than `PathBuf`. Both still deref to `Path` (`.display()`,
  `.file_name()`, sorting, etc. are unaffected), but code that pattern-matches
  or directly compares against a `PathBuf` (as this crate's own tests did)
  needs `.to_path_buf()`/`.as_ref()`/`.into()` at the boundary. No breaking
  change was needed in the CLI's user-facing behavior — confirmed by a
  manual smoke test (`--action delete` dry run against a real duplicate
  pair, unchanged output).
- This closes ADR-0004's "path storage" gap as far as it goes without
  benchmark evidence: redundant clone cost is gone, but prefix-compressed
  storage itself remains deliberately deferred, unchanged from ADR-0004's
  original reasoning. Not re-opening that question here.
