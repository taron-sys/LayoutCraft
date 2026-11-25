//! `cargo xtask provenance`: the provenance guard for the public tree (docs/41-legal-audit-loop.md, PROVENANCE.md).
//!
//! It fails when a file that may be published contains an unverifiable provenance claim, a private path or a reference to private notes,
//! when a private path or a third-party drawing is tracked, or when the provenance files are missing. It is deliberately a checklist of
//! phrases ([`rules`]), not a legal analysis: its job is to keep the public wording honest and to keep private material out of the public
//! tree. It replaces `tools/check-provenance.mjs`; the rules, the skip lists, the output lines and the exit code are the same.
//!
//! Its own sources contain the forbidden phrases (as test cases), so they are in [`SKIP_FILES`].

mod matcher;
mod rules;

use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, ExitCode};

use crate::js_compat::{collapse_spaces, slice_utf16, trim};
use crate::Outcome;

/// Directories that are never searched (matched against every part of a path).
const SKIP_DIRS: [&str; 8] = [".git", "node_modules", "target", "dist", "docs-private", "private", "test", "examples"];
/// Files that are never searched: generated files and notes that list the phrases on purpose, and the sources of this tool.
const SKIP_FILES: [&str; 8] = [
    ".gitignore",
    "THIRD-PARTY-NOTICES.txt",
    "rust/Cargo.lock",
    "package-lock.json",
    "docs/41-legal-audit-loop.md",
    "rust/xtask/src/provenance.rs",
    "rust/xtask/src/provenance/matcher.rs",
    "rust/xtask/src/provenance/rules.rs",
];
/// Extensions of files that are not text.
const BINARY_EXTENSIONS: [&str; 24] = [
    ".7z", ".avif", ".bin", ".bmp", ".emf", ".gif", ".gz", ".ico", ".jpg", ".jpeg", ".pdf", ".png", ".tar", ".ttf", ".wasm", ".webp", ".woff",
    ".woff2", ".zip", ".vsd", ".vsdx", ".vssx", ".vstx", ".vdx",
];
/// Larger files are not searched.
const MAX_TEXT_BYTES: u64 = 1024 * 1024;
/// A file whose first bytes contain a NUL is binary.
const BINARY_SNIFF_BYTES: usize = 8192;
/// The longest excerpt of a finding, in UTF-16 code units.
const EXCERPT_UNITS: usize = 180;
/// Paths that may exist only in the private repository.
const PRIVATE_PREFIXES: [&str; 5] = ["docs-private/", "private/", "test/", "examples/", "rust/layoutcraft-engine/tests/oracle/"];
/// Provenance material that must exist.
const REQUIRED_FILES: [&str; 4] =
    ["PROVENANCE.md", "web/fonts/LICENSE-Liberation.txt", "web/fonts/LICENSE-Carlito-OFL.txt", "web/fonts/LICENSE-Caladea-OFL.txt"];
const ICON_README: &str = "web/assets/icons/README.md";

pub fn run(root: &Path) -> Outcome {
    let (checked, findings) = check_tree(root).map_err(|e| format!("provenance guard: {e}"))?;
    for finding in &findings {
        eprintln!("FAIL {finding}");
    }
    if findings.is_empty() {
        println!("Provenance guard: {checked} public files checked, no findings.");
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!("\nProvenance guard failed. See PROVENANCE.md and docs/41-legal-audit-loop.md.");
        Ok(ExitCode::FAILURE)
    }
}

/// The number of text files searched, and every finding in report order: line rules, fixtures among the files, tracked paths, required files.
fn check_tree(root: &Path) -> io::Result<(usize, Vec<String>)> {
    let mut findings = Vec::new();
    let mut texts = Vec::new();
    for relative in publishable_files(root)? {
        if relative.split('/').any(|part| SKIP_DIRS.contains(&part)) {
            continue;
        }
        if let Some(text) = read_text_file(root, &relative)? {
            texts.push((relative, text));
        }
    }
    for (relative, text) in &texts {
        findings.extend(line_findings(relative, text));
    }
    for (relative, _) in &texts {
        if is_forbidden_fixture(relative) {
            findings.push(format!("{relative}: test/stencil package in the publishable tree"));
        }
    }
    findings.extend(tracked_findings(root));
    findings.extend(required_file_findings(root)?);
    Ok((texts.len(), findings))
}

/// The files that may be published: tracked and untracked files that git does not ignore. Without git (a source archive) the tree is
/// walked, honouring the same directory skip list.
fn publishable_files(root: &Path) -> io::Result<Vec<String>> {
    match git_files(root, &["ls-files", "-co", "--exclude-standard", "-z"]) {
        Some(files) => Ok(files),
        None => {
            let mut files = Vec::new();
            walk(root, root, &mut files)?;
            Ok(files)
        }
    }
}

