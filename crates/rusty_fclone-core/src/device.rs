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
    let major_minor = mount_device_id(&mountinfo, canonical.to_str()?)?;
    read_rotational_flag(&major_minor)
}

/// Parses `/proc/self/mountinfo` (see `proc_pid_mountinfo(5)`) to find the
/// `major:minor` device id of the mount entry covering `path` -- the entry
/// whose mount point is the longest matching prefix of `path`, to handle
/// nested mounts correctly.
#[cfg(target_os = "linux")]
fn mount_device_id(mountinfo: &str, path: &str) -> Option<String> {
    let mut best: Option<(usize, &str)> = None;
    for line in mountinfo.lines() {
        let fields: Vec<&str> = line.split(' ').collect();
        let major_minor = *fields.get(2)?;
        let mount_point = *fields.get(4)?;
        let covers = mount_point == "/"
            || path == mount_point
            || path.starts_with(&format!("{mount_point}/"));
        if !covers {
            continue;
        }
        let len = mount_point.len();
        if best.is_none_or(|(best_len, _)| len > best_len) {
            best = Some((len, major_minor));
        }
    }
    best.map(|(_, id)| id.to_string())
}

/// Reads `/sys/dev/block/<major:minor>/queue/rotational`. Partition device
/// nodes don't carry their own `queue/`, so if the direct path is missing,
/// falls back to the parent whole-disk directory the sysfs symlink resolves
/// under (e.g. `sda1`'s parent is `sda`).
#[cfg(target_os = "linux")]
fn read_rotational_flag(major_minor: &str) -> Option<bool> {
    let direct = format!("/sys/dev/block/{major_minor}/queue/rotational");
    if let Ok(contents) = std::fs::read_to_string(&direct) {
        return parse_rotational(&contents);
    }
    let link = std::fs::canonicalize(format!("/sys/dev/block/{major_minor}")).ok()?;
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
    fn mount_device_id_picks_the_longest_matching_prefix() {
        let mountinfo = "36 35 8:1 / / rw,relatime - ext4 /dev/sda1 rw\n\
                          37 36 0:30 / /mnt/data rw,relatime - ext4 /dev/sdb1 rw\n";
        assert_eq!(
            mount_device_id(mountinfo, "/mnt/data/some/file"),
            Some("0:30".to_string())
        );
        assert_eq!(
            mount_device_id(mountinfo, "/home/user/file"),
            Some("8:1".to_string())
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn mount_device_id_returns_none_for_malformed_input() {
        assert_eq!(mount_device_id("not mountinfo at all", "/"), None);
        assert_eq!(mount_device_id("", "/"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_rotational_reads_the_flag() {
        assert_eq!(parse_rotational("1\n"), Some(true));
        assert_eq!(parse_rotational("0\n"), Some(false));
        assert_eq!(parse_rotational("garbage\n"), None);
    }
}
