//! usage: fresh-write <model.json> <out.vsdx> [media-source.vsdx]
use std::collections::BTreeMap;
use std::env;
use layoutcraft_model::Document;
use layoutcraft_vsdx::{write_fresh_document, write_fresh_document_with_media, Package};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 || args.len() > 4 {
        eprintln!("usage: fresh-write <model.json> <out.vsdx> [media-source.vsdx]");
        std::process::exit(2);
    }
    let document = match Document::open(&args[1]) {
        Ok(document) => document,
        Err(error) => {
            eprintln!("could not open {}: {error}", args[1]);
            std::process::exit(1);
        }
    };
    let bytes = if let Some(media_source) = args.get(3) {
        let package = match Package::open(media_source) {
            Ok(package) => package,
            Err(error) => {
                eprintln!("could not open {media_source}: {error}");
                std::process::exit(1);
            }
        };
        let mut media = BTreeMap::new();
        for (id, record) in &document.media {
            if let Some(part) = record.part.as_deref() {
                if let Some(bytes) = package.entry(part) {
                    media.insert(id.clone(), bytes.to_vec());
                }
            }
        }
        write_fresh_document_with_media(&document, &media)
    } else {
        write_fresh_document(&document)
    };
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("fresh write failed: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = std::fs::write(&args[2], bytes) {
        eprintln!("could not write {}: {error}", args[2]);
        std::process::exit(1);
    }
}
