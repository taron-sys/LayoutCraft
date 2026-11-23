// Browser-free golden-fixture conformance runner.
//
// The manifest is the portable contract shared with the JS harness:
// source hashes, canonical model files, and untouched-patch ZIP entry hashes.
// Run with no candidate to verify the manifest/golden set, or point it at a
// model/patch directory produced by a port.
use image::{Rgb, RgbImage};
use layoutcraft_conformance::{canonical_hash, entry_hashes, sha256};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    manifest_version: u32,
    document_schema_version: u32,
    generated_by: String,
    fixtures: Vec<Fixture>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    id: String,
    #[allow(dead_code)]
    kind: String,
    source: String,
    source_bytes: u64,
    source_sha256: String,
    document_schema_version: u32,
    model: String,
    model_sha256: String,
    model_file_sha256: String,
    counts: Counts,
    untouched: Untouched,
}

#[derive(Debug, Deserialize)]
struct Counts {
    pages: usize,
    shapes: usize,
    connectors: usize,
    media: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Untouched {
    saved_name: String,
    entry_count: usize,
    entries: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VisualManifest {
    manifest_version: u32,
    document_schema_version: u32,
    renderer: String,
    dpi: u32,
    font_environment: String,
    #[allow(dead_code)]
    note: String,
    comparison: VisualComparison,
    pages: Vec<VisualPage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VisualComparison {
    width: u32,
    height: u32,
    pixel_delta: u8,
    max_mismatch: f64,
    max_mean_error: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VisualPage {
    fixture: String,
    #[allow(dead_code)]
    source: String,
    source_sha256: String,
    page: u32,
    page_name: String,
    file: String,
    sha256: String,
    bytes: u64,
    width: u32,
    height: u32,
}

#[derive(Debug)]
struct Check {
    fixture: String,
    name: &'static str,
    ok: bool,
    detail: String,
}

#[derive(Debug)]
struct Args {
    root: PathBuf,
    manifest: PathBuf,
    candidate_models: Option<PathBuf>,
    candidate_patches: Option<PathBuf>,
    visual_manifest: Option<PathBuf>,
    visuals: Option<PathBuf>,
    skip_visuals: bool,
    tolerance: f64,
}

fn usage() -> &'static str {
    "Usage: layoutcraft-conformance [options]\n\
     \n\
     Options:\n\
       --root <dir>              LayoutCraft app root (contains test/golden). Default: .\n\
       --manifest <file>        Manifest path. Default: <root>/test/golden/manifest.json\n\
       --candidate <dir>        Expected <dir>/models/<id>.json and <dir>/patches/...\n\
       --candidate-models <dir> Directory of candidate <id>.json models\n\
       --candidate-patches <dir> Directory of candidate patch VSDX/VSTX files\n\
       --tolerance <n>          Relative/absolute number tolerance. Default: 1e-9\n\
       --help                   Show this help\n\
     \n\
     With no candidate options this validates the manifest, sources and golden\n\
     models. Candidate patches are matched as <id>.<ext>, <id>-<savedName>, or\n\
     <savedName> inside the patch directory."
}

fn parse_args() -> Result<Args, String> {
    let mut root: Option<PathBuf> = None;
    let mut manifest: Option<PathBuf> = None;
    let mut candidate: Option<PathBuf> = None;
    let mut candidate_models: Option<PathBuf> = None;
    let mut candidate_patches: Option<PathBuf> = None;
    let mut visual_manifest: Option<PathBuf> = None;
    let mut visuals: Option<PathBuf> = None;
    let mut skip_visuals = false;
    let mut tolerance = 1e-9f64;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--allow-root" => {
                let dir = PathBuf::from(args.next().ok_or("--allow-root needs a value")?);
                let dir = dir.canonicalize().map_err(|e| format!("{}: {e}", dir.display()))?;
                ALLOWED_ROOTS.with(|roots| roots.borrow_mut().push(dir));
            }
            "--root" => root = Some(PathBuf::from(args.next().ok_or("--root needs a value")?)),
            "--manifest" => {
                manifest = Some(PathBuf::from(
                    args.next().ok_or("--manifest needs a value")?,
                ))
            }
            "--candidate" => {
                candidate = Some(PathBuf::from(
                    args.next().ok_or("--candidate needs a value")?,
                ))
            }
            "--candidate-models" => {
                candidate_models = Some(PathBuf::from(
                    args.next().ok_or("--candidate-models needs a value")?,
                ))
            }
            "--candidate-patches" => {
                candidate_patches = Some(PathBuf::from(
                    args.next().ok_or("--candidate-patches needs a value")?,
                ))
            }
            "--visual-manifest" => {
                visual_manifest = Some(PathBuf::from(
                    args.next().ok_or("--visual-manifest needs a value")?,
                ))
            }
            "--visuals" => {
                visuals = Some(PathBuf::from(args.next().ok_or("--visuals needs a value")?))
            }
            "--no-visuals" => skip_visuals = true,
            "--tolerance" => {
                tolerance = args
                    .next()
                    .ok_or("--tolerance needs a value")?
                    .parse()
                    .map_err(|_| "--tolerance must be a number")?;
            }
            "--help" | "-h" => {
                println!("{}", usage());
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}\n\n{}", usage())),
        }
    }
    let root = root.unwrap_or_else(|| PathBuf::from("."));
    let manifest = manifest.unwrap_or_else(|| root.join("test/golden/manifest.json"));
    if let Some(dir) = candidate {
        if candidate_models.is_none() {
            candidate_models = Some(dir.join("models"));
        }
        if candidate_patches.is_none() {
            candidate_patches = Some(dir.join("patches"));
        }
        if visuals.is_none() {
            let visual_dir = dir.join("visuals");
            if visual_dir.is_dir() {
                visuals = Some(visual_dir);
            }
        }
    }
    Ok(Args {
        root,
        manifest,
        candidate_models,
        candidate_patches,
        visual_manifest,
        visuals,
        skip_visuals,
        tolerance,
    })
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

thread_local! {
    /// Extra directories a manifest path may resolve into (`--allow-root`); empty by default.
    static ALLOWED_ROOTS: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Join a manifest-relative path, refusing escapes from the manifest root (and from the `--allow-root` directories).
fn safe_join(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("{}: {e}", root.display()))?;
    let joined = root.join(rel);
    let canonical = joined
        .canonicalize()
        .map_err(|e| format!("{}: {e}", joined.display()))?;
    let allowed = ALLOWED_ROOTS.with(|roots| roots.borrow().iter().any(|dir| canonical.starts_with(dir)));
    if !canonical.starts_with(&root) && !allowed {
        return Err(format!("path escapes manifest root: {rel}"));
    }
    Ok(canonical)
}

fn model_counts(model: &Value) -> Result<Counts, String> {
    let pages = model
        .get("pages")
        .and_then(Value::as_array)
        .ok_or("model has no pages array")?;
    let mut shapes = 0;
    let mut connectors = 0;
    for page in pages {
        shapes += page
            .get("shapes")
            .and_then(Value::as_array)
            .ok_or("page has no shapes array")?
            .len();
        connectors += page
            .get("connectors")
            .and_then(Value::as_array)
            .ok_or("page has no connectors array")?
            .len();
    }
    let media = model
        .get("media")
        .and_then(Value::as_object)
        .map(|m| m.len())
        .unwrap_or(0);
    Ok(Counts {
        pages: pages.len(),
        shapes,
        connectors,
        media,
    })
}

fn json_equal(a: &Value, b: &Value, tolerance: f64, path: &str, diffs: &mut Vec<String>) -> bool {
    if diffs.len() >= 12 {
        return false;
    }
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => {
            let (Some(x), Some(y)) = (x.as_f64(), y.as_f64()) else {
                if x == y {
                    return true;
                }
                diffs.push(format!("{path}: {x} != {y}"));
                return false;
            };
            if x == y {
                return true;
            }
            let scale = x.abs().max(y.abs()).max(1.0);
            if tolerance > 0.0 && (x - y).abs() <= tolerance * scale {
                return true;
            }
            diffs.push(format!("{path}: {x} != {y}"));
            false
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                diffs.push(format!("{path}: array length {} != {}", x.len(), y.len()));
                return false;
            }
            let mut ok = true;
            for (i, (xa, ya)) in x.iter().zip(y).enumerate() {
                ok &= json_equal(xa, ya, tolerance, &format!("{path}[{i}]"), diffs);
            }
            ok
        }
        (Value::Object(x), Value::Object(y)) => {
            let mut ok = true;
            for (key, xa) in x {
                match y.get(key) {
                    Some(ya) => {
                        ok &= json_equal(xa, ya, tolerance, &format!("{path}.{key}"), diffs)
                    }
                    None => {
                        diffs.push(format!("{path}.{key}: missing in candidate"));
                        ok = false;
                    }
                }
            }
            for key in y.keys() {
                if !x.contains_key(key) {
                    diffs.push(format!("{path}.{key}: extra in candidate"));
                    ok = false;
                }
            }
            ok
        }
        _ => {
            diffs.push(format!("{path}: type mismatch"));
            false
        }
    }
}

