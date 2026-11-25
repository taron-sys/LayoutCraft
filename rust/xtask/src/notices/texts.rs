//! The licence texts of the crates: found in the crate sources, borrowed from a crate with the same licence when a crate ships none,
//! and grouped by content so that each text is printed once.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::Row;
use crate::js_compat::trim;

/// File names (lower case prefixes) that hold a licence text.
const LICENCE_FILE_PREFIXES: [&str; 5] = ["licence", "license", "copying", "unlicense", "notice"];
/// The font crate keeps its licence texts next to the fonts.
const FONT_DIR: &str = "fonts";
/// How many crates a group names before it says "and N more".
const NAMED_USERS: usize = 6;
/// Length of the hash prefix that names a group.
const HASH_CHARS: usize = 12;

/// A licence file of a crate: `file` is relative to `dir`. `borrowed` marks a text taken from another crate with the same licence.
#[derive(Clone)]
pub struct Entry {
    pub dir: PathBuf,
    pub file: String,
    pub borrowed: bool,
}

/// One distinct text and the crates that use it.
#[derive(Debug)]
pub struct Group {
    pub hash: String,
    pub text: String,
    pub users: Vec<String>,
}

impl Group {
    /// `<hash>: used by a, b, c and N more`
    pub fn heading(&self) -> String {
        let named = self.users.iter().take(NAMED_USERS).cloned().collect::<Vec<_>>().join(", ");
        let more = self.users.len().saturating_sub(NAMED_USERS);
        let tail = if more > 0 { format!(" and {more} more") } else { String::new() };
        format!("{}: used by {named}{tail}", self.hash)
    }
}

/// Looks for the licence files of every crate; the font crate's texts follow its own.
pub fn discover(rows: &mut [Row]) -> io::Result<()> {
    for row in rows {
        let mut files = top_level_licence_files(&row.dir).unwrap_or_default(); // the source may not be unpacked
        files.extend(font_licence_files(&row.dir)?);
        row.entries = files.into_iter().map(|file| Entry { dir: row.dir.clone(), file, borrowed: false }).collect();
    }
    Ok(())
}

/// The names of the files of `dir` that start like a licence file (in any case), in name order.
fn top_level_licence_files(dir: &Path) -> io::Result<Vec<String>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        let lower = name.to_ascii_lowercase();
        if LICENCE_FILE_PREFIXES.iter().any(|prefix| lower.starts_with(prefix)) && fs::metadata(dir.join(&name))?.is_file() {
            files.push(name);
        }
    }
    files.sort();
    Ok(files)
}

/// `fonts/<name>.txt` for every text file in the crate's `fonts` folder, if it has one.
fn font_licence_files(dir: &Path) -> io::Result<Vec<String>> {
    let font_dir = dir.join(FONT_DIR);
    if !font_dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(font_dir)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.to_ascii_lowercase().ends_with(".txt") {
            files.push(format!("{FONT_DIR}/{name}"));
        }
    }
    files.sort();
    Ok(files)
}

/// A crate without a licence file of its own (the egui family keeps them at the workspace root) gets the standard text of its licence
/// from the first crate with the same licence expression that has a licence file. Crates are handled in list order, so a crate that
/// borrowed earlier can serve as the source for a later one; the text is the same.
pub fn borrow_missing(rows: &mut [Row]) {
    for index in 0..rows.len() {
        if !rows[index].entries.is_empty() {
            continue;
        }
        let donor = rows.iter().find(|d| d.license == rows[index].license && d.entries.iter().any(|e| !is_font_text(e)));
        let borrowed: Vec<Entry> =
            donor.map(|d| d.entries.iter().filter(|e| !is_font_text(e)).map(|e| Entry { borrowed: true, ..e.clone() }).collect()).unwrap_or_default();
        rows[index].entries = borrowed;
    }
}

fn is_font_text(entry: &Entry) -> bool {
    entry.file.starts_with(&format!("{FONT_DIR}/"))
}

/// Reads every entry, groups equal texts (LF line ends, trimmed) under the first 12 hex digits of their SHA-256, in order of first use.
pub fn group_by_content(rows: &[Row]) -> Result<Vec<Group>, String> {
    let mut groups: Vec<Group> = Vec::new();
    for row in rows {
        for entry in &row.entries {
            let path = entry.dir.join(&entry.file);
            let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let text = trim(&String::from_utf8_lossy(&bytes).replace("\r\n", "\n")).to_string();
            let hash = short_hash(&text);
            let source = if entry.borrowed { "standard text of the licence" } else { entry.file.as_str() };
            let user = format!("{} {} ({source})", row.name, row.version);
            match groups.iter_mut().find(|g| g.hash == hash) {
                Some(group) => group.users.push(user),
                None => groups.push(Group { hash, text, users: vec![user] }),
            }
        }
    }
    Ok(groups)
}

