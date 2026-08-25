//! `#[tauri::command]` entry points invoked from the frontend via
//! `window.__TAURI__.core.invoke`. Kept thin — all real logic lives in
//! `rusty_fclone_core`; this module only translates to/from
//! [`crate::payload`]'s wire types and drives the background scan thread.

use tauri::{AppHandle, Emitter, Runtime};

use rusty_fclone_core::action;

use crate::payload::{
    normalize_path_input, parse_action_kind, ActionResultPayload, GroupPayload, ScanEventPayload,
    ScanOptionsPayload,
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
    apply: bool,
) -> Result<ActionResultPayload, String> {
    let kind = parse_action_kind(&kind)?;
    let group = group.into();
    let plan = action::plan(&group, kind);
    let applied = if apply {
        Some((&action::apply(&plan)).into())
    } else {
        None
    };
    Ok(ActionResultPayload {
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
                super::run_action
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
                super::run_action
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
