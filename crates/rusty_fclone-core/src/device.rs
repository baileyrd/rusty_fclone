//! Device-type-aware I/O pool sizing (DETECTION-LINUX-FASTPATH, ADR-0013).
//!
//! Detection is Linux-only and best-effort: anything that doesn't parse or
//! doesn't exist (non-Linux, an unreadable `/proc`/`/sys`, an unrecognized
//! device) falls back to `None`, and the caller falls back to the safe,
//! already-benchmarked `cores` default (ADR-0008).

use std::path::Path;

/// Picks a default I/O-pool worker count for `root`'s filesystem:
/// oversubscribed on a rotational disk, where more in-flight requests hides
/// per-request seek latency, or `cores` otherwise -- matching ADR-0008's
/// default, which oversubscription measurably hurt on the non-rotational
/// storage benchmarked there.
pub(crate) fn default_io_threads(root: &Path) -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    if is_rotational(root) == Some(true) {
        (cores * 4).min(64)
    } else {
        cores
    }
}

#[cfg(target_os = "linux")]
fn is_rotational(root: &Path) -> Option<bool> {
    let canonical = std::fs::canonicalize(root).ok()?;
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
    let mount = covering_mount(&mountinfo, canonical.to_str()?)?;
    read_rotational_flag(&format!("/sys/dev/block/{}", mount.major_minor))
        .or_else(|| source_rotational_flag(mount.source))
}

/// The parts of a `/proc/self/mountinfo` entry device detection needs.
#[cfg(target_os = "linux")]
#[derive(Debug, PartialEq, Eq)]
struct MountEntry<'a> {
    /// The mount's `major:minor`. For a FUSE mount (e.g. ntfs-3g) this is
    /// an anonymous `0:N` with no block device behind it in sysfs.
    major_minor: &'a str,
    /// What was mounted, e.g. `/dev/sdb1` -- for `fuseblk` mounts the real
    /// block device, which is how a FUSE mount is traced to its disk.
    source: &'a str,
}

/// Falls back to the mount's source when its own device id isn't a block
/// device (FUSE): `/dev/sdb1` (or a `/dev/disk/by-*` symlink to it)
/// resolves to `/sys/class/block/sdb1`. Sources that aren't device nodes
/// (`sshfs`, `tmpfs`, ...) yield `None`.
#[cfg(target_os = "linux")]
fn source_rotational_flag(source: &str) -> Option<bool> {
    if !source.starts_with("/dev/") {
        return None;
    }
    let device = std::fs::canonicalize(source).ok()?;
    let name = device.file_name()?.to_str()?;
    read_rotational_flag(&format!("/sys/class/block/{name}"))
}

/// Parses `/proc/self/mountinfo` (see `proc_pid_mountinfo(5)`) to find the
/// mount entry covering `path` -- the entry whose mount point is the
/// longest matching prefix of `path`, to handle nested mounts correctly.
#[cfg(target_os = "linux")]
fn covering_mount<'a>(mountinfo: &'a str, path: &str) -> Option<MountEntry<'a>> {
    let mut best: Option<(usize, MountEntry<'a>)> = None;
    for line in mountinfo.lines() {
        let fields: Vec<&str> = line.split(' ').collect();
        let major_minor = *fields.get(2)?;
        let mount_point = *fields.get(4)?;
        // Optional fields end at a lone "-", followed by fstype and source.
        let separator = fields.iter().position(|f| *f == "-")?;
        let source = *fields.get(separator + 2)?;
        let covers = mount_point == "/"
            || path == mount_point
            || path.starts_with(&format!("{mount_point}/"));
        if !covers {
            continue;
        }
        let len = mount_point.len();
        if best.as_ref().is_none_or(|(best_len, _)| len > *best_len) {
            best = Some((
                len,
                MountEntry {
                    major_minor,
                    source,
                },
            ));
        }
    }
    best.map(|(_, entry)| entry)
}

/// Reads `queue/rotational` under a sysfs block-device directory
/// (`/sys/dev/block/<major:minor>` or `/sys/class/block/<name>`). Partition
/// device nodes don't carry their own `queue/`, so if the direct path is
/// missing, falls back to the parent whole-disk directory the sysfs symlink
/// resolves under (e.g. `sda1`'s parent is `sda`).
#[cfg(target_os = "linux")]
fn read_rotational_flag(sysfs_dir: &str) -> Option<bool> {
    if let Ok(contents) = std::fs::read_to_string(format!("{sysfs_dir}/queue/rotational")) {
        return parse_rotational(&contents);
    }
    let link = std::fs::canonicalize(sysfs_dir).ok()?;
    let parent = link.parent()?;
    let contents = std::fs::read_to_string(parent.join("queue/rotational")).ok()?;
    parse_rotational(&contents)
}

#[cfg(target_os = "linux")]
fn parse_rotational(contents: &str) -> Option<bool> {
    match contents.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

#[cfg(not(target_os = "linux"))]
fn is_rotational(_root: &Path) -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_io_threads_returns_a_sane_value_for_a_real_directory() {
        // Can't assert a specific rotational/non-rotational outcome --
        // that depends on the machine running the test -- but it must
        // always return a usable, non-zero thread count.
        let dir = tempfile::tempdir().unwrap();
        let threads = default_io_threads(dir.path());
        assert!(threads >= 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn covering_mount_picks_the_longest_matching_prefix() {
        let mountinfo = "36 35 8:1 / / rw,relatime - ext4 /dev/sda1 rw\n\
                          37 36 0:30 / /mnt/data rw,relatime - ext4 /dev/sdb1 rw\n";
        assert_eq!(
            covering_mount(mountinfo, "/mnt/data/some/file").map(|m| m.major_minor),
            Some("0:30")
        );
        assert_eq!(
            covering_mount(mountinfo, "/home/user/file").map(|m| m.major_minor),
            Some("8:1")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn covering_mount_reads_the_source_past_optional_fields() {
        // ntfs-3g: anonymous 0:N device id, real disk only in the source;
        // "shared:1 master:2" are optional fields before the "-".
        let mountinfo = "36 35 8:1 / / rw shared:1 - ext4 /dev/sda1 rw\n\
                          99 36 0:52 / /mnt/daarch rw shared:1 master:2 - fuseblk /dev/sdb1 rw\n";
        assert_eq!(
            covering_mount(mountinfo, "/mnt/daarch/photos"),
            Some(MountEntry {
                major_minor: "0:52",
                source: "/dev/sdb1",
            })
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn source_rotational_flag_ignores_non_device_sources() {
        assert_eq!(source_rotational_flag("user@host:/data"), None);
        assert_eq!(source_rotational_flag("tmpfs"), None);
        assert_eq!(source_rotational_flag("/dev/does-not-exist"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn covering_mount_returns_none_for_malformed_input() {
        assert_eq!(covering_mount("not mountinfo at all", "/"), None);
        assert_eq!(covering_mount("", "/"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_rotational_reads_the_flag() {
        assert_eq!(parse_rotational("1\n"), Some(true));
        assert_eq!(parse_rotational("0\n"), Some(false));
        assert_eq!(parse_rotational("garbage\n"), None);
    }
}
