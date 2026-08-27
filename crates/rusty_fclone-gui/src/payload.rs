//! Wire types shared between the Rust backend and the frontend, over Tauri's
//! `invoke`/`emit` JSON boundary. Deliberately separate from
//! `rusty_fclone_core`'s own types (which carry no `serde` impls) rather
//! than adding a `serde` feature to the core crate for one consumer — same
//! shape and field names as the CLI's existing `--format json` NDJSON
//! output (`rusty_fclone-cli/src/main.rs`, ADR-0015 / `CLI-UX-001`), so a
//! reader who knows one already knows the other.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use rusty_fclone_core::action::{ActionKind, ActionPlan, ApplyReport, FileAction};
use rusty_fclone_core::folder_action::{FolderActionPlan, FolderApplyReport};
use rusty_fclone_core::select::Rule as SelectRule;
use rusty_fclone_core::{
    DuplicateGroup, FileError, FolderMatch, ScanOptions, ScanProgress, ScanSummary,
};

/// Scan tunables sent from the frontend. Mirrors [`ScanOptions`]; every
/// field is optional here so the frontend only needs to send what the user
/// actually changed from the default.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOptionsPayload {
    #[serde(default)]
    pub follow_symlinks: bool,
    #[serde(default)]
    pub cross_filesystems: bool,
    #[serde(default)]
    pub verify_matches: bool,
    pub small_file_threshold: Option<u64>,
    pub partial_hash_sample_size: Option<u64>,
    pub io_threads: Option<usize>,
    pub cache_path: Option<String>,
    pub fclones_import_path: Option<String>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub include_extensions: Option<Vec<String>>,
    pub exclude_extensions: Option<Vec<String>>,
    #[serde(default)]
    pub exclude_paths: Vec<String>,
}

/// Turns a frontend-supplied list of path strings into normalized
/// [`PathBuf`]s -- shared by the reference-folder list
/// (`ACTION-REFERENCE-FOLDERS`) and `ScanOptionsPayload::exclude_paths`'s
/// existing `From` impl below.
pub fn normalize_path_list(paths: &[String]) -> Vec<PathBuf> {
    paths
        .iter()
        .map(|s| PathBuf::from(normalize_path_input(s)))
        .collect()
}

impl From<ScanOptionsPayload> for ScanOptions {
    fn from(p: ScanOptionsPayload) -> Self {
        let defaults = ScanOptions::default();
        ScanOptions {
            follow_symlinks: p.follow_symlinks,
            cross_filesystems: p.cross_filesystems,
            verify_matches: p.verify_matches,
            small_file_threshold: p
                .small_file_threshold
                .unwrap_or(defaults.small_file_threshold),
            partial_hash_sample_size: p
                .partial_hash_sample_size
                .unwrap_or(defaults.partial_hash_sample_size),
            io_threads: p.io_threads,
            cache_path: p
                .cache_path
                .as_deref()
                .map(normalize_path_input)
                .map(PathBuf::from),
            fclones_import_path: p
                .fclones_import_path
                .as_deref()
                .map(normalize_path_input)
                .map(PathBuf::from),
            min_size: p.min_size,
            max_size: p.max_size,
            include_extensions: p.include_extensions,
            exclude_extensions: p.exclude_extensions,
            exclude_paths: normalize_path_list(&p.exclude_paths),
        }
    }
}

/// One `ScanEvent`, shaped for `emit("scan-event", ...)`. A tagged enum so
/// the frontend can switch on `type` the same way the CLI's NDJSON readers
/// do.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScanEventPayload {
    DuplicateGroup {
        size: u64,
        paths: Vec<String>,
    },
    Error {
        path: String,
        message: String,
    },
    #[serde(rename_all = "camelCase")]
    Progress {
        files_scanned: u64,
        bytes_scanned: u64,
    },
    Finished(ScanSummaryPayload),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummaryPayload {
    pub files_scanned: u64,
    pub bytes_scanned: u64,
    pub duplicate_groups: u64,
    pub duplicate_files: u64,
}

impl From<&DuplicateGroup> for ScanEventPayload {
    fn from(group: &DuplicateGroup) -> Self {
        ScanEventPayload::DuplicateGroup {
            size: group.size,
            paths: group
                .paths
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
        }
    }
}

impl From<&FileError> for ScanEventPayload {
    fn from(err: &FileError) -> Self {
        ScanEventPayload::Error {
            path: err.path.display().to_string(),
            message: err.source.to_string(),
        }
    }
}

