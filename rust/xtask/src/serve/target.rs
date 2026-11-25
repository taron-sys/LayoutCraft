//! From the target of a request (`/web/shell.html?x=1`) to a file below the served folder, refusing everything that could leave it.
//!
//! The target is decoded once, split into segments, and every segment must be a plain name: no `..`, no hidden files (`.git`), no
//! separators, drive letters, stream names or device names of Windows. The path that results is then resolved with `canonicalize`, which
//! follows symbolic links, and must still be inside the served folder.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Why a target is refused.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Refusal {
    /// 400: malformed (bad escape, not UTF-8, control character, not an absolute path).
    Malformed,
    /// 403: it names something outside the served folder or something never served.
    Forbidden,
    /// 404: nothing there.
    NotFound,
    /// 500: the file system failed in another way.
    Failed,
}

/// A decoded target: the names below the served folder and whether the path ended in a slash.
#[derive(Debug, PartialEq, Eq)]
pub struct Target {
    pub segments: Vec<String>,
    pub trailing_slash: bool,
}

/// Names that Windows treats as devices, with or without an extension (`NUL`, `con.txt`, `COM1`).
const DEVICE_NAMES: [&str; 25] = [
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "CLOCK$", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
    "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];
/// Characters that no file name may contain on Windows; here they would only be an attempt at a drive, a stream, a wildcard or a separator.
const FORBIDDEN_CHARS: [char; 8] = ['\\', ':', '*', '?', '"', '<', '>', '|'];

/// Parses the request target (the second word of the request line).
pub fn parse_target(target: &str) -> Result<Target, Refusal> {
    if !target.starts_with('/') || !target.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(Refusal::Malformed);
    }
    let path = target.split(['?', '#']).next().unwrap_or_default();
    let decoded = percent_decode(path)?;
    if decoded.chars().any(char::is_control) {
        return Err(Refusal::Malformed);
    }
    if decoded.contains('\\') {
        return Err(Refusal::Forbidden);
    }
    let mut segments = Vec::new();
    for segment in decoded.split('/').filter(|s| !s.is_empty() && *s != ".") {
        check_segment(segment)?;
        segments.push(segment.to_string());
    }
    Ok(Target { segments, trailing_slash: decoded.ends_with('/') })
}

/// `%XX` escapes to the text they stand for; a bad escape or bytes that are not UTF-8 are malformed.
fn percent_decode(path: &str) -> Result<String, Refusal> {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'%' {
            let digits = bytes.get(i + 1..i + 3).filter(|pair| pair.iter().all(u8::is_ascii_hexdigit));
            let value = digits.and_then(|pair| std::str::from_utf8(pair).ok()).and_then(|hex| u8::from_str_radix(hex, 16).ok());
            out.push(value.ok_or(Refusal::Malformed)?);
            i += 3;
        } else {
            out.push(byte);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| Refusal::Malformed)
}

/// A segment must be a plain name that the file system cannot read as anything else.
fn check_segment(segment: &str) -> Result<(), Refusal> {
    let hidden = segment.starts_with('.'); // `..`, `.git`, `.claude`
    let windows_trims_it = segment.ends_with('.') || segment.ends_with(' ');
    let stem = segment.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    let refused = hidden || windows_trims_it || segment.contains(FORBIDDEN_CHARS) || DEVICE_NAMES.contains(&stem.as_str());
    if refused {
        Err(Refusal::Forbidden)
    } else {
        Ok(())
    }
}

/// The path to redirect to: `/a/b/` for the segments `a`, `b`, with every character that is not unreserved escaped.
pub fn directory_url(segments: &[String]) -> String {
    let mut url = String::from("/");
    for segment in segments {
        for byte in segment.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                url.push(char::from(byte));
            } else {
                url.push_str(&format!("%{byte:02X}"));
            }
        }
        url.push('/');
    }
    url
}

/// What a resolved path is.
#[derive(Debug, PartialEq, Eq)]
pub enum Found {
    File(PathBuf),
    Directory,
}

