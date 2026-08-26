//! Choosing which copy in a [`DuplicateGroup`] to keep by a named rule,
//! instead of always the alphabetically-first path — `SELECTION-RULES`.
//!
//! Every rule here breaks ties (and falls back on unreadable metadata) by
//! preferring the earliest path in `group.paths`' existing sorted order —
//! i.e. the alphabetically-first one — so choosing
//! [`Rule::AlphabeticallyFirst`] is bit-for-bit identical to this project's
//! long-standing default, and every other rule degrades to that default
//! whenever it can't actually distinguish two paths.
//!
//! Deliberately does not cover folder-level `Exact` cluster selection
//! (`FolderMatch::Exact`'s keep-choice stays alphabetically-first, matching
//! the CLI's/GUI's existing convention) — a folder's own timestamp is a
//! weaker, less obviously meaningful signal than a single file's, and
//! bundling it in here would widen this unit's scope beyond what the
//! research behind it (`docs/roadmap/DEDUP-GAP-IMPLEMENTATION-PLAN.md`)
//! actually asked for.
//!
//! Also deliberately has no `Largest`/`Smallest` rule: every path in a
//! `DuplicateGroup` shares the exact same size by definition (this
//! project's whole detection model is hash-verified exact-content
//! matching), so a size-based rule could never distinguish anything —
//! unlike competitors whose "keep the largest" rules make sense because
//! their matches aren't guaranteed byte-identical.

use std::path::Path;
use std::sync::Arc;

use crate::model::DuplicateGroup;

/// A named rule for choosing which path in a [`DuplicateGroup`] to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rule {
    /// Keep the alphabetically-first path — this project's existing
    /// default, kept as an explicit, nameable choice rather than only an
    /// implicit fallback.
    #[default]
    AlphabeticallyFirst,
    /// Keep the most recently modified copy.
    Newest,
    /// Keep the least recently modified copy.
    Oldest,
    /// Keep the copy at the shallowest path (fewest path components).
    ShortestPath,
    /// Keep the copy at the deepest path (most path components).
    LongestPath,
}

/// Chooses which path in `group.paths` to keep under `rule`, returning the
/// chosen path and a one-line, human-readable reason for the choice — a
/// cheap "why this one" explanation, without needing any ranking model to
/// justify it.
pub fn choose_keep(group: &DuplicateGroup, rule: Rule) -> (Arc<Path>, String) {
    match rule {
        Rule::AlphabeticallyFirst => (group.paths[0].clone(), "alphabetically first".to_string()),
        Rule::Newest => by_modified(group, true),
        Rule::Oldest => by_modified(group, false),
        Rule::ShortestPath => by_depth(group, true),
        Rule::LongestPath => by_depth(group, false),
    }
}

/// Picks the path with the newest (or oldest) modification time, skipping
/// any path whose metadata can't be read. Iterates in `group.paths`'
/// existing sorted order and only replaces the current best on a strict
/// improvement, so the first (alphabetically-first) path among ties — or
/// among several unreadable paths — wins, matching every other rule's tie-
/// breaking behavior.
fn by_modified(group: &DuplicateGroup, newest: bool) -> (Arc<Path>, String) {
    let mut best: Option<(usize, std::time::SystemTime)> = None;
    for (index, path) in group.paths.iter().enumerate() {
        let Some(modified) = std::fs::metadata(path).ok().and_then(|m| m.modified().ok()) else {
            continue;
        };
        let is_better = match best {
            None => true,
            Some((_, best_modified)) => {
                if newest {
                    modified > best_modified
                } else {
                    modified < best_modified
                }
            }
        };
        if is_better {
            best = Some((index, modified));
        }
    }
    match best {
        Some((index, _)) => (
            group.paths[index].clone(),
            format!(
                "{} modification time",
                if newest { "most recent" } else { "oldest" }
            ),
        ),
        None => (
            group.paths[0].clone(),
            "alphabetically first (no file's modification time was readable)".to_string(),
        ),
    }
}

