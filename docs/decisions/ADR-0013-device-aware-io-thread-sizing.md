# ADR-0013: Device-aware I/O thread pool sizing (Linux, best-effort)

- Status: Accepted
- Date: 2026-08-24
- Related: ADR-0002 (original oversubscription default), ADR-0008
  (revised default to `cores`, explicitly flagging this as the principled
  long-term fix)

## Context

ADR-0008 changed `io_threads`'s default from an oversubscribed multiple of
core count to plain core count, because oversubscription measurably hurt
throughput on the one environment benchmarked there. That ADR was explicit
that this was an environment-specific empirical result, not a universal
one: "It has not been validated on real spinning disks or high-latency
network filesystems, where the original oversubscription rationale may
still hold," and named exactly this unit — device-type-aware tuning
instead of one guessed constant — as the principled fix.

The roadmap's `DETECTION-LINUX-FASTPATH` bundles two genuinely different
pieces of work: (1) an io_uring/`FIEMAP`-based I/O fast path, which needs
an async runtime and unsafe FFI and deserves its own design decision, and
(2) picking `io_threads`'s default based on whether the scan root's
storage is actually rotational. Only (2) is in scope here — see the
roadmap for (1), tracked separately.

## Decision

- New `device` module in `rusty_fclone-core`: `default_io_threads(root:
  &Path) -> usize` returns an oversubscribed value (`(cores * 4).min(64)`
  — ADR-0002's original constant) when `root`'s filesystem is on a
  rotational disk, or plain `cores` (ADR-0008's default) otherwise or when
  detection fails for any reason.
- Detection is Linux-only (`#[cfg(target_os = "linux")]`) and best-effort:
  parse `/proc/self/mountinfo` to find the `major:minor` device backing
  the mount that covers `root` (longest-matching mount point, to handle
  nested mounts), then read `/sys/dev/block/<major:minor>/queue/rotational`
  (falling back to the parent whole-disk directory for partition device
  nodes, which don't carry their own `queue/`). Any failure along the way
  — file not found, unparseable, non-Linux — returns `None`, and the
  caller falls back to `cores`. Chosen over shelling out to `lsblk`/`udevadm`
  or a new dependency: `/proc` and `/sys` are always present on Linux, no
  new dependency, and the format is stable kernel ABI.
- `ScanOptions::io_threads` changes from `usize` to `Option<usize>`.
  `None` (the new default) means "auto-detect from `root` at scan time";
  `Some(n)` pins it explicitly, matching today's CLI behavior when
  `--io-threads` is passed. This is a necessary API shape change: the
  previous `usize` default was computed once, in `ScanOptions::default()`,
  which has no access to the scan root — detection can only happen once
  `root` is known, inside `pipeline::run_scan`. The CLI's `--io-threads`
  flag becomes `Option<usize>` too, with no `default_value_t` (omitting
  the flag now means "auto-detect," not "use N").
- Resolution happens once per scan, right before constructing the
  `IoPool`, and is logged at `debug` level when auto-detection actually
  ran (`tracing::debug!(io_threads = ..., "auto-detected io_threads from
  device type")` — silent when the caller passed an explicit value).

## Consequences

- No new dependencies.
- Breaking API change: `ScanOptions::io_threads` and the CLI's
  `--io-threads` flag change type. Both remain source-compatible for the
  common case (omit the field/flag, get a sensible default); only code
  that explicitly set a `usize` value needs a `Some(...)` wrapper.
- This container environment's `default_io_threads` resolves to plain
  `cores` in manual testing (detection returns `None`/`false` here, the
  same safe path as before ADR-0013), so it doesn't change today's
  benchmarked numbers (`docs/benchmarks/FCLONES-COMPARISON.md`) — the
  win is only realized on real rotational storage, which this environment
  doesn't have. Untested on an actual spinning disk; the mountinfo/sysfs
  parsing logic has unit tests against synthetic input instead.
- io_uring/`FIEMAP` extent-ordered reads remain a separate, still-open
  roadmap item — this ADR only closes the thread-sizing half of
  `DETECTION-LINUX-FASTPATH`.