/// Looks `segments` up below `root` (an already canonical path). Symbolic links are followed, and what they lead to must be inside `root`.
pub fn resolve(root: &Path, segments: &[String]) -> Result<Found, Refusal> {
    let mut path = root.to_path_buf();
    path.extend(segments);
    let canonical = fs::canonicalize(&path).map_err(refusal_for)?;
    if !canonical.starts_with(root) {
        return Err(Refusal::Forbidden);
    }
    let metadata = fs::metadata(&canonical).map_err(refusal_for)?;
    if metadata.is_dir() {
        Ok(Found::Directory)
    } else if metadata.is_file() {
        Ok(Found::File(canonical))
    } else {
        Err(Refusal::NotFound) // a device or a pipe is not served
    }
}

/// Opens a file that [`resolve`] found, with its length.
pub fn open(path: &Path) -> Result<(fs::File, u64), Refusal> {
    let file = fs::File::open(path).map_err(refusal_for)?;
    let length = file.metadata().map_err(refusal_for)?.len();
    Ok((file, length))
}

fn refusal_for(error: io::Error) -> Refusal {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory | io::ErrorKind::InvalidInput | io::ErrorKind::InvalidFilename => Refusal::NotFound,
        io::ErrorKind::PermissionDenied => Refusal::Forbidden,
        _ => Refusal::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn segments(target: &str) -> Vec<String> {
        parse_target(target).unwrap().segments
    }

    #[test]
    fn a_plain_path_is_split_into_names_and_a_query_or_fragment_is_dropped() {
        assert_eq!(segments("/web/shell.html"), ["web", "shell.html"]);
        assert_eq!(segments("/web/shell.html?x=../..#frag"), ["web", "shell.html"]);
        assert_eq!(segments("/a//b/./c"), ["a", "b", "c"]);
        assert_eq!(segments("/"), Vec::<String>::new());
        assert_eq!(segments("/?q"), Vec::<String>::new());
    }

    #[test]
    fn a_trailing_slash_is_remembered() {
        assert!(parse_target("/web/").unwrap().trailing_slash);
        assert!(parse_target("/").unwrap().trailing_slash);
        assert!(!parse_target("/web").unwrap().trailing_slash);
        assert!(parse_target("/web%2F").unwrap().trailing_slash);
    }

    #[test]
    fn escapes_are_decoded_once_including_multibyte_characters() {
        assert_eq!(segments("/a%20b/%C3%A9.txt"), ["a b", "\u{e9}.txt"]);
        assert_eq!(segments("/100%25"), ["100%"]);
        assert_eq!(segments("/%2525"), ["%25"]);
        assert_eq!(segments("/a+b"), ["a+b"]);
    }

    #[test]
    fn a_bad_escape_or_bytes_that_are_not_utf8_are_malformed() {
        for target in ["/%", "/%4", "/%zz", "/%g1", "/a%", "/%+1", "/%C3", "/%FF", "/%ff%fe"] {
            assert_eq!(parse_target(target), Err(Refusal::Malformed), "{target}");
        }
    }

    #[test]
    fn a_nul_a_control_character_or_a_byte_outside_ascii_is_malformed() {
        for target in ["/%00", "/a%00b", "/a%0Ab", "/a%7F", "/a%1b", "/a\u{e9}", "/a\tb", "/\u{0}"] {
            assert_eq!(parse_target(target), Err(Refusal::Malformed), "{target}");
        }
    }

    #[test]
    fn only_an_absolute_path_is_a_target() {
        for target in ["", "*", "web/shell.html", "http://example.invalid/web", "..", "\\x"] {
            assert!(parse_target(target).is_err(), "{target}");
        }
        assert!(parse_target("//host/share").is_ok()); // two slashes are an empty segment, not a network path
        assert_eq!(segments("//host/share"), ["host", "share"]);
    }

    #[test]
    fn parent_directory_segments_are_forbidden_however_they_are_written() {
        for target in ["/..", "/../Cargo.toml", "/web/../../x", "/%2e%2e/x", "/%2E%2E", "/web/%2e%2e%2f..%2fx", "/.%2e/x", "/..%2f..%2fetc"] {
            assert_eq!(parse_target(target), Err(Refusal::Forbidden), "{target}");
        }
    }

    #[test]
    fn a_backslash_a_drive_letter_a_stream_or_a_wildcard_is_forbidden() {
        for target in [
            "/web/..%5c..%5cx",
            "/a%5Cb",
            "/C:/Windows",
            "/C%3A/x",
            "/c:",
            "/x/file.txt:stream",
            "/file::$DATA",
            "/a%2Ab",
            "/a%3Fb",
            "/a%22b",
            "/a%3Cb",
            "/a%7Cb",
            "/%5C%5Cserver%5Cshare",
        ] {
            assert_eq!(parse_target(target), Err(Refusal::Forbidden), "{target}");
        }
    }

    #[test]
    fn hidden_files_and_names_that_windows_trims_or_reads_as_devices_are_forbidden() {
        for target in [
            "/.git/config",
            "/web/.env",
            "/.claude/x",
            "/a./b",
            "/a%20/b",
            "/NUL",
            "/nul.txt",
            "/Con",
            "/com1",
            "/LPT9.log",
            "/aux.x/y",
            "/CONIN$",
            "/web/prn",
            "/nul%20.x",
        ] {
            assert_eq!(parse_target(target), Err(Refusal::Forbidden), "{target}");
        }
        // names that merely contain or resemble them are fine
        for target in ["/console.js", "/null", "/com10", "/web/auxiliary.html", "/a.b.c", "/v1.2/x"] {
            assert!(parse_target(target).is_ok(), "{target}");
        }
    }

    #[test]
    fn a_redirect_target_is_rebuilt_from_the_names_and_escapes_everything_else() {
        assert_eq!(directory_url(&[]), "/");
        assert_eq!(directory_url(&["web".into(), "a b".into(), "\u{e9}%".into()]), "/web/a%20b/%C3%A9%25/");
        assert_eq!(directory_url(&["x\r\nSet-Cookie: a".into()]), "/x%0D%0ASet-Cookie%3A%20a/");
    }

    #[test]
    fn a_name_resolves_to_a_file_or_a_directory_below_the_root_and_a_missing_or_blocked_name_is_refused() {
        let dir = TempDir::new();
        dir.write("web/shell.html", b"<html>");
        dir.write("file.txt", b"x");
        let root = fs::canonicalize(dir.path()).unwrap();
        let names = |parts: &[&str]| parts.iter().map(|p| p.to_string()).collect::<Vec<_>>();
        assert!(matches!(resolve(&root, &names(&["web", "shell.html"])), Ok(Found::File(path)) if path.ends_with("shell.html")));
        assert_eq!(resolve(&root, &names(&["web"])), Ok(Found::Directory));
        assert_eq!(resolve(&root, &[]), Ok(Found::Directory));
        assert_eq!(resolve(&root, &names(&["nope"])), Err(Refusal::NotFound));
        assert_eq!(resolve(&root, &names(&["file.txt", "below"])), Err(Refusal::NotFound));
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_that_leaves_the_root_is_forbidden_and_one_that_stays_inside_is_followed() {
        let outside = TempDir::new();
        outside.write("secret.txt", b"secret");
        let dir = TempDir::new();
        dir.write("inside.txt", b"ok");
        std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.txt"), dir.path().join("secret-link.txt")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("inside.txt"), dir.path().join("inside-link.txt")).unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let names = |parts: &[&str]| parts.iter().map(|p| p.to_string()).collect::<Vec<_>>();
        assert_eq!(resolve(&root, &names(&["escape", "secret.txt"])), Err(Refusal::Forbidden));
        assert_eq!(resolve(&root, &names(&["escape"])), Err(Refusal::Forbidden));
        assert_eq!(resolve(&root, &names(&["secret-link.txt"])), Err(Refusal::Forbidden));
        assert!(matches!(resolve(&root, &names(&["inside-link.txt"])), Ok(Found::File(_))));
    }
}
