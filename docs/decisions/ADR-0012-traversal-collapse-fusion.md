# ADR-0012: Fuse traversal and hardlink-collapse into one streaming pass

- Status: Accepted
- Date: 2026-08-24
- Related: ADR-0002 (noted the streaming-overlap scope cut), ADR-0004
  (streaming API + finality contract this ADR deliberately doesn't touch)

## Context

`DETECTION-STREAMING-OVERLAP` was tracked on the roadmap as: "hashing
begins before traversal finishes (full pipeline overlap)." Before this
change, `pipeline::run_scan` ran three full passes over the file set before
any hashing started: `traversal::traverse` walked the whole tree and
collected every file into a `Vec<Candidate>`; `run_scan` then looped over
that `Vec` to collapse hardlink aliases into a `HashMap<FileId, ...>`; then
looped over *that* to group representatives by size.

Full pipeline overlap — starting to hash a file the moment it's found,
concurrently with traversal still walking unvisited subtrees — runs into a
real design problem: `ScanEvent`'s contract (ADR-0004) guarantees a
`DuplicateGroup`, once emitted, is never revised. But a group can only be
known *complete* once every file of its size has been seen, and traversal
is the only thing that knows when that's true. Starting to hash before
traversal finishes means a group could be emitted, and then traversal
finds one more same-size, same-content file deep in a subtree it hadn't
reached yet — which should have been in that group. Honoring the finality
contract as written means either delaying every emission until traversal
fully completes (defeating the purpose) or redesigning the contract itself
(revisable groups, or an explicit "traversal complete" boundary event) —
a bigger decision than this unit's scope, and not one to make as a side
effect of a performance pass.

Given that, the real, uncontroversial win available today is the redundant
*passes*, not the redundant *wait*: traversal already produces candidates
one at a time (`for entry in walker`); there's no reason to buffer them
into a `Vec` just to immediately loop over that `Vec` once more to build
the same `HashMap` a direct fold could build in the first pass.

## Decision

- `traversal::traverse` no longer returns `Vec<Candidate>`. It takes an
  `on_candidate: impl FnMut(Candidate)` callback (alongside the existing
  `on_error` callback) and calls it once per file as jwalk produces it.
- `pipeline::run_scan` folds the hardlink-collapse step (building
  `HashMap<FileId, (size, Vec<Arc<Path>>)>`) directly inside
  `on_candidate`, and updates `ScanSummary`'s `files_scanned`/
  `bytes_scanned` counters there too — both were previously computed with
  a separate `.len()`/`.iter().map(...).sum()` pass over the `Vec`.
- Size-grouping (`by_file_id.into_values()` → `by_size`) stays a separate
  loop: it can only run once every alias of a file-id is known, i.e. after
  traversal completes, and it's bounded by *distinct file* count rather
  than total path count, so there's nothing to fuse it into.
- Hashing still starts only after `traverse()` returns. This ADR does not
  implement `DETECTION-STREAMING-OVERLAP`'s original "hashing begins
  before traversal finishes" — see the roadmap entry for why that's kept
  open as a separate, larger unit pending a finality-contract redesign.

## Consequences

- One fewer full pass over the file set, and no intermediate
  `Vec<Candidate>` held in memory for the duration of a scan (previously
  sized to the whole tree, now nothing — candidates are consumed and
  dropped as they're folded into `by_file_id`).
- `ScanEvent`'s streaming/finality contract (ADR-0004) is unchanged;
  nothing about consumer-visible behavior changes. Confirmed via the full
  test suite (42/42) and a manual CLI smoke test with unchanged output.
- `DETECTION-STREAMING-OVERLAP` remains open on the roadmap as the bigger,
  separate unit: actually overlapping hashing with traversal, which
  requires deciding how (or whether) to relax `ScanEvent`'s finality
  guarantee first.
