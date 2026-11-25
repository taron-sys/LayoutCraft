//! Repository tools, run from `rust/` as `cargo xtask <command>` (the alias is in `.cargo/config.toml`).
//!
//! Std only, plus `serde_json` and `sha2`, which the workspace already uses: the maintainers build offline.
//! The commands replace the former Node scripts: `provenance` (tools/check-provenance.mjs), `notices` (tools/gen-third-party.mjs)
//! and `serve` (server.mjs); see docs/42-javascript-removal.md.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod js_compat;
mod notices;
mod provenance;
mod serve;
#[cfg(test)]
mod testutil;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  provenance             public wording, private paths and fixture provenance of the publishable files (CI runs this)
  notices [--check]      write THIRD-PARTY-NOTICES.txt and docs/36-licences.json from the crates of the two programs;
                         --check only reports whether they are current
                         (env: LC_TARGET = target triple, default x86_64-pc-windows-gnu;
                          CARGO_TOOLCHAIN = `+toolchain` argument, default +stable-x86_64-pc-windows-gnu, empty = none)
  serve [port] [--host <address>]
                         static files of the repository for local development, e.g. /web/shell.html
                         (default 127.0.0.1:8137, or the PORT environment variable)
";

/// What a command reports: success or failure of the check itself, or an error that stopped it.
pub type Outcome = Result<ExitCode, String>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let outcome = match args.first().map(String::as_str) {
        Some("provenance") => no_arguments(&rest).and_then(|()| provenance::run(&repo_root())),
        Some("notices") => notices::run(&repo_root(), &rest),
        Some("serve") => serve::run(&repo_root(), &rest),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match outcome {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn no_arguments(rest: &[&str]) -> Result<(), String> {
    match rest.first() {
        None => Ok(()),
        Some(extra) => Err(format!("unexpected argument `{extra}`\n\n{USAGE}")),
    }
}

/// The repository root: two levels above this crate (`<root>/rust/xtask`).
fn repo_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.ancestors().nth(2).unwrap_or(manifest_dir).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repository_root_holds_the_cargo_workspace_and_the_provenance_notes() {
        let root = repo_root();
        assert!(root.join("rust").join("Cargo.toml").is_file());
        assert!(root.join("PROVENANCE.md").is_file());
    }

    #[test]
    fn a_command_that_takes_no_arguments_rejects_one() {
        assert!(no_arguments(&[]).is_ok());
        assert!(no_arguments(&["--x"]).is_err());
    }
}
