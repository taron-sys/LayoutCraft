//! `cargo xtask notices [--check]`: the third-party notices of the native programs (docs/36-packaging.md): the Rust crates that end up in
//! `layoutcraft-app` and `layoutcraft-mcp` (normal dependencies only, for one target), their licences, and the licence texts found in the
//! crate sources. It writes `THIRD-PARTY-NOTICES.txt` and `docs/36-licences.json`, or with `--check` only says whether they are current.
//! It replaces `tools/gen-third-party.mjs` and produces the same bytes (the JSON has the layout of `JSON.stringify(value, null, 1)`).
//!
//! Environment: `LC_TARGET` is the target triple (default `x86_64-pc-windows-gnu`, the Windows build). `CARGO_TOOLCHAIN` is the `+toolchain`
//! argument given to cargo: unset means `+stable-x86_64-pc-windows-gnu`, the maintainers' Windows setup; set it to the empty string to
//! use the default toolchain (CI does).

mod licence;
mod metadata;
mod texts;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Serialize;

use crate::Outcome;
use metadata::{Metadata, Package};
use texts::{Entry, Group};

const DEFAULT_TARGET: &str = "x86_64-pc-windows-gnu";
const DEFAULT_TOOLCHAIN: &str = "+stable-x86_64-pc-windows-gnu";
const NOTICES_FILE: &str = "THIRD-PARTY-NOTICES.txt";
const SUMMARY_FILE: &str = "docs/36-licences.json";
const GENERATED_BY: &str = "by cargo xtask notices";
const TARGET_VARIABLE: &str = "LC_TARGET";
const TOOLCHAIN_VARIABLE: &str = "CARGO_TOOLCHAIN";

/// Where the crates come from: the target whose dependencies count and the toolchain that runs `cargo metadata`.
#[derive(Debug, PartialEq)]
struct Config {
    target: String,
    /// The `+toolchain` argument; empty for none.
    toolchain: String,
}

impl Config {
    /// An empty `LC_TARGET` means the default; for the toolchain an empty value is a choice (no argument) and only an unset one means the default.
    fn from_values(target: Option<String>, toolchain: Option<String>) -> Config {
        Config {
            target: target.filter(|t| !t.is_empty()).unwrap_or_else(|| DEFAULT_TARGET.to_string()),
            toolchain: toolchain.unwrap_or_else(|| DEFAULT_TOOLCHAIN.to_string()),
        }
    }

    fn from_env() -> Result<Config, String> {
        let read = |name: &str| match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} is not valid Unicode")),
        };
        Ok(Config::from_values(read(TARGET_VARIABLE)?, read(TOOLCHAIN_VARIABLE)?))
    }
}

/// One crate of the list.
pub(crate) struct Row {
    pub name: String,
    pub version: String,
    /// The licence expression, or `see <file>` or `UNKNOWN`.
    pub license: String,
    pub repository: String,
    /// Is the licence acceptable (see `licence::is_acceptable`)?
    pub ok: bool,
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
}

impl Row {
    fn from_package(package: &Package) -> Row {
        let expression = package.license.as_deref().unwrap_or_default();
        let license = if !expression.is_empty() {
            expression.to_string()
        } else {
            match package.license_file.as_deref().filter(|file| !file.is_empty()) {
                Some(file) => format!("see {file}"),
                None => "UNKNOWN".to_string(),
            }
        };
        Row {
            name: package.name.clone(),
            version: package.version.clone(),
            license,
            repository: package.repository.clone().unwrap_or_default(),
            ok: licence::is_acceptable(expression),
            dir: Path::new(&package.manifest_path).parent().map(Path::to_path_buf).unwrap_or_default(),
            entries: Vec::new(),
        }
    }
}

/// The content of both generated files and what is said about it.
struct Notices {
    text: String,
    summary_json: String,
    report: Vec<String>,
}

#[derive(Serialize)]
struct Summary<'a> {
    generated: &'a str,
    target: &'a str,
    crates: Vec<SummaryCrate<'a>>,
    flagged: Vec<String>,
    #[serde(rename = "withoutText")]
    without_text: Vec<String>,
}