fn find_patch(dir: &Path, fixture: &Fixture) -> Option<PathBuf> {
    let ext = Path::new(&fixture.source)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("vsdx");
    let candidates = [
        format!("{}.{}", fixture.id, ext),
        format!("{}-{}", fixture.id, fixture.untouched.saved_name),
        fixture.untouched.saved_name.clone(),
    ];
    candidates
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
}

fn check(
    checks: &mut Vec<Check>,
    fixture: &str,
    name: &'static str,
    ok: bool,
    detail: impl Into<String>,
) {
    checks.push(Check {
        fixture: fixture.to_string(),
        name,
        ok,
        detail: detail.into(),
    });
}

fn load_rgb_on_white(path: &Path) -> Result<RgbImage, String> {
    let image = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let rgba = image.to_rgba8();
    let mut out = RgbImage::new(rgba.width(), rgba.height());
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let alpha = pixel[3] as u32;
        let blend =
            |channel: u8| -> u8 { ((channel as u32 * alpha + 255 * (255 - alpha)) / 255) as u8 };
        out.put_pixel(
            x,
            y,
            Rgb([blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]),
        );
    }
    Ok(out)
}

fn fit_on_canvas(image: &RgbImage, width: u32, height: u32) -> RgbImage {
    let iw = image.width() as f64;
    let ih = image.height() as f64;
    let scale = (width as f64 / iw).min(height as f64 / ih);
    let nw = (iw * scale).round().max(1.0) as u32;
    let nh = (ih * scale).round().max(1.0) as u32;
    let resized = image::imageops::resize(
        image,
        nw.min(width).max(1),
        nh.min(height).max(1),
        image::imageops::FilterType::Triangle,
    );
    let mut canvas = RgbImage::from_pixel(width, height, Rgb([255, 255, 255]));
    let x = ((width - resized.width()) / 2) as i64;
    let y = ((height - resized.height()) / 2) as i64;
    image::imageops::overlay(&mut canvas, &resized, x, y);
    canvas
}

