//! Moves one end shape of a glued connector and compares the rerouted connector with the route
//! Visio itself produced for the same relative position. Two sweeps: `route-sweep-sites` (glued
//! to connection points; 24 offsets per page) and `route-sweep-shape` (glued to the whole shape,
//! so Visio picks the sides). Ids come from the .tsv manifests.
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::io::Cursor;
use std::path::PathBuf;
use layoutcraft_vsdx::groupedit::GroupEditor;
use layoutcraft_vsdx::writer::{patch_page_cells, ShapeCellPatch};
use layoutcraft_vsdx::Package;

fn corpus(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus").join(name)
}

struct Config {
    connector: String,
    a: String,
    b: String,
    dx: f64,
    dy: f64,
    ia: u32,
    ib: u32,
    page: usize,
}

fn pin(editor: &GroupEditor, id: &str) -> (f64, f64) {
    let frames = editor.describe();
    let node = frames.as_array().unwrap().iter().find(|f| f["id"] == id).unwrap().clone();
    (node["pin"][0].as_f64().unwrap(), node["pin"][1].as_f64().unwrap())
}

/// Returns (cases tried, cases whose rerouted connector equals Visio's route).
fn run(sweep: &str, pages: &[usize], from_step: usize, to_step: usize) -> (usize, usize) {
    let manifest = std::fs::read_to_string(corpus(&format!("{sweep}.tsv"))).unwrap();
    let visio: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(corpus(&format!("{sweep}.json"))).unwrap()).unwrap();
    let configs: Vec<Config> = manifest
        .lines()
        .map(|line| {
            let f: Vec<&str> = line.trim().split('\t').collect();
            Config {
                connector: f[0].into(),
                a: f[1].into(),
                b: f[2].into(),
                dx: f[3].parse().unwrap(),
                dy: f[4].parse().unwrap(),
                ia: f[5].parse().unwrap(),
                ib: f[6].parse().unwrap(),
                page: f[7].parse::<usize>().unwrap() - 1,
            }
        })
        .collect();
    let package = Package::open(corpus(&format!("{sweep}.vsdx"))).unwrap();
    let (mut tried, mut exact) = (0, 0);
    for &page in pages {
        let on_page: Vec<&Config> = configs.iter().filter(|c| c.page == page).collect();
        for (i, src) in on_page.iter().enumerate().step_by(from_step) {
            for (j, dst) in on_page.iter().enumerate().step_by(to_step) {
                if i == j || (src.ia, src.ib) != (dst.ia, dst.ib) {
                    continue;
                }
                let b_pin = pin(&GroupEditor::open(&package, page).unwrap(), &src.b);
                // Move B so its offset from A equals the destination configuration's.
                let patch = ShapeCellPatch::new(src.b.clone())
                    .set("PinX", format!("{}", b_pin.0 + dst.dx - src.dx))
                    .set("PinY", format!("{}", b_pin.1 + dst.dy - src.dy));
                let moved = Package::from_reader(Cursor::new(patch_page_cells(&package, page, &[patch]).unwrap())).unwrap();
                let part = moved.pages().unwrap()[page].part.clone();
                let connects = moved.connects(&part).unwrap();
                let editor = GroupEditor::open(&moved, page).unwrap();
                let a_centre = pin(&editor, &src.a);
                let patches = editor.reroute_connectors(&connects, &layoutcraft_model::LineJump::default());
                let Some(patch) = patches.iter().find(|p| p.visio_id == src.connector) else {
                    // Nothing to do when the new route equals the stored one.
                    continue;
                };
                let begin = (
                    patch.cached_cells["BeginX"].parse::<f64>().unwrap(),
                    patch.cached_cells["BeginY"].parse::<f64>().unwrap(),
                );
                let ours: Vec<(f64, f64)> = patch.geometry_rows.as_ref().unwrap().1
                    .iter()
                    .map(|r| (r.cells["X"].parse::<f64>().unwrap() + begin.0, r.cells["Y"].parse::<f64>().unwrap() + begin.1))
                    .collect();
                let want = visio
                    .iter()
                    .find(|v| v["ia"] == src.ia && v["ib"] == src.ib && v["dx"] == dst.dx && v["dy"] == dst.dy)
                    .unwrap();
                let want: Vec<(f64, f64)> = want["route"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| (p[0].as_f64().unwrap() + a_centre.0, p[1].as_f64().unwrap() + a_centre.1))
                    .collect();
                tried += 1;
                if ours.len() == want.len()
                    && ours.iter().zip(&want).all(|(p, q)| (p.0 - q.0).abs() < 1e-3 && (p.1 - q.1).abs() < 1e-3)
                {
                    exact += 1;
                }
            }
        }
    }
    (tried, exact)
}

#[test]
fn moving_a_point_glued_shape_reroutes_like_visio() {
    let (tried, exact) = run("route-sweep-sites", &[2, 5, 10], 3, 5);
    println!("point glue: rerouted {tried}, equal to Visio's {exact}");
    assert!(tried >= 60);
    assert!(exact * 100 >= tried * 80, "only {exact}/{tried} reroutes equal Visio's");
}

#[test]
fn moving_a_whole_shape_glued_shape_reroutes_like_visio() {
    let (tried, exact) = run("route-sweep-shape", &[0, 1, 2, 3, 4], 4, 3);
    println!("whole-shape glue: rerouted {tried}, equal to Visio's {exact}");
    assert!(tried >= 100);
    assert!(exact * 100 >= tried * 85, "only {exact}/{tried} reroutes equal Visio's");
}
