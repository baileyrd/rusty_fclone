//! `#[tauri::command]` entry points invoked from the frontend via
//! `window.__TAURI__.core.invoke`. Kept thin — all real logic lives in
//! `rusty_fclone_core`; this module only translates to/from
//! [`crate::payload`]'s wire types and drives the background scan thread.

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Runtime};

use rusty_fclone_core::select;
use rusty_fclone_core::{action, find_folder_duplicates, folder_action};

use crate::payload::{
    normalize_path_input, normalize_path_list, parse_action_kind, parse_keep_rule,
    ActionResultPayload, ChooseKeepPayload, DirEntryPayload, FolderActionResultPayload,
    FolderMatchPayload, GroupPayload, PreviewPayload, ScanEventPayload, ScanOptionsPayload,
    ScanProfilePayload, SimilarGroupPayload,
};
use crate::preview;
use crate::profiles;

/// Starts a scan on a background thread and streams results back to the
/// frontend as `scan-event` events, one per [`rusty_fclone_core::ScanEvent`]
/// (ADR-0004's streaming contract, carried across the IPC boundary instead
/// of collected first). Returns as soon as the scan thread is spawned —
/// callers don't await scan completion here, they listen for the
/// `Finished` event.
#[tauri::command]
pub fn start_scan<R: Runtime>(
    app: AppHandle<R>,
    root: String,
    options: ScanOptionsPayload,
) -> Result<(), String> {
    let root = normalize_path_input(&root);
    let options = options.into();
    std::thread::spawn(move || {
        let handle = match rusty_fclone_core::scan(root, options) {
            Ok(handle) => handle,
            Err(err) => {
                let _ = app.emit(
                    "scan-event",
                    ScanEventPayload::Error {
                        path: String::new(),
                        message: err.to_string(),
                    },
                );
                return;
            }
        };
        for event in handle {
            let payload: ScanEventPayload = match &event {
                rusty_fclone_core::ScanEvent::DuplicateGroup(group) => group.into(),
                rusty_fclone_core::ScanEvent::Error(err) => err.into(),
                rusty_fclone_core::ScanEvent::Progress(p) => (*p).into(),
                rusty_fclone_core::ScanEvent::Finished(summary) => summary.clone().into(),
            };
            if app.emit("scan-event", payload).is_err() {
                // Frontend window is gone; nothing left to stream to.
                break;
            }
        }
    });
    Ok(())
}

/// Plans (and, if `apply` is set, actually runs) `kind` over `group` —
/// mirrors the CLI's `--action <kind>` (preview) / `--action <kind>
/// --apply` (actually run) split, so the same "preview first" safety
/// property holds in the GUI (ADR-0009: destructive actions default to
/// preview, never implicit).
#[tauri::command]
pub fn run_action(
    group: GroupPayload,
    kind: String,
    keep_reason: Option<String>,
    apply: bool,
    reference_paths: Vec<String>,
    archive_dir: Option<String>,
) -> Result<ActionResultPayload, String> {
    let archive_dir = archive_dir.as_deref().map(normalize_path_input);
    let kind = parse_action_kind(&kind, archive_dir.as_deref().map(std::path::Path::new))?;
    let group = group.into();
    let reference_paths = normalize_path_list(&reference_paths);
    // The kept path is always `group.paths[0]` here -- the frontend
    // resolves which path that should be *before* calling this command
    // (either a manual keep-choice badge, or `choose_keep` below for a
    // non-default rule) and sends it as the group's first path, the same
    // reordering trick manual keep-choice already used before
    // `SELECTION-RULES` existed (no new core API for "which path to
    // keep"). `keep_reason` is likewise resolved by the frontend from
    // whichever of those two paths it took, and just passed through here
    // for display -- `action::plan` itself has no concept of "why".
    // `reference_paths` (ACTION-REFERENCE-FOLDERS) still overrides that
    // choice, the same defense-in-depth `action::plan`'s core caller
    // already relies on.
    let plan = action::plan(&group, kind, &reference_paths);
    let applied = if apply {
        Some((&action::apply(&plan)).into())
    } else {
        None
    };
    Ok(ActionResultPayload {
        plan: (&plan, keep_reason.as_deref().unwrap_or("your choice")).into(),
        applied,
    })
}