/// Picks the path with the fewest (or most) path components.
fn by_depth(group: &DuplicateGroup, shortest: bool) -> (Arc<Path>, String) {
    let mut best_index = 0;
    let mut best_depth = group.paths[0].components().count();
    for (index, path) in group.paths.iter().enumerate().skip(1) {
        let depth = path.components().count();
        let is_better = if shortest {
            depth < best_depth
        } else {
            depth > best_depth
        };
        if is_better {
            best_index = index;
            best_depth = depth;
        }
    }
    (
        group.paths[best_index].clone(),
        format!("{} path", if shortest { "shortest" } else { "longest" }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn group(paths: Vec<PathBuf>) -> DuplicateGroup {
        DuplicateGroup {
            size: 3,
            paths: paths.into_iter().map(Into::into).collect(),
        }
    }

    #[test]
    fn alphabetically_first_always_picks_paths_zero() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        let (keep, reason) = choose_keep(&group(vec![a.clone(), b]), Rule::AlphabeticallyFirst);
        assert_eq!(keep.as_ref(), a.as_path());
        assert_eq!(reason, "alphabetically first");
    }

    #[test]
    fn newest_picks_the_most_recently_modified_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(&b, b"dup").unwrap();

        let (keep, reason) = choose_keep(&group(vec![a, b.clone()]), Rule::Newest);
        assert_eq!(keep.as_ref(), b.as_path());
        assert_eq!(reason, "most recent modification time");
    }

    #[test]
    fn oldest_picks_the_least_recently_modified_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(&b, b"dup").unwrap();

        let (keep, reason) = choose_keep(&group(vec![a.clone(), b]), Rule::Oldest);
        assert_eq!(keep.as_ref(), a.as_path());
        assert_eq!(reason, "oldest modification time");
    }

    #[test]
    fn newest_falls_back_to_alphabetically_first_when_no_metadata_is_readable() {
        let missing_a = PathBuf::from("/definitely/does/not/exist/a.txt");
        let missing_b = PathBuf::from("/definitely/does/not/exist/b.txt");

        let (keep, reason) = choose_keep(&group(vec![missing_a.clone(), missing_b]), Rule::Newest);
        assert_eq!(keep.as_ref(), missing_a.as_path());
        assert!(reason.contains("alphabetically first"));
    }

    #[test]
    fn shortest_path_picks_the_shallowest_file() {
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("a/b/c/deep.txt");
        let shallow = dir.path().join("shallow.txt");
        fs::create_dir_all(deep.parent().unwrap()).unwrap();
        fs::write(&deep, b"dup").unwrap();
        fs::write(&shallow, b"dup").unwrap();

        let (keep, reason) = choose_keep(&group(vec![deep, shallow.clone()]), Rule::ShortestPath);
        assert_eq!(keep.as_ref(), shallow.as_path());
        assert_eq!(reason, "shortest path");
    }

    #[test]
    fn longest_path_picks_the_deepest_file() {
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("a/b/c/deep.txt");
        let shallow = dir.path().join("shallow.txt");
        fs::create_dir_all(deep.parent().unwrap()).unwrap();
        fs::write(&deep, b"dup").unwrap();
        fs::write(&shallow, b"dup").unwrap();

        let (keep, reason) = choose_keep(&group(vec![deep.clone(), shallow]), Rule::LongestPath);
        assert_eq!(keep.as_ref(), deep.as_path());
        assert_eq!(reason, "longest path");
    }

    #[test]
    fn ties_break_toward_the_alphabetically_first_path() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"dup").unwrap();
        fs::write(&b, b"dup").unwrap();

        // Same directory depth and (as close as this filesystem lets two
        // back-to-back writes get) the same modification time -- every
        // rule that can't find a real difference must still land on the
        // same, predictable choice.
        for rule in [
            Rule::Newest,
            Rule::Oldest,
            Rule::ShortestPath,
            Rule::LongestPath,
        ] {
            let (keep, _) = choose_keep(&group(vec![a.clone(), b.clone()]), rule);
            assert_eq!(
                keep.as_ref(),
                a.as_path(),
                "rule {rule:?} should break a tie toward the alphabetically-first path"
            );
        }
    }
}