impl From<ScanProgress> for ScanEventPayload {
    fn from(p: ScanProgress) -> Self {
        ScanEventPayload::Progress {
            files_scanned: p.files_scanned,
            bytes_scanned: p.bytes_scanned,
        }
    }
}

impl From<ScanSummary> for ScanEventPayload {
    fn from(s: ScanSummary) -> Self {
        ScanEventPayload::Finished(ScanSummaryPayload {
            files_scanned: s.files_scanned,
            bytes_scanned: s.bytes_scanned,
            duplicate_groups: s.duplicate_groups,
            duplicate_files: s.duplicate_files,
        })
    }
}

/// A [`DuplicateGroup`] as sent back from the frontend for an action —
/// the frontend already has this data from the `scan-event` stream, so
/// there's no need to re-scan to plan/apply an action on it.
#[derive(Debug, Deserialize)]
pub struct GroupPayload {
    pub size: u64,
    pub paths: Vec<String>,
}

impl From<GroupPayload> for DuplicateGroup {
    fn from(p: GroupPayload) -> Self {
        DuplicateGroup {
            size: p.size,
            paths: p
                .paths
                .into_iter()
                .map(|s| PathBuf::from(s).into())
                .collect(),
        }
    }
}

/// One `FolderMatch` (ADR-0021), shaped for `find_duplicate_folders`'s
/// response. A tagged enum matching `ScanEventPayload`'s convention
/// (snake_case `type` tag, camelCase fields) — mirrors the CLI's
/// `folder_exact`/`folder_contained` NDJSON shapes (`CLI-UX-001` FR-012).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FolderMatchPayload {
    #[serde(rename_all = "camelCase")]
    Exact {
        folders: Vec<String>,
        file_count: u64,
        bytes: u64,
    },
    #[serde(rename_all = "camelCase")]
    Contained {
        subset: String,
        superset: String,
        file_count: u64,
        bytes: u64,
    },
}

impl From<&FolderMatch> for FolderMatchPayload {
    fn from(m: &FolderMatch) -> Self {
        match m {
            FolderMatch::Exact {
                folders,
                file_count,
                bytes,
            } => FolderMatchPayload::Exact {
                folders: folders.iter().map(|p| p.display().to_string()).collect(),
                file_count: *file_count,
                bytes: *bytes,
            },
            FolderMatch::Contained {
                subset,
                superset,
                file_count,
                bytes,
            } => FolderMatchPayload::Contained {
                subset: subset.display().to_string(),
                superset: superset.display().to_string(),
                file_count: *file_count,
                bytes: *bytes,
            },
        }
    }
}

/// Strips whitespace and one layer of surrounding matching quotes from a
/// user-typed path field. Windows Explorer's "Copy as path" wraps the
/// copied path in double quotes (so it pastes cleanly into `cmd.exe`,
/// which needs quoting for paths containing spaces) — pasted verbatim
/// into a plain text input, those quotes become literal characters in
/// the string, and `Path::new("\"C:\\...\"").is_dir()` is false even
/// though the path itself is real. Confirmed against a real Windows GUI
/// session hitting exactly this.
pub fn normalize_path_input(s: &str) -> String {
    let trimmed = s.trim();
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
        })
        .unwrap_or(trimmed);
    unquoted.trim().to_string()
}

/// `archive_dir` is only consulted for `"move"`/`"copy"` (required there,
/// ignored otherwise) -- the archive-folder destination those two kinds
/// carry as part of `ActionKind` itself, not a separate parameter
/// (`ACTION-MOVE-COPY`, ADR-0026).
pub fn parse_action_kind(kind: &str, archive_dir: Option<&Path>) -> Result<ActionKind, String> {
    match kind {
        "delete" => Ok(ActionKind::Delete),
        "trash" => Ok(ActionKind::Trash),
        "hardlink" => Ok(ActionKind::Hardlink),
        "reflink" => Ok(ActionKind::Reflink),
        "move" => archive_dir
            .map(|dir| ActionKind::Move(dir.to_path_buf()))
            .ok_or_else(|| "action kind \"move\" requires an archive directory".to_string()),
        "copy" => archive_dir
            .map(|dir| ActionKind::Copy(dir.to_path_buf()))
            .ok_or_else(|| "action kind \"copy\" requires an archive directory".to_string()),
        other => Err(format!("unknown action kind: {other}")),
    }
}

