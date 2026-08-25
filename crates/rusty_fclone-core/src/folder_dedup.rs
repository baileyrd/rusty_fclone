//! Folder-level duplicate detection (ADR-0021): given a completed scan's
//! [`DuplicateGroup`]s, find directories whose entire recursive file
//! content is a duplicate — or a subset of a duplicate — of another
//! directory's. A post-scan analysis, not an extension of [`crate::scan`]'s
//! streaming contract (ADR-0004): a folder verdict needs the whole tree's
//! picture before it can be decided.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::ScanError;
use crate::model::{DuplicateGroup, ScanOptions};
use crate::traversal;

/// A folder-level duplicate relationship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderMatch {
    /// Two or more directories whose entire recursive file content is
    /// pairwise identical — same relative paths, same content, nothing
    /// extra on either side.
    Exact {
        folders: Vec<PathBuf>,
        file_count: u64,
        bytes: u64,
    },
    /// `subset`'s entire recursive file content exists, path-for-path and
    /// byte-for-byte, inside `superset` — which may have additional files
    /// `subset` doesn't.
    Contained {
        subset: PathBuf,
        superset: PathBuf,
        file_count: u64,
        bytes: u64,
    },
}

#[derive(Debug, Clone)]
struct Signature {
    /// Sorted `(path relative to the directory this signature is for,
    /// index into the caller's `groups` slice)` pairs — every file in the
    /// directory's recursive subtree, each pointing at the `DuplicateGroup`
    /// establishing its content identity.
    entries: Vec<(PathBuf, usize)>,
    file_count: u64,
    bytes: u64,
}

#[derive(Default)]
struct DirNode {
    /// Files directly inside this directory: (file name, group index if
    /// duplicated elsewhere — `None` means no duplicate exists anywhere in
    /// the tree, which disqualifies every ancestor from being a match's
    /// subset/exact side, size in bytes).
    files: Vec<(OsString, Option<usize>, u64)>,
    children: HashSet<PathBuf>,
}

/// Finds folder-level duplicates in the tree rooted at `root`, using the
/// `DuplicateGroup`s an earlier `scan(root, options)` call already
/// produced. `root` and `options` must match that earlier scan — this
/// function re-derives the complete file listing (including files with no
/// duplicate anywhere, which `scan` never surfaces) via its own
/// stat-only, no-hashing traversal.
pub fn find_folder_duplicates(
    root: &Path,
    groups: &[DuplicateGroup],
    options: &ScanOptions,
) -> Result<Vec<FolderMatch>, ScanError> {
    if !root.is_dir() {
        return Err(ScanError::InvalidRoot(root.to_path_buf()));
    }

    let mut path_to_group: HashMap<PathBuf, usize> = HashMap::new();
    for (idx, group) in groups.iter().enumerate() {
        for p in &group.paths {
            path_to_group.insert(p.to_path_buf(), idx);
        }
    }

    let nodes = build_tree(root, options, &path_to_group);
    let signatures = compute_all_signatures(&nodes);

    let mut by_signature: HashMap<Vec<(PathBuf, usize)>, Vec<PathBuf>> = HashMap::new();
    for (dir, sig) in &signatures {
        if let Some(sig) = sig {
            if sig.file_count > 0 {
                by_signature
                    .entry(sig.entries.clone())
                    .or_default()
                    .push(dir.clone());
            }
        }
    }

    let mut fully_duplicated: Vec<&PathBuf> = signatures
        .iter()
        .filter(|(_, sig)| sig.as_ref().is_some_and(|s| s.file_count > 0))
        .map(|(dir, _)| dir)
        .collect();
    fully_duplicated.sort_by_key(|d| d.components().count());

    let mut claimed: HashSet<PathBuf> = HashSet::new();
    let mut handled_signatures: HashSet<Vec<(PathBuf, usize)>> = HashSet::new();
    let mut matches = Vec::new();

    for dir in fully_duplicated {
        if is_or_is_descendant_of_claimed(dir, &claimed) {
            continue;
        }
        let sig = signatures[dir].as_ref().expect("filtered to Some above");

        let cluster = &by_signature[&sig.entries];
        if cluster.len() >= 2 {
            if !handled_signatures.insert(sig.entries.clone()) {
                continue;
            }
            let members: Vec<PathBuf> = cluster
                .iter()
                .filter(|m| !is_or_is_descendant_of_claimed(m, &claimed))
                .cloned()
                .collect();
            if members.len() >= 2 {
                for m in &members {
                    claimed.insert(m.clone());
                }
                matches.push(FolderMatch::Exact {
                    folders: members,
                    file_count: sig.file_count,
                    bytes: sig.bytes,
                });
            }
            continue;
        }

        // Not part of an exact cluster -- look for it as a proper subset
        // of some other directory.
        let supersets = find_supersets(dir, sig, groups);
        if !supersets.is_empty() {
            claimed.insert(dir.clone());
            for superset in supersets {
                matches.push(FolderMatch::Contained {
                    subset: dir.clone(),
                    superset,
                    file_count: sig.file_count,
                    bytes: sig.bytes,
                });
            }
        }
    }

    Ok(matches)
}