fn compare_pngs(
    reference: &Path,
    candidate: &Path,
    comparison: &VisualComparison,
) -> Result<(f64, f64), String> {
    let reference = fit_on_canvas(
        &load_rgb_on_white(reference)?,
        comparison.width,
        comparison.height,
    );
    let candidate = fit_on_canvas(
        &load_rgb_on_white(candidate)?,
        comparison.width,
        comparison.height,
    );
    let pixels = (comparison.width as u64) * (comparison.height as u64);
    let mut bad = 0u64;
    let mut sum = 0u64;
    for (r, c) in reference.pixels().zip(candidate.pixels()) {
        let mut delta = 0u8;
        let mut channel_sum = 0u64;
        for channel in 0..3 {
            let d = r[channel].abs_diff(c[channel]);
            delta = delta.max(d);
            channel_sum += d as u64;
        }
        if delta > comparison.pixel_delta {
            bad += 1;
        }
        sum += channel_sum;
    }
    let mismatch = bad as f64 / pixels as f64;
    let mean = sum as f64 / (pixels as f64 * 3.0 * 255.0);
    Ok((mismatch, mean))
}

fn check_visuals(
    manifest: &Manifest,
    args: &Args,
    golden_root: &Path,
    checks: &mut Vec<Check>,
) -> Result<(), String> {
    let visual_path = args
        .visual_manifest
        .clone()
        .unwrap_or_else(|| golden_root.join("visual-manifest.json"));
    if !visual_path.is_file() {
        if args.visuals.is_some() || args.visual_manifest.is_some() {
            return Err(format!(
                "visual manifest not found: {}",
                visual_path.display()
            ));
        }
        println!(
            "
No visual manifest; skipping visual goldens."
        );
        return Ok(());
    }
    let text =
        fs::read_to_string(&visual_path).map_err(|e| format!("{}: {e}", visual_path.display()))?;
    let visual: VisualManifest =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", visual_path.display()))?;
    let before_all = checks.len();
    check(
        checks,
        "*",
        "visual manifest version",
        visual.manifest_version == 1,
        visual.manifest_version.to_string(),
    );
    check(
        checks,
        "*",
        "visual document schema version",
        visual.document_schema_version == 1,
        visual.document_schema_version.to_string(),
    );
    check(
        checks,
        "*",
        "visual comparison size",
        visual.comparison.width > 0 && visual.comparison.height > 0,
        format!("{}x{}", visual.comparison.width, visual.comparison.height),
    );
    println!(
        "Visual goldens: {} pages, {} ({} dpi; {})",
        visual.pages.len(),
        visual.renderer,
        visual.dpi,
        visual.font_environment
    );
    let source_hashes: BTreeMap<&str, &str> = manifest
        .fixtures
        .iter()
        .map(|f| (f.id.as_str(), f.source_sha256.as_str()))
        .collect();
    for page in &visual.pages {
        let label = format!("{} p{} ({})", page.fixture, page.page, page.page_name);
        match source_hashes.get(page.fixture.as_str()) {
            Some(expected) => check(
                checks,
                &page.fixture,
                "visual source sha256",
                *expected == page.source_sha256,
                page.source_sha256.clone(),
            ),
            None => check(
                checks,
                &page.fixture,
                "visual source fixture",
                false,
                format!("unknown fixture for {label}"),
            ),
        }
        let path = match safe_join(golden_root, &page.file) {
            Ok(path) => path,
            Err(err) => {
                check(checks, &page.fixture, "visual reference path", false, err);
                continue;
            }
        };
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                check(
                    checks,
                    &page.fixture,
                    "visual reference bytes",
                    false,
                    err.to_string(),
                );
                continue;
            }
        };
        check(
            checks,
            &page.fixture,
            "visual reference bytes",
            bytes.len() as u64 == page.bytes,
            format!("{} != {}", bytes.len(), page.bytes),
        );
        check(
            checks,
            &page.fixture,
            "visual reference sha256",
            sha256(&bytes) == page.sha256,
            page.sha256.clone(),
        );
        match image::image_dimensions(&path) {
            Ok((w, h)) => check(
                checks,
                &page.fixture,
                "visual reference dimensions",
                w == page.width && h == page.height,
                format!("{w}x{h} != {}x{}", page.width, page.height),
            ),
            Err(err) => check(
                checks,
                &page.fixture,
                "visual reference dimensions",
                false,
                err.to_string(),
            ),
        }
        if let Some(candidate_dir) = &args.visuals {
            let name = Path::new(&page.file)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let candidate = candidate_dir.join(name);
            if !candidate.is_file() {
                check(
                    checks,
                    &page.fixture,
                    "visual candidate exists",
                    false,
                    format!("missing {label}: {}", candidate.display()),
                );
                continue;
            }
            match compare_pngs(&path, &candidate, &visual.comparison) {
                Ok((mismatch, mean)) => check(
                    checks,
                    &page.fixture,
                    "visual compare",
                    mismatch <= visual.comparison.max_mismatch
                        && mean <= visual.comparison.max_mean_error,
                    format!(
                        "{label}: mismatch {:.4} (max {:.4}), mean {:.4} (max {:.4})",
                        mismatch,
                        visual.comparison.max_mismatch,
                        mean,
                        visual.comparison.max_mean_error
                    ),
                ),
                Err(err) => check(checks, &page.fixture, "visual compare", false, err),
            }
        }
    }
    let visual_checks = &checks[before_all..];
    let passed = visual_checks.iter().filter(|c| c.ok).count();
    let total = visual_checks.len();
    println!("Visual checks: {passed}/{total}");
    if passed != total {
        for c in visual_checks.iter().filter(|c| !c.ok).take(20) {
            println!("  {} / {}: {}", c.fixture, c.name, c.detail);
        }
    }
    Ok(())
}

