# ADR-0018: Prebuilt release binaries via a tag-triggered workflow

- Status: Accepted
- Date: 2026-08-24
- Related: none (first release-infrastructure ADR; the existing `ci.yml`
  only runs `fmt`/`clippy`/`test`/`bench --no-run` on push/PR, it does not
  build or publish anything)

## Context

`v0.1.0` was tagged and its GitHub Release created manually, but the
release shipped with no attached assets — nothing in the repository
actually builds a binary and attaches it to a release. Anyone wanting to
run `rusty-fclone` without a Rust toolchain had no way to get one.

## Decision

- **A separate `release.yml` workflow, not an extension of `ci.yml`**:
  `ci.yml`'s job is fast feedback on every push/PR (lint, test, bench
  compile-check); building release binaries for four platforms is a
  slower, different-purpose job that should only run when a release is
  actually being cut, not on every commit.
- **Triggers: `push: tags: v*` and `workflow_dispatch` with an optional
  `tag` input**: the tag push is the normal path for every future
  release. `workflow_dispatch` exists specifically so `v0.1.0` (already
  tagged and released before this workflow existed) can be built and
  attached retroactively — but GitHub resolves a workflow's available
  triggers (including `workflow_dispatch`) from the workflow file *as it
  exists on the ref being dispatched*, so `v0.1.0`'s tree (which predates
  this file entirely) can't be dispatched from directly. The `tag` input
  lets the run be dispatched from `main` (where `release.yml` exists)
  while still building the `v0.1.0` source tree (via an explicit
  `actions/checkout` `ref:`) and attaching to the `v0.1.0` release (via
  `RELEASE_TAG = inputs.tag || github.ref_name`, used for the checkout
  ref, archive names, and `softprops/action-gh-release`'s `tag_name`).
  This was caught only after attempting the first real dispatch against
  `v0.1.0` directly and hitting "Workflow does not have 'workflow_dispatch'
  trigger" — not something a local YAML-syntax check could have caught.
- **Four targets**: `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`,
  `x86_64-apple-darwin`, `x86_64-pc-windows-msvc` — one per major desktop
  platform this project already claims to be cross-platform for (per
  `FCLONE-DETECTION-001`'s device-aware I/O sizing and the traversal
  layer's Windows/Unix path handling). No Linux ARM target yet; add one
  if there's real demand, not speculatively.
- **`softprops/action-gh-release`, not a hand-rolled `gh release upload`
  step**: it appends assets to whatever release already exists for the
  tag (so it works against the pre-existing `v0.1.0` release without
  touching its title/body) or creates one if none exists (so future tags
  don't need a separate manual release-creation step first).
- **Packaged as an archive with `README.md` + both license files, not a
  bare binary**: matches how most Rust CLI tools ship (`ripgrep`, `fd`,
  etc.) and means a downloaded archive is self-contained.
- **`cargo build --locked`**: pins to the committed `Cargo.lock` so a
  release binary's dependency versions are reproducible, not whatever the
  latest compatible versions happen to resolve to on build day.

## Consequences

- New file: `.github/workflows/release.yml`. No source code changes.
- `rusqlite`'s `bundled` feature needs a C toolchain at build time; this
  is expected to work out of the box on all four GitHub-hosted runners
  (`ubuntu-latest`, `macos-latest`, `windows-latest` all ship one), but
  this hasn't been exercised in this environment before merging — the
  first real workflow run (manually dispatched against `v0.1.0` right
  after merge) is the actual verification, not a local guess.
- Every future `vX.Y.Z` tag push now automatically produces four
  platform archives on the release; no manual build/upload step needed
  going forward.
- Not covered: code signing/notarization (macOS Gatekeeper, Windows
  SmartScreen) — downloaded binaries will show an "unidentified
  developer" warning on first run. Out of scope for a v0.1.0-era hobby
  project; revisit if this becomes something people install broadly.
