//! Client-side subtitle discovery for players that disable mpv's built-in
//! auto loading (IINA sets `sub-auto=no` and only scans when a file is opened
//! through its own UI, which never happens for socket-driven `loadfile`).
//!
//! This mirrors IINA's default smart-loading semantics: search the media
//! file's directory plus its `Subs` subdirectory (IINA's default
//! `subAutoLoadSearchPaths` of `["./Subs", "."]`), keep files whose stem
//! matches the media name, and load them via explicit `sub-add` commands.

use std::fs;
use std::path::{Path, PathBuf};

/// External subtitle extensions understood by mpv (and listed by IINA).
const SUBTITLE_EXTENSIONS: [&str; 15] = [
    "srt", "ass", "ssa", "smi", "vtt", "sub", "idx", "sup", "scc", "mks", "txt", "utf", "utf8",
    "lrc", "rt",
];

/// IINA's default `subAutoLoadSearchPaths`: `./Subs` first, then the media directory.
const SUBTITLE_SEARCH_DIR_NAMES: [&str; 1] = ["Subs"];

/// Upper bound on how many matched subtitles are handed to the player.
const MAX_MATCHED_SUBTITLES: usize = 16;

/// A candidate subtitle file discovered next to the media file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SubtitleCandidate {
    path: PathBuf,
    stem: String,
    score: u32,
}

/// Scan for subtitle files matching `video_path`, best matches first.
///
/// Returns an empty vector for non-local paths (URLs) and missing files.
pub fn scan_matching_subtitles(video_path: &str) -> Vec<PathBuf> {
    if video_path.contains("://") {
        return Vec::new();
    }
    let video = Path::new(video_path);
    if !video.is_file() {
        return Vec::new();
    }
    let Some(video_stem) = video
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_string)
    else {
        return Vec::new();
    };
    let Some(media_dir) = video.parent() else {
        return Vec::new();
    };

    let mut candidates = Vec::new();
    let mut dirs: Vec<PathBuf> = vec![media_dir.to_path_buf()];
    for name in SUBTITLE_SEARCH_DIR_NAMES {
        dirs.push(media_dir.join(name));
    }
    for dir in dirs {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || !is_subtitle_file(&path) {
                continue;
            }
            let Some(stem) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            if let Some(score) = match_score(&video_stem, &stem) {
                candidates.push(SubtitleCandidate { path, stem, score });
            }
        }
    }

    rank_matches(candidates)
}

fn is_subtitle_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            SUBTITLE_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
        .unwrap_or(false)
}

/// Lowercase for case-insensitive comparison; keeps separators and tags so
/// boundary detection still works.
fn normalize(stem: &str) -> String {
    stem.to_lowercase()
}

/// Remove bracketed release tags like `[1080p]`, `(BD)` or `{FLAC}`.
fn strip_bracketed_tags(stem: &str) -> String {
    let mut out = String::with_capacity(stem.len());
    let mut depth = 0usize;
    for ch in stem.chars() {
        match ch {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out.trim().to_string()
}

fn starts_with_boundary(haystack: &str, needle: &str) -> bool {
    let Some(rest) = haystack.strip_prefix(needle) else {
        return false;
    };
    // Treat any non-ASCII-alphanumeric boundary (punctuation, whitespace, and
    // non-Latin scripts like CJK) as a separator: "movie.eng" and "movie简体"
    // both extend "movie", but "moviename" does not.
    rest.chars()
        .next()
        .map(|ch| !ch.is_ascii_alphanumeric())
        .unwrap_or(true)
}

/// Score a subtitle stem against the media stem. Higher is better; `None`
/// means the subtitle does not look related to the media file.
fn match_score(video_stem: &str, subtitle_stem: &str) -> Option<u32> {
    let video = normalize(video_stem);
    let subtitle = normalize(subtitle_stem);
    if video.is_empty() || subtitle.is_empty() {
        return None;
    }
    if subtitle == video {
        return Some(100);
    }
    if starts_with_boundary(&subtitle, &video) {
        return Some(90);
    }
    // Retry with release tags stripped, e.g. "Show.Ep.01[1080p]" vs "Show.Ep.01.简体".
    let video_stripped = strip_bracketed_tags(&video);
    let subtitle_stripped = strip_bracketed_tags(&subtitle);
    if !video_stripped.is_empty() && subtitle_stripped == video_stripped {
        return Some(80);
    }
    if !video_stripped.is_empty() && starts_with_boundary(&subtitle_stripped, &video_stripped) {
        return Some(70);
    }
    None
}

/// Deduplicate by stem, sort by score (then shorter stem, then path) and cap.
fn rank_matches(mut candidates: Vec<SubtitleCandidate>) -> Vec<PathBuf> {
    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.stem.len().cmp(&b.stem.len()))
            .then_with(|| a.path.cmp(&b.path))
    });
    let mut seen_stems = std::collections::HashSet::new();
    let mut out = Vec::new();
    for candidate in candidates {
        // Same stem in several directories/extensions counts once, prefer the
        // already-best-ranked copy.
        if !seen_stems.insert(candidate.stem.clone()) {
            continue;
        }
        out.push(candidate.path);
        if out.len() >= MAX_MATCHED_SUBTITLES {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn touch(dir: &Path, name: &str) {
        File::create(dir.join(name)).unwrap();
    }

    #[test]
    fn match_score_prefers_exact_then_prefix() {
        assert_eq!(match_score("Movie", "Movie"), Some(100));
        assert_eq!(match_score("Movie", "movie"), Some(100));
        assert_eq!(match_score("Movie", "Movie.eng"), Some(90));
        assert_eq!(match_score("Movie", "MovieName"), None);
        assert_eq!(match_score("Movie", "Other"), None);
        assert_eq!(
            match_score("Show.Ep.01[1080p]", "Show.Ep.01.简体"),
            Some(70)
        );
        assert_eq!(match_score("Show.Ep.01[1080p]", "Show.Ep.01"), Some(80));
    }

    #[test]
    fn scan_collects_subtitles_from_video_dir_and_subs_dir() {
        let root = std::env::temp_dir().join(format!(
            "syncplay-subtitles-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("Subs")).unwrap();
        touch(&root, "Movie.mkv");
        touch(&root, "Movie.srt");
        touch(&root, "Movie.eng.ass");
        touch(&root, "MovieNotes.txt");
        touch(&root, "Other.srt");
        touch(&root, "Subs/Movie.chs.vtt");

        let matches = scan_matching_subtitles(&root.join("Movie.mkv").to_string_lossy());
        let names: Vec<String> = matches
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["Movie.srt", "Movie.eng.ass", "Movie.chs.vtt"]);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn scan_skips_urls_and_missing_files() {
        assert!(scan_matching_subtitles("https://example.com/video.mkv").is_empty());
        assert!(scan_matching_subtitles("/definitely/not/here/video.mkv").is_empty());
    }
}
