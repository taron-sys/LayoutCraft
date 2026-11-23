//! Generates the self-authored public golden fixture set (`fixtures/public/`).
//!
//! Every source package is produced by [`layoutcraft-engine`] from drawing operations written here; no Visio-authored file,
//! stencil artwork or Visio-rendered image enters the set. The golden model is what our importer reads back, and the untouched
//! entry hashes come from a no-op patch save of the package (the invariant "a save with nothing to change rewrites every part
//! byte for byte"). See `docs/39-fixture-replacement.md`.
use std::fs;
use std::path::Path;

use layoutcraft_conformance::{canonical_hash, entry_hashes_from_bytes, sha256};
use layoutcraft_engine::Engine;
use layoutcraft_vsdx::{import_document, patch_page, Package};
use serde_json::{json, Value};

pub const OUTPUT_DIR: &str = "fixtures/public";

pub struct Spec {
    pub id: &'static str,
    pub description: &'static str,
    pub build: fn(&mut Engine) -> Result<(), String>,
}

pub fn specs() -> Vec<Spec> {
    vec![
        Spec {
            id: "public-basic-shapes",
            description: "Rectangle, rounded rectangle, ellipse, diamond, triangle and text box with fills, strokes and labels",
            build: basic_shapes,
        },
        Spec {
            id: "public-connectors",
            description: "Two glued right-angle connectors between three shapes",
            build: connectors,
        },
        Spec {
            id: "public-freeform",
            description: "A spline, an arc and open and closed polylines",
            build: freeform,
        },
        Spec {
            id: "public-picture",
            description: "A picture inserted from our own generated PNG",
            build: picture,
        },
        Spec {
            id: "public-pages",
            description: "A two-page drawing with a named second page",
            build: pages,
        },
        Spec {
            id: "public-groups",
            description: "Three shapes grouped as one unit",
            build: groups,
        },
        Spec {
            id: "public-text-and-data",
            description: "Multi-line bold/italic/underlined text, a Shape Data row and alt text",
            build: text_and_data,
        },
    ]
}

pub struct Built {
    pub id: String,
    pub description: String,
    pub source: Vec<u8>,
    pub model_text: String,
    pub counts: Value,
    pub untouched: Value,
}

pub fn build_all() -> Result<Vec<Built>, String> {
    specs().into_iter().map(build_one).collect()
}

fn build_one(spec: Spec) -> Result<Built, String> {
    let mut engine = Engine::new();
    (spec.build)(&mut engine).map_err(|error| format!("{}: {error}", spec.id))?;
    let source = engine
        .export()
        .ok_or_else(|| format!("{}: the drawing has no package", spec.id))?
        .to_vec();
    let package = Package::from_bytes(&source).map_err(|error| format!("{}: {error}", spec.id))?;
    let document = import_document(&package).map_err(|error| format!("{}: {error}", spec.id))?;
    document
        .validate()
        .map_err(|error| format!("{}: {error}", spec.id))?;
    // The key order must not depend on whether another crate in the workspace enables serde_json's `preserve_order`
    // (layoutcraft-mcp does): write the model with sorted keys so the committed file is the same in every build.
    let model: Value =
        serde_json::to_value(&document).map_err(|error| format!("{}: {error}", spec.id))?;
    let model_text = pretty_sorted(&model).map_err(|error| format!("{}: {error}", spec.id))?;
    let counts = counts(&model)?;
    let patched =
        patch_page(&package, 0, &[], &[], &[]).map_err(|error| format!("{}: {error}", spec.id))?;
    let entries =
        entry_hashes_from_bytes(&patched).map_err(|error| format!("{}: {error}", spec.id))?;
    let untouched = json!({
        "savedName": format!("{}-edited.vsdx", spec.id),
        "entryCount": entries.len(),
        "entries": entries,
    });
    Ok(Built {
        id: spec.id.to_string(),
        description: spec.description.to_string(),
        source,
        model_text,
        counts,
        untouched,
    })
}