fn short_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex.truncate(HASH_CHARS);
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn row(dir: &Path, name: &str, license: &str) -> Row {
        Row {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            license: license.to_string(),
            repository: String::new(),
            ok: true,
            dir: dir.to_path_buf(),
            entries: Vec::new(),
        }
    }

    #[test]
    fn licence_files_are_found_by_prefix_in_any_case_and_a_font_crate_adds_its_texts() {
        let dir = TempDir::new();
        for file in [
            "LICENSE-MIT",
            "license-apache",
            "Copying",
            "UNLICENSE",
            "NOTICE.md",
            "LICENCE",
            "readme.md",
            "Cargo.toml",
            "src/licence.rs",
            "fonts/a.TXT",
            "fonts/b.ttf",
            "fonts/c.txt",
        ] {
            dir.write(file, b"x");
        }
        std::fs::create_dir(dir.path().join("license-dir")).unwrap();
        let mut rows = vec![row(dir.path(), "c", "MIT")];
        discover(&mut rows).unwrap();
        let files: Vec<&str> = rows[0].entries.iter().map(|e| e.file.as_str()).collect();
        assert_eq!(files, ["Copying", "LICENCE", "LICENSE-MIT", "NOTICE.md", "UNLICENSE", "license-apache", "fonts/a.TXT", "fonts/c.txt"]);
    }

    #[test]
    fn a_crate_whose_source_is_missing_has_no_licence_files_and_no_error() {
        let dir = TempDir::new();
        let mut rows = vec![row(&dir.path().join("not-unpacked"), "c", "MIT")];
        discover(&mut rows).unwrap();
        assert!(rows[0].entries.is_empty());
    }

    #[test]
    fn a_crate_without_a_text_borrows_the_text_of_the_first_crate_with_the_same_licence_but_never_a_font_text() {
        let dir = TempDir::new();
        dir.write("donor/LICENSE", b"MIT text");
        dir.write("fonty/fonts/f.txt", b"font text");
        let mut rows = vec![
            row(&dir.path().join("fonty"), "fonty", "MIT"),
            row(&dir.path().join("donor"), "donor", "MIT"),
            row(&dir.path().join("empty"), "empty", "MIT"),
            row(&dir.path().join("other"), "other", "Zlib"),
        ];
        discover(&mut rows).unwrap();
        borrow_missing(&mut rows);
        let borrowed = &rows[2].entries;
        assert_eq!(borrowed.len(), 1);
        assert!(borrowed[0].borrowed && borrowed[0].file == "LICENSE" && borrowed[0].dir.ends_with("donor"));
        assert!(rows[3].entries.is_empty(), "no crate has the Zlib licence with a text");
        assert!(!rows[0].entries[0].borrowed && !rows[1].entries[0].borrowed);
    }

    #[test]
    fn a_crate_that_borrowed_can_serve_the_next_one() {
        let dir = TempDir::new();
        dir.write("donor/LICENSE", b"text");
        let mut rows =
            vec![row(&dir.path().join("e1"), "e1", "MIT"), row(&dir.path().join("e2"), "e2", "MIT"), row(&dir.path().join("donor"), "donor", "MIT")];
        discover(&mut rows).unwrap();
        borrow_missing(&mut rows);
        assert!(rows[0].entries[0].borrowed && rows[1].entries[0].borrowed);
        assert!(rows[1].entries[0].dir.ends_with("donor"));
    }

    #[test]
    fn equal_texts_are_one_group_even_when_they_differ_in_line_ends_and_outer_whitespace() {
        let dir = TempDir::new();
        dir.write("a/LICENSE", b"\xef\xbb\xbf\n Same\r\ntext\r\n\r\n");
        dir.write("b/LICENSE", b"Same\ntext");
        dir.write("c/LICENSE", b"Other\rtext");
        let mut rows = vec![row(&dir.path().join("a"), "a", "MIT"), row(&dir.path().join("b"), "b", "MIT"), row(&dir.path().join("c"), "c", "Zlib")];
        discover(&mut rows).unwrap();
        let groups = group_by_content(&rows).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].text, "Same\ntext");
        assert_eq!(groups[0].users, ["a 1.0.0 (LICENSE)", "b 1.0.0 (LICENSE)"]);
        assert_eq!(groups[1].text, "Other\rtext");
        assert_eq!(groups[0].hash.len(), HASH_CHARS);
        assert_ne!(groups[0].hash, groups[1].hash);
    }

    #[test]
    fn the_hash_is_the_sha256_prefix_of_the_text() {
        // sha256("abc") = ba7816bf 8f01cfea 414140de 5dae2223 ...
        assert_eq!(short_hash("abc"), "ba7816bf8f01");
    }

    #[test]
    fn a_group_names_six_crates_and_counts_the_rest() {
        let users: Vec<String> = (1..=8).map(|n| format!("c{n} 1.0.0 (LICENSE)")).collect();
        let group = Group { hash: "0123456789ab".into(), text: String::new(), users: users.clone() };
        assert_eq!(group.heading(), format!("0123456789ab: used by {} and 2 more", users[..6].join(", ")));
        let few = Group { users: users[..6].to_vec(), ..group };
        assert!(!few.heading().contains("more"));
    }

    #[test]
    fn a_borrowed_text_is_named_as_the_standard_text_of_the_licence() {
        let dir = TempDir::new();
        dir.write("donor/LICENSE", b"text");
        let mut rows = vec![row(&dir.path().join("e"), "e", "MIT"), row(&dir.path().join("donor"), "donor", "MIT")];
        discover(&mut rows).unwrap();
        borrow_missing(&mut rows);
        let groups = group_by_content(&rows).unwrap();
        assert_eq!(groups[0].users, ["e 1.0.0 (standard text of the licence)", "donor 1.0.0 (LICENSE)"]);
    }

    #[test]
    fn an_unreadable_listed_file_is_an_error_naming_it() {
        let dir = TempDir::new();
        let mut rows = vec![row(dir.path(), "x", "MIT")];
        rows[0].entries.push(Entry { dir: dir.path().to_path_buf(), file: "LICENSE".into(), borrowed: false });
        let error = group_by_content(&rows).unwrap_err();
        assert!(error.contains("LICENSE"), "{error}");
    }
}
