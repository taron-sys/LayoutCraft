//! Dump the importer's merged ShapeSheet view and rendered model for the cell
//! oracle harness.
//!
//! usage: dump-model-cells <source.vsdx> <out.json>
//!
//! The output is intentionally redundant: `resolved` keeps page/master cells
//! and geometry before flattening, `model` keeps the final paint/parts/text.
//! `visio-test-harness/compare-cells.mjs` aligns both with a Visio COM dump.
use std::fs;
use std::path::PathBuf;

use serde_json::{json, Map, Value};
use layoutcraft_vsdx::{import_document, Package, ResolvedShape};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: dump-model-cells <source.vsdx> <out.json>");
        std::process::exit(2);
    }
    let package = match Package::open(&args[1]) {
        Ok(package) => package,
        Err(error) => {
            eprintln!("could not open {}: {error}", args[1]);
            std::process::exit(1);
        }
    };
    let document = match import_document(&package) {
        Ok(document) => document,
        Err(error) => {
            eprintln!("could not import {}: {error}", args[1]);
            std::process::exit(1);
        }
    };
    let package_pages = match package.pages() {
        Ok(pages) => pages,
        Err(error) => {
            eprintln!("could not list pages: {error}");
            std::process::exit(1);
        }
    };

    let mut pages = Vec::new();
    for (index, page) in package_pages.iter().enumerate() {
        let resolved = package
            .resolve_page_shapes(&page.part)
            .map(|shapes| shapes.iter().map(shape_json).collect::<Vec<_>>())
            .unwrap_or_default();
        let model = document
            .pages
            .get(index)
            .and_then(|page| serde_json::to_value(page).ok())
            .unwrap_or(Value::Null);
        pages.push(json!({
            "index": index + 1,
            "id": page.id,
            "name": page.name,
            "part": page.part,
            "model": model,
            "resolved": resolved,
        }));
    }

    let out = json!({
        "source": PathBuf::from(&args[1]).display().to_string(),
        "generatedBy": "layoutcraft-vsdx/examples/dump-model-cells.rs",
        "schemaVersion": document.schema_version,
        "media": document.media.len(),
        "pages": pages,
    });
    let text = match serde_json::to_string_pretty(&out) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("could not serialize: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = fs::write(&args[2], text) {
        eprintln!("could not write {}: {error}", args[2]);
        std::process::exit(1);
    }
    println!("wrote {}", args[2]);
}

fn shape_json(node: &ResolvedShape) -> Value {
    json!({
        "id": node.id,
        "parent": node.parent,
        "name": node.name,
        "type": node.shape_type,
        "master": node.master_id,
        "masterName": node.master_name,
        "masterShape": node.master_shape_id,
        "masterExplicit": node.master_explicit,
        "fillStyle": node.fill_style,
        "lineStyle": node.line_style,
        "textStyle": node.text_style,
        "hasText": node.has_text,
        "text": node.text,
        "cells": cells_json(&node.cells),
        "sections": sections_json(&node.sections),
        "ownSections": sections_json(&node.own_sections),
        "masterSections": sections_json(&node.master_sections),
        "masterChildren": node
            .master_children
            .iter()
            .map(|child| json!({
                "id": child.id,
                "parent": child.parent,
                "name": child.name,
                "type": child.shape_type,
                "fillStyle": child.fill_style,
                "lineStyle": child.line_style,
                "textStyle": child.text_style,
                "hasText": child.has_text,
                "text": child.text,
                "cells": cells_json(&child.cells),
                "sections": sections_json(&child.sections),
            }))
            .collect::<Vec<_>>(),
        "children": node.children.iter().map(shape_json).collect::<Vec<_>>(),
    })
}

fn cells_json(cells: &std::collections::BTreeMap<String, layoutcraft_vsdx::shapes::Cell>) -> Value {
    let mut out = Map::new();
    for (name, cell) in cells {
        out.insert(
            name.clone(),
            json!({
                "value": cell.value,
                "formula": cell.formula,
                "unit": cell.unit,
            }),
        );
    }
    Value::Object(out)
}

fn sections_json(sections: &[layoutcraft_vsdx::shapes::Section]) -> Value {
    Value::Array(
        sections
            .iter()
            .map(|section| {
                json!({
                    "name": section.name,
                    "index": section.index,
                    "cells": cells_json(&section.cells),
                    "rows": section.rows.iter().map(|row| json!({
                        "index": row.index,
                        "type": row.row_type,
                        "name": row.name,
                        "cells": cells_json(&row.cells),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}