#[derive(Serialize)]
struct SummaryCrate<'a> {
    name: &'a str,
    version: &'a str,
    license: &'a str,
    ok: bool,
}

pub fn run(root: &Path, args: &[&str]) -> Outcome {
    let check = match args {
        [] => false,
        ["--check"] => true,
        _ => return Err("usage: cargo xtask notices [--check]".to_string()),
    };
    let config = Config::from_env()?;
    let metadata = metadata::load(&root.join("rust"), &config.toolchain, &config.target)?;
    let notices = build(&metadata, &config)?;
    let notices_path = root.join(NOTICES_FILE);
    let summary_path = root.join(SUMMARY_FILE);
    if check {
        let current = file_is(&notices_path, &notices.text) && file_is(&summary_path, &notices.summary_json);
        if current {
            println!("third-party notices are current");
            return Ok(ExitCode::SUCCESS);
        }
        println!("third-party notices are STALE: run cargo xtask notices");
        return Ok(ExitCode::FAILURE);
    }
    fs::write(&notices_path, &notices.text).map_err(|e| format!("{}: {e}", notices_path.display()))?;
    fs::write(&summary_path, &notices.summary_json).map_err(|e| format!("{}: {e}", summary_path.display()))?;
    for line in &notices.report {
        println!("{line}");
    }
    Ok(ExitCode::SUCCESS)
}

/// Does the file hold exactly `expected`? A missing or unreadable file does not.
fn file_is(path: &Path, expected: &str) -> bool {
    fs::read(path).is_ok_and(|bytes| bytes == expected.as_bytes())
}

fn build(metadata: &Metadata, config: &Config) -> Result<Notices, String> {
    let packages = metadata.third_party_crates(&metadata::PROGRAMS)?;
    let mut rows: Vec<Row> = packages.iter().map(Row::from_package).collect();
    texts::discover(&mut rows).map_err(|e| format!("licence files: {e}"))?;
    texts::borrow_missing(&mut rows);
    let groups = texts::group_by_content(&rows)?;
    Ok(Notices {
        text: render_text(&config.target, &rows, &groups),
        summary_json: render_summary(&config.target, &rows)?,
        report: report(&rows, &groups),
    })
}

/// The text file: the list of crates, then the licence texts.
fn render_text(target: &str, rows: &[Row], groups: &[Group]) -> String {
    let mut out = String::from("LayoutCraft - third-party notices\n");
    out.push_str(&format!(
        "The programs layoutcraft-app and layoutcraft-mcp contain the following Rust crates (target {target}, normal dependencies).\n"
    ));
    out.push_str("Each crate is used under the licence shown (for \"A OR B\" one of the two). The licence texts follow the list.\n\n");
    out.push_str(&format!("{} crates\n\n", rows.len()));
    for row in rows {
        let repository = if row.repository.is_empty() { String::new() } else { format!("  -  {}", row.repository) };
        out.push_str(&format!("{} {}  -  {}{repository}\n", row.name, row.version, row.license));
    }
    out.push_str("\n\n==== Licence texts ====\n");
    for group in groups {
        out.push_str(&format!("\n---- {}\n\n{}\n", group.heading(), group.text));
    }
    out.push('\n');
    out
}

/// The JSON file, indented by one space per level and ended by a newline.
fn render_summary(target: &str, rows: &[Row]) -> Result<String, String> {
    let summary = Summary {
        generated: GENERATED_BY,
        target,
        crates: rows.iter().map(|r| SummaryCrate { name: &r.name, version: &r.version, license: &r.license, ok: r.ok }).collect(),
        flagged: rows.iter().filter(|r| !r.ok).map(|r| format!("{} {} {}", r.name, r.version, r.license)).collect(),
        without_text: rows.iter().filter(|r| r.entries.is_empty()).map(|r| format!("{} {}", r.name, r.version)).collect(),
    };
    let mut buffer = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    summary.serialize(&mut serde_json::Serializer::with_formatter(&mut buffer, formatter)).map_err(|e| format!("summary: {e}"))?;
    let mut json = String::from_utf8(buffer).map_err(|e| format!("summary: {e}"))?;
    json.push('\n');
    Ok(json)
}

