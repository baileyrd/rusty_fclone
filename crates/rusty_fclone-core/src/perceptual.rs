//! Opt-in perceptual image-similarity detection (`DETECTION-PERCEPTUAL-IMAGES`,
//! ADR-0030): a deliberately separate, best-effort "these look alike" mode.
//! [`find_similar_images`] never touches [`crate::scan`]'s byte-identical
//! [`crate::DuplicateGroup`] pipeline and never produces one — its output is
//! [`SimilarGroup`], a distinct type, so a caller can never mix up a
//! hash-verified duplicate with a merely-similar image. This keeps
//! `FCLONE-DETECTION-001`'s "zero false positives" precision guarantee
//! exclusive to the exact engine.
//!
//! A post-scan-shaped pass, same as [`crate::find_folder_duplicates`]: it
//! runs its own traversal (reusing [`ScanOptions`]'s traversal tunables —
//! symlinks, filesystem boundary, size/exclude-path filters — but locking
//! the extension filter to the supported image formats) rather than
//! consuming a completed scan's results, since perceptually similar images
//! are by definition not byte-identical and so would never share a
//! `DuplicateGroup` in the first place.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::ScanError;
use crate::model::ScanOptions;
use crate::traversal;

/// Extensions decoded for perceptual hashing — exactly the formats the
/// `image` crate is built with here (pure-Rust codecs only, no C
/// toolchain: `jpeg`, `png`, `gif`, `bmp`). Deliberately narrower than
/// `GUI-MEDIA-PREVIEW`'s preview support (which also covers `webp`/`svg`
/// for display purposes) — `webp`'s lossy decode pulls in a C-linked
/// codec in the `image` crate, and `svg` is vector, not a pixel grid a
/// perceptual hash can meaningfully apply to.
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "bmp"];

/// Tunables for [`find_similar_images`]. Separate from [`ScanOptions`]
/// since this is an independent, opt-in pass, not a `scan()` tunable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerceptualOptions {
    /// Maximum Hamming distance (0-64) between two images' difference-hashes
    /// for them to be considered similar. Lower is more conservative (fewer,
    /// more confident matches); 0 requires an identical hash.
    pub max_hamming_distance: u32,
}

impl Default for PerceptualOptions {
    /// 10 out of 64 bits — a commonly-cited dHash cutoff for "still clearly
    /// the same or a near-identical image" (resave at a different quality,
    /// a light crop/resize, a small watermark) without also matching
    /// unrelated images that merely share a similar composition.
    fn default() -> Self {
        Self {
            max_hamming_distance: 10,
        }
    }
}

/// A cluster of two or more images considered visually similar under
/// [`PerceptualOptions`] — **not** byte-identical; see [`crate::DuplicateGroup`]
/// for that guarantee. Every pair in `paths` has a Hamming distance no
/// greater than the `max_hamming_distance` used to produce this group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimilarGroup {
    /// Sorted for stable, diffable output (matches `DuplicateGroup::paths`'s
    /// own convention).
    pub paths: Vec<PathBuf>,
    /// The largest pairwise Hamming distance found within this cluster —
    /// 0 means every image hashed identically; reporting the actual max
    /// makes a loose cluster's looseness visible instead of hiding it
    /// behind a single yes/no.
    pub max_distance: u32,
}

/// Finds visually-similar image clusters in the tree rooted at `root`.
///
/// A file that fails to decode (corrupt, truncated, an extension that
/// doesn't match its actual content) or that traversal itself can't read
/// is silently excluded from clustering, not a hard error — the same
/// tolerance [`crate::find_folder_duplicates`]'s own traversal already
/// applies for a post-scan, best-effort pass (see its `build_tree`).
/// Clustering is pairwise (O(n²) hash comparisons) — a deliberate
/// simplicity-over-scale tradeoff for a first, opt-in version; revisit only
/// if a real photo library shows this is actually the bottleneck.
pub fn find_similar_images(
    root: &Path,
    scan_options: &ScanOptions,
    perceptual_options: &PerceptualOptions,
) -> Result<Vec<SimilarGroup>, ScanError> {
    if !root.is_dir() {
        return Err(ScanError::InvalidRoot(root.to_path_buf()));
    }

    let mut image_options = scan_options.clone();
    image_options.include_extensions =
        Some(IMAGE_EXTENSIONS.iter().map(|s| s.to_string()).collect());
    // A caller's `exclude_extensions` is meant for their normal duplicate
    // scan (e.g. skipping `.tmp`) and shouldn't accidentally suppress an
    // image format from this independent pass too.
    image_options.exclude_extensions = None;

    let mut candidate_paths: Vec<PathBuf> = Vec::new();
    traversal::traverse(
        root,
        &image_options,
        |_err| {
            // Traversal-level errors (unreadable file, vanished mid-walk)
            // just mean that file contributes no signal -- see this
            // function's own doc comment.
        },
        |candidate| candidate_paths.push(candidate.path.to_path_buf()),
    );

    let mut hashed: Vec<(PathBuf, u64)> = Vec::new();
    for path in candidate_paths {
        if let Some(hash) = hash_image(&path) {
            hashed.push((path, hash));
        }
    }

    Ok(cluster(hashed, perceptual_options.max_hamming_distance))
}

