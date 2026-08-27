//! Inline media preview for the Duplicate Review screen (`GUI-MEDIA-PREVIEW`,
//! ADR-0028) — reads a small, web-renderable image or audio file and
//! returns it as a `data:` URI the frontend can drop straight into an
//! `<img>`/`<audio>` element. No new dependency: base64 is hand-rolled
//! below (a small, easily-tested, well-defined transform, not worth a
//! crate the same way `trash`/`reflink-copy` were for genuinely
//! platform-specific behavior — see ADR-0028).
//!
//! Deliberately scoped to images and audio only. Video is not attempted:
//! typical video file sizes make whole-file base64 embedding impractical
//! (multi-hundred-MB memory spikes, a frozen UI thread while reading) —
//! doing it properly needs Tauri's asset/stream protocol, a separate,
//! not-yet-adopted prerequisite this project already tracks elsewhere
//! (the GUI's native file-picker work).

use std::path::Path;

/// Images: generous, since a single photo rarely exceeds this.
const IMAGE_SIZE_LIMIT: u64 = 25 * 1024 * 1024;
/// Audio: same limit — a full album track still fits comfortably, and
/// whole-file base64 embedding stops being reasonable well before typical
/// video sizes.
const AUDIO_SIZE_LIMIT: u64 = 25 * 1024 * 1024;

/// Maps a file extension to a browser-renderable MIME type, or `None` if
/// this file isn't a supported preview target. Deliberately excludes
/// formats most webview engines (WebKitGTK, WebView2, WKWebView) don't
/// render natively even with a correct MIME type — HEIC and TIFF in
/// particular, despite being image formats this project's own
/// `EXT_CATEGORY` groups under "photo" for filtering purposes.
fn mime_for_extension(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "ogg" => "audio/ogg",
        _ => return None,
    })
}

/// Reads `path` and returns it as a `data:<mime>;base64,<...>` URI,
/// or a human-readable reason it can't be previewed (unsupported
/// extension, too large, or a real I/O error) — never a partial/silently
/// truncated preview.
pub fn build_data_url(path: &Path) -> Result<String, String> {
    let mime =
        mime_for_extension(path).ok_or_else(|| "unsupported file type for preview".to_string())?;
    let metadata = std::fs::metadata(path).map_err(|err| err.to_string())?;
    let limit = if mime.starts_with("image/") {
        IMAGE_SIZE_LIMIT
    } else {
        AUDIO_SIZE_LIMIT
    };
    if metadata.len() > limit {
        return Err(format!(
            "file too large to preview ({} bytes, limit {limit} bytes)",
            metadata.len()
        ));
    }
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    Ok(format!("data:{mime};base64,{}", base64_encode(&bytes)))
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard RFC 4648 base64 encoding (with `=` padding) — every browser
/// engine's `data:` URI parser expects exactly this, not the URL-safe
/// variant.
fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied();
        let b2 = chunk.get(2).copied();

        let n = (b0 as u32) << 16 | (b1.unwrap_or(0) as u32) << 8 | (b2.unwrap_or(0) as u32);

        out.push(BASE64_ALPHABET[(n >> 18 & 0x3f) as usize] as char);
        out.push(BASE64_ALPHABET[(n >> 12 & 0x3f) as usize] as char);
        out.push(if b1.is_some() {
            BASE64_ALPHABET[(n >> 6 & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if b2.is_some() {
            BASE64_ALPHABET[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn base64_encode_matches_known_vectors() {
        // RFC 4648 test vectors.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn mime_for_extension_recognizes_supported_image_and_audio_formats() {
        assert_eq!(mime_for_extension(Path::new("a.jpg")), Some("image/jpeg"));
        assert_eq!(
            mime_for_extension(Path::new("a.JPEG")),
            Some("image/jpeg"),
            "extension matching is case-insensitive"
        );
        assert_eq!(mime_for_extension(Path::new("a.png")), Some("image/png"));
        assert_eq!(mime_for_extension(Path::new("a.mp3")), Some("audio/mpeg"));
        assert_eq!(mime_for_extension(Path::new("a.flac")), Some("audio/flac"));
    }

    #[test]
    fn mime_for_extension_rejects_heic_tiff_video_and_unknown_extensions() {
        // HEIC/TIFF are real image formats but not reliably renderable by
        // this project's target webview engines -- excluded on purpose,
        // not an oversight.
        assert_eq!(mime_for_extension(Path::new("a.heic")), None);
        assert_eq!(mime_for_extension(Path::new("a.tiff")), None);
        assert_eq!(mime_for_extension(Path::new("a.mp4")), None);
        assert_eq!(mime_for_extension(Path::new("a.txt")), None);
        assert_eq!(mime_for_extension(Path::new("noextension")), None);
    }

    #[test]
    fn build_data_url_returns_a_correctly_shaped_data_uri_for_a_real_image() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        std::fs::write(&path, b"not a real png, just bytes to encode").unwrap();

        let url = build_data_url(&path).unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        let encoded = url.strip_prefix("data:image/png;base64,").unwrap();
        assert_eq!(
            encoded,
            base64_encode(b"not a real png, just bytes to encode")
        );
    }

    #[test]
    fn build_data_url_rejects_an_unsupported_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.mp4");
        std::fs::write(&path, b"video bytes").unwrap();

        let err = build_data_url(&path).unwrap_err();
        assert!(err.contains("unsupported file type"));
    }

    #[test]
    fn build_data_url_rejects_a_file_over_the_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        // One byte over the limit -- doesn't need to actually write 25MB
        // to prove the boundary, just needs metadata().len() to exceed it.
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(IMAGE_SIZE_LIMIT + 1).unwrap();

        let err = build_data_url(&path).unwrap_err();
        assert!(err.contains("too large"));
    }

    #[test]
    fn build_data_url_reports_a_real_io_error_for_a_missing_file() {
        let err = build_data_url(&PathBuf::from("/does/not/exist.png")).unwrap_err();
        assert!(!err.contains("unsupported"));
        assert!(!err.contains("too large"));
    }
}