pub fn generate(root: &Path) -> Result<(), String> {
    let out = root.join(OUTPUT_DIR);
    fs::create_dir_all(out.join("sources")).map_err(|error| error.to_string())?;
    fs::create_dir_all(out.join("models")).map_err(|error| error.to_string())?;
    let built = build_all()?;
    let mut fixtures = Vec::new();
    let mut provenance = Vec::new();
    for item in &built {
        let source_name = format!("{}.vsdx", item.id);
        let model_name = format!("{}.json", item.id);
        fs::write(out.join("sources").join(&source_name), &item.source)
            .map_err(|error| error.to_string())?;
        fs::write(out.join("models").join(&model_name), &item.model_text)
            .map_err(|error| error.to_string())?;
        let model: Value =
            serde_json::from_str(&item.model_text).map_err(|error| error.to_string())?;
        let source_sha = sha256(&item.source);
        fixtures.push(json!({
            "id": item.id,
            "kind": "self-authored",
            "source": format!("{OUTPUT_DIR}/sources/{source_name}"),
            "sourceBytes": item.source.len(),
            "sourceSha256": source_sha,
            "documentSchemaVersion": 1,
            "model": format!("models/{model_name}"),
            "modelSha256": canonical_hash(&model)?,
            "modelFileSha256": sha256(item.model_text.as_bytes()),
            "counts": item.counts,
            "untouched": item.untouched,
        }));
        provenance.push(json!({
            "fixture": item.id,
            "source": format!("{OUTPUT_DIR}/sources/{source_name}"),
            "sourceSha256": source_sha,
            "origin": "self-authored",
            "status": "cleared",
            "redistributable": true,
            "licence": "MIT OR Apache-2.0",
            "notes": format!("{}; generated by layoutcraft-fixtures from our own drawing operations (no third-party artwork, no Visio-authored file).", item.description),
        }));
    }
    let manifest = json!({
        "manifestVersion": 1,
        "documentSchemaVersion": 1,
        "generatedBy": "layoutcraft-fixtures make-public-fixtures (self-authored set, 2026-11-22)",
        "fixtures": fixtures,
    });
    write_pretty(&out.join("manifest.json"), &manifest)?;
    let provenance = json!({
        "manifestVersion": 1,
        "reviewStatus": "cleared",
        "reviewedAt": "2026-11-22",
        "policy": "Every fixture in this set is generated by LayoutCraft itself from drawing operations written in layoutcraft-fixtures. No Microsoft, vendor or Visio-authored material is included, so this set may be redistributed under the project licence. The private Visio-authored corpus stays validation-only and is never distributed (docs/38-private-fixtures.md, docs/39-fixture-replacement.md).",
        "fixtures": provenance,
        "visuals": {
            "manifest": "",
            "renderer": "none",
            "status": "cleared",
            "redistributable": true,
            "notes": "No visual reference images are shipped with the public set; visual comparison against Visio remains a private validation check."
        }
    });
    write_pretty(&out.join("provenance.json"), &provenance)?;
    Ok(())
}

/// Regenerate in memory and compare with what is committed. Used by the test and the `verify` command.
pub fn verify(root: &Path) -> Result<(), String> {
    let out = root.join(OUTPUT_DIR);
    let manifest_text =
        fs::read_to_string(out.join("manifest.json")).map_err(|error| error.to_string())?;
    let manifest: Value =
        serde_json::from_str(&manifest_text).map_err(|error| error.to_string())?;
    let listed = manifest["fixtures"]
        .as_array()
        .ok_or("manifest has no fixtures array")?;
    let built = build_all()?;
    if listed.len() != built.len() {
        return Err(format!(
            "manifest lists {} fixtures, generator builds {}",
            listed.len(),
            built.len()
        ));
    }
    for (item, entry) in built.iter().zip(listed) {
        let source_path = out.join("sources").join(format!("{}.vsdx", item.id));
        let committed = fs::read(&source_path).map_err(|error| error.to_string())?;
        if committed != item.source {
            return Err(format!(
                "{}: committed source differs from the generator output (regenerate with make-public-fixtures)",
                item.id
            ));
        }
        let model_path = out.join("models").join(format!("{}.json", item.id));
        let committed_model = fs::read_to_string(&model_path).map_err(|error| error.to_string())?;
        if committed_model != item.model_text {
            return Err(format!(
                "{}: committed golden model differs from the importer output",
                item.id
            ));
        }
        if entry["id"] != item.id.as_str() {
            return Err(format!("{}: manifest id mismatch", item.id));
        }
        if entry["sourceBytes"].as_u64() != Some(item.source.len() as u64) {
            return Err(format!("{}: manifest source byte count mismatch", item.id));
        }
        if entry["sourceSha256"] != sha256(&item.source).as_str() {
            return Err(format!("{}: manifest source hash mismatch", item.id));
        }
        if entry["modelFileSha256"] != sha256(item.model_text.as_bytes()).as_str() {
            return Err(format!("{}: manifest model file hash mismatch", item.id));
        }
        let model: Value =
            serde_json::from_str(&item.model_text).map_err(|error| error.to_string())?;
        if entry["modelSha256"] != canonical_hash(&model)?.as_str() {
            return Err(format!(
                "{}: manifest canonical model hash mismatch",
                item.id
            ));
        }
        if entry["counts"] != item.counts {
            return Err(format!("{}: manifest counts mismatch", item.id));
        }
        if entry["untouched"] != item.untouched {
            return Err(format!("{}: manifest untouched entries mismatch", item.id));
        }
    }
    let provenance_text =
        fs::read_to_string(out.join("provenance.json")).map_err(|error| error.to_string())?;
    let provenance: Value =
        serde_json::from_str(&provenance_text).map_err(|error| error.to_string())?;
    if provenance["reviewStatus"] != "cleared" {
        return Err("public provenance is not marked cleared".to_string());
    }
    for entry in provenance["fixtures"]
        .as_array()
        .ok_or("provenance has no fixtures array")?
    {
        if entry["origin"] != "self-authored" || entry["redistributable"] != true {
            return Err(format!(
                "{}: public provenance must be self-authored and redistributable",
                entry["fixture"]
            ));
        }
    }
    Ok(())
}

