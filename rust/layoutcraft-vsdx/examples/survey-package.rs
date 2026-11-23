//! Survey Visio packages: open, masters, pages, import, timings, panics.
//!
//! usage: survey-package <file>...
//! Prints one tab-separated line per file:
//! path, status, open_ms, masters, pages, import_ms, shapes, connectors, groups, note
use std::time::Instant;

use layoutcraft_vsdx::{import_document, MasterCatalog, Package};

fn main() {
    for path in std::env::args().skip(1) {
        let result = std::panic::catch_unwind(|| survey(&path));
        match result {
            Ok(line) => println!("{line}"),
            Err(_) => println!("{path}\tPANIC\t\t\t\t\t\t\t\timporter panicked"),
        }
    }
}

fn survey(path: &str) -> String {
    let mut head = [0u8; 8];
    if let Ok(mut file) = std::fs::File::open(path) {
        use std::io::Read;
        let _ = file.read(&mut head);
    }
    if head[..2] != *b"PK" {
        return format!("{path}\tNOT-OOXML\t\t\t\t\t\t\t\tlegacy/unknown binary");
    }
    let started = Instant::now();
    let package = match Package::open(path) {
        Ok(package) => package,
        Err(error) => return format!("{path}\tOPEN-ERR\t\t\t\t\t\t\t\t{error}"),
    };
    let open_ms = started.elapsed().as_millis();
    let masters = MasterCatalog::load(&package)
        .map(|catalog| catalog.masters.len().to_string())
        .unwrap_or_else(|error| format!("err:{error}"));
    let pages = package.pages().map(|p| p.len()).unwrap_or(0);
    if pages == 0 {
        return format!("{path}\tSTENCIL\t{open_ms}\t{masters}\t0\t\t\t\t\tno pages");
    }
    let started = Instant::now();
    match import_document(&package) {
        Ok(document) => {
            let shapes: usize = document.pages.iter().map(|p| p.shapes.len()).sum();
            let connectors: usize = document.pages.iter().map(|p| p.connectors.len()).sum();
            let groups: usize = document.pages.iter().map(|p| p.groups.len()).sum();
            format!(
                "{path}\tOK\t{open_ms}\t{masters}\t{pages}\t{}\t{shapes}\t{connectors}\t{groups}\t",
                started.elapsed().as_millis()
            )
        }
        Err(error) => format!("{path}\tIMPORT-ERR\t{open_ms}\t{masters}\t{pages}\t\t\t\t\t{error}"),
    }
}