fn hash_image(path: &Path) -> Option<u64> {
    let img = image::open(path).ok()?;
    Some(dhash(&img))
}

/// Difference hash (dHash): shrink to a 9x8 grayscale grid and record, for
/// every row, whether each pixel is brighter than its right neighbor —
/// 8 columns of comparisons x 8 rows = 64 bits. Robust to resizing,
/// recompression, and small color/exposure shifts (the comparison is
/// relative, not absolute), and — unlike a simpler average-hash — also
/// somewhat sensitive to horizontal structure, not just overall brightness.
fn dhash(img: &image::DynamicImage) -> u64 {
    let small = img
        .resize_exact(9, 8, image::imageops::FilterType::Triangle)
        .to_luma8();

    let mut hash: u64 = 0;
    let mut bit = 0u32;
    for y in 0..8 {
        for x in 0..8 {
            let left = small.get_pixel(x, y).0[0];
            let right = small.get_pixel(x + 1, y).0[0];
            if left > right {
                hash |= 1 << bit;
            }
            bit += 1;
        }
    }
    hash
}

fn hamming_distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Union-find over every pairwise Hamming distance within `max_distance`,
/// then groups by root and reports clusters of two or more.
fn cluster(hashed: Vec<(PathBuf, u64)>, max_distance: u32) -> Vec<SimilarGroup> {
    let n = hashed.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], x: usize) -> usize {
        if parent[x] != x {
            parent[x] = find(parent, parent[x]);
        }
        parent[x]
    }

    for i in 0..n {
        for j in (i + 1)..n {
            if hamming_distance(hashed[i].1, hashed[j].1) <= max_distance {
                let ri = find(&mut parent, i);
                let rj = find(&mut parent, j);
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }

    let mut members_by_root: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        members_by_root.entry(root).or_default().push(i);
    }

    let mut groups: Vec<SimilarGroup> = members_by_root
        .into_values()
        .filter(|members| members.len() >= 2)
        .map(|members| {
            let mut max_dist = 0u32;
            for a in 0..members.len() {
                for b in (a + 1)..members.len() {
                    let d = hamming_distance(hashed[members[a]].1, hashed[members[b]].1);
                    max_dist = max_dist.max(d);
                }
            }
            let mut paths: Vec<PathBuf> = members.iter().map(|&i| hashed[i].0.clone()).collect();
            paths.sort();
            SimilarGroup {
                paths,
                max_distance: max_dist,
            }
        })
        .collect();
    groups.sort_by(|a, b| a.paths.cmp(&b.paths));
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use std::fs;

    /// A deterministic, non-uniform gradient so its dHash is meaningfully
    /// distinguishable from `checkerboard` below -- a solid-color image
    /// would hash to all-zero regardless of which color, a known dHash
    /// blind spot not exercised here.
    fn gradient(width: u32, height: u32) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
        ImageBuffer::from_fn(width, height, |x, y| {
            let v = ((x * 255 / width.max(1)) + (y * 255 / height.max(1))) as u8;
            Rgb([v, v, v])
        })
    }

    fn checkerboard(width: u32, height: u32) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
        ImageBuffer::from_fn(width, height, |x, y| {
            if (x / 4 + y / 4) % 2 == 0 {
                Rgb([255, 255, 255])
            } else {
                Rgb([0, 0, 0])
            }
        })
    }

    fn dhash_of(buf: &ImageBuffer<Rgb<u8>, Vec<u8>>) -> u64 {
        dhash(&image::DynamicImage::ImageRgb8(buf.clone()))
    }

    #[test]
    fn identical_images_hash_identically() {
        let a = gradient(64, 64);
        let b = gradient(64, 64);
        assert_eq!(hamming_distance(dhash_of(&a), dhash_of(&b)), 0);
    }

    #[test]
    fn very_different_images_hash_far_apart() {
        let a = gradient(64, 64);
        let b = checkerboard(64, 64);
        assert!(
            hamming_distance(dhash_of(&a), dhash_of(&b)) > PerceptualOptions::default().max_hamming_distance,
            "a gradient and a checkerboard should not be considered similar under the default threshold"
        );
    }

    #[test]
    fn a_lightly_perturbed_copy_stays_within_the_default_threshold() {
        let a = gradient(64, 64);
        // Nudge every pixel up slightly (simulates a brightness/exposure
        // shift or a lossy re-save) -- dHash compares *relative* brightness
        // between adjacent pixels, so a uniform shift should barely move
        // the hash.
        let mut b = a.clone();
        for pixel in b.pixels_mut() {
            pixel.0[0] = pixel.0[0].saturating_add(5);
            pixel.0[1] = pixel.0[1].saturating_add(5);
            pixel.0[2] = pixel.0[2].saturating_add(5);
        }
        assert!(
            hamming_distance(dhash_of(&a), dhash_of(&b))
                <= PerceptualOptions::default().max_hamming_distance,
            "a small uniform brightness shift should still be considered similar"
        );
    }

    #[test]
    fn hamming_distance_counts_differing_bits() {
        assert_eq!(hamming_distance(0, 0), 0);
        assert_eq!(hamming_distance(0, u64::MAX), 64);
        assert_eq!(hamming_distance(0b1010, 0b1000), 1);
    }

    #[test]
    fn cluster_groups_only_pairs_within_the_threshold() {
        let hashed = vec![
            (PathBuf::from("/a"), 0b0000),
            (PathBuf::from("/b"), 0b0001), // distance 1 from /a
            (PathBuf::from("/c"), 0b1111), // distance 4 from /a, 3 from /b
        ];
        let groups = cluster(hashed, 1);
        assert_eq!(groups.len(), 1, "only /a and /b are within distance 1");
        assert_eq!(
            groups[0].paths,
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
        assert_eq!(groups[0].max_distance, 1);
    }

    #[test]
    fn cluster_never_reports_a_singleton() {
        let hashed = vec![(PathBuf::from("/a"), 0), (PathBuf::from("/b"), u64::MAX)];
        let groups = cluster(hashed, 5);
        assert!(groups.is_empty());
    }

    #[test]
    fn find_similar_images_groups_a_real_near_identical_pair_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let a = gradient(64, 64);
        let mut b = a.clone();
        for pixel in b.pixels_mut() {
            pixel.0[0] = pixel.0[0].saturating_add(5);
        }
        image::DynamicImage::ImageRgb8(a)
            .save(dir.path().join("a.png"))
            .unwrap();
        image::DynamicImage::ImageRgb8(b)
            .save(dir.path().join("b.png"))
            .unwrap();

        let groups = find_similar_images(
            dir.path(),
            &ScanOptions::default(),
            &PerceptualOptions::default(),
        )
        .unwrap();

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].paths.len(), 2);
    }

    #[test]
    fn find_similar_images_does_not_group_unrelated_images() {
        let dir = tempfile::tempdir().unwrap();
        image::DynamicImage::ImageRgb8(gradient(64, 64))
            .save(dir.path().join("gradient.png"))
            .unwrap();
        image::DynamicImage::ImageRgb8(checkerboard(64, 64))
            .save(dir.path().join("checkerboard.png"))
            .unwrap();

        let groups = find_similar_images(
            dir.path(),
            &ScanOptions::default(),
            &PerceptualOptions::default(),
        )
        .unwrap();

        assert!(groups.is_empty());
    }

    #[test]
    fn find_similar_images_ignores_non_image_files_without_erroring() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("notes.txt"), b"not an image").unwrap();

        let groups = find_similar_images(
            dir.path(),
            &ScanOptions::default(),
            &PerceptualOptions::default(),
        )
        .unwrap();

        assert!(groups.is_empty());
    }

    #[test]
    fn find_similar_images_skips_a_file_with_an_image_extension_but_bogus_content() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("fake.png"), b"this is not really a png").unwrap();
        image::DynamicImage::ImageRgb8(gradient(64, 64))
            .save(dir.path().join("real.png"))
            .unwrap();

        // The bogus file fails to decode and is silently skipped -- with
        // only one real image left, no cluster is possible (needs 2+).
        let groups = find_similar_images(
            dir.path(),
            &ScanOptions::default(),
            &PerceptualOptions::default(),
        )
        .unwrap();
        assert!(groups.is_empty());
    }

    #[test]
    fn find_similar_images_rejects_a_nonexistent_root() {
        let err = find_similar_images(
            Path::new("/does/not/exist/at/all"),
            &ScanOptions::default(),
            &PerceptualOptions::default(),
        )
        .expect_err("a nonexistent root must be rejected");
        assert!(matches!(err, ScanError::InvalidRoot(_)));
    }
}