fn write_pretty(path: &Path, value: &Value) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    text.push('\n');
    fs::write(path, text).map_err(|error| error.to_string())
}

/// Pretty JSON with every object's keys sorted. `serde_json`'s object order depends on the `preserve_order` feature, which a
/// workspace-wide build can enable through another crate; sorting here makes the model file stable in every build.
fn pretty_sorted(value: &Value) -> Result<String, String> {
    fn sort(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut sorted = serde_json::Map::new();
                for key in keys {
                    sorted.insert(key.clone(), sort(&map[key]));
                }
                Value::Object(sorted)
            }
            Value::Array(items) => Value::Array(items.iter().map(sort).collect()),
            other => other.clone(),
        }
    }
    let mut text = serde_json::to_string_pretty(&sort(value)).map_err(|error| error.to_string())?;
    text.push('\n');
    Ok(text)
}

fn counts(model: &Value) -> Result<Value, String> {
    let pages = model["pages"]
        .as_array()
        .ok_or("model has no pages array")?;
    let mut shapes = 0usize;
    let mut connectors = 0usize;
    for page in pages {
        shapes += page["shapes"]
            .as_array()
            .ok_or("page has no shapes array")?
            .len();
        connectors += page["connectors"]
            .as_array()
            .ok_or("page has no connectors array")?
            .len();
    }
    let media = model["media"].as_object().map_or(0, |map| map.len());
    Ok(json!({
        "pages": pages.len(),
        "shapes": shapes,
        "connectors": connectors,
        "media": media,
    }))
}

fn label(engine: &mut Engine, page: usize, id: &str, text: &str) -> Result<(), String> {
    engine
        .edit(page, 8, &format!("{id}\n{text}"), 0.0, 0.0, 0.0, 0.0, 0)
        .map_err(|error| format!("shape {id} on page {page}: {error}"))
}

fn style(engine: &mut Engine, page: usize, id: &str, value: &str) -> Result<(), String> {
    engine
        .edit(page, 10, &format!("{id}\n{value}"), 0.0, 0.0, 0.0, 0.0, 0)
        .map_err(|error| format!("shape {id} on page {page}: {error}"))
}

fn blank(name: &str) -> Result<Engine, String> {
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, name)?;
    Ok(engine)
}

