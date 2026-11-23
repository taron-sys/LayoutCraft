//! usage: make-public-fixtures [--check] [root]
//!
//! Writes (or with `--check`, verifies) the self-authored public golden fixture set under `fixtures/public/`.
use std::env;
use std::path::PathBuf;

fn main() {
    let mut check = false;
    let mut root = None;
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--check" => check = true,
            "--help" | "-h" => {
                println!("usage: make-public-fixtures [--check] [root]");
                return;
            }
            other => root = Some(PathBuf::from(other)),
        }
    }
    let root = match root {
        Some(root) => root,
        None => match layoutcraft_fixtures::repository_root() {
            Ok(root) => root,
            Err(error) => {
                eprintln!("error: {error}");
                std::process::exit(2);
            }
        },
    };
    let result = if check {
        layoutcraft_fixtures::verify(&root)
    } else {
        layoutcraft_fixtures::generate(&root)
    };
    match result {
        Ok(()) => {
            if check {
                println!("public fixtures verify: OK ({})", root.display());
            } else {
                println!(
                    "wrote public fixtures: {}",
                    root.join(layoutcraft_fixtures::OUTPUT_DIR).display()
                );
            }
        }
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