/// Chooses which path in `group` to keep under `rule`, without planning or
/// applying any action — lets the frontend resolve (and display) a rule's
/// pick before the user confirms, the same way it already resolves a
/// manual keep-choice badge (`SELECTION-RULES`).
#[tauri::command]
pub fn choose_keep(
    group: GroupPayload,
    rule: String,
    reference_paths: Vec<String>,
) -> Result<ChooseKeepPayload, String> {
    let rule = parse_keep_rule(&rule)?;
    let group = group.into();
    let reference_paths = normalize_path_list(&reference_paths);
    let (keep, reason) = select::choose_keep(&group, rule, &reference_paths);
    Ok(ChooseKeepPayload {
        keep: keep.display().to_string(),
        reason,
    })
}

/// Finds folder-level duplicates (ADR-0021) among the duplicate groups a
/// prior `start_scan` already produced. `root` and `options` must match
/// that earlier scan — the frontend already holds both (it sent them to
/// `start_scan`) plus every `duplicate_group` event it received, so this
/// takes all three back rather than re-scanning. A post-scan, on-demand
/// call (not part of the `scan-event` stream): a folder verdict needs the
/// whole tree's picture, so it can't be produced incrementally the way a
/// `DuplicateGroup` can.
#[tauri::command]
pub fn find_duplicate_folders(
    root: String,
    groups: Vec<GroupPayload>,
    options: ScanOptionsPayload,
) -> Result<Vec<FolderMatchPayload>, String> {
    let root = PathBuf::from(normalize_path_input(&root));
    let groups: Vec<_> = groups.into_iter().map(Into::into).collect();
    let options = options.into();
    find_folder_duplicates(&root, &groups, &options)
        .map(|matches| matches.iter().map(Into::into).collect())
        .map_err(|err| err.to_string())
}

/// Plans (and, if `apply` is set, actually runs) `kind` over every file in
/// `removed` against its confirmed partner in `kept` — the folder-level
/// counterpart of [`run_action`], closing the gap the "Delete Duplicate
/// Folder" button was shipped disabled for (ADR-0023). `groups` and
/// `options` must match the scan that originally produced the `FolderMatch`
/// this is acting on; the frontend already holds both, the same way it
/// already does for `find_duplicate_folders`. `rusty_fclone_core::
/// folder_action::plan_folder` re-verifies every file's confirmed partner
/// and current on-disk size before planning, failing closed (an `Err`, not
/// a partial plan) if the scan has gone stale since.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn run_folder_action(
    removed: String,
    kept: String,
    groups: Vec<GroupPayload>,
    options: ScanOptionsPayload,
    kind: String,
    apply: bool,
    reference_paths: Vec<String>,
    archive_dir: Option<String>,
) -> Result<FolderActionResultPayload, String> {
    let archive_dir = archive_dir.as_deref().map(normalize_path_input);
    let kind = parse_action_kind(&kind, archive_dir.as_deref().map(std::path::Path::new))?;
    let removed = PathBuf::from(normalize_path_input(&removed));
    let kept = PathBuf::from(normalize_path_input(&kept));
    let groups: Vec<_> = groups.into_iter().map(Into::into).collect();
    let options = options.into();
    let reference_paths = normalize_path_list(&reference_paths);
    let plan =
        folder_action::plan_folder(&removed, &kept, &groups, &options, kind, &reference_paths)
            .map_err(|err| err.to_string())?;
    let applied = if apply {
        Some((&folder_action::apply_folder(&plan)).into())
    } else {
        None
    };
    Ok(FolderActionResultPayload {
        plan: (&plan).into(),
        applied,
    })
}

/// Reads `path` (a real, small image or audio file) and returns it as a
/// ready-to-embed `data:` URI for the Duplicate Review screen's
/// compare-card thumbnails (`GUI-MEDIA-PREVIEW`, ADR-0028). Fails closed
/// on an unsupported extension, a file over the size cap, or a real I/O
/// error — never a partial preview.
#[tauri::command]
pub fn read_preview(path: String) -> Result<PreviewPayload, String> {
    let path = PathBuf::from(normalize_path_input(&path));
    preview::build_data_url(&path).map(|data_url| PreviewPayload { data_url })
}

/// Lists every saved scan profile (`SCAN-PROFILES`, ADR-0029), in the order
/// they're stored — the frontend decides how to display them.
#[tauri::command]
pub fn list_scan_profiles() -> Result<Vec<ScanProfilePayload>, String> {
    let dir = profiles::default_profiles_dir()?;
    profiles::load(&dir)
}