fn basic_shapes(engine: &mut Engine) -> Result<(), String> {
    *engine = blank("Basic shapes")?;
    let rectangle = engine.draw(0, 0, 1.0, 5.6, 3.0, 6.8)?;
    label(engine, 0, &rectangle, "Rectangle")?;
    style(
        engine,
        0,
        &rectangle,
        r##"{"fill":"#4472c4","line":"#1f3864","weight":2}"##,
    )?;
    let rounded = engine.draw(0, 1, 4.0, 5.6, 6.0, 6.8)?;
    label(engine, 0, &rounded, "Rounded")?;
    style(
        engine,
        0,
        &rounded,
        r##"{"fill":"#a5a5a5","line":"#3f3f3f"}"##,
    )?;
    let ellipse = engine.draw(0, 2, 7.0, 5.6, 9.0, 6.8)?;
    label(engine, 0, &ellipse, "Ellipse")?;
    style(engine, 0, &ellipse, r##"{"fill":"#ed7d31"}"##)?;
    let diamond = engine.draw(0, 3, 1.0, 3.2, 3.0, 4.4)?;
    label(engine, 0, &diamond, "Diamond")?;
    let triangle = engine.draw(0, 4, 4.0, 3.2, 6.0, 4.4)?;
    label(engine, 0, &triangle, "Triangle")?;
    let text_box = engine.draw(0, 5, 7.0, 3.2, 9.0, 4.4)?;
    label(engine, 0, &text_box, "Text box")?;
    engine.draw(0, 6, 1.0, 1.2, 3.0, 2.4)?;
    style(engine, 0, &text_box, r##"{"fontPt":14,"bold":true}"##)?;
    Ok(())
}

fn connectors(engine: &mut Engine) -> Result<(), String> {
    *engine = blank("Connectors")?;
    let left = engine.draw(0, 0, 1.0, 5.6, 2.5, 6.6)?;
    label(engine, 0, &left, "Start")?;
    let middle = engine.draw(0, 0, 4.5, 5.6, 6.0, 6.6)?;
    label(engine, 0, &middle, "Middle")?;
    let bottom = engine.draw(0, 2, 4.5, 1.6, 6.0, 2.8)?;
    label(engine, 0, &bottom, "End")?;
    engine.add_connector(0, &left, None, &middle, None)?;
    engine.add_connector(0, &middle, None, &bottom, None)?;
    Ok(())
}

fn freeform(engine: &mut Engine) -> Result<(), String> {
    *engine = blank("Freeform")?;
    engine.draw_curve(0, &[(1.0, 1.0), (2.0, 2.6), (3.0, 1.2), (4.0, 2.4)])?;
    engine.draw_arc(0, (1.0, 4.8), (4.0, 4.8), 0.8)?;
    engine.draw_polyline(0, &[(6.0, 1.0), (7.0, 2.4), (8.0, 1.2), (9.0, 2.6)], false)?;
    engine.draw_polyline(0, &[(6.0, 4.0), (6.8, 5.2), (7.6, 4.0), (8.4, 5.2)], true)?;
    Ok(())
}

fn picture(engine: &mut Engine) -> Result<(), String> {
    *engine = blank("Picture")?;
    let png = sample_png()?;
    engine.insert_picture(0, &png, 2.0, 3.2, 6.0, 6.2)?;
    Ok(())
}

fn pages(engine: &mut Engine) -> Result<(), String> {
    *engine = blank("Overview")?;
    let overview = engine.draw(0, 0, 2.0, 4.0, 8.0, 5.6)?;
    label(engine, 0, &overview, "Overview")?;
    engine.page_op(1, 0, 0, "Details")?;
    let details = engine.draw(1, 0, 2.0, 4.0, 5.0, 5.2)?;
    label(engine, 1, &details, "Details")?;
    let note = engine.draw(1, 5, 5.5, 4.0, 9.0, 5.2)?;
    label(engine, 1, &note, "A second page written by LayoutCraft")?;
    Ok(())
}

fn groups(engine: &mut Engine) -> Result<(), String> {
    *engine = blank("Groups")?;
    let one = engine.draw(0, 0, 1.0, 5.0, 3.0, 6.2)?;
    let two = engine.draw(0, 2, 4.0, 5.0, 6.0, 6.2)?;
    let three = engine.draw(0, 4, 7.0, 5.0, 9.0, 6.2)?;
    label(engine, 0, &one, "One")?;
    label(engine, 0, &two, "Two")?;
    label(engine, 0, &three, "Three")?;
    engine.group_op(0, 0, &format!("{one}\n{two}\n{three}"))?;
    Ok(())
}

fn text_and_data(engine: &mut Engine) -> Result<(), String> {
    *engine = blank("Text and data")?;
    let text = engine.draw(0, 5, 1.0, 4.0, 6.0, 6.4)?;
    label(engine, 0, &text, "First line\nsecond line")?;
    style(
        engine,
        0,
        &text,
        r##"{"bold":true,"italic":true,"underline":true,"fontPt":18}"##,
    )?;
    engine.edit(
        0,
        17,
        &format!(
            "{text}\n{}",
            json!({
                "name": "Cost",
                "label": "Monthly cost",
                "value": "1200",
                "prompt": "What the component costs each month",
                "format": "0",
                "type": 0,
                "invisible": false,
            })
        ),
        0.0,
        0.0,
        0.0,
        0.0,
        0,
    )?;
    engine.page_op(
        39,
        0,
        0,
        &format!("{text}|A text box describing a monthly cost"),
    )?;
    Ok(())
}

fn sample_png() -> Result<Vec<u8>, String> {
    use image::{ImageFormat, Rgb, RgbImage};
    use std::io::Cursor;
    let mut image = RgbImage::new(64, 48);
    for y in 0..48 {
        for x in 0..64 {
            let colour = if (x / 8 + y / 8) % 2 == 0 {
                Rgb([68u8, 114, 196])
            } else {
                Rgb([237u8, 125, 49])
            };
            image.put_pixel(x, y, colour);
        }
    }
    let mut out = Cursor::new(Vec::new());
    image
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(out.into_inner())
}

/// A convenience for the binary and tests: the repository root.
pub fn repository_root() -> Result<std::path::PathBuf, String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "cannot find the repository root".to_string())
}
