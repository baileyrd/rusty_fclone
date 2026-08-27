# ADR-0030: Opt-in perceptual image similarity (dHash, `image` crate, always separate from exact detection)

- Status: Accepted
- Date: 2026-08-27
- Related: ADR-0001 (xxh3-128 exact hashing, the "zero false positives"
  engine this deliberately never touches), ADR-0028 (`GUI-MEDIA-PREVIEW`,
  whose data-URI preview plumbing this reuses in the GUI), `docs/roadmap/
  DEDUP-GAP-IMPLEMENTATION-PLAN.md` (`DETECTION-PERCEPTUAL-IMAGES`, Phase 3,
  third and final unit — reverses `SYSTEM-ARCHITECTURE.md`'s "near-
  duplicate/fuzzy matching" v1 non-goal, explicitly marked reversible there)

## Context

`FCLONE-DETECTION-001`'s core guarantee is byte-identical content —
"zero false positives" — established in ADR-0001 and repeated throughout
this project's documentation as its actual differentiator. Near-duplicate
images (the same photo re-exported at a different quality, resized,
lightly cropped, or re-encoded to a different container) never share a
hash and so are structurally invisible to that engine, no matter how
obviously similar they are to a human. The gap-analysis research behind
this plan flags this as a real, named market gap this project doesn't
cover at all — but the plan is explicit that closing it must not dilute
the exact engine's own guarantee: "must stay opt-in and clearly separated
from the hash-verified exact engine." This is the largest, riskiest unit
in the plan, requiring its own dependency and its own ADR per `AGENTS.md`.

## Decision

- **A brand-new, independent pass — `find_similar_images` — never a
  change to `scan()`, `DuplicateGroup`, or `ScanEvent`.** Its output type,
  `SimilarGroup`, is structurally distinct from `DuplicateGroup` (no
  shared fields, no shared enum variant), so no caller in this codebase
  can accidentally treat a "looks similar" result as a "confirmed
  identical" one. This is the direct, load-bearing implementation of the
  plan's "clearly separated" requirement — it isn't just documentation,
  it's the type system.
- **The `image` crate, restricted to pure-Rust codecs, no C toolchain.**
  Image decoding (JPEG/PNG/GIF/BMP container and pixel-format handling)
  is exactly the kind of genuinely platform/format-specific complexity
  ADR-0024 already drew the line at for `trash` — not a small, hand-
  rollable transform like ADR-0028's base64 encoder. `image = { version =
  "0.25", default-features = false, features = ["jpeg", "png", "gif",
  "bmp"] }` was verified in a scratch project to pull in zero `-sys`/C-
  linked crates (every transitive dependency — `zune-jpeg`, `png`, `gif`,
  `flate2`/`miniz_oxide`, `weezl`, `color_quant`, `moxcms`, `pxfm` — is
  pure Rust), satisfying `AGENTS.md`'s "no dependency that requires a C
  toolchain" rule the same way ADR-0024's own scratch-project check did.
  `webp`/`avif` were deliberately excluded even though `image` supports
  them, since their decoders pull in C-linked codecs — narrower than
  `GUI-MEDIA-PREVIEW`'s preview support, which covers `webp`/`svg` for
  *display* purposes where that tradeoff doesn't apply the same way.
- **Difference hash (dHash), hand-rolled, not a perceptual-hashing
  crate.** Once an image is decoded to pixels, dHash itself — shrink to a
  9x8 grayscale grid, record whether each pixel is brighter than its
  right neighbor, 64 bits — is a small, fully-specified, easily-testable
  transform, the same category ADR-0028 already drew the line at for
  base64 (as opposed to genuinely complex, error-prone-to-reimplement
  logic like image *decoding* itself, which stays a dependency). Chosen
  over a simpler average-hash for being relatively robust to resizing and
  recompression while also capturing some structural information, not
  just overall brightness.
- **Its own traversal, not a consumer of `scan()`'s results.** Unlike
  `find_folder_duplicates` (which needs a completed scan's
  `DuplicateGroup`s to build its directory picture), perceptually similar
  images are by definition *not* byte-identical, so they'd never appear
  together in any `DuplicateGroup` in the first place — there's nothing
  for this pass to consume from a normal scan. It runs its own traversal,
  reusing `ScanOptions`'s traversal tunables (symlinks, filesystem
  boundary, size/exclude-path filters) but overriding
  `include_extensions`/clearing `exclude_extensions` to lock onto exactly
  the formats the enabled `image` codecs support.
- **Report-only — no `--action`/`--apply`/`run_action` interaction of any
  kind, anywhere.** A `SimilarGroup` never designates a "kept" path,
  never reaches `action::plan`, and the CLI's `--find-similar-images`
  flag has zero interaction with `--action`. This isn't a scope
  simplification worth revisiting later — it's the same reasoning
  ADR-0021 originally applied to folder matches before `FOLDER-ACTION`
  deliberately reversed it for the *exact* engine: unlike a folder
  match's "byte-identical, just at directory granularity" guarantee, a
  `SimilarGroup`'s members are explicitly *not* confirmed identical, so
  building a delete/trash/hardlink/reflink/move/copy action on top of it
  would let this project's actual destructive-action layer act on a
  probabilistic judgment for the first time — a materially different
  safety posture than everything else the action layer touches, not a
  detail to add later without deciding it deliberately.
- **Silent, best-effort tolerance for undecodable files — no per-file
  error reporting, unlike `scan()`/`find_folder_duplicates`.** A file
  that fails to decode (corrupt, truncated, or an extension mismatching
  its real content) is simply excluded from clustering, matching
  `find_folder_duplicates`'s own `build_tree` precedent (`|_err| {}`,
  documented as acceptable for a post-scan, best-effort pass). Adding a
  parallel `FileError`-based reporting channel for decode failures
  specifically (distinct from traversal-level I/O errors, which really
  are the same shape) was judged not worth the complexity for a first,
  opt-in version of an already-probabilistic feature.
- **Pairwise (O(n²)) clustering via union-find, not an approximate-
  nearest-neighbor index.** Finding every connected component under a
  Hamming-distance threshold is inherently a full pairwise comparison
  problem without a more sophisticated indexing structure (e.g. a BK-tree
  or LSH); building one is real engineering effort this project's
  existing benchmark-driven-optimization posture doesn't justify before
  real usage shows it's actually the bottleneck for a real photo
  library's size. A deliberate simplicity-over-scale tradeoff, explicit
  rather than silently accepted.
- **GUI: reuses the existing, previously-disabled "Similar content" seg-
  control on Scan Setup** (shipped disabled in `GUI-REDESIGN`, ADR-0022,
  with exactly this feature in mind) **rather than adding a new toggle.**
  Selecting it doesn't *replace* the exact scan — the mockup's segmented-
  control framing visually suggests an either/or mode switch, but this
  project's "opt-in, always alongside the exact engine" requirement rules
  that out; selecting "Similar content" runs `find_similar_images`
  *in addition to* the normal exact scan once it finishes, and Duplicate
  Review shows the resulting clusters as their own, visually distinct,
  read-only cards (a `var(--warning)`-tinted "not confirmed identical"
  banner, no keep-choice control, no action bar beyond "Skip") — a
  documented, deliberate deviation from the mockup's literal semantics,
  the same kind ADR-0022 already recorded several of.

## Consequences

- `rusty_fclone-core` gains a new `perceptual` module, `find_similar_images`,
  `PerceptualOptions`, and `SimilarGroup`; `image` becomes a direct
  dependency (pure-Rust codecs only, verified no C toolchain requirement).
  `rusty_fclone-cli`/`rusty_fclone-gui` each gain `image` as a
  *dev*-dependency only, for constructing real synthetic image files in
  tests — production CLI/GUI code never imports `image` directly, only
  `rusty_fclone_core::find_similar_images`.
- The CLI gains `--find-similar-images` and `--similarity-threshold
  <0-64>` (default 10), entirely independent of `--action`/`--apply`/
  `--history` — a similar-images run never appears in `--history`'s
  recorded data, matching its complete non-interaction with the action
  layer.
- The GUI's "Similar content" match-sensitivity option goes from
  permanently disabled to real; a first, deliberately conservative wiring
  with no similarity-threshold control exposed in the UI yet (always the
  10/64 default) — a narrower surface than the CLI, worth widening later
  if real usage wants it tunable from the GUI too.
- `FCLONE-DETECTION-001`'s "near-duplicate/fuzzy/perceptual matching" and
  `SYSTEM-ARCHITECTURE.md`'s "near-duplicate/fuzzy matching" v1 non-goals
  are both reversed — both had explicitly marked this as a possible,
  deliberately-deferred future direction, not a permanent boundary.
- A real risk this ADR doesn't resolve: dHash (like any perceptual hash)
  is not resistant to intentional distinguishing full false positives —
  a hostile or merely bad-luck image can hash close to an unrelated one.
  This project's "zero false positives" claim continues to apply only to
  the exact engine; `SimilarGroup`'s consumers (CLI text/JSON output, the
  GUI's read-only cards) are worded to never claim confirmed identity for
  perceptual results.
- Verified end-to-end in this environment: real synthetic JPEG/PNG photos
  (a base image, a brightness-shifted "re-export," and a resized
  thumbnail — all genuinely different byte content, decoded and
  re-encoded through the same `image` crate) correctly clustered together
  via the compiled CLI binary's `--find-similar-images`, while an
  unrelated photo was correctly excluded, and the exact engine
  simultaneously reported zero `DuplicateGroup`s for the same tree —
  concrete confirmation the two engines' outputs stay genuinely disjoint,
  not just structurally different in name.
