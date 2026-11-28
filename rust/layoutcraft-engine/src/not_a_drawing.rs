//! What a file that is not a drawing most likely is, so that the refusal can say so instead of reporting a broken ZIP.

use layoutcraft_vsdx::Package;

const WANTED: &str = "Open a .vsdx, .vstx, .vsd or .vdx drawing.";

/// Why `bytes`, which could not be read as a package (`error`), are not a drawing.
pub(crate) fn unreadable(bytes: &[u8], error: &str) -> String {
    if bytes.is_empty() {
        return format!("The file is empty. {WANTED}");
    }
    if bytes.starts_with(b"PK") {
        return format!("The file is damaged: it starts like a drawing but cannot be unpacked ({error}).");
    }
    match kind(bytes) {
        Some(kind) => format!("This is {kind}, not a drawing. {WANTED}"),
        None => format!("This is not a drawing: the file is in no format that can be opened. {WANTED}"),
    }
}

/// Why the package that opened but holds no drawing is not one, if that is so.
pub(crate) fn other_package(package: &Package) -> Option<String> {
    if package.names().any(|name| name.starts_with("visio/")) {
        return None;
    }
    let has = |folder: &str| package.names().any(|name| name.starts_with(folder));
    let kind = if has("word/") {
        "a word processing document"
    } else if has("xl/") {
        "a spreadsheet"
    } else if has("ppt/") {
        "a presentation"
    } else {
        "a ZIP archive without a drawing in it"
    };
    Some(format!("This is {kind}, not a drawing. {WANTED}"))
}

fn kind(bytes: &[u8]) -> Option<&'static str> {
    const MAGIC: [(&[u8], &str); 8] = [
        (b"%PDF", "a PDF document"),
        (b"\x89PNG", "a PNG picture"),
        (b"\xff\xd8\xff", "a JPEG picture"),
        (b"GIF8", "a GIF picture"),
        (&[0xd0, 0xcf, 0x11, 0xe0], "an old-style office file without a drawing in it"),
        (b"\x1f\x8b", "a compressed (gzip) file"),
        (b"{\\rtf", "a rich text document"),
        (b"Rar!", "a RAR archive"),
    ];
    if let Some((_, kind)) = MAGIC.iter().find(|(magic, _)| bytes.starts_with(magic)) {
        return Some(kind);
    }
    let start = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).trim_start_matches('\u{feff}').trim_start().to_ascii_lowercase();
    if start.starts_with("<svg") || (start.starts_with("<?xml") && start.contains("<svg")) {
        Some("an SVG picture")
    } else if start.starts_with("<!doctype html") || start.starts_with("<html") {
        Some("a web page")
    } else if start.starts_with("<mxfile") || start.starts_with("<mxgraphmodel") {
        Some("a draw.io file (give it the .drawio ending to import it)")
    } else if start.starts_with('{') || start.starts_with('[') {
        Some("a JSON text file")
    } else {
        None
    }
}