/// Saves the current `{root, options}` as a named profile, overwriting any
/// existing profile with the same name, and returns the full updated list.
#[tauri::command]
pub fn save_scan_profile(
    name: String,
    root: String,
    options: ScanOptionsPayload,
) -> Result<Vec<ScanProfilePayload>, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("a scan profile needs a name".to_string());
    }
    let root = normalize_path_input(&root);
    let dir = profiles::default_profiles_dir()?;
    profiles::upsert(
        &dir,
        ScanProfilePayload {
            name,
            root,
            options,
        },
    )
}

/// Deletes the named profile, if any, and returns the remaining list.
#[tauri::command]
pub fn delete_scan_profile(name: String) -> Result<Vec<ScanProfilePayload>, String> {
    let dir = profiles::default_profiles_dir()?;
    profiles::remove(&dir, &name)
}

/// Finds visually-similar (not byte-identical) image clusters under `root`
/// (`DETECTION-PERCEPTUAL-IMAGES`, ADR-0030) — a deliberately separate,
/// opt-in pass from `start_scan`'s exact-duplicate results, run on demand
/// rather than automatically for every scan. `max_hamming_distance`
/// defaults to `PerceptualOptions::default()` (10/64) when omitted.
#[tauri::command]
pub fn find_similar_images(
    root: String,
    options: ScanOptionsPayload,
    max_hamming_distance: Option<u32>,
) -> Result<Vec<SimilarGroupPayload>, String> {
    let root = PathBuf::from(normalize_path_input(&root));
    let options = options.into();
    let perceptual_options = rusty_fclone_core::PerceptualOptions {
        max_hamming_distance: max_hamming_distance
            .unwrap_or(rusty_fclone_core::PerceptualOptions::default().max_hamming_distance),
    };
    rusty_fclone_core::find_similar_images(&root, &options, &perceptual_options)
        .map(|groups| groups.iter().map(Into::into).collect())
        .map_err(|err| err.to_string())
}

/// Lists the real, immediate subdirectories of `path` — or, when `path` is
/// `None`, the platform's natural browse starting points (the user's home
/// directory, plus `/` on Unix or each drive letter on Windows) — for the
/// Scan Setup "Browse…" folder picker and the Duplicate Review file-system
/// panel (`GUI-FS-BROWSE`). Directories only (files can't be a scan root
/// or a filter target); hidden (dot-prefixed) entries are skipped, same as
/// every shell's default `ls`. Sorted case-insensitively so the tree reads
/// the same regardless of the OS's raw directory order. An unreadable
/// directory (permission denied, vanished mid-read) degrades to an empty
/// list rather than failing the whole call — one inaccessible branch
/// shouldn't break browsing everywhere else.
#[tauri::command]
pub fn list_directory(path: Option<String>) -> Result<Vec<DirEntryPayload>, String> {
    let dir = match path {
        Some(p) => PathBuf::from(normalize_path_input(&p)),
        None => return Ok(browse_roots()),
    };
    Ok(subdirectories_of(&dir))
}

fn subdirectories_of(dir: &std::path::Path) -> Vec<DirEntryPayload> {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };
    let mut entries: Vec<DirEntryPayload> = read_dir
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            !entry.file_name().to_string_lossy().starts_with('.')
                && entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
        })
        .map(|entry| {
            let full_path = entry.path();
            let has_children = has_subdirectory(&full_path);
            DirEntryPayload {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: full_path.display().to_string(),
                has_children,
            }
        })
        .collect();
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries
}