fn is_or_is_descendant_of_claimed(dir: &Path, claimed: &HashSet<PathBuf>) -> bool {
    let mut cur = Some(dir);
    while let Some(d) = cur {
        if claimed.contains(d) {
            return true;
        }
        cur = d.parent();
    }
    false
}

/// For every path recorded elsewhere in `subset`'s files' `DuplicateGroup`s,
/// checks whether stripping the file's path-relative-to-`subset` suffix
/// yields a valid candidate base directory containing a full duplicate of
/// `subset` — intersected across every file in `subset`, so only
/// directories matching *all* of `subset`'s content survive.
fn find_supersets(subset: &Path, sig: &Signature, groups: &[DuplicateGroup]) -> Vec<PathBuf> {
    let mut candidates: Option<HashSet<PathBuf>> = None;
    for (rel, group_idx) in &sig.entries {
        let mut this_file_bases: HashSet<PathBuf> = HashSet::new();
        for p in &groups[*group_idx].paths {
            if let Some(base) = strip_path_suffix(p, rel) {
                if base != subset {
                    this_file_bases.insert(base);
                }
            }
        }
        candidates = Some(match candidates {
            None => this_file_bases,
            Some(prev) => prev.intersection(&this_file_bases).cloned().collect(),
        });
        if candidates.as_ref().is_some_and(HashSet::is_empty) {
            return Vec::new();
        }
    }
    let mut result: Vec<PathBuf> = candidates.unwrap_or_default().into_iter().collect();
    result.sort();
    result
}

/// If `full` ends with `suffix` (component-wise), returns the remaining
/// prefix directory. `None` if `full` doesn't end with `suffix`, or ends
/// with nothing left over (i.e. `full == suffix`, no base directory).
fn strip_path_suffix(full: &Path, suffix: &Path) -> Option<PathBuf> {
    let full_components: Vec<_> = full.components().collect();
    let suffix_components: Vec<_> = suffix.components().collect();
    if suffix_components.len() >= full_components.len() {
        return None;
    }
    let split = full_components.len() - suffix_components.len();
    if full_components[split..] != suffix_components[..] {
        return None;
    }
    Some(full_components[..split].iter().collect())
}

fn build_tree(
    root: &Path,
    options: &ScanOptions,
    path_to_group: &HashMap<PathBuf, usize>,
) -> HashMap<PathBuf, DirNode> {
    let mut nodes: HashMap<PathBuf, DirNode> = HashMap::new();
    nodes.entry(root.to_path_buf()).or_default();

    traversal::traverse(
        root,
        options,
        |_err| {
            // A per-file error here just means that file is missing from
            // the folder-level picture, same tolerance as the main scan
            // (ADR-0004) -- it doesn't abort the analysis.
        },
        |candidate| {
            let path = candidate.path.to_path_buf();
            let Some(parent) = path.parent().map(Path::to_path_buf) else {
                return;
            };
            let Some(file_name) = path.file_name().map(|n| n.to_os_string()) else {
                return;
            };
            let group_id = path_to_group.get(&path).copied();
            nodes.entry(parent.clone()).or_default().files.push((
                file_name,
                group_id,
                candidate.size,
            ));

            let mut cur = parent;
            loop {
                if cur == root {
                    break;
                }
                let Some(cur_parent) = cur.parent().map(Path::to_path_buf) else {
                    break;
                };
                let newly_linked = nodes
                    .entry(cur_parent.clone())
                    .or_default()
                    .children
                    .insert(cur.clone());
                cur = cur_parent;
                if !newly_linked {
                    break;
                }
            }
        },
    );

    nodes
}