/// What the command prints after it wrote the files: the counts, the flagged crates and how many crates use each licence.
fn report(rows: &[Row], groups: &[Group]) -> Vec<String> {
    let flagged: Vec<&Row> = rows.iter().filter(|r| !r.ok).collect();
    let without_text = rows.iter().filter(|r| r.entries.is_empty()).count();
    let mut lines = vec![format!(
        "{} crates, {} distinct licence texts, {} flagged, {without_text} without a licence file",
        rows.len(),
        groups.len(),
        flagged.len()
    )];
    lines.extend(flagged.iter().map(|r| format!("FLAGGED {} {} {}", r.name, r.version, r.license)));
    lines.extend(licence_counts(rows).into_iter().map(|(license, count)| format!("{count} x {license}")));
    lines
}

/// How many crates use each licence expression: the most used first, equal counts in order of first appearance.
fn licence_counts(rows: &[Row]) -> Vec<(&str, usize)> {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for row in rows {
        match counts.iter_mut().find(|(license, _)| *license == row.license) {
            Some((_, count)) => *count += 1,
            None => counts.push((&row.license, 1)),
        }
    }
    counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn an_empty_toolchain_means_none_and_an_unset_one_means_the_default() {
        assert_eq!(Config::from_values(None, None), Config { target: DEFAULT_TARGET.into(), toolchain: DEFAULT_TOOLCHAIN.into() });
        assert_eq!(Config::from_values(None, Some(String::new())).toolchain, "");
        assert_eq!(Config::from_values(None, Some("+nightly".into())).toolchain, "+nightly");
    }

    #[test]
    fn an_empty_target_means_the_default_target() {
        assert_eq!(Config::from_values(Some(String::new()), None).target, DEFAULT_TARGET);
        assert_eq!(Config::from_values(Some("aarch64-apple-darwin".into()), None).target, "aarch64-apple-darwin");
    }

    fn package(license: Option<&str>, license_file: Option<&str>) -> Package {
        Package {
            id: "id".into(),
            name: "c".into(),
            version: "1.0.0".into(),
            license: license.map(str::to_string),
            license_file: license_file.map(str::to_string),
            repository: None,
            manifest_path: "/crates/c-1.0.0/Cargo.toml".into(),
        }
    }

    #[test]
    fn a_crate_shows_its_expression_else_its_licence_file_else_unknown() {
        assert_eq!(Row::from_package(&package(Some("MIT"), Some("LICENSE"))).license, "MIT");
        assert_eq!(Row::from_package(&package(None, Some("LICENSE.txt"))).license, "see LICENSE.txt");
        assert_eq!(Row::from_package(&package(Some(""), None)).license, "UNKNOWN");
        assert_eq!(Row::from_package(&package(None, None)).license, "UNKNOWN");
    }

    #[test]
    fn a_crate_without_an_expression_is_not_flagged_and_a_copyleft_one_is() {
        assert!(Row::from_package(&package(None, None)).ok);
        assert!(!Row::from_package(&package(Some("GPL-3.0-only"), None)).ok);
        assert!(Row::from_package(&package(Some("MIT OR Apache-2.0"), None)).ok);
    }

    #[test]
    fn a_crate_directory_is_the_folder_of_its_manifest() {
        assert_eq!(Row::from_package(&package(None, None)).dir, Path::new("/crates/c-1.0.0"));
    }

    fn row(name: &str, license: &str, ok: bool, with_text: bool) -> Row {
        Row {
            name: name.into(),
            version: "1.0.0".into(),
            license: license.into(),
            repository: if name == "a" { "https://example.invalid/a".into() } else { String::new() },
            ok,
            dir: PathBuf::new(),
            entries: if with_text { vec![Entry { dir: PathBuf::new(), file: "LICENSE".into(), borrowed: false }] } else { Vec::new() },
        }
    }

    #[test]
    fn the_text_file_lists_the_crates_then_the_texts_with_the_layout_of_the_node_tool() {
        let rows = [row("a", "MIT", true, true), row("b", "Zlib", true, true)];
        let groups = [Group { hash: "0123456789ab".into(), text: "Licence text".into(), users: vec!["a 1.0.0 (LICENSE)".into()] }];
        assert_eq!(
            render_text("t-t", &rows, &groups),
            "LayoutCraft - third-party notices\n\
             The programs layoutcraft-app and layoutcraft-mcp contain the following Rust crates (target t-t, normal dependencies).\n\
             Each crate is used under the licence shown (for \"A OR B\" one of the two). The licence texts follow the list.\n\n\
             2 crates\n\n\
             a 1.0.0  -  MIT  -  https://example.invalid/a\n\
             b 1.0.0  -  Zlib\n\
             \n\n==== Licence texts ====\n\
             \n---- 0123456789ab: used by a 1.0.0 (LICENSE)\n\nLicence text\n\n"
        );
    }

    #[test]
    fn the_summary_is_json_stringify_with_one_space_of_indent_and_the_keys_in_order() {
        let rows = [row("a", "MIT", true, true), row("b", "GPL-3.0", false, false)];
        assert_eq!(
            render_summary("t-t", &rows).unwrap(),
            "{\n \"generated\": \"by cargo xtask notices\",\n \"target\": \"t-t\",\n \"crates\": [\n  {\n   \"name\": \"a\",\n   \"version\": \"1.0.0\",\n   \"license\": \"MIT\",\n   \"ok\": true\n  },\n  {\n   \"name\": \"b\",\n   \"version\": \"1.0.0\",\n   \"license\": \"GPL-3.0\",\n   \"ok\": false\n  }\n ],\n \"flagged\": [\n  \"b 1.0.0 GPL-3.0\"\n ],\n \"withoutText\": [\n  \"b 1.0.0\"\n ]\n}\n"
        );
        let empty = render_summary("t", &[]).unwrap();
        assert!(empty.contains("\"crates\": [],\n \"flagged\": [],\n \"withoutText\": []\n}"));
    }

    #[test]
    fn the_summary_escapes_like_json_stringify_and_keeps_non_ascii_text() {
        let mut odd = row("a", "quote\" back\\ tab\t slash/ \u{e9}", true, true);
        odd.version = "\u{1}".into();
        let json = render_summary("t", &[odd]).unwrap();
        assert!(json.contains(r#""license": "quote\" back\\ tab\t slash/ é""#), "{json}");
        assert!(json.contains(r#""version": "\u0001""#), "{json}");
    }

    #[test]
    fn the_report_counts_crates_texts_flagged_and_licences_most_used_first_with_ties_in_list_order() {
        let rows = [
            row("a", "Zlib", true, true),
            row("b", "MIT", true, true),
            row("c", "GPL-3.0", false, false),
            row("d", "MIT", true, true),
            row("e", "Zlib", true, true),
            row("f", "ISC", true, true),
        ];
        let groups = [Group { hash: String::new(), text: String::new(), users: Vec::new() }];
        assert_eq!(
            report(&rows, &groups),
            [
                "6 crates, 1 distinct licence texts, 1 flagged, 1 without a licence file",
                "FLAGGED c 1.0.0 GPL-3.0",
                "2 x Zlib",
                "2 x MIT",
                "1 x GPL-3.0",
                "1 x ISC",
            ]
        );
    }

    #[test]
    fn a_file_is_current_only_when_its_bytes_are_equal_and_a_missing_file_is_not() {
        let dir = TempDir::new();
        dir.write("n.txt", b"text\n");
        assert!(file_is(&dir.path().join("n.txt"), "text\n"));
        assert!(!file_is(&dir.path().join("n.txt"), "text"));
        assert!(!file_is(&dir.path().join("n.txt"), "text\r\n"));
        assert!(!file_is(&dir.path().join("missing"), ""));
    }

    #[test]
    fn unknown_arguments_are_refused_before_anything_is_written() {
        let dir = TempDir::new();
        assert!(run(dir.path(), &["--chek"]).is_err());
        assert!(run(dir.path(), &["--check", "--check"]).is_err());
    }
}