fn check_fixture(
    fixture: &Fixture,
    args: &Args,
    golden_root: &Path,
    checks: &mut Vec<Check>,
) -> Result<(), String> {
    let source_path = safe_join(&args.root, &fixture.source)?;
    let source = fs::read(&source_path).map_err(|e| format!("{}: {e}", source_path.display()))?;
    check(
        checks,
        &fixture.id,
        "source bytes",
        source.len() as u64 == fixture.source_bytes,
        format!("{} != {}", source.len(), fixture.source_bytes),
    );
    check(
        checks,
        &fixture.id,
        "source sha256",
        sha256(&source) == fixture.source_sha256,
        fixture.source_sha256.clone(),
    );

    let model_path = safe_join(golden_root, &fixture.model)?;
    let model_bytes =
        fs::read(&model_path).map_err(|e| format!("{}: {e}", model_path.display()))?;
    check(
        checks,
        &fixture.id,
        "model file sha256",
        sha256(&model_bytes) == fixture.model_file_sha256,
        fixture.model_file_sha256.clone(),
    );
    let golden = serde_json::from_slice::<Value>(&model_bytes)
        .map_err(|e| format!("{}: {e}", model_path.display()))?;
    let canonical = canonical_hash(&golden)?;
    check(
        checks,
        &fixture.id,
        "model canonical sha256",
        canonical == fixture.model_sha256,
        format!("{canonical} != {}", fixture.model_sha256),
    );
    let schema = golden.get("schemaVersion").and_then(Value::as_u64);
    check(
        checks,
        &fixture.id,
        "golden schema version",
        schema == Some(fixture.document_schema_version as u64),
        format!("{schema:?} != {}", fixture.document_schema_version),
    );
    let counts = model_counts(&golden)?;
    check(
        checks,
        &fixture.id,
        "golden counts",
        counts.pages == fixture.counts.pages
            && counts.shapes == fixture.counts.shapes
            && counts.connectors == fixture.counts.connectors
            && counts.media == fixture.counts.media,
        format!(
            "{}p/{}s/{}c/{}m != {}p/{}s/{}c/{}m",
            counts.pages,
            counts.shapes,
            counts.connectors,
            counts.media,
            fixture.counts.pages,
            fixture.counts.shapes,
            fixture.counts.connectors,
            fixture.counts.media
        ),
    );

    if let Some(models_dir) = &args.candidate_models {
        let candidate_path = models_dir.join(format!("{}.json", fixture.id));
        if !candidate_path.is_file() {
            check(
                checks,
                &fixture.id,
                "candidate model exists",
                false,
                candidate_path.display().to_string(),
            );
        } else {
            match read_json(&candidate_path) {
                Ok(candidate) => {
                    let schema = candidate.get("schemaVersion").and_then(Value::as_u64);
                    check(
                        checks,
                        &fixture.id,
                        "candidate schema version",
                        schema == Some(fixture.document_schema_version as u64),
                        format!("{schema:?} != {}", fixture.document_schema_version),
                    );
                    let mut diffs = Vec::new();
                    let equal = json_equal(&golden, &candidate, args.tolerance, "$", &mut diffs);
                    check(
                        checks,
                        &fixture.id,
                        "candidate model",
                        equal,
                        diffs.join("; "),
                    );
                }
                Err(err) => check(checks, &fixture.id, "candidate model parses", false, err),
            }
        }
    }

    if let Some(patches_dir) = &args.candidate_patches {
        match find_patch(patches_dir, fixture) {
            Some(path) => match entry_hashes(&path) {
                Ok(entries) => {
                    check(
                        checks,
                        &fixture.id,
                        "patch entry count",
                        entries.len() == fixture.untouched.entry_count,
                        format!("{} != {}", entries.len(), fixture.untouched.entry_count),
                    );
                    let mut diffs = Vec::new();
                    for (name, expected) in &fixture.untouched.entries {
                        match entries.get(name) {
                            Some(actual) if actual == expected => {}
                            Some(actual) => diffs.push(format!("{name}: {actual} != {expected}")),
                            None => diffs.push(format!("{name}: missing")),
                        }
                    }
                    for name in entries.keys() {
                        if !fixture.untouched.entries.contains_key(name) {
                            diffs.push(format!("{name}: extra"));
                        }
                    }
                    check(
                        checks,
                        &fixture.id,
                        "patch entry hashes",
                        diffs.is_empty(),
                        diffs.join("; "),
                    );
                }
                Err(err) => check(checks, &fixture.id, "patch zip", false, err),
            },
            None => check(
                checks,
                &fixture.id,
                "candidate patch exists",
                false,
                format!("no patch for {} in {}", fixture.id, patches_dir.display()),
            ),
        }
    }

    Ok(())
}