fn has_subdirectory(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut rd| {
            rd.any(|entry| {
                entry
                    .ok()
                    .map(|e| {
                        !e.file_name().to_string_lossy().starts_with('.')
                            && e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn browse_roots() -> Vec<DirEntryPayload> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(DirEntryPayload {
            name: "Home".to_string(),
            has_children: has_subdirectory(&home),
            path: home.display().to_string(),
        });
    }
    #[cfg(windows)]
    {
        for letter in b'A'..=b'Z' {
            let root = PathBuf::from(format!("{}:\\", letter as char));
            if root.is_dir() {
                roots.push(DirEntryPayload {
                    name: root.display().to_string(),
                    has_children: has_subdirectory(&root),
                    path: root.display().to_string(),
                });
            }
        }
    }
    #[cfg(not(windows))]
    {
        let root = PathBuf::from("/");
        roots.push(DirEntryPayload {
            name: "/".to_string(),
            has_children: has_subdirectory(&root),
            path: root.display().to_string(),
        });
    }
    roots
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, INVOKE_KEY};
    use tauri::webview::InvokeRequest;
    use tauri::WebviewWindowBuilder;

    fn invoke(cmd: &str, body: serde_json::Value) -> Result<serde_json::Value, serde_json::Value> {
        let app = mock_builder()
            .invoke_handler(tauri::generate_handler![
                super::start_scan,
                super::run_action,
                super::choose_keep,
                super::find_duplicate_folders,
                super::run_folder_action,
                super::read_preview,
                super::list_scan_profiles,
                super::save_scan_profile,
                super::delete_scan_profile,
                super::find_similar_images,
                super::list_directory
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("failed to build mock app");
        let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map(|b: tauri::ipc::InvokeResponseBody| b.deserialize::<serde_json::Value>().unwrap())
    }

    #[test]
    fn run_action_delete_preview_does_not_touch_the_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let response = invoke(
            "run_action",
            json!({
                "group": {"size": 3, "paths": [a.display().to_string(), b.display().to_string()]},
                "kind": "delete",
                "apply": false,
                "referencePaths": [],
            }),
        )
        .expect("run_action should succeed");

        assert_eq!(response["plan"]["kept"], a.display().to_string());
        assert_eq!(
            response["plan"]["planned"],
            json!([b.display().to_string()])
        );
        assert!(response["applied"].is_null());
        assert!(a.exists());
        assert!(
            b.exists(),
            "preview (apply: false) must not delete anything"
        );
    }

    #[test]
    fn run_action_delete_apply_removes_the_redundant_copy() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let response = invoke(
            "run_action",
            json!({
                "group": {"size": 3, "paths": [a.display().to_string(), b.display().to_string()]},
                "kind": "delete",
                "apply": true,
                "referencePaths": [],
            }),
        )
        .expect("run_action should succeed");

        assert_eq!(
            response["applied"]["succeeded"],
            json!([b.display().to_string()])
        );
        assert!(a.exists());
        assert!(
            !b.exists(),
            "apply: true must actually delete the redundant copy"
        );
    }

    #[test]
    fn run_action_keep_reason_defaults_to_a_placeholder_when_omitted() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let response = invoke(
            "run_action",
            json!({
                "group": {"size": 3, "paths": [a.display().to_string(), b.display().to_string()]},
                "kind": "delete",
                "apply": false,
                "referencePaths": [],
            }),
        )
        .expect("run_action should succeed");

        assert_eq!(response["plan"]["keepReason"], "your choice");
    }

    #[test]
    fn run_action_passes_through_an_explicit_keep_reason() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let response = invoke(
            "run_action",
            json!({
                "group": {"size": 3, "paths": [a.display().to_string(), b.display().to_string()]},
                "kind": "delete",
                "keepReason": "most recent modification time",
                "apply": false,
                "referencePaths": [],
            }),
        )
        .expect("run_action should succeed");

        assert_eq!(
            response["plan"]["keepReason"],
            "most recent modification time"
        );
    }

    /// ADR-0025: a reference-folder path passed via `referencePaths`
    /// overrides `group.paths[0]` (the caller-chosen "keep") the same way
    /// `action::plan_with_keep`'s core test already covers -- this is the
    /// IPC boundary re-verifying that guarantee still holds once the path
    /// list has been through JSON.
    #[test]
    fn run_action_reference_path_overrides_the_chosen_keep_and_is_never_acted_on() {
        let dir = tempfile::tempdir().unwrap();
        let reference = dir.path().join("reference");
        fs::create_dir_all(&reference).unwrap();
        let protected = reference.join("protected.txt");
        let other = dir.path().join("other.txt");
        fs::write(&protected, b"dup").unwrap();
        fs::write(&other, b"dup").unwrap();

        let response = invoke(
            "run_action",
            json!({
                // `other` is first, so it would normally be the "keep".
                "group": {"size": 3, "paths": [other.display().to_string(), protected.display().to_string()]},
                "kind": "trash",
                "apply": true,
                "referencePaths": [reference.display().to_string()],
            }),
        )
        .expect("run_action should succeed");

        assert_eq!(response["plan"]["kept"], protected.display().to_string());
        assert!(protected.exists(), "the protected file must survive");
        assert!(
            !other.exists(),
            "its unprotected duplicate is still removed"
        );
    }

    /// ADR-0026: `kind: "move"` relocates the redundant copy into
    /// `archiveDir`, mirroring its original path -- the IPC-boundary
    /// counterpart of the core `apply_move_relocates_the_redundant_copy_into_the_archive_directory`
    /// test.
    #[test]
    fn run_action_move_relocates_the_redundant_copy_into_the_archive_directory() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("archive");
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let response = invoke(
            "run_action",
            json!({
                "group": {"size": 3, "paths": [a.display().to_string(), b.display().to_string()]},
                "kind": "move",
                "apply": true,
                "referencePaths": [],
                "archiveDir": archive.display().to_string(),
            }),
        )
        .expect("run_action should succeed");

        assert_eq!(
            response["applied"]["succeeded"],
            json!([b.display().to_string()])
        );
        assert!(a.exists());
        assert!(!b.exists(), "the redundant copy is gone from its path");
        assert!(
            archive.join(b.strip_prefix("/").unwrap()).exists(),
            "the redundant copy must survive at its archived path"
        );
    }

    /// `kind: "copy"` leaves the original in place, unlike every other
    /// action kind.
    #[test]
    fn run_action_copy_leaves_the_original_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("archive");
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let response = invoke(
            "run_action",
            json!({
                "group": {"size": 3, "paths": [a.display().to_string(), b.display().to_string()]},
                "kind": "copy",
                "apply": true,
                "referencePaths": [],
                "archiveDir": archive.display().to_string(),
            }),
        )
        .expect("run_action should succeed");

        assert_eq!(response["applied"]["bytesReclaimed"], 0);
        assert!(a.exists());
        assert!(b.exists(), "Copy must not touch the original");
        assert!(archive.join(b.strip_prefix("/").unwrap()).exists());
    }

    /// `kind: "move"`/`"copy"` without `archiveDir` must be rejected, not
    /// silently ignored.
    #[test]
    fn run_action_move_without_archive_dir_is_rejected() {
        let response = invoke(
            "run_action",
            json!({
                "group": {"size": 1, "paths": ["/a", "/b"]},
                "kind": "move",
                "apply": false,
                "referencePaths": [],
            }),
        );
        assert!(
            response.is_err(),
            "\"move\" without an archive directory must be rejected"
        );
    }

    #[test]
    fn choose_keep_resolves_the_newest_file_without_touching_the_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(&b, b"dup").unwrap();

        let response = invoke(
            "choose_keep",
            json!({
                "group": {"size": 3, "paths": [a.display().to_string(), b.display().to_string()]},
                "rule": "newest",
                "referencePaths": [],
            }),
        )
        .expect("choose_keep should succeed");

        assert_eq!(response["keep"], b.display().to_string());
        assert_eq!(response["reason"], "most recent modification time");
        assert!(a.exists(), "choose_keep must never touch the filesystem");
        assert!(b.exists(), "choose_keep must never touch the filesystem");
    }

    #[test]
    fn choose_keep_rejects_an_unknown_rule() {
        let response = invoke(
            "choose_keep",
            json!({
                "group": {"size": 1, "paths": ["/a", "/b"]},
                "rule": "frobnicate",
                "referencePaths": [],
            }),
        );
        assert!(response.is_err(), "an unknown keep rule must be rejected");
    }

    #[test]
    fn find_duplicate_folders_reports_a_contained_folder_match() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"dup").unwrap();
        fs::write(big.join("1.txt"), b"dup").unwrap();
        fs::write(big.join("extra.txt"), b"only in big").unwrap();

        let response = invoke(
            "find_duplicate_folders",
            json!({
                "root": dir.path().display().to_string(),
                "groups": [
                    {"size": 3, "paths": [small.join("1.txt").display().to_string(), big.join("1.txt").display().to_string()]},
                ],
                "options": {},
            }),
        )
        .expect("find_duplicate_folders should succeed");

        let matches = response.as_array().expect("response should be an array");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0]["type"], "contained");
        assert_eq!(matches[0]["subset"], small.display().to_string());
        assert_eq!(matches[0]["superset"], big.display().to_string());
        assert_eq!(matches[0]["fileCount"], 1);
    }

    #[test]
    fn run_folder_action_delete_preview_does_not_touch_the_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"dup").unwrap();
        fs::write(big.join("1.txt"), b"dup").unwrap();

        let response = invoke(
            "run_folder_action",
            json!({
                "removed": small.display().to_string(),
                "kept": big.display().to_string(),
                "groups": [
                    {"size": 3, "paths": [small.join("1.txt").display().to_string(), big.join("1.txt").display().to_string()]},
                ],
                "options": {},
                "kind": "delete",
                "apply": false,
                "referencePaths": [],
            }),
        )
        .expect("run_folder_action should succeed");

        assert_eq!(response["plan"]["kept"], big.display().to_string());
        assert_eq!(response["plan"]["removed"], small.display().to_string());
        assert_eq!(response["plan"]["fileCount"], 1);
        assert!(response["applied"].is_null());
        assert!(
            small.join("1.txt").exists(),
            "preview must not delete anything"
        );
    }

    #[test]
    fn run_folder_action_delete_apply_removes_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"dup").unwrap();
        fs::write(big.join("1.txt"), b"dup").unwrap();

        let response = invoke(
            "run_folder_action",
            json!({
                "removed": small.display().to_string(),
                "kept": big.display().to_string(),
                "groups": [
                    {"size": 3, "paths": [small.join("1.txt").display().to_string(), big.join("1.txt").display().to_string()]},
                ],
                "options": {},
                "kind": "delete",
                "apply": true,
                "referencePaths": [],
            }),
        )
        .expect("run_folder_action should succeed");

        assert_eq!(response["applied"]["directoryRemoved"], true);
        assert!(!small.exists(), "apply: true must prune the emptied folder");
        assert!(
            big.join("1.txt").exists(),
            "the kept side must be untouched"
        );
    }

    /// ADR-0026, folder-level: `kind: "move"` relocates every file and
    /// prunes the emptied folder, same as delete/trash.
    #[test]
    fn run_folder_action_move_relocates_every_file_and_prunes_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        let archive = dir.path().join("archive");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"dup").unwrap();
        fs::write(big.join("1.txt"), b"dup").unwrap();

        let response = invoke(
            "run_folder_action",
            json!({
                "removed": small.display().to_string(),
                "kept": big.display().to_string(),
                "groups": [
                    {"size": 3, "paths": [small.join("1.txt").display().to_string(), big.join("1.txt").display().to_string()]},
                ],
                "options": {},
                "kind": "move",
                "apply": true,
                "referencePaths": [],
                "archiveDir": archive.display().to_string(),
            }),
        )
        .expect("run_folder_action should succeed");

        assert_eq!(response["applied"]["directoryRemoved"], true);
        assert!(
            !small.exists(),
            "Move prunes the emptied folder like delete/trash"
        );
        assert!(
            big.join("1.txt").exists(),
            "the kept side must be untouched"
        );
        let archived = small.join("1.txt");
        assert!(
            archive.join(archived.strip_prefix("/").unwrap()).exists(),
            "the moved file must survive at its archived path"
        );
    }

    /// ADR-0025, folder-level: a protected file inside `removed` survives
    /// and blocks the directory prune, the same guarantee the CLI's
    /// `find_duplicate_folders_with_reference_protects_a_file_and_blocks_the_prune`
    /// test covers -- re-verified here at the IPC boundary.
    #[test]
    fn run_folder_action_reference_path_protects_a_file_and_blocks_the_prune() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"dup").unwrap();
        fs::write(big.join("1.txt"), b"dup").unwrap();

        let response = invoke(
            "run_folder_action",
            json!({
                "removed": small.display().to_string(),
                "kept": big.display().to_string(),
                "groups": [
                    {"size": 3, "paths": [small.join("1.txt").display().to_string(), big.join("1.txt").display().to_string()]},
                ],
                "options": {},
                "kind": "delete",
                "apply": true,
                "referencePaths": [small.display().to_string()],
            }),
        )
        .expect("run_folder_action should succeed");

        assert_eq!(response["applied"]["directoryRemoved"], false);
        assert!(
            small.join("1.txt").exists(),
            "the protected file must survive"
        );
        assert!(
            small.exists(),
            "the directory must not be pruned while it still holds a protected file"
        );
        assert!(
            big.join("1.txt").exists(),
            "the kept side must be untouched"
        );
    }

    #[test]
    fn run_folder_action_rejects_a_stale_scan() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        // A file exists on disk that `groups` never mentions -- as if it
        // were added after the scan that produced `groups` ran.
        fs::write(small.join("1.txt"), b"dup").unwrap();
        fs::write(big.join("1.txt"), b"dup").unwrap();

        let err = invoke(
            "run_folder_action",
            json!({
                "removed": small.display().to_string(),
                "kept": big.display().to_string(),
                "groups": [],
                "options": {},
                "kind": "delete",
                "apply": false,
                "referencePaths": [],
            }),
        )
        .expect_err("a folder with no confirmed duplicate in `groups` must be rejected");

        assert!(err.as_str().unwrap().contains("no confirmed duplicate"));
        assert!(
            small.join("1.txt").exists(),
            "a rejected plan must not touch the filesystem"
        );
    }

    /// `GUI-MEDIA-PREVIEW`: a real, small, supported image file round-trips
    /// through `read_preview` as a correctly-shaped `data:` URI.
    #[test]
    fn read_preview_returns_a_data_url_for_a_supported_image() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.png");
        fs::write(&path, b"pretend png bytes").unwrap();

        let response = invoke(
            "read_preview",
            json!({ "path": path.display().to_string() }),
        )
        .expect("read_preview should succeed for a supported image");

        let data_url = response["dataUrl"].as_str().unwrap();
        assert!(data_url.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn read_preview_rejects_an_unsupported_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("video.mp4");
        fs::write(&path, b"video bytes").unwrap();

        let err = invoke(
            "read_preview",
            json!({ "path": path.display().to_string() }),
        )
        .expect_err("video is not a supported preview target yet");
        assert!(err.as_str().unwrap().contains("unsupported"));
    }

    #[test]
    fn read_preview_rejects_a_nonexistent_file() {
        let err = invoke(
            "read_preview",
            json!({ "path": "/does/not/exist/at/all.png" }),
        )
        .expect_err("a missing file must be rejected");
        assert!(!err.as_str().unwrap().is_empty());
    }

    /// `SCAN-PROFILES`: an empty name is rejected before any directory
    /// resolution or disk I/O happens — this is the one `save_scan_profile`
    /// behavior testable at the IPC layer without touching the real OS
    /// config directory (`profiles::default_profiles_dir()`'s actual
    /// resolution has no test seam; see `profiles.rs`'s own doc comment and
    /// its hermetic tempdir-based tests for the rest of the module's
    /// coverage).
    #[test]
    fn save_scan_profile_rejects_an_empty_name() {
        let err = invoke(
            "save_scan_profile",
            json!({ "name": "", "root": "/data", "options": {} }),
        )
        .expect_err("an empty profile name must be rejected");
        assert!(err.as_str().unwrap().contains("name"));
    }

    /// `DETECTION-PERCEPTUAL-IMAGES`, ADR-0030: two near-identical (but not
    /// byte-identical) real images are clustered by `find_similar_images`.
    /// Mirrors `perceptual::tests::find_similar_images_groups_a_real_near_identical_pair_on_disk`
    /// at the IPC boundary.
    #[test]
    fn find_similar_images_groups_a_real_near_identical_pair() {
        let dir = tempfile::tempdir().unwrap();
        let gradient = |offset: u8| {
            image::ImageBuffer::from_fn(64, 64, |x, y| {
                let v = ((x * 255 / 64) + (y * 255 / 64)) as u8;
                image::Rgb([v.saturating_add(offset); 3])
            })
        };
        image::DynamicImage::ImageRgb8(gradient(0))
            .save(dir.path().join("a.png"))
            .unwrap();
        image::DynamicImage::ImageRgb8(gradient(5))
            .save(dir.path().join("b.png"))
            .unwrap();

        let response = invoke(
            "find_similar_images",
            json!({ "root": dir.path().display().to_string(), "options": {} }),
        )
        .expect("find_similar_images should succeed");

        let groups = response.as_array().expect("response should be an array");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["paths"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn find_similar_images_rejects_a_nonexistent_root() {
        let err = invoke(
            "find_similar_images",
            json!({ "root": "/does/not/exist/at/all", "options": {} }),
        )
        .expect_err("a nonexistent root must be rejected");
        assert!(err.as_str().unwrap().contains("does not exist"));
    }

    #[test]
    fn save_scan_profile_rejects_a_whitespace_only_name() {
        let err = invoke(
            "save_scan_profile",
            json!({ "name": "   ", "root": "/data", "options": {} }),
        )
        .expect_err("a whitespace-only profile name must be rejected");
        assert!(err.as_str().unwrap().contains("name"));
    }

    #[test]
    fn find_duplicate_folders_rejects_a_nonexistent_root() {
        let err = invoke(
            "find_duplicate_folders",
            json!({
                "root": "/does/not/exist/at/all",
                "groups": [],
                "options": {},
            }),
        )
        .expect_err("a nonexistent root must be rejected");

        assert!(err.as_str().unwrap().contains("does not exist"));
    }

    #[test]
    fn run_action_rejects_an_unknown_kind() {
        let err = invoke(
            "run_action",
            json!({
                "group": {"size": 1, "paths": ["/a", "/b"]},
                "kind": "frobnicate",
                "apply": false,
                "referencePaths": [],
            }),
        )
        .expect_err("an unknown action kind must be rejected");

        assert!(err.as_str().unwrap().contains("frobnicate"));
    }

    #[test]
    fn start_scan_accepts_a_valid_root_and_returns_immediately() {
        let dir = tempfile::tempdir().unwrap();
        let response = invoke(
            "start_scan",
            json!({
                "root": dir.path().display().to_string(),
                "options": {},
            }),
        );
        assert!(
            response.is_ok(),
            "start_scan should accept a valid directory root"
        );
    }

    #[test]
    fn start_scan_treats_a_windows_copy_as_path_quoted_root_as_the_real_directory() {
        // Reproduces a real Windows GUI session: Explorer's "Copy as path"
        // wraps the path in double quotes, and pasting that verbatim into
        // the root-path field used to make the background scan thread
        // emit a "root path does not exist" error event -- the quotes
        // were literal characters in the string, invisible until the
        // actual scan-event stream is inspected (start_scan's own IPC
        // response is always Ok(()) regardless of root validity, since
        // it just spawns the scan thread and returns immediately).
        use std::sync::mpsc;
        use std::time::{Duration, Instant};
        use tauri::Listener;

        let dir = tempfile::tempdir().unwrap();
        let quoted_root = format!("\"{}\"", dir.path().display());

        let app = mock_builder()
            .invoke_handler(tauri::generate_handler![
                super::start_scan,
                super::run_action,
                super::choose_keep,
                super::find_duplicate_folders,
                super::run_folder_action,
                super::read_preview,
                super::list_scan_profiles,
                super::save_scan_profile,
                super::delete_scan_profile,
                super::find_similar_images,
                super::list_directory
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("failed to build mock app");
        let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        let (tx, rx) = mpsc::channel::<String>();
        app.listen("scan-event", move |event| {
            let _ = tx.send(event.payload().to_string());
        });

        let response = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "start_scan".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: InvokeBody::Json(json!({"root": quoted_root, "options": {}})),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        );
        assert!(response.is_ok(), "start_scan's own IPC call should succeed");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut finished = false;
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(payload) => {
                    assert!(
                        !payload.contains("does not exist"),
                        "the quoted root should resolve to a real directory, not error: {payload}"
                    );
                    if payload.contains("\"type\":\"finished\"") {
                        finished = true;
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        assert!(finished, "expected a finished scan-event within 5s");
    }

    #[test]
    fn list_directory_lists_only_real_subdirectories_sorted_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("zeta")).unwrap();
        fs::create_dir(dir.path().join("Alpha")).unwrap();
        fs::write(dir.path().join("not-a-dir.txt"), b"file").unwrap();
        fs::create_dir(dir.path().join(".hidden")).unwrap();

        let response = invoke(
            "list_directory",
            json!({ "path": dir.path().display().to_string() }),
        )
        .expect("list_directory should succeed");

        let names: Vec<String> = response
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            vec!["Alpha".to_string(), "zeta".to_string()],
            "files and dotfiles must be excluded, real directories sorted case-insensitively"
        );
    }

    #[test]
    fn list_directory_reports_has_children_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let with_child = dir.path().join("with-child");
        fs::create_dir(&with_child).unwrap();
        fs::create_dir(with_child.join("nested")).unwrap();
        fs::create_dir(dir.path().join("leaf")).unwrap();

        let response = invoke(
            "list_directory",
            json!({ "path": dir.path().display().to_string() }),
        )
        .expect("list_directory should succeed");

        let by_name = |name: &str| {
            response
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["name"] == name)
                .unwrap()
        };
        assert_eq!(by_name("with-child")["hasChildren"], true);
        assert_eq!(by_name("leaf")["hasChildren"], false);
    }

    #[test]
    fn list_directory_on_an_unreadable_path_returns_an_empty_list_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");

        let response = invoke(
            "list_directory",
            json!({ "path": missing.display().to_string() }),
        )
        .expect("a missing directory must degrade to an empty list, not an IPC error");

        assert_eq!(response, json!([]));
    }

    #[test]
    fn list_directory_without_a_path_returns_at_least_one_browse_root() {
        let response = invoke("list_directory", json!({ "path": null }))
            .expect("list_directory with no path should succeed");

        let roots = response.as_array().unwrap();
        assert!(
            !roots.is_empty(),
            "expected at least one platform browse root (home directory and/or filesystem root)"
        );
    }
}