/// The output of a `git` command that lists paths separated by NUL; `None` when git is missing or fails (outside a repository).
fn git_files(root: &Path, args: &[&str]) -> Option<Vec<String>> {
    let output = Command::new("git").args(args).current_dir(root).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(text.split('\0').filter(|path| !path.is_empty()).map(str::to_string).collect())
}

/// All files below `dir` as `/`-separated paths relative to `root`, in name order; skipped directories are not entered.
/// A symbolic link counts as a file and is not followed.
fn walk(root: &Path, dir: &Path, files: &mut Vec<String>) -> io::Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if entry.file_type()?.is_dir() {
            if !entry.file_name().to_str().is_some_and(|name| SKIP_DIRS.contains(&name)) {
                walk(root, &entry.path(), files)?;
            }
        } else if let Ok(relative) = entry.path().strip_prefix(root) {
            files.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// `path.extname(relative)`: the last `.` and what follows it in the file name, empty for a name that only starts with a dot.
fn extension(relative: &str) -> &str {
    let name = relative.rsplit('/').next().unwrap_or(relative);
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[dot..],
        _ => "",
    }
}

/// The contents of a file that is searched: not skipped, not binary by extension, a regular file of at most 1 MiB without a NUL in its
/// first 8 KiB. A file that git lists but that is gone from the working tree (deleted, not yet staged) is not searched.
fn read_text_file(root: &Path, relative: &str) -> io::Result<Option<String>> {
    if SKIP_FILES.contains(&relative) || BINARY_EXTENSIONS.contains(&extension(relative).to_lowercase().as_str()) {
        return Ok(None);
    }
    let path = root.join(relative);
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io::Error::new(e.kind(), format!("{relative}: {e}"))),
    };
    if !metadata.is_file() || metadata.len() > MAX_TEXT_BYTES {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|e| io::Error::new(e.kind(), format!("{relative}: {e}")))?;
    if bytes.iter().take(BINARY_SNIFF_BYTES).any(|&b| b == 0) {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

/// A finding for every line and rule that matches: `path:line: rule: excerpt`.
fn line_findings(relative: &str, text: &str) -> Vec<String> {
    let mut findings = Vec::new();
    for (index, line) in text.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        for rule in rules::RULES.iter().filter(|rule| rule.flags(line)) {
            let excerpt = slice_utf16(&collapse_spaces(trim(line)), EXCERPT_UNITS);
            findings.push(format!("{relative}:{}: {}: {excerpt}", index + 1, rule.name));
        }
    }
    findings
}

/// The public tree must not carry third-party drawings or stencils: test and example folders, and every Visio file except the
/// self-authored fixtures.
fn is_forbidden_fixture(relative: &str) -> bool {
    let visio_file = [".vsdx", ".vssx", ".vstx", ".vsd", ".vdx"].contains(&extension(relative).to_lowercase().as_str());
    relative.starts_with("test/") || relative.starts_with("examples/") || (visio_file && !relative.starts_with("fixtures/public/sources/"))
}

/// Tracked files that must not be: third-party drawings and private paths. Without git there is nothing to check.
fn tracked_findings(root: &Path) -> Vec<String> {
    let mut findings = Vec::new();
    for relative in git_files(root, &["ls-files", "-z"]).unwrap_or_default() {
        if is_forbidden_fixture(&relative) {
            findings.push(format!("{relative}: test/stencil package is tracked"));
        }
        if PRIVATE_PREFIXES.iter().any(|prefix| relative.starts_with(prefix)) {
            findings.push(format!("{relative}: private path is tracked"));
        }
    }
    findings
}