fn run() -> Result<i32, String> {
    let args = parse_args()?;
    let manifest_text = fs::read_to_string(&args.manifest)
        .map_err(|e| format!("{}: {e}", args.manifest.display()))?;
    let manifest: Manifest = serde_json::from_str(&manifest_text)
        .map_err(|e| format!("{}: {e}", args.manifest.display()))?;
    let golden_root = args
        .manifest
        .parent()
        .ok_or("manifest has no parent directory")?;
    let mut checks = Vec::new();
    check(
        &mut checks,
        "*",
        "manifest version",
        manifest.manifest_version == 1,
        manifest.manifest_version.to_string(),
    );
    check(
        &mut checks,
        "*",
        "document schema version",
        manifest.document_schema_version == 1,
        manifest.document_schema_version.to_string(),
    );
    println!(
        "Manifest: {} ({} fixtures, generated by {})",
        args.manifest.display(),
        manifest.fixtures.len(),
        manifest.generated_by
    );

    for fixture in &manifest.fixtures {
        let before = checks.len();
        if let Err(err) = check_fixture(fixture, &args, golden_root, &mut checks) {
            check(&mut checks, &fixture.id, "fixture", false, err);
        }
        let fixture_checks = &checks[before..];
        let passed = fixture_checks.iter().filter(|c| c.ok).count();
        let total = fixture_checks.len();
        println!(
            "{}  {}  {}/{}",
            if passed == total { "PASS" } else { "FAIL" },
            fixture.source,
            passed,
            total
        );
        if passed != total {
            for check in fixture_checks.iter().filter(|c| !c.ok) {
                println!("      {}: {}", check.name, check.detail);
            }
        }
    }

    if !args.skip_visuals {
        if let Err(err) = check_visuals(&manifest, &args, golden_root, &mut checks) {
            check(&mut checks, "*", "visual goldens", false, err);
        }
    }

    let passed = checks.iter().filter(|c| c.ok).count();
    let total = checks.len();
    if passed == total {
        println!("\n{passed}/{total} checks passed");
        Ok(0)
    } else {
        println!(
            "\n{passed}/{total} checks passed; {} failed",
            total - passed
        );
        for check in checks.iter().filter(|c| !c.ok).take(20) {
            println!("  {} / {}: {}", check.fixture, check.name, check.detail);
        }
        Ok(1)
    }
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_respect_tolerance() {
        let mut diffs = Vec::new();
        assert!(json_equal(&json!(1.0), &json!(1.0), 0.0, "$", &mut diffs));
        assert!(json_equal(
            &json!(1.0),
            &json!(1.0 + 1e-12),
            1e-9,
            "$",
            &mut diffs
        ));
        assert!(!json_equal(&json!(1.0), &json!(1.1), 1e-9, "$", &mut diffs));
    }

    #[test]
    fn object_order_is_irrelevant_and_missing_keys_fail() {
        let mut diffs = Vec::new();
        assert!(json_equal(
            &json!({"b": 1, "a": [true, null]}),
            &json!({"a": [true, null], "b": 1}),
            0.0,
            "$",
            &mut diffs
        ));
        assert!(!json_equal(
            &json!({"a": 1}),
            &json!({"b": 1}),
            0.0,
            "$",
            &mut diffs
        ));
    }

}
