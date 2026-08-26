//! `#[tauri::command]` entry points invoked from the frontend via
//! `window.__TAURI__.core.invoke`. Kept thin — all real logic lives in
//! `rusty_fclone_core`; this module only translates to/from
//! [`crate::payload`]'s wire types and drives the background scan thread.

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Runtime};

use rusty_fclone_core::select;
use rusty_fclone_core::{action, find_folder_duplicates, folder_action};

use crate::payload::{
    normalize_path_input, parse_action_kind, parse_keep_rule, ActionResultPayload,
    ChooseKeepPayload, FolderActionResultPayload, FolderMatchPayload, GroupPayload,
    ScanEventPayload, ScanOptionsPayload,
};

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
) -> Result<ActionResultPayload, String> {
    let kind = parse_action_kind(&kind)?;
    let group = group.into();
    // The kept path is always `group.paths[0]` here -- the frontend
    // resolves which path that should be *before* calling this command
    // (either a manual keep-choice badge, or `choose_keep` below for a
    // non-default rule) and sends it as the group's first path, the same
    // reordering trick manual keep-choice already used before
    // `SELECTION-RULES` existed (no new core API for "which path to
    // keep"). `keep_reason` is likewise resolved by the frontend from
    // whichever of those two paths it took, and just passed through here
    // for display -- `action::plan` itself has no concept of "why".
    let plan = action::plan(&group, kind);
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
pub fn choose_keep(group: GroupPayload, rule: String) -> Result<ChooseKeepPayload, String> {
    let rule = parse_keep_rule(&rule)?;
    let group = group.into();
    let (keep, reason) = select::choose_keep(&group, rule);
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
pub fn run_folder_action(
    removed: String,
    kept: String,
    groups: Vec<GroupPayload>,
    options: ScanOptionsPayload,
    kind: String,
    apply: bool,
) -> Result<FolderActionResultPayload, String> {
    let kind = parse_action_kind(&kind)?;
    let removed = PathBuf::from(normalize_path_input(&removed));
    let kept = PathBuf::from(normalize_path_input(&kept));
    let groups: Vec<_> = groups.into_iter().map(Into::into).collect();
    let options = options.into();
    let plan = folder_action::plan_folder(&removed, &kept, &groups, &options, kind)
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
                super::run_folder_action
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
            }),
        )
        .expect("run_action should succeed");

        assert_eq!(
            response["plan"]["keepReason"],
            "most recent modification time"
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
            }),
        )
        .expect_err("a folder with no confirmed duplicate in `groups` must be rejected");

        assert!(err.as_str().unwrap().contains("no confirmed duplicate"));
        assert!(
            small.join("1.txt").exists(),
            "a rejected plan must not touch the filesystem"
        );
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
                super::run_folder_action
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
}