/// The provenance files must exist, and the icon README must carry its provenance line.
fn required_file_findings(root: &Path) -> io::Result<Vec<String>> {
    let mut findings: Vec<String> =
        REQUIRED_FILES.iter().filter(|file| !root.join(file).exists()).map(|file| format!("{file}: required provenance file is missing")).collect();
    let has_provenance_line = match fs::read(root.join(ICON_README)) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).to_ascii_lowercase().contains("provenance:"),
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(io::Error::new(e.kind(), format!("{ICON_README}: {e}"))),
    };
    if !has_provenance_line {
        findings.push(format!("{ICON_README}: missing the icon provenance line"));
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn a_finding_names_the_file_the_line_the_rule_and_a_shortened_excerpt() {
        let text = "fine\r\n  built  clean-room\tthen\r\nPublished by archlinuxuser\n";
        let findings = line_findings("docs/x.md", text);
        assert_eq!(
            findings,
            ["docs/x.md:2: unverified clean-room claim: built clean-room then", "docs/x.md:3: private working path: Published by archlinuxuser",]
        );
    }

    #[test]
    fn two_rules_on_one_line_report_in_rule_order_and_a_long_line_is_cut_at_180_units() {
        let line = format!("{} clean room and docs-private", "x".repeat(170));
        let findings = line_findings("a.md", &line);
        assert_eq!(findings.len(), 2);
        assert!(findings[0].contains("unverified clean-room claim") && findings[1].contains("private working path"));
        assert!(findings[0].ends_with(&format!("{} clean roo", "x".repeat(170))));
    }

    #[test]
    fn the_extension_is_what_follows_the_last_dot_of_the_file_name_and_a_dotfile_has_none() {
        assert_eq!(extension("a/b.tar.GZ"), ".GZ");
        assert_eq!(extension(".gitignore"), "");
        assert_eq!(extension("dir.d/file"), "");
        assert_eq!(extension("a/.hidden.txt"), ".txt");
        assert_eq!(extension("name."), ".");
    }

    #[test]
    fn only_self_authored_visio_files_and_no_test_folders_may_be_published() {
        assert!(is_forbidden_fixture("test/anything.txt"));
        assert!(is_forbidden_fixture("examples/a.md"));
        assert!(is_forbidden_fixture("web/stencil.VSSX"));
        assert!(is_forbidden_fixture("fixtures/public/other/a.vsdx"));
        assert!(!is_forbidden_fixture("fixtures/public/sources/a.vsdx"));
        assert!(!is_forbidden_fixture("docs/test/readme.md"));
        assert!(!is_forbidden_fixture("fixtures/public/sources/a.vsdx.txt"));
    }

    #[test]
    fn a_file_is_searched_unless_it_is_skipped_binary_large_or_gone() {
        let dir = TempDir::new();
        dir.write("a.md", b"hello");
        dir.write("b.png", b"clean room");
        dir.write("c.md", b"bin\0ary");
        dir.write("rust/Cargo.lock", b"clean room");
        dir.write("big.md", &vec![b'x'; MAX_TEXT_BYTES as usize + 1]);
        dir.write("exact.md", &vec![b'x'; MAX_TEXT_BYTES as usize]);
        dir.write("late-nul.md", &[vec![b'x'; BINARY_SNIFF_BYTES], vec![0]].concat());
        let read = |name: &str| read_text_file(dir.path(), name).unwrap();
        assert_eq!(read("a.md").as_deref(), Some("hello"));
        for skipped in ["b.png", "c.md", "rust/Cargo.lock", "big.md", "missing.md"] {
            assert_eq!(read(skipped), None, "{skipped}");
        }
        assert!(read("exact.md").is_some());
        assert!(read("late-nul.md").is_some()); // only the first 8 KiB are looked at
    }

    #[test]
    fn the_walk_skips_the_directories_of_the_skip_list_at_any_depth_and_lists_the_rest_in_name_order() {
        let dir = TempDir::new();
        for file in
            ["b.md", "a.md", "sub/x.md", "sub/target/y.md", "target/z.md", "private/p.md", ".git/config", "sub/test/t.md", "docs/examples/e.md"]
        {
            dir.write(file, b"");
        }
        let mut files = Vec::new();
        walk(dir.path(), dir.path(), &mut files).unwrap();
        assert_eq!(files, ["a.md", "b.md", "sub/x.md"]);
    }

    #[test]
    fn a_tree_with_one_violation_of_each_kind_reports_each_and_a_clean_tree_reports_none() {
        let dir = TempDir::new();
        for file in REQUIRED_FILES {
            dir.write(file, b"x");
        }
        dir.write(ICON_README, b"Provenance: original");
        dir.write("docs/ok.md", b"A clean statement. We do not claim a clean-room process.");
        let (checked, findings) = check_tree(dir.path()).unwrap();
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(checked, 6);

        dir.write("docs/bad.md", b"ok\nbuilt only from public docs\n");
        dir.write(ICON_README, b"no such line");
        let missing = REQUIRED_FILES[1];
        fs::remove_file(dir.path().join(missing)).unwrap();
        let (_, findings) = check_tree(dir.path()).unwrap();
        assert_eq!(
            findings,
            [
                "docs/bad.md:2: exclusive public-source claim: built only from public docs".to_string(),
                format!("{missing}: required provenance file is missing"),
                format!("{ICON_README}: missing the icon provenance line"),
            ]
        );
    }

    #[test]
    fn the_icon_readme_line_is_found_in_any_case_and_a_missing_readme_is_a_finding() {
        let dir = TempDir::new();
        for file in REQUIRED_FILES {
            dir.write(file, b"x");
        }
        assert_eq!(required_file_findings(dir.path()).unwrap().len(), 1);
        dir.write(ICON_README, b"PROVENANCE: ours");
        assert!(required_file_findings(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn the_skipped_source_files_of_this_tool_exist() {
        let root = crate::repo_root();
        for file in SKIP_FILES.iter().filter(|file| file.starts_with("rust/xtask/")) {
            assert!(root.join(file).is_file(), "{file}");
        }
    }
}