/// Parses the frontend's `keepRule` string into a [`SelectRule`]
/// (`SELECTION-RULES`). Mirrors `parse_action_kind`'s shape.
pub fn parse_keep_rule(rule: &str) -> Result<SelectRule, String> {
    match rule {
        "alphabetical" => Ok(SelectRule::AlphabeticallyFirst),
        "newest" => Ok(SelectRule::Newest),
        "oldest" => Ok(SelectRule::Oldest),
        "shortest_path" => Ok(SelectRule::ShortestPath),
        "longest_path" => Ok(SelectRule::LongestPath),
        other => Err(format!("unknown keep rule: {other}")),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionPlanPayload {
    pub kept: String,
    pub keep_reason: String,
    pub planned: Vec<String>,
    pub bytes_reclaimed: u64,
}

/// Response shape for the `choose_keep` command — [`SelectRule`] applied to
/// a group, without planning or applying an action (`SELECTION-RULES`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChooseKeepPayload {
    pub keep: String,
    pub reason: String,
}

impl From<(&ActionPlan, &str)> for ActionPlanPayload {
    fn from((plan, keep_reason): (&ActionPlan, &str)) -> Self {
        ActionPlanPayload {
            kept: plan.kept.display().to_string(),
            keep_reason: keep_reason.to_string(),
            planned: plan
                .actions
                .iter()
                .map(|a: &FileAction| a.path.display().to_string())
                .collect(),
            bytes_reclaimed: plan.bytes_reclaimed,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyReportPayload {
    pub succeeded: Vec<String>,
    pub failed: Vec<String>,
    pub bytes_reclaimed: u64,
}

impl From<&ApplyReport> for ApplyReportPayload {
    fn from(report: &ApplyReport) -> Self {
        ApplyReportPayload {
            succeeded: report
                .succeeded
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            failed: report
                .failed
                .iter()
                .map(|e| e.path.display().to_string())
                .collect(),
            bytes_reclaimed: report.bytes_reclaimed,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResultPayload {
    pub plan: ActionPlanPayload,
    pub applied: Option<ApplyReportPayload>,
}

/// A [`FolderActionPlan`] (ADR-0023), shaped for `run_folder_action`'s
/// response — the folder-level counterpart of [`ActionPlanPayload`].
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderActionPlanPayload {
    pub kept: String,
    pub removed: String,
    pub file_count: u64,
    pub bytes_reclaimed: u64,
}

impl From<&FolderActionPlan> for FolderActionPlanPayload {
    fn from(plan: &FolderActionPlan) -> Self {
        FolderActionPlanPayload {
            kept: plan.kept.display().to_string(),
            removed: plan.removed.display().to_string(),
            file_count: plan.pairs.len() as u64,
            bytes_reclaimed: plan.bytes_reclaimed,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderApplyReportPayload {
    pub succeeded: Vec<String>,
    pub failed: Vec<String>,
    pub bytes_reclaimed: u64,
    pub directory_removed: bool,
}

impl From<&FolderApplyReport> for FolderApplyReportPayload {
    fn from(report: &FolderApplyReport) -> Self {
        FolderApplyReportPayload {
            succeeded: report
                .succeeded
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            failed: report
                .failed
                .iter()
                .map(|e| e.path.display().to_string())
                .collect(),
            bytes_reclaimed: report.bytes_reclaimed,
            directory_removed: report.directory_removed,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderActionResultPayload {
    pub plan: FolderActionPlanPayload,
    pub applied: Option<FolderApplyReportPayload>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn normalize_path_input_strips_windows_copy_as_path_quoting() {
        assert_eq!(
            normalize_path_input("\"C:\\Users\\me\\Downloads\""),
            "C:\\Users\\me\\Downloads"
        );
    }

    #[test]
    fn normalize_path_input_strips_surrounding_single_quotes_too() {
        assert_eq!(normalize_path_input("'/home/me/photos'"), "/home/me/photos");
    }

    #[test]
    fn normalize_path_input_trims_whitespace_around_and_inside_the_quotes() {
        assert_eq!(
            normalize_path_input("  \"  /home/me/photos  \"  "),
            "/home/me/photos"
        );
    }

    #[test]
    fn normalize_path_input_leaves_an_unquoted_path_unchanged() {
        assert_eq!(normalize_path_input("/home/me/photos"), "/home/me/photos");
    }

    #[test]
    fn normalize_path_input_does_not_strip_a_lone_leading_or_trailing_quote() {
        // Mismatched quotes are left alone rather than guessed at -- a
        // path that genuinely starts or ends with a quote character is
        // vanishingly unlikely, but silently mangling one that does would
        // be worse than leaving it (and failing the same "not found"
        // error as before this fix, not a new failure mode).
        assert_eq!(
            normalize_path_input("\"/home/me/photos"),
            "\"/home/me/photos"
        );
        assert_eq!(
            normalize_path_input("/home/me/photos\""),
            "/home/me/photos\""
        );
    }

    #[test]
    fn scan_options_payload_applies_defaults_for_omitted_fields() {
        let payload = ScanOptionsPayload {
            follow_symlinks: true,
            cross_filesystems: false,
            verify_matches: false,
            small_file_threshold: None,
            partial_hash_sample_size: None,
            io_threads: Some(4),
            cache_path: Some("cache.redb".into()),
            fclones_import_path: None,
            min_size: None,
            max_size: None,
            include_extensions: None,
            exclude_extensions: None,
            exclude_paths: Vec::new(),
        };
        let options: ScanOptions = payload.into();
        let defaults = ScanOptions::default();

        assert!(options.follow_symlinks);
        assert_eq!(options.small_file_threshold, defaults.small_file_threshold);
        assert_eq!(
            options.partial_hash_sample_size,
            defaults.partial_hash_sample_size
        );
        assert_eq!(options.io_threads, Some(4));
        assert_eq!(options.cache_path, Some(PathBuf::from("cache.redb")));
        assert_eq!(options.fclones_import_path, None);
    }

    #[test]
    fn scan_options_payload_normalizes_pasted_quoted_paths_too() {
        let payload = ScanOptionsPayload {
            follow_symlinks: false,
            cross_filesystems: false,
            verify_matches: false,
            small_file_threshold: None,
            partial_hash_sample_size: None,
            io_threads: None,
            cache_path: Some("\"C:\\cache\\hashes.redb\"".into()),
            fclones_import_path: Some("\"C:\\Users\\me\\.cache\\fclones\"".into()),
            min_size: None,
            max_size: None,
            include_extensions: None,
            exclude_extensions: None,
            exclude_paths: Vec::new(),
        };
        let options: ScanOptions = payload.into();

        assert_eq!(
            options.cache_path,
            Some(PathBuf::from("C:\\cache\\hashes.redb"))
        );
        assert_eq!(
            options.fclones_import_path,
            Some(PathBuf::from("C:\\Users\\me\\.cache\\fclones"))
        );
    }

    #[test]
    fn scan_options_payload_passes_through_filters_and_normalizes_exclude_paths() {
        let payload = ScanOptionsPayload {
            follow_symlinks: false,
            cross_filesystems: false,
            verify_matches: false,
            small_file_threshold: None,
            partial_hash_sample_size: None,
            io_threads: None,
            cache_path: None,
            fclones_import_path: None,
            min_size: Some(1024),
            max_size: Some(1_000_000),
            include_extensions: Some(vec!["jpg".to_string(), "png".to_string()]),
            exclude_extensions: Some(vec!["tmp".to_string()]),
            exclude_paths: vec!["\"/home/me/node_modules\"".to_string()],
        };
        let options: ScanOptions = payload.into();

        assert_eq!(options.min_size, Some(1024));
        assert_eq!(options.max_size, Some(1_000_000));
        assert_eq!(
            options.include_extensions,
            Some(vec!["jpg".to_string(), "png".to_string()])
        );
        assert_eq!(options.exclude_extensions, Some(vec!["tmp".to_string()]));
        assert_eq!(
            options.exclude_paths,
            vec![PathBuf::from("/home/me/node_modules")]
        );
    }

    #[test]
    fn parse_action_kind_accepts_the_four_known_words_and_rejects_others() {
        assert_eq!(parse_action_kind("delete", None), Ok(ActionKind::Delete));
        assert_eq!(parse_action_kind("trash", None), Ok(ActionKind::Trash));
        assert_eq!(
            parse_action_kind("hardlink", None),
            Ok(ActionKind::Hardlink)
        );
        assert_eq!(parse_action_kind("reflink", None), Ok(ActionKind::Reflink));
        assert!(parse_action_kind("frobnicate", None).is_err());
    }

    #[test]
    fn parse_action_kind_move_and_copy_require_an_archive_directory() {
        let archive = Path::new("/archive");
        assert_eq!(
            parse_action_kind("move", Some(archive)),
            Ok(ActionKind::Move(archive.to_path_buf()))
        );
        assert_eq!(
            parse_action_kind("copy", Some(archive)),
            Ok(ActionKind::Copy(archive.to_path_buf()))
        );
        assert!(parse_action_kind("move", None).is_err());
        assert!(parse_action_kind("copy", None).is_err());
    }

    #[test]
    fn parse_keep_rule_accepts_the_five_known_words_and_rejects_others() {
        assert_eq!(
            parse_keep_rule("alphabetical"),
            Ok(SelectRule::AlphabeticallyFirst)
        );
        assert_eq!(parse_keep_rule("newest"), Ok(SelectRule::Newest));
        assert_eq!(parse_keep_rule("oldest"), Ok(SelectRule::Oldest));
        assert_eq!(
            parse_keep_rule("shortest_path"),
            Ok(SelectRule::ShortestPath)
        );
        assert_eq!(parse_keep_rule("longest_path"), Ok(SelectRule::LongestPath));
        assert!(parse_keep_rule("frobnicate").is_err());
    }

    #[test]
    fn group_payload_round_trips_into_a_duplicate_group() {
        let group: DuplicateGroup = GroupPayload {
            size: 42,
            paths: vec!["/a".into(), "/b".into()],
        }
        .into();

        assert_eq!(group.size, 42);
        assert_eq!(group.paths.len(), 2);
        assert_eq!(group.paths[0].as_ref(), Path::new("/a"));
        assert_eq!(group.paths[1].as_ref(), Path::new("/b"));
    }

    #[test]
    fn scan_event_payload_serializes_with_a_snake_case_type_tag() {
        let payload = ScanEventPayload::from(ScanProgress {
            files_scanned: 3,
            bytes_scanned: 100,
        });
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["type"], "progress");
        assert_eq!(json["filesScanned"], 3);
        assert_eq!(json["bytesScanned"], 100);
    }

    #[test]
    fn folder_match_exact_serializes_with_a_snake_case_type_tag_and_camel_case_fields() {
        let m = FolderMatch::Exact {
            folders: vec![PathBuf::from("/a"), PathBuf::from("/b")],
            file_count: 3,
            bytes: 42,
        };
        let payload = FolderMatchPayload::from(&m);
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["type"], "exact");
        assert_eq!(json["folders"], serde_json::json!(["/a", "/b"]));
        assert_eq!(json["fileCount"], 3);
        assert_eq!(json["bytes"], 42);
    }

    #[test]
    fn folder_match_contained_serializes_with_subset_and_superset_paths() {
        let m = FolderMatch::Contained {
            subset: PathBuf::from("/small"),
            superset: PathBuf::from("/big"),
            file_count: 1,
            bytes: 7,
        };
        let payload = FolderMatchPayload::from(&m);
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["type"], "contained");
        assert_eq!(json["subset"], "/small");
        assert_eq!(json["superset"], "/big");
        assert_eq!(json["fileCount"], 1);
        assert_eq!(json["bytes"], 7);
    }

    #[test]
    fn folder_action_plan_converts_with_camel_case_fields() {
        let plan = FolderActionPlan {
            kind: ActionKind::Delete,
            kept: PathBuf::from("/big"),
            removed: PathBuf::from("/small"),
            pairs: vec![],
            bytes_reclaimed: 42,
            protected_files_skipped: 0,
        };
        let payload = FolderActionPlanPayload::from(&plan);
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["kept"], "/big");
        assert_eq!(json["removed"], "/small");
        assert_eq!(json["fileCount"], 0);
        assert_eq!(json["bytesReclaimed"], 42);
    }

    #[test]
    fn duplicate_group_converts_to_a_scan_event_payload_with_display_paths() {
        let group = DuplicateGroup {
            size: 7,
            paths: vec![PathBuf::from("/x/a").into(), PathBuf::from("/x/b").into()],
        };
        let payload = ScanEventPayload::from(&group);
        match payload {
            ScanEventPayload::DuplicateGroup { size, paths } => {
                assert_eq!(size, 7);
                assert_eq!(paths, vec!["/x/a".to_string(), "/x/b".to_string()]);
            }
            other => panic!("expected DuplicateGroup, got {other:?}"),
        }
    }
}