fn compute_all_signatures(
    nodes: &HashMap<PathBuf, DirNode>,
) -> HashMap<PathBuf, Option<Signature>> {
    let mut memo: HashMap<PathBuf, Option<Signature>> = HashMap::new();
    for dir in nodes.keys() {
        signature_of(dir, nodes, &mut memo);
    }
    memo
}

fn signature_of(
    dir: &Path,
    nodes: &HashMap<PathBuf, DirNode>,
    memo: &mut HashMap<PathBuf, Option<Signature>>,
) -> Option<Signature> {
    if let Some(cached) = memo.get(dir) {
        return cached.clone();
    }
    let node = nodes.get(dir).expect("every visited dir is in `nodes`");

    let mut ok = true;
    let mut entries = Vec::new();
    let mut file_count = 0u64;
    let mut bytes = 0u64;
    for (name, group_id, size) in &node.files {
        match group_id {
            Some(gid) => {
                entries.push((PathBuf::from(name), *gid));
                file_count += 1;
                bytes += size;
            }
            None => ok = false,
        }
    }

    let mut child_results = Vec::new();
    for child in &node.children {
        match signature_of(child, nodes, memo) {
            Some(sig) => child_results.push((child.clone(), sig)),
            None => ok = false,
        }
    }

    let result = if ok {
        for (child, sig) in child_results {
            let child_name = child.file_name().expect("a directory has a name");
            for (rel, gid) in sig.entries {
                entries.push((Path::new(child_name).join(rel), gid));
            }
            file_count += sig.file_count;
            bytes += sig.bytes;
        }
        entries.sort();
        Some(Signature {
            entries,
            file_count,
            bytes,
        })
    } else {
        None
    };

    memo.insert(dir.to_path_buf(), result.clone());
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;

    fn group(size: u64, paths: &[&Path]) -> DuplicateGroup {
        DuplicateGroup {
            size,
            paths: paths.iter().map(|p| Arc::from(*p)).collect(),
        }
    }

    #[test]
    fn two_identical_folders_are_an_exact_match() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("1.txt"), b"one").unwrap();
        fs::write(b.join("1.txt"), b"one").unwrap();
        fs::write(a.join("2.txt"), b"two").unwrap();
        fs::write(b.join("2.txt"), b"two").unwrap();

        let groups = vec![
            group(3, &[&a.join("1.txt"), &b.join("1.txt")]),
            group(3, &[&a.join("2.txt"), &b.join("2.txt")]),
        ];

        let matches = find_folder_duplicates(dir.path(), &groups, &ScanOptions::default()).unwrap();

        assert_eq!(matches.len(), 1);
        match &matches[0] {
            FolderMatch::Exact {
                folders,
                file_count,
                bytes,
            } => {
                let mut folders = folders.clone();
                folders.sort();
                assert_eq!(folders, vec![a.clone(), b.clone()]);
                assert_eq!(*file_count, 2);
                assert_eq!(*bytes, 6);
            }
            other => panic!("expected Exact, got {other:?}"),
        }
    }

    #[test]
    fn a_folder_with_one_unmatched_file_can_never_be_a_subset_or_exact_side() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("1.txt"), b"one").unwrap();
        fs::write(b.join("1.txt"), b"one").unwrap();
        // a/only.txt has no duplicate anywhere -- a itself can never be a
        // match's subset/exact side. b is still fully duplicated, so it is
        // legitimately found as a Contained subset of a (a superset is
        // allowed to have extra files of its own).
        fs::write(a.join("only.txt"), b"unique").unwrap();

        let groups = vec![group(3, &[&a.join("1.txt"), &b.join("1.txt")])];

        let matches = find_folder_duplicates(dir.path(), &groups, &ScanOptions::default()).unwrap();

        for m in &matches {
            match m {
                FolderMatch::Exact { folders, .. } => assert!(
                    !folders.contains(&a),
                    "a has an unmatched file, must never be in an Exact match: {matches:?}"
                ),
                FolderMatch::Contained { subset, .. } => assert_ne!(
                    subset, &a,
                    "a has an unmatched file, must never be a Contained subset: {matches:?}"
                ),
            }
        }
    }

    #[test]
    fn a_smaller_folder_fully_contained_in_a_bigger_one_is_reported_as_contained() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        fs::create_dir_all(&small).unwrap();
        fs::create_dir_all(&big).unwrap();
        fs::write(small.join("1.txt"), b"one").unwrap();
        fs::write(big.join("1.txt"), b"one").unwrap();
        fs::write(big.join("extra.txt"), b"extra, not in small").unwrap();

        let groups = vec![group(3, &[&small.join("1.txt"), &big.join("1.txt")])];

        let matches = find_folder_duplicates(dir.path(), &groups, &ScanOptions::default()).unwrap();

        assert_eq!(matches.len(), 1);
        match &matches[0] {
            FolderMatch::Contained {
                subset,
                superset,
                file_count,
                bytes,
            } => {
                assert_eq!(subset, &small);
                assert_eq!(superset, &big);
                assert_eq!(*file_count, 1);
                assert_eq!(*bytes, 3);
            }
            other => panic!("expected Contained, got {other:?}"),
        }
    }

    #[test]
    fn nested_subdirectory_matches_are_not_redundantly_reported() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::create_dir_all(a.join("sub")).unwrap();
        fs::create_dir_all(b.join("sub")).unwrap();
        fs::write(a.join("top.txt"), b"top").unwrap();
        fs::write(b.join("top.txt"), b"top").unwrap();
        fs::write(a.join("sub/nested.txt"), b"nested").unwrap();
        fs::write(b.join("sub/nested.txt"), b"nested").unwrap();

        let groups = vec![
            group(3, &[&a.join("top.txt"), &b.join("top.txt")]),
            group(6, &[&a.join("sub/nested.txt"), &b.join("sub/nested.txt")]),
        ];

        let matches = find_folder_duplicates(dir.path(), &groups, &ScanOptions::default()).unwrap();

        // Only the top-level a/b match is reported -- a/sub and b/sub are
        // implied, not separately listed.
        assert_eq!(
            matches.len(),
            1,
            "expected only the top-level match: {matches:?}"
        );
        match &matches[0] {
            FolderMatch::Exact { folders, .. } => {
                let mut folders = folders.clone();
                folders.sort();
                assert_eq!(folders, vec![a, b]);
            }
            other => panic!("expected Exact, got {other:?}"),
        }
    }

    #[test]
    fn an_independent_match_nested_inside_an_unmatched_directory_is_still_found() {
        let dir = tempfile::tempdir().unwrap();
        let outer = dir.path().join("outer");
        let elsewhere = dir.path().join("elsewhere");
        fs::create_dir_all(outer.join("sub")).unwrap();
        fs::create_dir_all(&elsewhere).unwrap();
        // outer/unique.txt has no duplicate -- outer itself can't match.
        fs::write(outer.join("unique.txt"), b"only here").unwrap();
        fs::write(outer.join("sub/dup.txt"), b"dup").unwrap();
        fs::write(elsewhere.join("dup.txt"), b"dup").unwrap();

        let groups = vec![group(
            3,
            &[&outer.join("sub/dup.txt"), &elsewhere.join("dup.txt")],
        )];

        let matches = find_folder_duplicates(dir.path(), &groups, &ScanOptions::default()).unwrap();

        assert_eq!(matches.len(), 1);
        match &matches[0] {
            FolderMatch::Exact { folders, .. } => {
                let mut folders = folders.clone();
                folders.sort();
                let mut expected = vec![outer.join("sub"), elsewhere];
                expected.sort();
                assert_eq!(folders, expected);
            }
            other => panic!("expected Exact, got {other:?}"),
        }
    }

    #[test]
    fn no_duplicate_groups_means_no_folder_matches() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        fs::create_dir_all(&a).unwrap();
        fs::write(a.join("1.txt"), b"one").unwrap();

        let matches = find_folder_duplicates(dir.path(), &[], &ScanOptions::default()).unwrap();
        assert!(matches.is_empty());
    }

    #[test]
    fn rejects_a_nonexistent_root() {
        let err =
            find_folder_duplicates(Path::new("/does/not/exist"), &[], &ScanOptions::default())
                .expect_err("a nonexistent root must be rejected");
        assert!(matches!(err, ScanError::InvalidRoot(_)));
    }
}
