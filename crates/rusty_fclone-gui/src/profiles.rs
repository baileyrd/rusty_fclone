//! Persisted scan profiles (`SCAN-PROFILES`) — a named `{root, ScanOptions}`
//! preset saved as a small JSON file, so a saved setup survives a GUI
//! restart instead of staying session-only. Deliberately a flat JSON file
//! rather than SQLite (the CLI's `--history` choice, `rusqlite`): a handful
//! of named presets is exactly the shape a plain `Vec<ScanProfilePayload>`
//! round-trips through `serde_json` without any query capability actually
//! being needed. See ADR-0029.

use std::fs;
use std::path::{Path, PathBuf};

use crate::payload::ScanProfilePayload;

const PROFILES_FILE: &str = "scan_profiles.json";

/// The OS-appropriate per-user config directory for this app
/// (`$XDG_CONFIG_HOME`/`AppData\Roaming`/`~/Library/Application Support`,
/// via the `dirs` crate — the same lookup Tauri's own `app_config_dir()`
/// performs internally, used directly here so the scan-profile commands
/// stay plain functions rather than needing an `AppHandle`), joined with a
/// project-specific subdirectory. Not independently covered by an
/// automated test — trusted the same way `trash`'s and reflink's
/// non-Linux platform behavior already are (ADR-0014/ADR-0024's
/// precedent): the real OS-directory lookup is delegated entirely to a
/// well-established crate, and exercising it for real would write into
/// the host's actual config directory instead of a hermetic tempdir.
pub fn default_profiles_dir() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|dir| dir.join("rusty-fclone"))
        .ok_or_else(|| "could not determine the OS config directory".to_string())
}

fn profiles_path(dir: &Path) -> PathBuf {
    dir.join(PROFILES_FILE)
}

/// Reads every saved profile from `dir`. A missing file (nothing saved yet)
/// or an empty one is `Ok(vec![])`, not an error.
pub fn load(dir: &Path) -> Result<Vec<ScanProfilePayload>, String> {
    let path = profiles_path(dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read_to_string(&path).map_err(|err| err.to_string())?;
    if data.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&data).map_err(|err| err.to_string())
}

fn save(dir: &Path, profiles: &[ScanProfilePayload]) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    let data = serde_json::to_string_pretty(profiles).map_err(|err| err.to_string())?;
    fs::write(profiles_path(dir), data).map_err(|err| err.to_string())
}

/// Inserts `profile`, or overwrites the existing entry with the same name,
/// then persists the whole list and returns it. Saving again under a name
/// already in use is a deliberate update, not an error — matches how
/// "Save" behaves for an existing preset in most apps.
pub fn upsert(dir: &Path, profile: ScanProfilePayload) -> Result<Vec<ScanProfilePayload>, String> {
    let mut profiles = load(dir)?;
    match profiles.iter_mut().find(|p| p.name == profile.name) {
        Some(existing) => *existing = profile,
        None => profiles.push(profile),
    }
    save(dir, &profiles)?;
    Ok(profiles)
}

/// Removes the profile named `name`, if any, then persists the remaining
/// list and returns it. Removing a name that isn't there is a no-op, not an
/// error.
pub fn remove(dir: &Path, name: &str) -> Result<Vec<ScanProfilePayload>, String> {
    let mut profiles = load(dir)?;
    profiles.retain(|p| p.name != name);
    save(dir, &profiles)?;
    Ok(profiles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::ScanOptionsPayload;

    fn profile(name: &str, root: &str) -> ScanProfilePayload {
        ScanProfilePayload {
            name: name.to_string(),
            root: root.to_string(),
            options: ScanOptionsPayload::default(),
        }
    }

    #[test]
    fn load_returns_an_empty_list_when_no_file_exists_yet() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn upsert_inserts_a_new_profile_and_persists_it() {
        let dir = tempfile::tempdir().unwrap();
        let profiles = upsert(dir.path(), profile("Downloads", "/home/me/Downloads")).unwrap();

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "Downloads");
        // Re-reading from disk (a fresh `load`) confirms it was actually
        // written, not just returned from the in-memory upsert.
        let reloaded = load(dir.path()).unwrap();
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].root, "/home/me/Downloads");
    }

    #[test]
    fn upsert_overwrites_an_existing_profile_with_the_same_name() {
        let dir = tempfile::tempdir().unwrap();
        upsert(dir.path(), profile("Downloads", "/home/me/Downloads")).unwrap();
        let profiles = upsert(dir.path(), profile("Downloads", "/home/me/Downloads2")).unwrap();

        assert_eq!(profiles.len(), 1, "same name must update, not duplicate");
        assert_eq!(profiles[0].root, "/home/me/Downloads2");
    }

    #[test]
    fn upsert_preserves_other_saved_profiles() {
        let dir = tempfile::tempdir().unwrap();
        upsert(dir.path(), profile("A", "/a")).unwrap();
        let profiles = upsert(dir.path(), profile("B", "/b")).unwrap();

        assert_eq!(profiles.len(), 2);
        assert!(profiles.iter().any(|p| p.name == "A" && p.root == "/a"));
        assert!(profiles.iter().any(|p| p.name == "B" && p.root == "/b"));
    }

    #[test]
    fn remove_deletes_the_named_profile_and_persists_the_change() {
        let dir = tempfile::tempdir().unwrap();
        upsert(dir.path(), profile("A", "/a")).unwrap();
        upsert(dir.path(), profile("B", "/b")).unwrap();

        let profiles = remove(dir.path(), "A").unwrap();

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "B");
        let reloaded = load(dir.path()).unwrap();
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].name, "B");
    }

    #[test]
    fn remove_a_name_that_does_not_exist_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        upsert(dir.path(), profile("A", "/a")).unwrap();

        let profiles = remove(dir.path(), "does-not-exist").unwrap();

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "A");
    }

    #[test]
    fn options_round_trip_through_the_saved_json_file() {
        let dir = tempfile::tempdir().unwrap();
        let options = ScanOptionsPayload {
            min_size: Some(1024),
            exclude_paths: vec!["/home/me/node_modules".to_string()],
            ..ScanOptionsPayload::default()
        };
        upsert(
            dir.path(),
            ScanProfilePayload {
                name: "Filtered".to_string(),
                root: "/data".to_string(),
                options,
            },
        )
        .unwrap();

        let reloaded = load(dir.path()).unwrap();
        assert_eq!(reloaded[0].options.min_size, Some(1024));
        assert_eq!(
            reloaded[0].options.exclude_paths,
            vec!["/home/me/node_modules".to_string()]
        );
    }
}
