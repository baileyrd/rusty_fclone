# ADR-0014: Reflink action via the `reflink-copy` crate

- Status: Accepted
- Date: 2026-08-24
- Related: ADR-0009 (action layer, deferred reflink from v1), ADR-0002
  (cross-platform-first stance this respects)

## Context

ADR-0009 shipped `Delete` and `Hardlink` as the only action kinds in v1,
explicitly deferring `Reflink` (copy-on-write clone) because it's
platform/filesystem-specific and would need either a new dependency or
unsafe FFI to a Linux ioctl (`FICLONE`) or equivalent platform call
(`clonefile` on macOS/APFS, `FSCTL_DUPLICATE_EXTENTS_TO_FILE` on Windows/
ReFS). That deferral is now being closed.

Two design questions: how to talk to the platform (hand-rolled unsafe FFI
per platform vs. a dependency), and what to do when the target filesystem
doesn't support cloning (silently fall back to a full copy vs. fail).

## Decision

- **Dependency, not hand-rolled FFI**: use the `reflink-copy` crate
  (workspace dependency, `rusty_fclone-core`-only). It already implements
  the platform-specific ioctls/syscalls for Linux, macOS, and Windows
  behind one portable function, which is exactly the kind of
  well-trodden, easy-to-get-wrong systems code this project's
  "no dependency added without a one-line justification" rule exists to
  weigh against hand-rolling. Enabled with its optional `tracing` feature
  for consistency with ADR-0010's observability work, at no extra cost.
- **Fail, don't silently copy**: use `reflink_copy::reflink` (the strict
  function that errors when cloning isn't supported), not
  `reflink_or_copy` (which transparently falls back to a full byte copy).
  A silent copy fallback would report success while not actually freeing
  any space — the entire reason to choose `Reflink` over `Hardlink` in the
  first place — and would do so invisibly, which is worse than an
  honest per-file failure a user can see and decide how to handle (e.g.
  fall back to `--action hardlink` themselves).
- **Same safety pattern as `Hardlink`**: `reflink_over` links to a
  temporary sibling name first, then renames over the target path — the
  same sequence `hardlink_over` already uses (ADR-0009), so the target
  path is never observably missing mid-operation. One difference: unlike
  a failed `fs::hard_link` (which creates nothing on failure),
  `reflink_copy::reflink`'s underlying ioctl needs the destination file to
  already exist before the clone call runs, so a failed clone can leave an
  empty stub at the temp path. `reflink_over` cleans that up (best-effort
  `fs::remove_file`) before returning the error, so a failed reflink never
  leaves filesystem litter behind.

## Consequences

- New dependency: `reflink-copy` (plus its transitive Windows-only deps,
  inert on Linux/macOS builds).
- `ActionKind` gains a third variant; every exhaustive match on it (the
  `apply` dispatch, the CLI's `action_word`) needed updating — caught at
  compile time, not a runtime gap.
- Untested end-to-end on a real CoW filesystem: this environment's storage
  isn't Btrfs/XFS-with-reflink/APFS/etc., so `cargo test` and the manual
  CLI smoke test both only exercise the graceful-failure path (confirmed:
  a reported per-file error, zero bytes reclaimed, both files left with
  correct unmodified content, no stray temp file). The success path is
  implemented and delegated entirely to `reflink-copy`'s own
  platform-specific implementation, which is exercised by that crate's own
  test suite — not re-verified here beyond trusting the dependency.
- The `apply_reflink_*` unit test is written to accept either outcome
  (success or a clean per-file failure) rather than asserting one,
  since which one occurs depends on the filesystem running the test, not
  on this code.
