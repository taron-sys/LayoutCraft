//! Native LayoutCraft shell: open a VSDX, render a page through the shared
//! SVG/`resvg` path, and move/resize shapes with patch-save and undo.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use eframe::egui;
use layoutcraft_engine::Engine;
use layoutcraft_model::{Document, Shape};
use layoutcraft_render::{first_font_name, font_substitution, installed_font_families, render_region_rgba, render_rgba, scene_font_substitutions, scene_from_document_with_media, FontSubstitution, Scene, SceneItem, SceneShape};
use layoutcraft_vsdx::{
    patch_page_cells, ShapeCellPatch,
};

mod help;
mod mcpinfo;
mod ribbon;

/// The current time as `YYYY-MM-DDTHH:MM:SS` (UTC; no date-time dependency: days since 1970 -> civil date).
fn iso_now() -> String {
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    iso_from_unix(seconds)
}

fn iso_from_unix(seconds: i64) -> String {
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}", rest / 3600, rest % 3600 / 60, rest % 60)
}

#[cfg(test)]
mod time_tests {
    use super::iso_from_unix;

    #[test]
    fn unix_seconds_become_an_iso_time() {
        assert_eq!(iso_from_unix(0), "1970-01-01T00:00:00");
        assert_eq!(iso_from_unix(1_700_000_000), "2023-11-14T22:13:20");
        assert_eq!(iso_from_unix(1_709_164_800 + 3_661), "2024-02-29T01:01:01");
    }
}

#[cfg(test)]
mod resize_tests {
    use super::{resized_rect, Handle};
    use eframe::egui::{pos2, Rect};

    fn start() -> Rect {
        Rect::from_min_max(pos2(100.0, 100.0), pos2(200.0, 150.0))
    }

    #[test]
    fn a_side_handle_moves_one_edge_and_keeps_the_other_three() {
        let r = resized_rect(start(), Handle::E, pos2(260.0, 10.0), 6.0, false);
        assert_eq!((r.min, r.max), (pos2(100.0, 100.0), pos2(260.0, 150.0)));
        let r = resized_rect(start(), Handle::N, pos2(0.0, 80.0), 6.0, false);
        assert_eq!((r.min, r.max), (pos2(100.0, 80.0), pos2(200.0, 150.0)));
        let r = resized_rect(start(), Handle::W, pos2(130.0, 0.0), 6.0, false);
        assert_eq!((r.min, r.max), (pos2(130.0, 100.0), pos2(200.0, 150.0)));
        let r = resized_rect(start(), Handle::S, pos2(0.0, 170.0), 6.0, false);
        assert_eq!((r.min, r.max), (pos2(100.0, 100.0), pos2(200.0, 170.0)));
    }

    #[test]
    fn a_corner_moves_two_edges_and_never_goes_below_the_minimum() {
        let r = resized_rect(start(), Handle::Se, pos2(220.0, 170.0), 6.0, false);
        assert_eq!((r.min, r.max), (pos2(100.0, 100.0), pos2(220.0, 170.0)));
        let r = resized_rect(start(), Handle::Nw, pos2(500.0, 500.0), 6.0, false);
        assert_eq!(r.max, pos2(200.0, 150.0));
        assert!((r.width() - 6.0).abs() < 1e-4 && (r.height() - 6.0).abs() < 1e-4);
    }

    #[test]
    fn shift_on_a_corner_keeps_the_proportions_and_the_larger_change_wins() {
        let r = resized_rect(start(), Handle::Se, pos2(300.0, 160.0), 6.0, true);
        assert!((r.width() / r.height() - 2.0).abs() < 1e-4, "{r:?}");
        assert!((r.width() - 200.0).abs() < 1e-3, "the wider change (x: 2x) wins: {r:?}");
        assert_eq!(r.min, pos2(100.0, 100.0));
        // a side handle ignores Shift (it changes one dimension only)
        let r = resized_rect(start(), Handle::E, pos2(300.0, 160.0), 6.0, true);
        assert_eq!((r.min, r.max), (pos2(100.0, 100.0), pos2(300.0, 150.0)));
    }
}

const DPI: f64 = 96.0;
const MIN_SHAPE_PX: f32 = 6.0;
const HANDLE_SIZE: f32 = 9.0;

/// Best snap on one axis (the web shell's `snapAxis`): `values` are the moving positions, `targets` the positions to snap to, `grid` the grid step (0 = off),
/// `threshold` the reach. Returns (the shift to apply, the position snapped to); the smallest shift wins.
fn snap_axis(values: &[f64], targets: &[f64], grid: f64, threshold: f64) -> Option<(f64, f64)> {
    let mut best: Option<(f64, f64)> = None;
    let mut consider = |target: f64, value: f64| {
        let delta = target - value;
        if delta.abs() <= threshold && best.is_none_or(|(d, _)| delta.abs() < d.abs() - 1e-12) {
            best = Some((delta, target));
        }
    };
    for &value in values {
        if grid > 0.0 {
            consider((value / grid).round() * grid, value);
        }
        for &target in targets {
            consider(target, value);
        }
    }
    best
}

/// A moved box (x0, y0, x1, y1 in page inches, y up) shifted by (dx, dy), snapped (the web's `snappedDelta`): left / middle / right and bottom / middle / top
/// against the grid and the targets. Returns the corrected (dx, dy) and the guide positions snapped to.
#[allow(clippy::type_complexity)]
fn snap_move(b: (f64, f64, f64, f64), dx: f64, dy: f64, grid: f64, xs: &[f64], ys: &[f64], threshold: f64) -> (f64, f64, Option<f64>, Option<f64>) {
    let sx = snap_axis(&[b.0 + dx, (b.0 + b.2) / 2.0 + dx, b.2 + dx], xs, grid, threshold);
    let sy = snap_axis(&[b.1 + dy, (b.1 + b.3) / 2.0 + dy, b.3 + dy], ys, grid, threshold);
    (dx + sx.map_or(0.0, |s| s.0), dy + sy.map_or(0.0, |s| s.0), sx.map(|s| s.1), sy.map(|s| s.1))
}

#[cfg(test)]
mod snap_tests {
    use super::{snap_axis, snap_move};

    #[test]
    fn the_grid_and_the_targets_pull_within_the_threshold_and_the_nearest_wins() {
        // 1.03 is 0.03 from the grid line 1.0 (step 0.25), 0.02 from the target 1.05: the target wins
        assert_eq!(snap_axis(&[1.03], &[1.05], 0.25, 0.05), Some((0.020000000000000018, 1.05)));
        assert_eq!(snap_axis(&[1.03], &[], 0.25, 0.05).map(|s| s.1), Some(1.0));
        assert_eq!(snap_axis(&[1.12], &[], 0.25, 0.05), None, "0.12 from the nearest grid line: out of reach");
        assert_eq!(snap_axis(&[1.0], &[9.0], 0.0, 0.05), None);
        // the three positions of a box: its centre can snap too
        assert_eq!(snap_axis(&[0.0, 1.0, 2.0], &[1.04], 0.0, 0.05).map(|s| s.1), Some(1.04));
    }

    #[test]
    fn a_moved_box_snaps_its_edges_to_other_shapes_on_each_axis() {
        // box (1,1)-(3,2) moved by (0.98, 0.02): its left edge 1.98 snaps to the target x 2.0, its top 2.02 to the target y 2.0
        let (dx, dy, gx, gy) = snap_move((1.0, 1.0, 3.0, 2.0), 0.98, 0.02, 0.0, &[2.0], &[2.0], 0.05);
        assert!((dx - 1.0).abs() < 1e-9 && (dy - 0.0).abs() < 1e-9, "{dx} {dy}");
        assert_eq!((gx, gy), (Some(2.0), Some(2.0)));
        // nothing within reach: unchanged
        let (dx, dy, gx, gy) = snap_move((1.0, 1.0, 3.0, 2.0), 0.5, 0.5, 0.0, &[9.0], &[9.0], 0.05);
        assert_eq!((dx, dy, gx, gy), (0.5, 0.5, None, None));
    }
}

/// The answer of `Engine::preview_move` (`[{"id", "route": [[x, y] ..]}]`, page inches, y up) as (connector id, route in scene px).
fn routes_from_preview(json: &str, page_height_in: f64) -> Vec<(String, Vec<egui::Pos2>)> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    value
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|entry| {
                    let id = entry.get("id")?.as_str()?.to_string();
                    let points: Vec<egui::Pos2> = entry
                        .get("route")?
                        .as_array()?
                        .iter()
                        .filter_map(|p| Some(egui::pos2((p.get(0)?.as_f64()? * DPI) as f32, ((page_height_in - p.get(1)?.as_f64()?) * DPI) as f32)))
                        .collect();
                    (points.len() >= 2).then_some((id, points))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod preview_tests {
    use super::routes_from_preview;
    use eframe::egui::pos2;

    #[test]
    fn a_preview_route_is_flipped_to_scene_pixels() {
        let routes = routes_from_preview(r#"[{"id":"7","route":[[1.0,2.0],[3.0,2.0]]},{"id":"8","route":[[0.0,0.0]]}]"#, 8.5);
        assert_eq!(routes.len(), 1, "a route needs two points");
        assert_eq!(routes[0].0, "7");
        assert_eq!(routes[0].1, vec![pos2(96.0, (8.5 - 2.0) as f32 * 96.0), pos2(288.0, (8.5 - 2.0) as f32 * 96.0)]);
        assert!(routes_from_preview("nonsense", 8.5).is_empty());
    }
}

/// The ids whose box lies COMPLETELY inside the rubber band (the web shell's `enclosed`): `candidates` are (id, box) of the page-level shapes in drawing order.
fn enclosed_ids(candidates: &[(String, egui::Rect)], band: egui::Rect) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for (id, rect) in candidates {
        if band.contains_rect(*rect) && !found.contains(id) {
            found.push(id.clone());
        }
    }
    found
}

#[cfg(test)]
mod band_tests {
    use super::enclosed_ids;
    use eframe::egui::{pos2, Rect};

    fn r(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
    }

    #[test]
    fn the_band_selects_only_what_it_fully_encloses() {
        let shapes = vec![("1".to_string(), r(10.0, 10.0, 50.0, 40.0)), ("2".to_string(), r(60.0, 10.0, 120.0, 40.0)), ("3".to_string(), r(200.0, 10.0, 240.0, 40.0)), ("1".to_string(), r(10.0, 10.0, 50.0, 40.0))];
        // 2 sticks out of the band on the right, 3 is outside: only 1 (listed twice, counted once)
        assert_eq!(enclosed_ids(&shapes, r(0.0, 0.0, 100.0, 100.0)), vec!["1".to_string()]);
        assert_eq!(enclosed_ids(&shapes, r(0.0, 0.0, 130.0, 100.0)), vec!["1".to_string(), "2".to_string()]);
        assert!(enclosed_ids(&shapes, r(300.0, 300.0, 400.0, 400.0)).is_empty());
        // a band that merely touches (intersects) a shape does not take it
        assert!(enclosed_ids(&shapes, r(20.0, 20.0, 40.0, 30.0)).is_empty());
    }
}

/// The page offset (inches, y up) of an arrow key press: 0.05 in, with Shift 0.5 in (`shell.js`: `const step = event.shiftKey ? 0.5 : 0.05`).
fn nudge_delta(key: egui::Key, shift: bool) -> Option<(f64, f64)> {
    let step = if shift { 0.5 } else { 0.05 };
    match key {
        egui::Key::ArrowLeft => Some((-step, 0.0)),
        egui::Key::ArrowRight => Some((step, 0.0)),
        egui::Key::ArrowUp => Some((0.0, step)),
        egui::Key::ArrowDown => Some((0.0, -step)),
        _ => None,
    }
}

#[cfg(test)]
mod nudge_tests {
    use super::nudge_delta;
    use eframe::egui::Key;

    #[test]
    fn arrow_keys_nudge_like_the_web_shell() {
        assert_eq!(nudge_delta(Key::ArrowLeft, false), Some((-0.05, 0.0)));
        assert_eq!(nudge_delta(Key::ArrowUp, false), Some((0.0, 0.05)));
        assert_eq!(nudge_delta(Key::ArrowDown, true), Some((0.0, -0.5)));
        assert_eq!(nudge_delta(Key::ArrowRight, true), Some((0.5, 0.0)));
        assert_eq!(nudge_delta(Key::A, false), None);
    }
}

/// The keys of the arrange commands. The first key listed for a command is the one Visio documents (`docs/43-ui-parity.md` 4.1);
/// `[`, `]`, `H` and `V` are the earlier LayoutCraft keys, kept as aliases (4.2). egui ignores an extra Shift when it matches a
/// shortcut, so a Shift binding comes before a plain one on the same key.
const ARRANGE_KEYS: &[(egui::Modifiers, egui::Key, ArrangeCmd)] = &[
    (CTRL_SHIFT, egui::Key::U, ArrangeCmd::Ungroup),
    (egui::Modifiers::CTRL, egui::Key::G, ArrangeCmd::Group),
    (CTRL_SHIFT, egui::Key::F, ArrangeCmd::Order(2)),
    (CTRL_SHIFT, egui::Key::B, ArrangeCmd::Order(3)),
    (egui::Modifiers::CTRL, egui::Key::L, ArrangeCmd::RotateLeft),
    (egui::Modifiers::CTRL, egui::Key::R, ArrangeCmd::RotateQuarter),
    (egui::Modifiers::CTRL, egui::Key::H, ArrangeCmd::Flip(false)),
    (egui::Modifiers::CTRL, egui::Key::J, ArrangeCmd::Flip(true)),
    (egui::Modifiers::NONE, egui::Key::OpenBracket, ArrangeCmd::RotateLeft),
    (egui::Modifiers::NONE, egui::Key::CloseBracket, ArrangeCmd::RotateQuarter),
    (egui::Modifiers::NONE, egui::Key::H, ArrangeCmd::Flip(false)),
    (egui::Modifiers::NONE, egui::Key::V, ArrangeCmd::Flip(true)),
];

const CTRL_SHIFT: egui::Modifiers = egui::Modifiers { alt: false, ctrl: true, shift: true, mac_cmd: false, command: false };

/// Takes the first arrange shortcut pressed in this frame out of the input.
fn consume_arrange_key(input: &mut egui::InputState) -> Option<ArrangeCmd> {
    ARRANGE_KEYS.iter().find(|(modifiers, key, _)| input.consume_key(*modifiers, *key)).map(|(_, _, command)| *command)
}

#[cfg(test)]
mod arrange_key_tests {
    use super::{consume_arrange_key, ArrangeCmd};
    use eframe::egui::{self, Key, Modifiers};

    fn press(key: Key, ctrl: bool, shift: bool) -> Option<ArrangeCmd> {
        // what egui reports on Windows and Linux: Ctrl also sets `command`
        let modifiers = Modifiers { alt: false, ctrl, shift, mac_cmd: false, command: ctrl };
        let event = egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        let mut input = egui::InputState::default();
        input.events.push(event);
        consume_arrange_key(&mut input)
    }

    #[test]
    fn group_and_ungroup_follow_the_documented_keys() {
        assert!(press(Key::G, true, false) == Some(ArrangeCmd::Group));
        assert!(press(Key::G, true, true) == Some(ArrangeCmd::Group));
        assert!(press(Key::U, true, true) == Some(ArrangeCmd::Ungroup));
        assert!(press(Key::U, true, false).is_none());
    }

    #[test]
    fn turn_and_flip_take_the_ctrl_keys_and_the_old_aliases() {
        for (key, alias, command) in [
            (Key::L, Key::OpenBracket, ArrangeCmd::RotateLeft),
            (Key::R, Key::CloseBracket, ArrangeCmd::RotateQuarter),
            (Key::H, Key::H, ArrangeCmd::Flip(false)),
            (Key::J, Key::V, ArrangeCmd::Flip(true)),
        ] {
            assert!(press(key, true, false) == Some(command));
            assert!(press(alias, false, false) == Some(command));
        }
        // Ctrl+V is paste, never a flip
        assert!(press(Key::V, true, false).is_none());
    }
}

/// What a tool or view shortcut asks for.
#[derive(Clone, Copy, PartialEq)]
enum ViewKey {
    Tool(Tool),
    FitPage,
    NextPage,
    PreviousPage,
    Find,
}

/// Tool and view shortcuts for features the app has, with Visio's documented keys (`docs/43-ui-parity.md` 4.1). The draw
/// numbers are the positions in the ribbon's draw tools: 0 rectangle, 2 ellipse, 6 line.
const VIEW_KEYS: &[(egui::Modifiers, egui::Key, ViewKey)] = &[
    (egui::Modifiers::CTRL, egui::Key::Num1, ViewKey::Tool(Tool::Select)),
    (egui::Modifiers::CTRL, egui::Key::Num3, ViewKey::Tool(Tool::Connector)),
    (egui::Modifiers::CTRL, egui::Key::Num6, ViewKey::Tool(Tool::Draw(6))),
    (egui::Modifiers::CTRL, egui::Key::Num8, ViewKey::Tool(Tool::Draw(0))),
    (egui::Modifiers::CTRL, egui::Key::Num9, ViewKey::Tool(Tool::Draw(2))),
    (CTRL_SHIFT, egui::Key::W, ViewKey::FitPage),
    (egui::Modifiers::CTRL, egui::Key::PageDown, ViewKey::NextPage),
    (egui::Modifiers::CTRL, egui::Key::PageUp, ViewKey::PreviousPage),
    (egui::Modifiers::CTRL, egui::Key::F, ViewKey::Find),
];

/// Takes the first tool or view shortcut pressed in this frame out of the input.
fn consume_view_key(input: &mut egui::InputState) -> Option<ViewKey> {
    VIEW_KEYS.iter().find(|(modifiers, key, _)| input.consume_key(*modifiers, *key)).map(|(_, _, command)| *command)
}

#[cfg(test)]
mod view_key_tests {
    use super::{consume_view_key, Tool, ViewKey};
    use eframe::egui::{self, Key, Modifiers};

    fn press(key: Key, ctrl: bool, shift: bool) -> Option<ViewKey> {
        let modifiers = Modifiers { alt: false, ctrl, shift, mac_cmd: false, command: ctrl };
        let mut input = egui::InputState::default();
        input.events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
        consume_view_key(&mut input)
    }

    #[test]
    fn ctrl_digits_pick_the_tools() {
        assert!(press(Key::Num1, true, false) == Some(ViewKey::Tool(Tool::Select)));
        assert!(press(Key::Num3, true, false) == Some(ViewKey::Tool(Tool::Connector)));
        assert!(press(Key::Num8, true, false) == Some(ViewKey::Tool(Tool::Draw(0))));
        // a digit typed without Ctrl is not a tool change
        assert!(press(Key::Num1, false, false).is_none());
    }

    #[test]
    fn fit_and_page_keys() {
        assert!(press(Key::W, true, true) == Some(ViewKey::FitPage));
        assert!(press(Key::W, true, false).is_none());
        assert!(press(Key::PageDown, true, false) == Some(ViewKey::NextPage));
        assert!(press(Key::PageUp, true, false) == Some(ViewKey::PreviousPage));
        // Ctrl+Shift+F is "bring to front" and is taken by the arrange keys first
        assert!(press(Key::F, true, false) == Some(ViewKey::Find));
    }
}

/// The resizes of "same width / height": every other page-level shape of the selection takes the width and / or height of the
/// FIRST one (the rule of `web/shell.js` `sameSize`). `frames` is `Engine::group_info_json`; connectors, group members and
/// shapes without an editable frame are left out. Returns (id, width, height) in inches.
fn same_size_plan(frames: &str, ids: &[String], is_connector: impl Fn(&str) -> bool, width: bool, height: bool) -> Result<Vec<(String, f64, f64)>, String> {
    let frames: Vec<serde_json::Value> = serde_json::from_str(frames).map_err(|error| error.to_string())?;
    let sizes: Vec<(&String, f64, f64)> = ids
        .iter()
        .filter(|id| !is_connector(id))
        .filter_map(|id| {
            let frame = frames.iter().find(|frame| frame["id"].as_str() == Some(id.as_str()))?;
            if !frame["parentId"].is_null() || frame["editable"].as_bool() != Some(true) {
                return None;
            }
            Some((id, frame["size"][0].as_f64()?, frame["size"][1].as_f64()?))
        })
        .collect();
    let [(_, first_w, first_h), rest @ ..] = sizes.as_slice() else {
        return Err("select two or more shapes of the page (shift-click)".to_string());
    };
    if rest.is_empty() {
        return Err("select two or more shapes of the page (shift-click)".to_string());
    }
    Ok(rest.iter().map(|(id, w, h)| ((*id).clone(), if width { *first_w } else { *w }, if height { *first_h } else { *h })).collect())
}

#[cfg(test)]
mod same_size_tests {
    use super::same_size_plan;

    const FRAMES: &str = r#"[
        {"id":"1","parentId":null,"editable":true,"size":[2.0,1.0]},
        {"id":"2","parentId":null,"editable":true,"size":[3.0,4.0]},
        {"id":"3","parentId":"2","editable":true,"size":[5.0,5.0]},
        {"id":"4","parentId":null,"editable":true,"size":[6.0,7.0]}
    ]"#;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn the_others_take_the_size_of_the_first_selected() {
        let plan = same_size_plan(FRAMES, &ids(&["2", "1", "4"]), |_| false, true, false).unwrap();
        assert_eq!(plan, vec![("1".to_string(), 3.0, 1.0), ("4".to_string(), 3.0, 7.0)]);
        let plan = same_size_plan(FRAMES, &ids(&["1", "2"]), |_| false, true, true).unwrap();
        assert_eq!(plan, vec![("2".to_string(), 2.0, 1.0)]);
    }

    #[test]
    fn connectors_and_group_members_are_left_out() {
        let plan = same_size_plan(FRAMES, &ids(&["4", "1", "3", "2"]), |id| id == "4", false, true).unwrap();
        assert_eq!(plan, vec![("2".to_string(), 3.0, 1.0)]);
        assert!(same_size_plan(FRAMES, &ids(&["1", "3"]), |_| false, true, true).is_err());
    }
}

/// The status verb of a shape operation (`Engine::group_op` 6..12) and how many selected shapes it needs.
fn shape_op_info(op: i32) -> (&'static str, usize) {
    match op {
        6 => ("Joined into one outline", 2),
        7 => ("Combined", 2),
        8 => ("Cut into pieces", 2),
        9 => ("Intersected", 2),
        10 => ("Subtracted", 2),
        11 => ("Joined the lines", 1),
        _ => ("Trimmed", 2),
    }
}

#[cfg(test)]
mod shape_op_tests {
    use crate::ribbon::{self, Native};
    use layoutcraft_engine::Engine;

    /// The number of shapes the ribbon command `id` makes from two overlapping rectangles.
    fn made_by(id: &str) -> usize {
        let mut engine = Engine::default();
        engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
        let first = engine.draw(0, 0, 1.0, 1.0, 3.0, 3.0).unwrap();
        let second = engine.draw(0, 0, 2.0, 2.0, 4.0, 4.0).unwrap();
        let Some(Native::ShapeOp(op)) = ribbon::native(id) else { panic!("{id} is not a shape operation") };
        let made = engine.group_op(0, op, &format!("{first}
{second}")).unwrap();
        serde_json::from_str::<Vec<String>>(&made).unwrap().len()
    }

    #[test]
    fn each_ribbon_command_runs_its_own_operation() {
        assert_eq!(made_by("opUnion"), 1);
        assert_eq!(made_by("opIntersect"), 1);
        assert_eq!(made_by("opSubtract"), 1);
        assert_eq!(made_by("opCombine"), 1);
        // the two rectangles cut each other into the overlap and the two remainders
        assert!(made_by("opFragment") >= 3);
    }
}

/// (owner id, length in characters) of the text each of the shapes shows; shapes without text are left out.
fn text_targets(engine: &Engine, page: usize, ids: &[String]) -> Vec<(String, usize)> {
    ids.iter()
        .filter_map(|id| {
            let info: serde_json::Value = serde_json::from_str(&engine.text_info_json(page, id).ok()?).ok()?;
            let length = info["text"].as_str()?.chars().count();
            let owner = info["owner"].as_str()?.to_string();
            (length > 0).then_some((owner, length))
        })
        .collect()
}

/// Format the whole text of the shapes (`Engine::edit` op 13 with `spec`, a JSON object of `cells` and / or `para`) as one
/// undo step, all or nothing. Returns the number of texts changed.
fn format_whole_text(engine: &mut Engine, page: usize, ids: &[String], spec: &serde_json::Value) -> Result<usize, String> {
    let targets = text_targets(engine, page, ids);
    if targets.is_empty() {
        return Err("the selection has no text".to_string());
    }
    let depth = engine.undo_depth();
    let result = targets.iter().try_for_each(|(owner, length)| {
        let mut job = spec.clone();
        job["start"] = serde_json::json!(0);
        job["end"] = serde_json::json!(length);
        engine.edit(page, 13, &format!("{owner}\n{job}"), 0.0, 0.0, 0.0, 0.0, 0)
    });
    engine.collapse_history(depth);
    if result.is_err() && engine.undo_depth() > depth {
        let _ = engine.undo();
    }
    result.map(|()| targets.len())
}

/// Whether the whole text of every one of the shapes that has text is struck through.
fn all_struck(engine: &Engine, page: usize, ids: &[String]) -> bool {
    let targets = text_targets(engine, page, ids);
    !targets.is_empty()
        && targets.iter().all(|(owner, length)| {
            engine
                .text_state_json(page, owner, 0, *length)
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .is_some_and(|state| state["char"]["Strikethru"].as_str() == Some("1"))
        })
}

#[cfg(test)]
mod text_format_tests {
    use super::{all_struck, format_whole_text};
    use layoutcraft_engine::Engine;

    fn two_boxes() -> (Engine, Vec<String>) {
        let mut engine = Engine::default();
        engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
        let ids: Vec<String> = [1.0, 5.0].iter().map(|x| engine.draw(0, 0, *x, 1.0, x + 2.0, 3.0).unwrap()).collect();
        (engine, ids)
    }

    #[test]
    fn strikethrough_covers_every_text_in_one_undo_step() {
        let (mut engine, ids) = two_boxes();
        for id in &ids {
            engine.edit(0, 8, &format!("{id}\nlabel"), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
        }
        assert!(!all_struck(&engine, 0, &ids));
        let depth = engine.undo_depth();
        assert_eq!(format_whole_text(&mut engine, 0, &ids, &serde_json::json!({ "cells": { "Strikethru": "1" } })), Ok(2));
        assert!(all_struck(&engine, 0, &ids));
        assert_eq!(engine.undo_depth(), depth + 1);
        let _ = engine.undo();
        assert!(!all_struck(&engine, 0, &ids[..1]) && !all_struck(&engine, 0, &ids[1..]));
    }

    #[test]
    fn a_selection_without_text_is_reported_and_changes_nothing() {
        let (mut engine, ids) = two_boxes();
        let depth = engine.undo_depth();
        assert!(format_whole_text(&mut engine, 0, &ids, &serde_json::json!({ "para": { "HorzAlign": "2" } })).is_err());
        assert_eq!(engine.undo_depth(), depth);
    }
}

/// The left indents the Paragraph group offers (inches) and the line spacings (multiples of the line height).
const INDENTS_IN: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
const LINE_SPACINGS: [f64; 4] = [1.0, 1.15, 1.5, 2.0];

/// `Engine::edit` op 13 specs: `IndLeft` is in inches, `SpLine` is a negative multiple of the line height (as `web/shell.js`).
fn indent_spec(inches: f64) -> serde_json::Value {
    serde_json::json!({ "para": { "IndLeft": inches.to_string() } })
}

fn line_spacing_spec(multiple: f64) -> serde_json::Value {
    serde_json::json!({ "para": { "SpLine": (-multiple).to_string() } })
}

#[cfg(test)]
mod paragraph_tests {
    use super::{format_whole_text, indent_spec, line_spacing_spec};
    use layoutcraft_engine::Engine;

    #[test]
    fn indent_and_spacing_reach_the_paragraph() {
        let mut engine = Engine::default();
        engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
        let id = engine.draw(0, 0, 1.0, 1.0, 3.0, 3.0).unwrap();
        engine.edit(0, 8, &format!("{id}\ntwo words"), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
        let ids = [id.clone()];
        format_whole_text(&mut engine, 0, &ids, &indent_spec(0.5)).unwrap();
        format_whole_text(&mut engine, 0, &ids, &line_spacing_spec(1.5)).unwrap();
        let state: serde_json::Value = serde_json::from_str(&engine.text_state_json(0, &id, 0, 9).unwrap()).unwrap();
        let cell = |name: &str| state["para"][name].as_str().and_then(|value| value.parse::<f64>().ok()).unwrap();
        // a left indent to the right of the margin, and a relative (negative) spacing wider than single
        assert!(cell("IndLeft") > 0.0);
        assert!(cell("SpLine") < -1.0);
    }
}

/// The state of the Find window.
#[derive(Default)]
struct Find {
    text: String,
    replace_with: String,
    match_case: bool,
    /// (page, position in the page's shape list) of the last match shown.
    last: Option<(usize, usize)>,
    status: String,
    /// Put the keyboard focus in the text field on the next frame.
    focus: bool,
}

/// Search the texts of a page's shapes (one entry per shape, in page order). Returns how many shapes match and the first
/// match after position `after`, starting again at the top past the end.
fn find_match(texts: &[String], needle: &str, match_case: bool, after: Option<usize>) -> (usize, Option<usize>) {
    if needle.is_empty() {
        return (0, None);
    }
    let fold = |text: &str| if match_case { text.to_string() } else { text.to_lowercase() };
    let needle = fold(needle);
    let matches: Vec<usize> = texts.iter().enumerate().filter(|(_, text)| fold(text).contains(&needle)).map(|(at, _)| at).collect();
    let next = matches.iter().copied().find(|at| after.is_none_or(|last| *at > last)).or_else(|| matches.first().copied());
    (matches.len(), next)
}

#[cfg(test)]
mod find_tests {
    use super::find_match;

    fn texts() -> Vec<String> {
        ["Router", "switch", "", "Core ROUTER"].iter().map(|text| text.to_string()).collect()
    }

    #[test]
    fn find_next_walks_the_matches_and_starts_again() {
        let (count, first) = find_match(&texts(), "router", false, None);
        assert_eq!((count, first), (2, Some(0)));
        assert_eq!(find_match(&texts(), "router", false, first).1, Some(3));
        assert_eq!(find_match(&texts(), "router", false, Some(3)).1, Some(0));
    }

    #[test]
    fn match_case_and_empty_or_missing_text() {
        assert_eq!(find_match(&texts(), "ROUTER", true, None), (1, Some(3)));
        assert_eq!(find_match(&texts(), "", false, None), (0, None));
        assert_eq!(find_match(&texts(), "firewall", false, None), (0, None));
    }
}

/// `text` with every occurrence of `needle` replaced by `with`; `None` when there is none. Without `match_case` two
/// characters are equal when their lower-case forms are.
fn replace_matches(text: &str, needle: &str, with: &str, match_case: bool) -> Option<String> {
    let needle: Vec<char> = needle.chars().collect();
    if needle.is_empty() {
        return None;
    }
    let same = |a: char, b: char| a == b || (!match_case && a.to_lowercase().eq(b.to_lowercase()));
    let chars: Vec<char> = text.chars().collect();
    let (mut out, mut at, mut found) = (String::new(), 0, false);
    while at < chars.len() {
        if chars.len() - at >= needle.len() && needle.iter().zip(&chars[at..]).all(|(a, b)| same(*a, *b)) {
            out.push_str(with);
            at += needle.len();
            found = true;
        } else {
            out.push(chars[at]);
            at += 1;
        }
    }
    found.then_some(out)
}

/// Replace in the texts the shapes show (`Engine::edit` op 8 on each text's owner, once) as one undo step, all or nothing.
/// Returns the number of texts changed.
fn replace_in_shapes(engine: &mut Engine, page: usize, ids: &[String], needle: &str, with: &str, match_case: bool) -> Result<usize, String> {
    let mut jobs: Vec<(String, String)> = Vec::new();
    for id in ids {
        let Some(info) = engine.text_info_json(page, id).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()) else {
            continue;
        };
        let (Some(owner), Some(text)) = (info["owner"].as_str(), info["text"].as_str()) else {
            continue;
        };
        if jobs.iter().any(|(known, _)| known == owner) {
            continue;
        }
        if let Some(new) = replace_matches(text, needle, with, match_case) {
            jobs.push((owner.to_string(), new));
        }
    }
    let depth = engine.undo_depth();
    let result = jobs.iter().try_for_each(|(owner, text)| engine.edit(page, 8, &format!("{owner}\n{text}"), 0.0, 0.0, 0.0, 0.0, 0));
    engine.collapse_history(depth);
    if result.is_err() && engine.undo_depth() > depth {
        let _ = engine.undo();
    }
    result.map(|()| jobs.len())
}

#[cfg(test)]
mod replace_tests {
    use super::{replace_in_shapes, replace_matches};
    use layoutcraft_engine::Engine;

    #[test]
    fn every_occurrence_is_replaced_with_or_without_case() {
        assert_eq!(replace_matches("Port a, PORT b", "port", "Link", false).as_deref(), Some("Link a, Link b"));
        assert_eq!(replace_matches("Port a, PORT b", "PORT", "Link", true).as_deref(), Some("Port a, Link b"));
        // the replacement is not searched again
        assert_eq!(replace_matches("aa", "a", "aa", true).as_deref(), Some("aaaa"));
        assert_eq!(replace_matches("abc", "x", "y", false), None);
        assert_eq!(replace_matches("abc", "", "y", false), None);
    }

    #[test]
    fn replacing_in_several_shapes_is_one_undo_step() {
        let mut engine = Engine::default();
        engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
        let ids: Vec<String> = [1.0, 5.0].iter().map(|x| engine.draw(0, 0, *x, 1.0, x + 2.0, 3.0).unwrap()).collect();
        for id in &ids {
            engine.edit(0, 8, &format!("{id}\nold name"), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
        }
        let text = |engine: &Engine, id: &str| engine.text_info_json(0, id).unwrap();
        let depth = engine.undo_depth();
        assert_eq!(replace_in_shapes(&mut engine, 0, &ids, "old", "new", false), Ok(2));
        assert!(ids.iter().all(|id| text(&engine, id).contains("new name")));
        assert_eq!(engine.undo_depth(), depth + 1);
        let _ = engine.undo();
        assert!(ids.iter().all(|id| text(&engine, id).contains("old name")));
        // nothing to replace: no undo step
        assert_eq!(replace_in_shapes(&mut engine, 0, &ids, "absent", "new", false), Ok(0));
        assert_eq!(engine.undo_depth(), depth);
    }
}

/// The `Engine::edit` op 10 style that switches the shadow on or off.
fn shadow_style(on: bool) -> String {
    serde_json::json!({ "shadow": on }).to_string()
}

#[cfg(test)]
mod shadow_tests {
    use super::shadow_style;
    use layoutcraft_engine::Engine;

    #[test]
    fn the_shadow_goes_on_and_off() {
        let mut engine = Engine::default();
        engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
        let id = engine.draw(0, 0, 1.0, 1.0, 3.0, 3.0).unwrap();
        let shadowed = |engine: &Engine| {
            let page = &engine.document().unwrap().pages[0];
            page.shapes.iter().any(|shape| shape.visio_id.as_deref() == Some(id.as_str()) && shape.shadow.is_some())
        };
        assert!(!shadowed(&engine));
        for on in [true, false] {
            engine.edit(0, 10, &format!("{id}\n{}", shadow_style(on)), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
            assert_eq!(shadowed(&engine), on);
        }
    }
}

/// The (name, address) pairs of `Engine::hyperlinks_json`.
fn parse_links(json: &str) -> Vec<(String, String)> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    value
        .as_array()
        .map(|list| {
            list.iter()
                .map(|l| {
                    let text = |key: &str| l.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let address = if text("address").is_empty() { text("subAddress") } else { text("address") };
                    (text("name"), address)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod link_tests {
    use super::parse_links;

    #[test]
    fn links_show_their_address_or_sub_address() {
        let links = parse_links(r#"[{"name":"Site","address":"https://a.b"},{"name":"Page","address":"","subAddress":"Page-2"}]"#);
        assert_eq!(links, vec![("Site".to_string(), "https://a.b".to_string()), ("Page".to_string(), "Page-2".to_string())]);
        assert!(parse_links("nonsense").is_empty());
    }
}

/// The window picture of `App::detail`: the texture, the scene-px window it shows and its device pixels per scene pixel.
struct Detail {
    texture: egui::TextureHandle,
    region: [f64; 4],
    scale: f32,
}

/// The part of the page (x, y, w, h in scene px) that `viewport` shows when the page's top-left corner is at `image_min` and a scene pixel is
/// `view` points wide, grown by `pad` of its size on every side, kept on the page and at most 8192 device pixels (`device_scale` per scene px) wide.
fn detail_window(image_min: egui::Pos2, viewport: egui::Rect, view: f32, page: (f64, f64), device_scale: f32, pad: f64) -> Option<[f64; 4]> {
    if !(view > 0.0) || !(device_scale > 0.0) {
        return None;
    }
    let x0 = f64::from((viewport.min.x - image_min.x) / view);
    let x1 = f64::from((viewport.max.x - image_min.x) / view);
    let y0 = f64::from((viewport.min.y - image_min.y) / view);
    let y1 = f64::from((viewport.max.y - image_min.y) / view);
    let limit = 8192.0 / f64::from(device_scale);
    let span = |a: f64, b: f64, total: f64| {
        let (a, b) = (a.max(0.0), b.min(total));
        if b <= a {
            return None;
        }
        let grow = (b - a) * pad;
        let (mut a, mut b) = ((a - grow).max(0.0), (b + grow).min(total));
        if b - a > limit {
            let mid = (a + b) / 2.0;
            a = (mid - limit / 2.0).max(0.0);
            b = (a + limit).min(total);
        }
        Some((a, b))
    };
    let (ax, bx) = span(x0, x1, page.0)?;
    let (ay, by) = span(y0, y1, page.1)?;
    Some([ax, ay, bx - ax, by - ay])
}

#[cfg(test)]
mod detail_window_tests {
    use super::*;

    #[test]
    fn the_window_covers_what_is_visible_and_stays_on_the_page() {
        let viewport = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1000.0, 800.0));
        // a 5000 x 4000 scene px page at 8 points per scene px, its corner far off to the left: the viewport shows scene x 250..375
        let image_min = egui::pos2(-2000.0, -1000.0);
        let visible = detail_window(image_min, viewport, 8.0, (5000.0, 4000.0), 8.0, 0.0).unwrap();
        assert!((visible[0] - 250.0).abs() < 1e-6 && (visible[2] - 125.0).abs() < 1e-6, "{visible:?}");
        let padded = detail_window(image_min, viewport, 8.0, (5000.0, 4000.0), 8.0, 0.25).unwrap();
        assert!(padded[0] < visible[0] && padded[0] + padded[2] > visible[0] + visible[2], "padded {padded:?} contains {visible:?}");
        // a page smaller than the view: the window ends at the page edge
        let small = detail_window(egui::pos2(100.0, 100.0), viewport, 8.0, (50.0, 40.0), 8.0, 0.25).unwrap();
        assert!(small[0] >= 0.0 && small[0] + small[2] <= 50.0 && small[1] + small[3] <= 40.0, "{small:?}");
        // a page entirely off screen has no window
        assert!(detail_window(egui::pos2(2000.0, 0.0), viewport, 8.0, (50.0, 40.0), 8.0, 0.25).is_none());
    }

    #[test]
    fn a_window_never_asks_for_more_than_8192_device_pixels() {
        let viewport = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(6000.0, 6000.0));
        let window = detail_window(egui::pos2(0.0, 0.0), viewport, 4.0, (4000.0, 4000.0), 4.0, 0.25).unwrap();
        assert!(window[2] * 4.0 <= 8192.0 + 1e-6 && window[3] * 4.0 <= 8192.0 + 1e-6, "{window:?}");
    }
}

/// A page to draw on the raster thread (the picture of the page is the slow part of an edit: 80-200 ms on a real drawing).
struct RasterJob {
    ticket: u64,
    scene: Scene,
    scale: f64,
    keep_view: bool,
    /// Draw only this window (x, y, width, height in scene px from the page's top-left corner) instead of the whole page.
    region: Option<[f64; 4]>,
    ctx: egui::Context,
}

/// What the raster thread sends back: the decoded RGBA picture (or why it failed).
struct RasterDone {
    ticket: u64,
    scale: f64,
    keep_view: bool,
    region: Option<[f64; 4]>,
    result: Result<([usize; 2], Vec<u8>), String>,
}

/// The raster thread: takes the newest job (older queued ones are skipped), draws it, sends the picture and wakes the UI.
fn spawn_raster_thread() -> (std::sync::mpsc::Sender<RasterJob>, std::sync::mpsc::Receiver<RasterDone>) {
    let (job_tx, job_rx) = std::sync::mpsc::channel::<RasterJob>();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<RasterDone>();
    std::thread::Builder::new()
        .name("layoutcraft-raster".into())
        .spawn(move || {
            while let Ok(mut job) = job_rx.recv() {
                while let Ok(newer) = job_rx.try_recv() {
                    job = newer;
                }
                let timer = Timer::new("  raster thread (render)", 0.0);
                let result = match job.region {
                    Some(region) => render_region_rgba(&job.scene, job.scale, region).map(|(w, h, rgba)| ([w as usize, h as usize], rgba)),
                    None => render_rgba(&job.scene, job.scale).map(|(w, h, rgba)| ([w as usize, h as usize], rgba)),
                };
                drop(timer);
                let _ = done_tx.send(RasterDone { ticket: job.ticket, scale: job.scale, keep_view: job.keep_view, region: job.region, result });
                job.ctx.request_repaint();
            }
        })
        .expect("raster thread");
    (job_tx, done_rx)
}

/// `LAYOUTCRAFT_PROFILE=1`: print how long the slow phases take (page rebuilds, frames above 16 ms) to stderr.
fn profiling() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("LAYOUTCRAFT_PROFILE").is_some())
}

struct Timer {
    label: &'static str,
    start: std::time::Instant,
    min_ms: f64,
}

impl Timer {
    fn new(label: &'static str, min_ms: f64) -> Self {
        Self { label, start: std::time::Instant::now(), min_ms }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        let ms = self.start.elapsed().as_secs_f64() * 1000.0;
        if profiling() && ms >= self.min_ms {
            eprintln!("[profile] {} {:.1} ms", self.label, ms);
        }
    }
}

/// The window / taskbar icon (the same picture as the exe's icon resource, `rust/resources/layoutcraft-256.png`).
fn window_icon() -> Option<egui::IconData> {
    let image = image::load_from_memory(include_bytes!("../../resources/layoutcraft-256.png")).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData { rgba: image.into_raw(), width, height })
}

fn main() -> eframe::Result {
    let initial = std::env::args().nth(1);
    let options = eframe::NativeOptions {
        viewport: {
            let viewport = egui::ViewportBuilder::default().with_inner_size([1280.0, 860.0]).with_min_inner_size([720.0, 480.0]).with_title("LayoutCraft");
            match window_icon() {
                Some(icon) => viewport.with_icon(icon),
                None => viewport,
            }
        },
        ..Default::default()
    };
    eframe::run_native(
        "LayoutCraft",
        options,
        Box::new(move |cc| Ok(Box::new(LayoutCraftApp::new(cc, initial)))),
    )
}

#[cfg(windows)]
mod file_dialog {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_OVERWRITEPROMPT,
        OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn run(save: bool, title: &str, default_name: Option<&str>) -> Option<PathBuf> {
        let filter: Vec<u16> =
            "Visio, Excalidraw and draw.io files (*.vsdx;*.vstx;*.vsdm;*.vstm;*.excalidraw;*.drawio) *.vsdx;*.vstx;*.vsdm;*.vstm;*.excalidraw;*.drawio All files (*.*) *.*  "
                .encode_utf16()
                .collect();
        let title = wide(title);
        let extension = wide("vsdx");
        let mut buffer = [0u16; 32768];
        if let Some(name) = default_name {
            let name: Vec<u16> = name.encode_utf16().collect();
            let count = name.len().min(buffer.len() - 1);
            buffer[..count].copy_from_slice(&name[..count]);
        }
        let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
        dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        dialog.lpstrFilter = filter.as_ptr();
        dialog.lpstrFile = buffer.as_mut_ptr();
        dialog.nMaxFile = buffer.len() as u32;
        dialog.lpstrTitle = title.as_ptr();
        dialog.lpstrDefExt = extension.as_ptr();
        dialog.Flags = OFN_EXPLORER
            | OFN_PATHMUSTEXIST
            | OFN_FILEMUSTEXIST
            | if save { OFN_OVERWRITEPROMPT } else { 0 };
        let ok = unsafe {
            if save {
                GetSaveFileNameW(&mut dialog)
            } else {
                GetOpenFileNameW(&mut dialog)
            }
        };
        if ok == 0 {
            return None;
        }
        let end = buffer.iter().position(|code| *code == 0)?;
        Some(PathBuf::from(OsString::from_wide(&buffer[..end])))
    }

    fn run_csv(save: bool, title: &str, default_name: Option<&str>) -> Option<PathBuf> {
        let filter: Vec<u16> =
            "CSV files (*.csv)\0*.csv\0All files (*.*)\0*.*\0\0"
                .encode_utf16()
                .collect();
        let title = wide(title);
        let extension = wide("csv");
        let mut buffer = [0u16; 32768];
        if let Some(name) = default_name {
            let name: Vec<u16> = name.encode_utf16().collect();
            let count = name.len().min(buffer.len() - 1);
            buffer[..count].copy_from_slice(&name[..count]);
        }
        let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
        dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        dialog.lpstrFilter = filter.as_ptr();
        dialog.lpstrFile = buffer.as_mut_ptr();
        dialog.nMaxFile = buffer.len() as u32;
        dialog.lpstrTitle = title.as_ptr();
        dialog.lpstrDefExt = extension.as_ptr();
        dialog.Flags = OFN_EXPLORER
            | OFN_PATHMUSTEXIST
            | OFN_FILEMUSTEXIST
            | if save { OFN_OVERWRITEPROMPT } else { 0 };
        let ok = unsafe {
            if save {
                GetSaveFileNameW(&mut dialog)
            } else {
                GetOpenFileNameW(&mut dialog)
            }
        };
        if ok == 0 {
            return None;
        }
        let end = buffer.iter().position(|code| *code == 0)?;
        Some(PathBuf::from(OsString::from_wide(&buffer[..end])))
    }

    fn run_ext(save: bool, title: &str, default_name: Option<&str>, label: &str, ext: &str) -> Option<PathBuf> {
        let filter: Vec<u16> =
            format!("{label} (*.{ext})\0*.{ext}\0All files (*.*)\0*.*\0\0")
                .encode_utf16()
                .collect();
        let title = wide(title);
        let extension = wide(ext);
        let mut buffer = [0u16; 32768];
        if let Some(name) = default_name {
            let name: Vec<u16> = name.encode_utf16().collect();
            let count = name.len().min(buffer.len() - 1);
            buffer[..count].copy_from_slice(&name[..count]);
        }
        let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
        dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        dialog.lpstrFilter = filter.as_ptr();
        dialog.lpstrFile = buffer.as_mut_ptr();
        dialog.nMaxFile = buffer.len() as u32;
        dialog.lpstrTitle = title.as_ptr();
        dialog.lpstrDefExt = extension.as_ptr();
        dialog.Flags = OFN_EXPLORER
            | OFN_PATHMUSTEXIST
            | OFN_FILEMUSTEXIST
            | if save { OFN_OVERWRITEPROMPT } else { 0 };
        let ok = unsafe {
            if save {
                GetSaveFileNameW(&mut dialog)
            } else {
                GetOpenFileNameW(&mut dialog)
            }
        };
        if ok == 0 {
            return None;
        }
        let end = buffer.iter().position(|code| *code == 0)?;
        Some(PathBuf::from(OsString::from_wide(&buffer[..end])))
    }

    pub fn save_ext(default_name: &str, label: &str, ext: &str) -> Option<PathBuf> {
        run_ext(true, "Export as", Some(default_name), label, ext)
    }

    pub fn open_stencil() -> Option<PathBuf> {
        run_ext(false, "Open stencil", None, "Visio stencils", "vssx")
    }

    pub fn save_csv(default_name: &str) -> Option<PathBuf> {
        run_csv(true, "Save list as", Some(default_name))
    }

    pub fn open() -> Option<PathBuf> {
        run(false, "Open Visio drawing", None)
    }

    pub fn save_as(default_name: &str) -> Option<PathBuf> {
        run(true, "Save Visio drawing as", Some(default_name))
    }
}

#[cfg(not(windows))]
mod file_dialog {
    //! Open and save dialogs through the desktop's file chooser (xdg-desktop-portal on Linux, the system panel on macOS).
    use rfd::FileDialog;
    use std::path::PathBuf;

    const DRAWINGS: &[&str] = &["vsdx", "vstx", "vsdm", "vstm", "excalidraw", "drawio"];

    pub fn open() -> Option<PathBuf> {
        FileDialog::new()
            .set_title("Open Visio drawing")
            .add_filter("Visio, Excalidraw and draw.io files", DRAWINGS)
            .add_filter("All files", &["*"])
            .pick_file()
    }
    pub fn save_as(default_name: &str) -> Option<PathBuf> {
        FileDialog::new()
            .set_title("Save Visio drawing as")
            .set_file_name(default_name)
            .add_filter("Visio drawings", &["vsdx"])
            .save_file()
    }
    pub fn save_csv(default_name: &str) -> Option<PathBuf> {
        FileDialog::new().set_title("Save list as").set_file_name(default_name).add_filter("CSV files", &["csv"]).save_file()
    }
    pub fn save_ext(default_name: &str, label: &str, ext: &str) -> Option<PathBuf> {
        FileDialog::new().set_title("Export as").set_file_name(default_name).add_filter(label, &[ext]).save_file()
    }
    pub fn open_stencil() -> Option<PathBuf> {
        FileDialog::new().set_title("Open stencil").add_filter("Visio stencils", &["vssx"]).pick_file()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Handle {
    Nw,
    N,
    Ne,
    W,
    E,
    Sw,
    S,
    Se,
}
impl Handle {
    fn cursor(self) -> egui::CursorIcon {
        match self {
            Handle::Nw | Handle::Se => egui::CursorIcon::ResizeNwSe,
            Handle::Ne | Handle::Sw => egui::CursorIcon::ResizeNeSw,
            Handle::N | Handle::S => egui::CursorIcon::ResizeVertical,
            Handle::W | Handle::E => egui::CursorIcon::ResizeHorizontal,
        }
    }

    /// Where the handle sits on the box as fractions (x left to right, y TOP to bottom), like `HANDLES` of the web shell.
    fn fractions(self) -> (f32, f32) {
        match self {
            Handle::Nw => (0.0, 0.0),
            Handle::N => (0.5, 0.0),
            Handle::Ne => (1.0, 0.0),
            Handle::W => (0.0, 0.5),
            Handle::E => (1.0, 0.5),
            Handle::Sw => (0.0, 1.0),
            Handle::S => (0.5, 1.0),
            Handle::Se => (1.0, 1.0),
        }
    }
}

/// The box that results from dragging `handle` of `start` to `pointer` (all in the same space, y down): the opposite edge stays; no side gets
/// smaller than `min`; with `keep_proportions` a corner keeps the aspect ratio (the larger change wins, like Shift in the web shell).
fn resized_rect(start: egui::Rect, handle: Handle, pointer: egui::Pos2, min_side: f32, keep_proportions: bool) -> egui::Rect {
    let (fx, fy) = handle.fractions();
    let (mut min, mut max) = (start.min, start.max);
    if fx == 0.0 {
        min.x = pointer.x.min(max.x - min_side);
    } else if fx == 1.0 {
        max.x = pointer.x.max(min.x + min_side);
    }
    if fy == 0.0 {
        min.y = pointer.y.min(max.y - min_side);
    } else if fy == 1.0 {
        max.y = pointer.y.max(min.y + min_side);
    }
    if keep_proportions && fx != 0.5 && fy != 0.5 {
        let (w, h) = (start.width().max(1e-6), start.height().max(1e-6));
        let scale = ((max.x - min.x) / w).max((max.y - min.y) / h).max(min_side / w.min(h));
        let (nw, nh) = (w * scale, h * scale);
        if fx == 0.0 {
            min.x = max.x - nw;
        } else {
            max.x = min.x + nw;
        }
        if fy == 0.0 {
            min.y = max.y - nh;
        } else {
            max.y = min.y + nh;
        }
    }
    egui::Rect::from_min_max(min, max)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragMode {
    Move,
    Resize(Handle),
    /// Draw a new shape of this `Engine::draw` kind (0 rectangle .. 6 line) from the press to the release.
    Draw(i32),
    /// Press on a shape, release on another: a glued connector between them (`visio_id` is the first shape).
    Connect,
    /// Drag the rotate handle above the selected shape (`App::rotating` holds the angles).
    Rotate,
    /// Drag on empty canvas: a rubber band that selects what it encloses.
    Marquee,
    /// Drag one end of the selected connector (`App::conn_end` says which): glue it to a shape / connection point, or leave it loose.
    ConnEnd,
}

/// A connector end being dragged: which end, and where the other end stays (scene px).
struct ConnEndState {
    begin: bool,
    fixed: egui::Pos2,
}

/// The rotation in progress: the shape, the pointer angle at the press (screen, y down), the shape's angle then (`Shape::rot` is in degrees, clockwise; here radians)
/// and the angle the pointer asks for now (`theta`: counter-clockwise, as `Engine::edit` op 7 wants it).
struct RotateState {
    id: String,
    a0: f32,
    rot0: f64,
    theta: f64,
}

/// How far above the top edge of the selected shape the rotate handle sits (screen px), and the radius within which a connection point is hit.
const ROTATE_STEM: f32 = 24.0;
const SITE_HIT_PX: f32 = 12.0;

/// An arrangement of the selected shapes (`Engine::group_op` / `Engine::edit` ops 11, 12, 14, 15).
#[derive(Clone, Copy, PartialEq)]
enum ArrangeCmd {
    Group,
    Ungroup,
    /// 2 to front, 3 to back, 4 forward, 5 backward
    Order(i32),
    /// 0 left, 1 centre, 2 right, 3 top, 4 middle, 5 bottom (to the first selected shape)
    Align(u32),
    /// `true` = vertically
    Distribute(bool),
    RotateQuarter,
    RotateLeft,
    /// `true` = about the horizontal axis
    Flip(bool),
    /// (width, height): give the other shapes the first one's width and / or height.
    SameSize(bool, bool),
    /// `Engine::group_op` 6 union, 7 combine, 8 fragment, 9 intersect, 10 subtract, 11 join, 12 trim.
    Shapes(i32),
}

/// What a drag on the canvas does: select / move / resize, or draw a shape.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    /// The Hand tool (ribbon Pan): a left-button drag moves the view, as Space + drag or the middle button do.
    Hand,
    Select,
    Draw(i32),
    Connector,
}

/// One open stencil of the Shapes pane: the engine's slot number, its title and its masters (id, name).
struct StencilEntry {
    index: usize,
    name: String,
    masters: Vec<(String, String)>,
    /// The file it was opened from (None for the built-in one): remembered between sessions.
    path: Option<std::path::PathBuf>,
}

/// The file that remembers the opened stencils (one path per line): `%APPDATA%\LayoutCraft\stencils.txt`.
fn stencil_memory_file() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("XDG_CONFIG_HOME")).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config").into_os_string()))?;
    Some(std::path::PathBuf::from(base).join("LayoutCraft").join("stencils.txt"))
}

/// Whether a master name matches the pane's search text (case-insensitive substring; empty matches all).
fn master_matches(name: &str, search: &str) -> bool {
    let needle = search.trim().to_lowercase();
    needle.is_empty() || name.to_lowercase().contains(&needle)
}

#[cfg(test)]
mod pane_tests {
    use super::master_matches;

    #[test]
    fn the_pane_search_is_a_case_insensitive_substring() {
        assert!(master_matches("Rounded rectangle", "RECT"));
        assert!(master_matches("Rounded rectangle", "  "));
        assert!(!master_matches("Ellipse", "rect"));
    }
}

struct DragState {
    visio_id: String,
    mode: DragMode,
    start_pointer: egui::Pos2,
    end_pointer: egui::Pos2,
    start_rect: egui::Rect,
}

#[derive(Clone, Copy)]
struct TextMetrics {
    pin_x: f64,
    pin_y: f64,
    width: f64,
    height: f64,
    loc_x: f64,
    loc_y: f64,
}

#[derive(Clone, Copy)]
struct ShapeMetrics {
    loc_ratio_x: f64,
    loc_ratio_y: f64,
    text: Option<TextMetrics>,
}

#[derive(Clone)]
enum PendingOpen {
    /// A new empty drawing.
    New,
    Reload,
    Load(String),
}

/// The style values the inspector edits for the selected shape; "Apply style" writes the ones that changed (one undo step).
struct StyleDraft {
    id: String,
    fill: [u8; 3],
    has_fill: bool,
    line: [u8; 3],
    has_line: bool,
    weight_px: f64,
    pattern: u32,
    /// the values when the draft was made (fill, has_fill, line, has_line, weight, pattern)
    initial: ([u8; 3], bool, [u8; 3], bool, f64, u32),
}

const LINE_PATTERNS: [(u32, &str); 4] = [(1, "Solid"), (2, "Dashed"), (3, "Dotted"), (4, "Dash-dot")];

fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn hex(color: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}

impl StyleDraft {
    fn from_shape(id: &str, shape: &Shape) -> Self {
        // a shape drawn from several parts (the example's 3-D boxes) has no fill of its own: take the first visible part fill
        let fill = shape
            .fill
            .as_deref()
            .and_then(parse_hex)
            .or_else(|| shape.parts.iter().filter_map(|part| part.fill.as_deref()).find_map(parse_hex));
        let line = shape.stroke.as_deref().and_then(parse_hex);
        let weight_px = shape.stroke_width.unwrap_or(1.0);
        let pattern = match shape.stroke_style.as_deref() {
            None | Some("solid") => 1,
            _ => 2,
        };
        let (fill_rgb, has_fill) = (fill.unwrap_or([255, 255, 255]), fill.is_some());
        let (line_rgb, has_line) = (line.unwrap_or([0, 0, 0]), line.is_some());
        Self {
            id: id.to_string(),
            fill: fill_rgb,
            has_fill,
            line: line_rgb,
            has_line,
            weight_px,
            pattern,
            initial: (fill_rgb, has_fill, line_rgb, has_line, weight_px, pattern),
        }
    }

    /// The op 10 style JSON of what changed, or `None`.
    fn changes(&self) -> Option<String> {
        let (fill, has_fill, line, has_line, weight, pattern) = self.initial;
        let mut map = serde_json::Map::new();
        if self.fill != fill || self.has_fill != has_fill {
            map.insert("fill".to_string(), if self.has_fill { hex(self.fill) } else { "none".to_string() }.into());
        }
        if self.line != line || self.has_line != has_line {
            map.insert("line".to_string(), if self.has_line { hex(self.line) } else { "none".to_string() }.into());
        }
        if (self.weight_px - weight).abs() > 1e-9 {
            map.insert("weight".to_string(), self.weight_px.into());
        }
        if self.pattern != pattern {
            map.insert("pattern".to_string(), self.pattern.into());
        }
        (!map.is_empty()).then(|| serde_json::Value::Object(map).to_string())
    }
}

struct TextEditor {
    visio_id: String,
    text: String,
    bold: bool,
    italic: bool,
    underline: bool,
    font_pt: f64,
    /// bold, italic, underline and size as the shape had them when the editor opened (only changes are written)
    initial: (bool, bool, bool, f64),
}

struct LayoutCraftApp {
    path_input: String,
    loaded_path: Option<PathBuf>,
    engine: Engine,
    baseline: Option<Document>,
    page_index: usize,
    page_height_in: f64,
    page_width_in: f64,
    scene: Option<Scene>,
    texture: Option<egui::TextureHandle>,
    texture_size: egui::Vec2,
    raster_scale: f64,
    /// The raster scale the view asks for (device pixels per scene pixel at the current zoom); the page is drawn again at this scale once the zoom settles.
    wanted_raster: f64,
    raster_tx: std::sync::mpsc::Sender<RasterJob>,
    raster_rx: std::sync::mpsc::Receiver<RasterDone>,
    raster_ticket: u64,
    raster_inflight: bool,
    /// A sharp picture of the visible window, drawn over the page picture when the page picture cannot reach the zoom (size cap): tiny shapes stay crisp.
    detail: Option<Detail>,
    detail_tx: std::sync::mpsc::Sender<RasterJob>,
    detail_rx: std::sync::mpsc::Receiver<RasterDone>,
    detail_ticket: u64,
    detail_inflight: bool,
    last_viewport: egui::Rect,
    last_view_px: f32,
    zoom_changed_at: std::time::Instant,
    zoom: f32,
    pan: egui::Vec2,
    fit_pending: bool,
    panning: bool,
    selected: Option<String>,
    tool: Tool,
    /// More selected shapes (shift-click); `selected` stays the primary one.
    extra_selected: Vec<String>,
    drag: Option<DragState>,
    preview: Option<egui::Rect>,
    dirty: bool,
    pending_open: Option<PendingOpen>,
    text_editor: Option<TextEditor>,
    style_draft: Option<StyleDraft>,
    /// The page name being edited in the Rename page window.
    page_rename: Option<String>,
    /// The name typed for a new layer.
    layer_name: String,
    /// Pastes since the last copy (each lands a step further from the original).
    paste_count: u32,
    ribbon: ribbon::Ribbon,
    ribbon_tab: usize,
    ribbon_collapsed: bool,
    ribbon_peek: bool,
    ribbon_compact: bool,
    /// Installed font families for the picker (listed on first use) and the picker's search text.
    font_list: Option<Vec<String>>,
    font_filter: String,
    /// The fonts of the open page that are not installed here, with what is drawn instead.
    font_warnings: Vec<FontSubstitution>,
    doc_props_open: bool,
    help_shortcuts_open: bool,
    /// The Find window, while it is open.
    find: Option<Find>,
    /// (selected ids, whether all their text is struck through) for the Strike button.
    struck_cache: Option<(String, bool)>,
    page_setup_open: bool,
    check_open: bool,
    shapes_open: bool,
    links_cache: std::cell::RefCell<Option<((String, usize, usize), Vec<(String, String)>)>>,
    show_grid: bool,
    pane_drag: Option<String>,
    pending_drop: Option<(usize, String, String, egui::Pos2)>,
    shape_search: String,
    snap_grid: bool,
    snap_shapes: bool,
    snap_guides: bool,
    grid_step: f64,
    snap_guide: (Option<f32>, Option<f32>),
    conn_end: Option<ConnEndState>,
    live_routes: Vec<(String, Vec<egui::Pos2>)>,
    last_live_delta: egui::Vec2,
    space_down: bool,
    routes_cache: std::cell::RefCell<Option<(usize, Vec<(String, Vec<egui::Pos2>)>)>>,
    rotating: Option<RotateState>,
    sites_cache: std::collections::HashMap<String, Vec<(usize, egui::Pos2)>>,
    connect_from_site: Option<usize>,
    stencil_autoopen_done: bool,
    stencils: Vec<StencilEntry>,
    stencil_textures: std::collections::HashMap<(usize, String), egui::TextureHandle>,
    armed_master: Option<(usize, String, String)>,
    export_open: bool,
    export_format: usize,
    borders_open: bool,
    border_style: usize,
    border_title: String,
    report_open: bool,
    report_all: bool,
    report_override: std::collections::HashMap<String, bool>,
    report_rows: Vec<layoutcraft_engine::inventory::Row>,
    report_cols: Vec<layoutcraft_engine::inventory::DataCol>,
    compare_open: bool,
    compare_name: String,
    compare_diffs: Vec<layoutcraft_engine::compare::Diff>,
    check_alt: bool,
    check_issues: Vec<layoutcraft_engine::check::Issue>,
    page_setup_size: (f64, f64),
    comments_open: bool,
    comment_text: String,
    help_about_open: bool,
    mcp_open: bool,
    mcp_server: String,
    mcp_root: String,
    mcp_message: String,
    ribbon_textures: BTreeMap<String, egui::TextureHandle>,
    /// Shift was held at the start of the frame (read once per frame).
    shift_down: bool,
    status: String,
}

impl LayoutCraftApp {
    fn new(cc: &eframe::CreationContext<'_>, initial: Option<String>) -> Self {
        let (raster_tx, raster_rx) = spawn_raster_thread();
        let (detail_tx, detail_rx) = spawn_raster_thread();
        let mut app = Self {
            raster_tx,
            raster_rx,
            raster_ticket: 0,
            raster_inflight: false,
            detail: None,
            detail_tx,
            detail_rx,
            detail_ticket: 0,
            detail_inflight: false,
            last_viewport: egui::Rect::NOTHING,
            path_input: initial.clone().unwrap_or_default(),
            loaded_path: None,
            engine: Engine::new(),
            baseline: None,
            page_index: 0,
            page_height_in: 11.0,
            page_width_in: 8.5,
            scene: None,
            texture: None,
            texture_size: egui::Vec2::ZERO,
            raster_scale: 1.0,
            wanted_raster: 1.0,
            last_view_px: 0.0,
            zoom_changed_at: std::time::Instant::now(),
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            fit_pending: true,
            panning: false,
            selected: None,
            tool: Tool::Select,
            extra_selected: Vec::new(),
            drag: None,
            preview: None,
            dirty: false,
            pending_open: None,
            text_editor: None,
            style_draft: None,
            page_rename: None,
            layer_name: String::new(),
            paste_count: 0,
            ribbon: ribbon::Ribbon::load(),
            ribbon_tab: 1,
            ribbon_collapsed: false,
            ribbon_peek: false,
            ribbon_compact: false,
            font_list: None,
            font_filter: String::new(),
            font_warnings: Vec::new(),
            doc_props_open: false,
            help_shortcuts_open: false,
            find: None,
            struck_cache: None,
            page_setup_open: false,
            check_open: false,
            shapes_open: true,
            links_cache: std::cell::RefCell::new(None),
            show_grid: false,
            pane_drag: None,
            pending_drop: None,
            shape_search: String::new(),
            snap_grid: false,
            snap_shapes: false,
            snap_guides: true,
            grid_step: 0.25,
            snap_guide: (None, None),
            conn_end: None,
            live_routes: Vec::new(),
            last_live_delta: egui::Vec2::ZERO,
            space_down: false,
            routes_cache: Default::default(),
            rotating: None,
            sites_cache: Default::default(),
            connect_from_site: None,
            stencil_autoopen_done: false,
            stencils: Vec::new(),
            stencil_textures: Default::default(),
            armed_master: None,
            export_open: false,
            export_format: 0,
            borders_open: false,
            border_style: 0,
            border_title: String::new(),
            report_open: false,
            report_all: false,
            report_override: Default::default(),
            report_rows: Vec::new(),
            report_cols: Vec::new(),
            compare_open: false,
            compare_name: String::new(),
            compare_diffs: Vec::new(),
            check_alt: false,
            check_issues: Vec::new(),
            page_setup_size: (11.0, 8.5),
            comments_open: false,
            comment_text: String::new(),
            help_about_open: false,
            mcp_open: false,
            mcp_server: mcpinfo::server_placeholder().to_string(),
            mcp_root: "/path/to/your/drawings".to_string(),
            mcp_message: String::new(),
            ribbon_textures: BTreeMap::new(),
            shift_down: false,
            status: "Open a .vsdx/.vstx file or drop one on the window.".to_string(),
        };
        if let Some(path) = initial {
            app.load(&cc.egui_ctx, Path::new(&path));
        }
        app
    }

    fn load(&mut self, ctx: &egui::Context, path: &Path) {
        self.status = format!("Opening {}...", path.display());
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.status = format!("Could not read {}: {error}", path.display());
                return;
            }
        };
        let is_drawio = path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("drawio"));
        let is_excalidraw = path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("excalidraw"));
        let is_scene = is_drawio || is_excalidraw;
        let opened: Result<(), String> = if is_drawio {
            self.engine.import_drawio(&bytes).map(|_| ())
        } else if is_excalidraw {
            String::from_utf8(bytes).map_err(|_| "This is not an Excalidraw file (it is not UTF-8 text)".to_string()).and_then(|text| self.engine.import_excalidraw(&text).map(|_| ()))
        } else {
            self.engine.open(bytes)
        };
        if let Err(error) = opened {
            self.status = format!("Could not open {}: {error}", path.display());
            return;
        }
        let document = match self.engine.document() {
            Some(document) => document.clone(),
            None => return,
        };
        self.baseline = Some(document);
        self.loaded_path = if is_scene { None } else { Some(path.to_path_buf()) };
        self.path_input = path.display().to_string();
        self.page_index = 0;
        self.selected = None;
        self.drag = None;
        self.preview = None;
        self.dirty = is_scene;
        self.pending_open = None;
        self.build_page(ctx, true);
        let pages = self
            .engine
            .document
            .as_ref()
            .map_or(0, |document| document.pages.len());
        self.status = if is_scene {
            format!("Imported {} as a new drawing; use Save as to keep it", path.display())
        } else {
            format!("{} - {pages} page(s)", path.display())
        };
    }

    /// A new empty drawing (one page, 11 x 8.5 in), as the web's New.
    fn new_drawing(&mut self, ctx: &egui::Context) {
        if let Err(error) = self.engine.new_blank(11.0, 8.5, false, "Page-1") {
            self.status = format!("Could not create a drawing: {error}");
            return;
        }
        self.baseline = self.engine.document().cloned();
        self.loaded_path = None;
        self.path_input.clear();
        self.page_index = 0;
        self.selected = None;
        self.extra_selected.clear();
        self.drag = None;
        self.preview = None;
        self.dirty = false;
        self.pending_open = None;
        self.build_page(ctx, true);
        self.status = "New drawing; Save as keeps it.".to_string();
    }

    fn reload(&mut self, ctx: &egui::Context) {
        if let Some(path) = self.loaded_path.clone() {
            self.load(ctx, &path);
        }
    }

    fn go_to_page(&mut self, ctx: &egui::Context, page: usize) {
        let pages = self
            .engine
            .document
            .as_ref()
            .map_or(0, |document| document.pages.len());
        if pages == 0 {
            return;
        }
        let page = page.min(pages - 1);
        if page == self.page_index && self.texture.is_some() {
            return;
        }
        self.page_index = page;
        self.selected = None;
        self.drag = None;
        self.preview = None;
        self.build_page(ctx, true);
    }

    fn build_page(&mut self, ctx: &egui::Context, reset_view: bool) {
        let _timer = Timer::new("build_page (whole)", 0.0);
        self.sites_cache.clear();
        self.struck_cache = None;
        *self.routes_cache.borrow_mut() = None;
        let (Some(package), Some(document)) = (self.engine.package.as_ref(), self.engine.document.as_ref())
        else {
            return;
        };
        let mut page_width_in = 8.5;
        if let Ok(pages) = package.pages() {
            if let Some(page) = pages.get(self.page_index) {
                self.page_height_in = page.height.unwrap_or(11.0);
                page_width_in = page.width.unwrap_or(8.5);
                self.page_width_in = page_width_in;
            }
        }
        let mut media: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for (id, record) in &document.media {
            // media_bytes converts what the renderer cannot draw itself (BMP to PNG, EMF / WMF to SVG): the raw part bytes of a stencil picture are an EMF
            if let Some(bytes) = layoutcraft_vsdx::media_bytes(package, record) {
                media.insert(id.clone(), bytes);
            }
        }
        let scene_timer = Timer::new("  scene_from_document", 0.0);
        let mut scene = match scene_from_document_with_media(document, self.page_index, &media) {
            Ok(scene) => scene,
            Err(error) => {
                self.status = format!("Could not build page scene: {error}");
                return;
            }
        };
        // An empty template page has no content bounds; show the actual page
        // rectangle instead of a 1x1 white square.
        // Likewise a page with shapes shows the WHOLE page (a shape that sticks out of it widens the view), not the bounding box of its
        // shapes: creating the first shape must not shrink the canvas to that shape.
        {
            // the sheet's PageWidth / PageHeight are in drawing units; the scene's shapes are already scaled to paper (page.scale, e.g. 1:50)
            let paper = document.pages.get(self.page_index).map_or(1.0, |page| page.scale).max(1e-6);
            let (page_w, page_h) = ((page_width_in * paper * DPI).max(1.0), (self.page_height_in * paper * DPI).max(1.0));
            let (min_x, min_y) = if scene.items.is_empty() { (0.0, 0.0) } else { (scene.min_x.min(0.0), scene.min_y.min(0.0)) };
            let (max_x, max_y) = if scene.items.is_empty() { (page_w, page_h) } else { ((scene.min_x + scene.width).max(page_w), (scene.min_y + scene.height).max(page_h)) };
            scene.min_x = min_x;
            scene.min_y = min_y;
            scene.width = max_x - min_x;
            scene.height = max_y - min_y;
        }
        drop(scene_timer);
        let keep_view = !reset_view;
        self.font_warnings = scene_font_substitutions(&scene);
        self.scene = Some(scene);
        if keep_view && self.texture.is_some() {
            // an edit: the old picture stays on screen until the new one is drawn on the raster thread
            self.clear_detail();
            self.start_raster(ctx);
        } else if !self.raster_scene(ctx, keep_view) {
            return;
        }
        if reset_view {
            self.selected = None;
            self.fit_pending = true;
        } else {
            self.fit_pending = false;
        }
    }

    /// The raster scale for a page of `w` x `h` scene px that wants `wanted` device pixels per scene pixel: never below what the page always had
    /// (1.0, less for huge pages: 8192 px on the long side) and capped by 8192 px on the long side and about 24 million pixels.
    fn raster_scale_for(w: f64, h: f64, wanted: f64) -> f64 {
        let longest = w.max(h).max(1.0);
        let base = (8192.0 / longest).min(1.0);
        let cap = (8192.0 / longest).min((24.0e6 / (w.max(1.0) * h.max(1.0))).sqrt()).max(base);
        wanted.clamp(base, cap)
    }

    /// Draw `self.scene` at the wanted raster scale and make it the page texture. With `keep_view` the zoom is adjusted so the picture keeps its
    /// size on screen (the texture just gets more or fewer pixels). Returns false when it failed (the status says why).
    fn raster_scene(&mut self, ctx: &egui::Context, keep_view: bool) -> bool {
        let Some(scene) = self.scene.take() else {
            return false;
        };
        self.raster_ticket += 1;
        self.raster_inflight = false;
        self.clear_detail();
        let (width, height) = scene.raster_size();
        let raster_scale = Self::raster_scale_for(width as f64, height as f64, self.wanted_raster);
        let render_timer = Timer::new("  render_rgba", 0.0);
        let rendered = render_rgba(&scene, raster_scale);
        drop(render_timer);
        let (image_width, image_height, rgba) = match rendered {
            Ok(image) => image,
            Err(error) => {
                self.status = format!("Could not rasterize page: {error}");
                self.scene = Some(scene);
                return false;
            }
        };
        let size = [image_width as usize, image_height as usize];
        let _upload_timer = Timer::new("  texture upload", 0.0);
        let pixels = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
        let texture = ctx.load_texture(format!("layoutcraft-page-{}", self.page_index), pixels, egui::TextureOptions::LINEAR);
        if keep_view && self.raster_scale > 0.0 && self.texture.is_some() {
            self.zoom = (f64::from(self.zoom) * self.raster_scale / raster_scale) as f32;
        }
        self.texture_size = texture.size_vec2();
        self.raster_scale = raster_scale;
        self.texture = Some(texture);
        self.scene = Some(scene);
        true
    }

    /// Draw `self.scene` on the raster thread at the wanted scale; `apply_raster` swaps the picture in when it is done.
    fn start_raster(&mut self, ctx: &egui::Context) {
        let Some(scene) = self.scene.clone() else {
            return;
        };
        let (width, height) = scene.raster_size();
        let scale = Self::raster_scale_for(width as f64, height as f64, self.wanted_raster);
        self.raster_ticket += 1;
        self.raster_inflight = true;
        let _ = self.raster_tx.send(RasterJob { ticket: self.raster_ticket, scene, scale, keep_view: true, region: None, ctx: ctx.clone() });
    }

    /// The window picture belongs to the old scene: drop it (and any one being drawn).
    fn clear_detail(&mut self) {
        self.detail = None;
        self.detail_ticket += 1;
        self.detail_inflight = false;
    }

    /// Keep the sharp window picture in step with the view: needed only where the page picture is too coarse for the zoom (its size cap).
    fn settle_detail(&mut self, ctx: &egui::Context, device_scale: f32) {
        let have = self.raster_scale as f32;
        if device_scale <= have * 1.2 {
            if self.detail.is_some() {
                self.clear_detail();
            }
            return;
        }
        if self.detail_inflight || self.drag.is_some() || self.last_viewport == egui::Rect::NOTHING {
            return;
        }
        let Some(scene) = self.scene.clone() else {
            return;
        };
        let (width, height) = scene.raster_size();
        let page = (f64::from(width), f64::from(height));
        let image_min = self.image_rect(self.last_viewport).min;
        let view = self.view_scale();
        let Some(visible) = detail_window(image_min, self.last_viewport, view, page, device_scale, 0.0) else {
            return;
        };
        if let Some(detail) = &self.detail {
            let r = detail.region;
            let covers = r[0] <= visible[0] + 1e-6 && r[1] <= visible[1] + 1e-6 && r[0] + r[2] >= visible[0] + visible[2] - 1e-6 && r[1] + r[3] >= visible[1] + visible[3] - 1e-6;
            if covers && (detail.scale - device_scale).abs() <= detail.scale * 0.2 {
                return;
            }
        }
        let Some(region) = detail_window(image_min, self.last_viewport, view, page, device_scale, 0.25) else {
            return;
        };
        self.detail_ticket += 1;
        self.detail_inflight = true;
        let _ = self.detail_tx.send(RasterJob { ticket: self.detail_ticket, scene, scale: f64::from(device_scale), keep_view: false, region: Some(region), ctx: ctx.clone() });
    }

    /// Take the finished pictures of the raster thread (only the newest job counts; an older one is dropped).
    fn apply_raster(&mut self, ctx: &egui::Context) {
        while let Ok(done) = self.detail_rx.try_recv() {
            if done.ticket != self.detail_ticket {
                continue;
            }
            self.detail_inflight = false;
            if let (Ok((size, rgba)), Some(region)) = (done.result, done.region) {
                let pixels = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                let texture = ctx.load_texture("layoutcraft-detail", pixels, egui::TextureOptions::LINEAR);
                self.detail = Some(Detail { texture, region, scale: done.scale as f32 });
            }
        }
        while let Ok(done) = self.raster_rx.try_recv() {
            if done.ticket != self.raster_ticket {
                continue;
            }
            self.raster_inflight = false;
            match done.result {
                Ok((size, rgba)) => {
                    let pixels = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                    let texture = ctx.load_texture(format!("layoutcraft-page-{}", self.page_index), pixels, egui::TextureOptions::LINEAR);
                    if done.keep_view && self.raster_scale > 0.0 && self.texture.is_some() {
                        self.zoom = (f64::from(self.zoom) * self.raster_scale / done.scale) as f32;
                    }
                    self.texture_size = texture.size_vec2();
                    self.raster_scale = done.scale;
                    self.texture = Some(texture);
                    self.last_view_px = self.view_scale() * ctx.pixels_per_point();
                }
                Err(error) => self.status = format!("Could not draw the page: {error}"),
            }
        }
    }

    /// Called every frame: once the zoom has stood still for a moment, draw the page again at the resolution the screen needs (crisp when zoomed
    /// in, no wasted pixels when zoomed out).
    fn settle_raster(&mut self, ctx: &egui::Context) {
        if self.texture.is_none() || self.scene.is_none() {
            return;
        }
        let device_scale = self.view_scale() * ctx.pixels_per_point();
        if (device_scale - self.last_view_px).abs() > 1e-4 {
            self.last_view_px = device_scale;
            self.zoom_changed_at = std::time::Instant::now();
        }
        let settle = std::time::Duration::from_millis(220);
        let waited = self.zoom_changed_at.elapsed();
        if waited < settle {
            ctx.request_repaint_after(settle - waited + std::time::Duration::from_millis(10));
            return;
        }
        self.settle_detail(ctx, device_scale);
        let have = self.raster_scale as f32;
        if self.drag.is_some() || self.raster_inflight || (device_scale <= have * 1.2 && device_scale >= have * 0.6) {
            return;
        }
        if (f64::from(device_scale) - self.wanted_raster).abs() < 1e-3 {
            return; // already asked for this scale (the cap or the floor kept it)
        }
        self.wanted_raster = f64::from(device_scale);
        self.last_view_px = device_scale;
        self.start_raster(ctx);
    }

    fn fit_to_view(&mut self, available: egui::Vec2) {
        let some_size = self.texture.is_some() && self.texture_size.x > 0.0;
        if !some_size || available.x <= 1.0 || available.y <= 1.0 {
            return;
        }
        self.zoom = (available.x / self.texture_size.x)
            .min(available.y / self.texture_size.y)
            .min(8.0)
            * 0.96;
        self.pan = egui::Vec2::ZERO;
        self.fit_pending = false;
    }

    fn image_rect(&self, viewport: egui::Rect) -> egui::Rect {
        let size = self.texture_size * self.zoom;
        egui::Rect::from_center_size(viewport.center() + self.pan, size)
    }

    fn view_scale(&self) -> f32 {
        (self.zoom * self.raster_scale as f32).max(1e-6)
    }

    fn scene_origin(&self) -> egui::Pos2 {
        self.scene.as_ref().map_or(egui::Pos2::ZERO, |scene| {
            egui::pos2(scene.min_x as f32, scene.min_y as f32)
        })
    }

    fn screen_to_scene(&self, pointer: egui::Pos2, image_rect: egui::Rect) -> egui::Pos2 {
        let origin = self.scene_origin();
        let scale = self.view_scale();
        egui::pos2(
            origin.x + (pointer.x - image_rect.min.x) / scale,
            origin.y + (pointer.y - image_rect.min.y) / scale,
        )
    }

    fn scene_rect_to_screen(&self, rect: egui::Rect, image_rect: egui::Rect) -> egui::Rect {
        let origin = self.scene_origin();
        let scale = self.view_scale();
        egui::Rect::from_min_max(
            image_rect.min + (rect.min - origin) * scale,
            image_rect.min + (rect.max - origin) * scale,
        )
    }

    /// The page-level connectors as (id, route in scene px) (`Engine::connectors_json`: inches, y up).
    fn connector_routes(&self) -> Vec<(String, Vec<egui::Pos2>)> {
        // asking the engine for every route costs 100+ ms on a real drawing and this runs every frame with a connector selected: keep the answer until the page is rebuilt
        if let Some((page, routes)) = &*self.routes_cache.borrow() {
            if *page == self.page_index {
                return routes.clone();
            }
        }
        let routes = self.connector_routes_uncached();
        *self.routes_cache.borrow_mut() = Some((self.page_index, routes.clone()));
        routes
    }

    fn connector_routes_uncached(&self) -> Vec<(String, Vec<egui::Pos2>)> {
        let _timer = Timer::new("  connectors_json + parse", 2.0);
        let Ok(text) = self.engine.connectors_json(self.page_index) else {
            return Vec::new();
        };
        let Ok(list) = serde_json::from_str::<serde_json::Value>(&text) else {
            return Vec::new();
        };
        let page_h = self.page_height_in;
        // the engine lists every 1-D shape (begin and end point): a rack-unit master of the Dell stencils is one. Only the connectors of the page are
        // connectors here; a press on such a shape must select and move the shape, not "the connector" along its hairline.
        let real: std::collections::HashSet<String> = self
            .engine
            .document
            .as_ref()
            .and_then(|document| document.pages.get(self.page_index))
            .map(|page| page.connectors.iter().filter_map(|c| c.visio_id.clone().or_else(|| c.id.clone())).collect())
            .unwrap_or_default();
        list.as_array()
            .map(|connectors| {
                connectors
                    .iter()
                    .filter_map(|connector| {
                        let id = connector.get("id")?.as_str()?.to_string();
                        if !real.contains(&id) {
                            return None;
                        }
                        let route = connector.get("route")?.as_array()?;
                        let points: Vec<egui::Pos2> = route
                            .iter()
                            .filter_map(|point| {
                                let (x, y) = (point.get(0)?.as_f64()?, point.get(1)?.as_f64()?);
                                Some(egui::pos2((x * DPI) as f32, ((page_h - y) * DPI) as f32))
                            })
                            .collect();
                        (points.len() >= 2).then_some((id, points))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn is_connector(&self, id: &str) -> bool {
        self.connector_routes().iter().any(|(other, _)| other == id)
    }

    /// The connector whose route passes within a few screen pixels of the point (scene px).
    fn pick_connector(&self, scene_pointer: egui::Pos2) -> Option<String> {
        let tolerance = 6.0 / self.view_scale().max(0.01);
        let distance = |a: egui::Pos2, b: egui::Pos2| {
            let (ab, ap) = (b - a, scene_pointer - a);
            let len2 = ab.length_sq();
            let t = if len2 <= f32::EPSILON { 0.0 } else { (ap.dot(ab) / len2).clamp(0.0, 1.0) };
            (scene_pointer - (a + ab * t)).length()
        };
        self.connector_routes()
            .into_iter()
            .map(|(id, points)| {
                let best = points.windows(2).map(|pair| distance(pair[0], pair[1])).fold(f32::MAX, f32::min);
                (id, best)
            })
            .filter(|(_, best)| *best <= tolerance)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// A click on a member of a group selects the whole group (its top-level root), as in Visio: the member itself cannot be moved or deleted alone.
    fn group_root(&self, id: &str) -> String {
        let root = self
            .doc_shape(id)
            .and_then(|shape| shape.group.as_ref())
            .and_then(|group| group.get("rootId"))
            .and_then(|root| root.as_str())
            .map(str::to_string);
        match root {
            Some(root) if root != id && self.doc_shape(&root).is_some() => root,
            _ => id.to_string(),
        }
    }

    fn pick_shape(&self, scene_pointer: egui::Pos2) -> Option<String> {
        let id = self.pick_shape_raw(scene_pointer)?;
        Some(self.group_root(&id))
    }

    /// The topmost shape under the point, a member of a group included (what a connector glues to: Visio and the web glue to the shape itself).
    fn pick_shape_raw(&self, scene_pointer: egui::Pos2) -> Option<String> {
        let scene = self.scene.as_ref()?;
        for item in scene.items.iter().rev() {
            let SceneItem::Shape(shape) = item else {
                continue;
            };
            // the painted extent, so a group whose frame is a hairline under its picture can be hit on the picture
            if Self::selection_scene_rect(shape).contains(scene_pointer) {
                return shape.visio_id.clone();
            }
        }
        None
    }

    fn scene_shape(&self, visio_id: &str) -> Option<&SceneShape> {
        let scene = self.scene.as_ref()?;
        scene.items.iter().find_map(|item| match item {
            SceneItem::Shape(shape) if shape.visio_id.as_deref() == Some(visio_id) => {
                Some(shape.as_ref())
            }
            _ => None,
        })
    }

    /// The font stack of the selected shape's text (as the drawing names it) and, when that font is not installed, what is drawn instead.
    fn selected_font(&self) -> Option<(String, Option<FontSubstitution>)> {
        let shape = self.scene_shape(self.selected.as_deref()?)?;
        let stack = shape.text.as_ref().or(shape.texts.first())?.font_family.clone();
        let replaced = font_substitution(&stack);
        Some((first_font_name(&stack), replaced))
    }

    fn doc_shape(&self, visio_id: &str) -> Option<&Shape> {
        let document = self.engine.document.as_ref()?;
        let page = document.pages.get(self.page_index)?;
        page.shapes
            .iter()
            .find(|shape| shape.visio_id.as_deref() == Some(visio_id))
    }

    /// The rectangle to outline for a selected shape: its frame, widened to what it paints (a group whose frame is a hairline, like the 1-D rack
    /// masters of the Dell stencils, shows its picture well outside the frame).
    fn selection_scene_rect(shape: &SceneShape) -> egui::Rect {
        let frame = Self::shape_scene_rect(shape);
        if shape.rot.abs() > 1e-9 || frame.height() >= 12.0 {
            return frame; // a turned shape keeps its frame (the outline is turned with it); a normal frame is already what the shape is
        }
        let (x, y, w, h) = layoutcraft_render::shape_painted_bounds(shape);
        let painted = egui::Rect::from_min_size(egui::pos2(x as f32, y as f32), egui::vec2(w as f32, h as f32));
        // a text-only shape or shadows must not stretch the outline much: keep the union only when the frame is clearly smaller than the picture
        if painted.height() > frame.height() * 2.0 { frame.union(painted) } else { frame }
    }

    fn shape_scene_rect(shape: &SceneShape) -> egui::Rect {
        egui::Rect::from_min_size(
            egui::pos2(shape.x as f32, shape.y as f32),
            egui::vec2(shape.w as f32, shape.h as f32),
        )
    }

    /// Whether a selected shape is big enough on screen for its eight resize handles: on a shape smaller than the handles in both directions
    /// (the SFP modules of the HPE stencils are 0.05 in) they would cover all of it, so a press could only resize it and never move it.
    fn handles_fit(rect: egui::Rect) -> bool {
        rect.width() >= 2.5 * HANDLE_SIZE || rect.height() >= 2.5 * HANDLE_SIZE
    }

    fn handle_rects(rect: egui::Rect) -> [(Handle, egui::Rect); 8] {
        let size = egui::vec2(HANDLE_SIZE, HANDLE_SIZE);
        let at = |handle: Handle| {
            let (fx, fy) = handle.fractions();
            (handle, egui::Rect::from_center_size(egui::pos2(rect.min.x + rect.width() * fx, rect.min.y + rect.height() * fy), size))
        };
        [at(Handle::Nw), at(Handle::N), at(Handle::Ne), at(Handle::W), at(Handle::E), at(Handle::Sw), at(Handle::S), at(Handle::Se)]
    }

    /// The positions other shapes and the ruler guides offer to snap to, in page inches: (x values, y values). The shapes being moved are left out.
    fn snap_targets(&self) -> (Vec<f64>, Vec<f64>) {
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        let page_h = self.page_height_in;
        if self.snap_shapes {
            let moving = self.all_selected();
            if let Some(scene) = self.scene.as_ref() {
                for item in &scene.items {
                    let SceneItem::Shape(shape) = item else { continue };
                    let Some(id) = shape.visio_id.as_deref() else { continue };
                    if moving.iter().any(|m| m == id) || self.doc_shape(id).is_none_or(|doc| doc.group.is_some()) {
                        continue;
                    }
                    let r = Self::selection_scene_rect(shape);
                    let (x0, x1) = (f64::from(r.min.x) / DPI, f64::from(r.max.x) / DPI);
                    let (y1, y0) = (page_h - f64::from(r.min.y) / DPI, page_h - f64::from(r.max.y) / DPI);
                    xs.extend([x0, (x0 + x1) / 2.0, x1]);
                    ys.extend([y0, (y0 + y1) / 2.0, y1]);
                }
            }
        }
        if self.snap_guides {
            if let Some(page) = self.engine.document.as_ref().and_then(|d| d.pages.get(self.page_index)) {
                for guide in &page.guides {
                    let turn = guide.angle.rem_euclid(std::f64::consts::PI);
                    if turn.abs() < 0.01 || (turn - std::f64::consts::PI).abs() < 0.01 {
                        ys.push(guide.y_in);
                    } else if (turn - std::f64::consts::FRAC_PI_2).abs() < 0.01 {
                        xs.push(guide.x_in);
                    }
                }
            }
        }
        (xs, ys)
    }

    fn snapping_on(&self) -> bool {
        self.snap_grid || self.snap_shapes || self.snap_guides
    }

    /// `delta` (scene px) of a drag of `start` corrected by the snapping rules; also the guide positions in scene px.
    fn snapped_move(&self, start: egui::Rect, delta: egui::Vec2) -> (egui::Vec2, (Option<f32>, Option<f32>)) {
        if !self.snapping_on() {
            return (delta, (None, None));
        }
        let page_h = self.page_height_in;
        let (xs, ys) = self.snap_targets();
        let box_in = (f64::from(start.min.x) / DPI, page_h - f64::from(start.max.y) / DPI, f64::from(start.max.x) / DPI, page_h - f64::from(start.min.y) / DPI);
        let threshold = 8.0 / (DPI * f64::from(self.view_scale().max(0.01)));
        let grid = if self.snap_grid { self.grid_step } else { 0.0 };
        let (dx, dy, gx, gy) = snap_move(box_in, f64::from(delta.x) / DPI, -f64::from(delta.y) / DPI, grid, &xs, &ys, threshold);
        (egui::vec2((dx * DPI) as f32, (-dy * DPI) as f32), (gx.map(|x| (x * DPI) as f32), gy.map(|y| ((page_h - y) * DPI) as f32)))
    }

    /// A point (scene px) pulled to the grid and the snap targets: the corners of a shape being drawn.
    fn snapped_point(&self, point: egui::Pos2) -> egui::Pos2 {
        if !self.snapping_on() {
            return point;
        }
        let (xs, ys) = self.snap_targets();
        let threshold = 8.0 / (DPI * f64::from(self.view_scale().max(0.01)));
        let grid = if self.snap_grid { self.grid_step } else { 0.0 };
        let page_h = self.page_height_in;
        let x = snap_axis(&[f64::from(point.x) / DPI], &xs, grid, threshold).map_or(point.x, |(d, _)| point.x + (d * DPI) as f32);
        let y_up = page_h - f64::from(point.y) / DPI;
        let y = snap_axis(&[y_up], &ys, grid, threshold).map_or(point.y, |(d, _)| point.y - (d * DPI) as f32);
        egui::pos2(x, y)
    }

    /// A resized box (scene px): the edges the handle moves snap to the grid and the targets.
    fn snapped_resize(&self, rect: egui::Rect, handle: Handle) -> (egui::Rect, (Option<f32>, Option<f32>)) {
        if !self.snapping_on() {
            return (rect, (None, None));
        }
        let page_h = self.page_height_in;
        let (xs, ys) = self.snap_targets();
        let threshold = 8.0 / (DPI * f64::from(self.view_scale().max(0.01)));
        let grid = if self.snap_grid { self.grid_step } else { 0.0 };
        let (fx, fy) = handle.fractions();
        let (mut min, mut max) = (rect.min, rect.max);
        let mut guides = (None, None);
        if fx != 0.5 {
            let edge = if fx == 0.0 { &mut min.x } else { &mut max.x };
            if let Some((delta, at)) = snap_axis(&[f64::from(*edge) / DPI], &xs, grid, threshold) {
                *edge += (delta * DPI) as f32;
                guides.0 = Some((at * DPI) as f32);
            }
        }
        if fy != 0.5 {
            let edge = if fy == 0.0 { &mut min.y } else { &mut max.y };
            if let Some((delta, at)) = snap_axis(&[page_h - f64::from(*edge) / DPI], &ys, grid, threshold) {
                *edge -= (delta * DPI) as f32;
                guides.1 = Some(((page_h - at) * DPI) as f32);
            }
        }
        if max.x - min.x < MIN_SHAPE_PX || max.y - min.y < MIN_SHAPE_PX {
            return (rect, (None, None));
        }
        (egui::Rect::from_min_max(min, max), guides)
    }

    /// The two ends of a connector's route in scene px (first and last point), when it is a connector of this page.
    fn connector_end_points(&self, id: &str) -> Option<(egui::Pos2, egui::Pos2)> {
        let (_, route) = self.connector_routes().into_iter().find(|(other, _)| other == id)?;
        Some((*route.first()?, *route.last()?))
    }

    fn scene_to_screen(&self, point: egui::Pos2, image_rect: egui::Rect) -> egui::Pos2 {
        self.scene_rect_to_screen(egui::Rect::from_min_size(point, egui::Vec2::ZERO), image_rect).min
    }

    /// The connection points of a shape in scene px, as (site number for `Engine::add_connector`, position); cached until the page is rebuilt.
    fn sites_of(&mut self, id: &str) -> Vec<(usize, egui::Pos2)> {
        if let Some(list) = self.sites_cache.get(id) {
            return list.clone();
        }
        let page_h = self.page_height_in;
        let list: Vec<(usize, egui::Pos2)> = self
            .engine
            .connection_points_json(self.page_index, id)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .map(|value| {
                value.as_array().map_or_else(Vec::new, |points| {
                    points
                        .iter()
                        .filter_map(|p| Some((p["n"].as_u64()? as usize, egui::pos2((p["x"].as_f64()? * DPI) as f32, ((page_h - p["y"].as_f64()?) * DPI) as f32))))
                        .collect()
                })
            })
            .unwrap_or_default();
        self.sites_cache.insert(id.to_string(), list.clone());
        list
    }

    /// The connection point of shape `id` within reach of the scene point `at`: its position and site number; else `at` and `None` (the whole shape).
    fn nearest_site(&mut self, id: &str, at: egui::Pos2) -> (egui::Pos2, Option<usize>) {
        let reach = SITE_HIT_PX / self.view_scale().max(0.01);
        let best = self.sites_of(id).into_iter().map(|(n, pos)| ((pos - at).length(), n, pos)).filter(|(d, _, _)| *d <= reach).min_by(|a, b| a.0.total_cmp(&b.0));
        match best {
            Some((_, n, pos)) => (pos, Some(n)),
            None => (at, None),
        }
    }

    /// The shape at the scene point, also when the point is a few pixels outside it (connection points sit on the edge).
    fn pick_shape_near(&self, at: egui::Pos2) -> Option<String> {
        let r = 6.0 / self.view_scale().max(0.01);
        [(0.0, 0.0), (r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r)].into_iter().find_map(|(dx, dy)| self.pick_shape_raw(at + egui::vec2(dx, dy)))
    }

    /// The rotate handle of a shape on screen: (middle of the top edge, the handle, the centre of the shape). `None` for connectors and grouped shapes.
    fn rotate_handle_screen(&self, id: &str, image_rect: egui::Rect) -> Option<(egui::Pos2, egui::Pos2, egui::Pos2)> {
        let shape = self.doc_shape(id)?;
        if shape.group.is_some() {
            return None;
        }
        let scene = self.scene_shape(id)?;
        let rect = self.scene_rect_to_screen(Self::selection_scene_rect(scene), image_rect);
        let rot = (match &self.rotating {
            Some(r) if r.id == id => -r.theta,
            _ => shape.rot.to_radians(), // the model keeps the angle in degrees, clockwise on screen
        }) as f32;
        let centre = rect.center();
        let up = egui::vec2(rot.sin(), -rot.cos());
        let top = centre + up * (rect.height() / 2.0);
        Some((top, top + up * ROTATE_STEM, centre))
    }

    fn begin_pointer(&mut self, pointer: egui::Pos2, image_rect: egui::Rect) {
        let scene_pointer = self.screen_to_scene(pointer, image_rect);
        let shift = self.shift_down;
        if self.tool == Tool::Hand || self.space_down {
            // the Hand tool and Space + drag move the view and touch nothing else
            self.drag = None;
            self.preview = None;
            self.panning = true;
            return;
        }
        if shift && self.tool == Tool::Select {
            if let Some(id) = self.pick_shape(scene_pointer) {
                self.toggle_selected(&id);
                self.drag = None;
                self.preview = None;
                return;
            }
        } else {
            // pressing a shape of a multi-selection keeps the selection: the drag then moves all of them
            let keep = !self.extra_selected.is_empty() && self.pick_shape(scene_pointer).is_some_and(|id| self.all_selected().contains(&id));
            if !keep {
                self.extra_selected.clear();
            }
        }
        if let Tool::Draw(kind) = self.tool {
            let start = egui::Rect::from_min_size(scene_pointer, egui::Vec2::ZERO);
            self.drag = Some(DragState {
                visio_id: String::new(),
                mode: DragMode::Draw(kind),
                start_pointer: scene_pointer,
                end_pointer: scene_pointer,
                start_rect: start,
            });
            self.preview = Some(start);
            self.selected = None;
            self.status = "Drag on the page to draw the shape.".to_string();
            return;
        }
        if self.tool == Tool::Connector {
            match self.pick_shape_near(scene_pointer) {
                Some(id) => {
                    let (start_at, site) = self.nearest_site(&id, scene_pointer);
                    self.connect_from_site = site;
                    let start = egui::Rect::from_min_size(start_at, egui::Vec2::ZERO);
                    self.status = format!("Connector from shape {id}{}: release on another shape (on a connection point to glue it there).", site.map_or(String::new(), |n| format!(" point {n}")));
                    self.drag = Some(DragState {
                        visio_id: id,
                        mode: DragMode::Connect,
                        start_pointer: start_at,
                        end_pointer: start_at,
                        start_rect: start,
                    });
                    self.preview = Some(start);
                }
                None => self.status = "Press on a shape to start the connector.".to_string(),
            }
            return;
        }
        if let Some(selected_id) = self.selected.clone() {
            if let Some((begin, end)) = self.connector_end_points(&selected_id) {
                for (is_begin, at, other) in [(true, begin, end), (false, end, begin)] {
                    if (self.scene_to_screen(at, image_rect) - pointer).length() <= 10.0 {
                        self.conn_end = Some(ConnEndState { begin: is_begin, fixed: other });
                        let start = egui::Rect::from_min_size(at, egui::Vec2::ZERO);
                        self.drag = Some(DragState { visio_id: selected_id.clone(), mode: DragMode::ConnEnd, start_pointer: at, end_pointer: at, start_rect: start });
                        self.preview = Some(start);
                        self.status = format!("Dragging the {} of connector {selected_id}: release on a shape or connection point to glue it, elsewhere to leave it loose.", if is_begin { "start" } else { "end" });
                        return;
                    }
                }
            }
            if let Some((_, handle, centre)) = self.rotate_handle_screen(&selected_id, image_rect) {
                if (handle - pointer).length() <= 9.0 {
                    let rot0 = self.doc_shape(&selected_id).map_or(0.0, |shape| shape.rot.to_radians());
                    let a0 = (pointer.y - centre.y).atan2(pointer.x - centre.x);
                    self.rotating = Some(RotateState { id: selected_id.clone(), a0, rot0, theta: -rot0 });
                    let rect = self.scene_shape(&selected_id).map(Self::shape_scene_rect).unwrap_or(egui::Rect::NOTHING);
                    self.drag = Some(DragState { visio_id: selected_id.clone(), mode: DragMode::Rotate, start_pointer: scene_pointer, end_pointer: scene_pointer, start_rect: rect });
                    self.preview = Some(rect);
                    self.status = format!("Rotating shape {selected_id} (Shift snaps to 15 degrees)");
                    return;
                }
            }
            if let Some(shape) = self.doc_shape(&selected_id) {
                let frame_is_shape = self.scene_shape(&selected_id).is_some_and(|scene| Self::selection_scene_rect(scene) == Self::shape_scene_rect(scene));
                if shape.group.is_none() && shape.rot.abs() < 1e-6 && frame_is_shape {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(shape.x as f32, shape.y as f32),
                        egui::vec2(shape.w as f32, shape.h as f32),
                    );
                    let screen_rect = self.scene_rect_to_screen(rect, image_rect);
                    for (handle, hit) in Self::handle_rects(screen_rect).into_iter().filter(|_| Self::handles_fit(screen_rect)) {
                        if hit.expand(2.0).contains(pointer) {
                            self.drag = Some(DragState {
                                visio_id: selected_id.clone(),
                                mode: DragMode::Resize(handle),
                                start_pointer: scene_pointer,
                                end_pointer: scene_pointer,
                                start_rect: rect,
                            });
                            self.preview = Some(rect);
                            self.status = format!("Resizing shape {selected_id}");
                            return;
                        }
                    }
                }
            }
        }

        // a press on a connector's line selects the connector (a press without movement arrives here as often as in the click handler)
        if let Some(id) = self.pick_connector(scene_pointer) {
            self.selected = Some(id);
            self.drag = None;
            self.preview = None;
            return;
        }
        if let Some(id) = self.pick_shape(scene_pointer) {
            if self.extra_selected.is_empty() {
                self.selected = Some(id.clone());
            } else if self.selected.as_deref() != Some(id.as_str()) {
                // a member of the multi-selection became the primary one: the one under the pointer
                if let Some(at) = self.extra_selected.iter().position(|other| *other == id) {
                    self.extra_selected.remove(at);
                }
                if let Some(old) = self.selected.replace(id.clone()) {
                    self.extra_selected.push(old);
                }
            }
            let Some(shape) = self.doc_shape(&id) else {
                return;
            };
            if shape.group.is_some() {
                self.status =
                    "Grouped shapes are read-only in this slice; move the whole group later."
                        .to_string();
                self.drag = None;
                return;
            }
            let frame = egui::Rect::from_min_size(
                egui::pos2(shape.x as f32, shape.y as f32),
                egui::vec2(shape.w as f32, shape.h as f32),
            );
            // the outline that moves with the pointer is the painted extent (the frame of a 1-D group is a hairline)
            let rect = self.scene_shape(&id).map_or(frame, Self::selection_scene_rect);
            self.drag = Some(DragState {
                visio_id: id.clone(),
                mode: DragMode::Move,
                start_pointer: scene_pointer,
                end_pointer: scene_pointer,
                start_rect: rect,
            });
            self.preview = Some(rect);
            self.status = format!("Moving shape {id}");
        } else {
            // empty canvas: a rubber band (Shift keeps what is selected and adds to it)
            if !shift {
                self.selected = None;
                self.extra_selected.clear();
            }
            let start = egui::Rect::from_min_size(scene_pointer, egui::Vec2::ZERO);
            self.drag = Some(DragState { visio_id: String::new(), mode: DragMode::Marquee, start_pointer: scene_pointer, end_pointer: scene_pointer, start_rect: start });
            self.preview = Some(start);
            self.panning = false;
            self.status = "Drag to select the shapes inside the rectangle (Space + drag, middle button or the Pan tool move the view).".to_string();
        }
    }

    fn update_drag(&mut self, pointer: egui::Pos2, image_rect: egui::Rect) {
        let Some(drag) = &self.drag else {
            return;
        };
        let pointer = self.screen_to_scene(pointer, image_rect);
        let mut guides: (Option<f32>, Option<f32>) = (None, None);
        let rect = match drag.mode {
            DragMode::Draw(_) => egui::Rect::from_two_pos(self.snapped_point(drag.start_pointer), self.snapped_point(pointer)),
            DragMode::Connect | DragMode::Marquee | DragMode::ConnEnd => egui::Rect::from_two_pos(drag.start_pointer, pointer),
            DragMode::Rotate => {
                let start_rect = drag.start_rect;
                let shift = self.shift_down;
                if let Some(r) = self.rotating.as_mut() {
                    let centre = start_rect.center();
                    let a = (pointer.y - centre.y).atan2(pointer.x - centre.x);
                    let mut theta = -(r.rot0 + f64::from(a - r.a0));
                    if shift {
                        let step = std::f64::consts::PI / 12.0;
                        theta = (theta / step).round() * step;
                    }
                    r.theta = theta;
                }
                start_rect
            }
            DragMode::Move => {
                let (delta, found) = self.snapped_move(drag.start_rect, pointer - drag.start_pointer);
                guides = found;
                egui::Rect::from_min_size(drag.start_rect.min + delta, drag.start_rect.size())
            }
            DragMode::Resize(handle) => {
                let free = resized_rect(drag.start_rect, handle, pointer, MIN_SHAPE_PX, self.shift_down);
                // Shift keeps the proportions and, as in the web, then does not snap
                if self.shift_down {
                    free
                } else {
                    let (snapped, found) = self.snapped_resize(free, handle);
                    guides = found;
                    snapped
                }
            }
        };
        self.preview = Some(rect);
        self.snap_guide = guides;
        // the connectors glued to the shapes being moved follow live (Engine::preview_move keeps its editor between calls)
        let move_start = match &self.drag {
            Some(DragState { mode: DragMode::Move, start_rect, .. }) => Some(*start_rect),
            _ => None,
        };
        if let Some(start) = move_start {
            let delta = rect.min - start.min;
            if (delta - self.last_live_delta).length() > 1.0 {
                self.last_live_delta = delta;
                let ids = self.all_selected();
                let page = self.page_index;
                if let Ok(json) = self.engine.preview_move(page, &ids, (f64::from(delta.x) / DPI, -f64::from(delta.y) / DPI)) {
                    self.live_routes = routes_from_preview(&json, self.page_height_in);
                }
            }
        }
        if let Some(drag) = &mut self.drag {
            drag.end_pointer = pointer;
        }
    }

    fn finish_drag(&mut self, ctx: &egui::Context) {
        self.panning = false;
        self.live_routes.clear();
        self.last_live_delta = egui::Vec2::ZERO;
        self.snap_guide = (None, None);
        let Some(drag) = self.drag.take() else {
            self.preview = None;
            return;
        };
        let Some(rect) = self.preview.take() else {
            return;
        };
        if drag.mode == DragMode::Connect {
            self.commit_connector(ctx, &drag.visio_id, drag.end_pointer);
            return;
        }
        if drag.mode == DragMode::Marquee {
            self.finish_marquee(rect);
            return;
        }
        if drag.mode == DragMode::ConnEnd {
            if let Some(state) = self.conn_end.take() {
                self.commit_connector_end(ctx, &drag.visio_id, state.begin, drag.end_pointer);
            }
            return;
        }
        if drag.mode == DragMode::Rotate {
            let Some(state) = self.rotating.take() else {
                return;
            };
            if (state.theta + state.rot0).abs() < 1e-4 {
                return;
            }
            let theta = state.theta.rem_euclid(std::f64::consts::TAU);
            let result = self.engine.edit(self.page_index, 7, &state.id, theta, 0.0, 0.0, 0.0, 0);
            self.finish_edit(ctx, result, "Rotated", Some(state.id));
            return;
        }
        if let DragMode::Draw(kind) = drag.mode {
            self.commit_draw(ctx, kind, drag.start_pointer, drag.end_pointer, rect);
            return;
        }
        let changed = (rect.min - drag.start_rect.min).length() > 0.4
            || (rect.size() - drag.start_rect.size()).length() > 0.4;
        if !changed {
            return;
        }
        if let DragMode::Resize(handle) = drag.mode {
            self.commit_resize(ctx, &drag.visio_id, rect, handle);
            return;
        }
        let verb = match drag.mode {
            DragMode::Move => "Moved",
            DragMode::Resize(_) => "Resized",
            DragMode::Draw(_) | DragMode::Connect | DragMode::Rotate | DragMode::Marquee | DragMode::ConnEnd => "Drew",
        };
        let resized = matches!(drag.mode, DragMode::Resize(_));
        self.commit_rect(ctx, &drag.visio_id, rect, verb, resized);
    }

    /// A connector end was released at `end` (scene px): glued to the shape under it (at the connection point within reach, else the whole shape), or
    /// left loose at that page position: one `Engine::move_connector_end`, as the web shell's end handles.
    fn commit_connector_end(&mut self, ctx: &egui::Context, connector: &str, begin: bool, end: egui::Pos2) {
        let page = self.page_index;
        let target = self.pick_shape_near(end);
        let result = match &target {
            Some(shape) => {
                let site = self.nearest_site(shape, end).1;
                self.engine.move_connector_end(page, connector, begin, Some((shape.as_str(), site)), (0.0, 0.0))
            }
            None => self.engine.move_connector_end(page, connector, begin, None, (f64::from(end.x) / DPI, self.page_height_in - f64::from(end.y) / DPI)),
        };
        let verb = match &target {
            Some(shape) => format!("Glued the {} of the connector to shape {shape}", if begin { "start" } else { "end" }),
            None => format!("Left the {} of the connector loose", if begin { "start" } else { "end" }),
        };
        self.finish_edit(ctx, result, &verb, Some(connector.to_string()));
    }

    /// The rubber band was released: select the page-level shapes it completely encloses (a click without a band only clears the selection).
    fn finish_marquee(&mut self, band: egui::Rect) {
        if band.width() < 3.0 && band.height() < 3.0 {
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let candidates: Vec<(String, egui::Rect)> = scene
            .items
            .iter()
            .filter_map(|item| match item {
                SceneItem::Shape(shape) => {
                    let id = shape.visio_id.clone()?;
                    self.doc_shape(&id).is_some_and(|doc| doc.group.is_none()).then(|| (id, Self::selection_scene_rect(shape)))
                }
                _ => None,
            })
            .collect();
        let found = enclosed_ids(&candidates, band);
        let mut chosen: Vec<String> = self.all_selected();
        if !self.shift_down {
            chosen.clear();
        }
        for id in found {
            if !chosen.contains(&id) {
                chosen.push(id);
            }
        }
        let mut ids = chosen.into_iter();
        self.selected = ids.next();
        self.extra_selected = ids.collect();
        self.status = format!("{} shape(s) selected", self.all_selected().len());
    }

    /// A resize drag ended: the engine's resize (the same operation as the web shell: sub-shapes and text follow, glued connectors are routed again)
    /// with the edge opposite to the handle held fixed.
    fn commit_resize(&mut self, ctx: &egui::Context, id: &str, rect: egui::Rect, handle: Handle) {
        let (fx, fy) = handle.fractions();
        let (w, h) = (f64::from(rect.width()) / DPI, f64::from(rect.height()) / DPI);
        // the fixed point as fractions of the shape's own box, y UP: dragging the left edge keeps the right one, the top edge keeps the bottom one
        let ax = if fx == 0.0 { 1.0 } else if fx == 1.0 { 0.0 } else { 0.5 };
        let ay = if fy == 0.0 { 0.0 } else if fy == 1.0 { 1.0 } else { 0.5 };
        let result = self.engine.edit(self.page_index, 3, id, w, h, ax, ay, 0);
        self.finish_edit(ctx, result, "Resized", Some(id.to_string()));
    }

    /// The drag of the connector tool ended at `end`: glue a connector from shape `from` to the shape under it (one undo step).
    fn commit_connector(&mut self, ctx: &egui::Context, from: &str, end: egui::Pos2) {
        let from_site = self.connect_from_site.take();
        let Some(to) = self.pick_shape_near(end) else {
            self.status = "Release on a shape to connect to it.".to_string();
            return;
        };
        if to == from {
            self.status = "Release on a different shape.".to_string();
            return;
        }
        let to_site = self.nearest_site(&to, end).1;
        let page = self.page_index;
        match self.engine.add_connector(page, from, from_site, &to, to_site) {
            Ok(id) => {
                self.tool = Tool::Select;
                self.finish_edit(ctx, Ok(()), &format!("Connected {from} to {to}"), Some(id));
            }
            Err(error) => self.status = format!("Could not connect: {error}"),
        }
    }

    /// The drag of a draw tool ended: draw the shape through the engine (one undo step), select it and go back to the Select tool.
    fn commit_draw(&mut self, ctx: &egui::Context, kind: i32, start: egui::Pos2, end: egui::Pos2, rect: egui::Rect) {
        let is_line = kind == 6;
        let too_small = if is_line { (end - start).length() < MIN_SHAPE_PX } else { rect.width() < MIN_SHAPE_PX || rect.height() < MIN_SHAPE_PX };
        if too_small {
            self.status = "Drag a larger area to draw the shape.".to_string();
            return;
        }
        let page_h = self.page_height_in;
        let inch = |p: egui::Pos2| (p.x as f64 / DPI, page_h - p.y as f64 / DPI);
        let ((x0, y0), (x1, y1)) = if is_line { (inch(start), inch(end)) } else { (inch(rect.left_bottom()), inch(rect.right_top())) };
        let page = self.page_index;
        match self.engine.draw(page, kind, x0, y0, x1, y1) {
            Ok(id) => {
                self.tool = Tool::Select;
                self.finish_edit(ctx, Ok(()), "Drew shape", Some(id));
            }
            Err(error) => self.status = format!("Could not draw: {error}"),
        }
    }

    fn commit_rect(
        &mut self,
        ctx: &egui::Context,
        visio_id: &str,
        rect: egui::Rect,
        verb: &str,
        resized: bool,
    ) {
        if self.engine.package.is_none() {
            return;
        }
        // only a resize can make a shape too small; a move keeps its size (stencil parts like the 0.05 in SFP modules are tiny on purpose)
        if resized && (rect.width() < MIN_SHAPE_PX || rect.height() < MIN_SHAPE_PX) {
            self.status = "Shape would be too small.".to_string();
            return;
        }
        if !resized {
            if let Some(old) = self.scene_shape(visio_id).map(Self::selection_scene_rect) {
                let (dx, dy) = ((rect.min.x - old.min.x) as f64 / DPI, -((rect.min.y - old.min.y) as f64) / DPI);
                let page = self.page_index;
                let edit_timer = Timer::new("  engine.edit (move)", 0.0);
                let everyone = self.all_selected();
                let result = if everyone.len() > 1 && everyone.iter().any(|id| id == visio_id) {
                    self.engine.edit(page, 5, &everyone.join("\n"), dx, dy, 0.0, 0.0, 0)
                } else {
                    self.engine.edit(page, 2, visio_id, dx, dy, 0.0, 0.0, 0)
                };
                drop(edit_timer);
                if let Err(error) = result {
                    self.status = format!("Could not move {visio_id}: {error}");
                    return;
                }
                let changes_timer = Timer::new("  has_changes", 0.0);
                self.dirty = self.has_changes();
                drop(changes_timer);
                self.selected = Some(visio_id.to_string());
                self.build_page(ctx, false);
                self.status = format!("{verb} shape {visio_id}; save a copy to keep it.");
                return;
            }
        }
        let width = rect.width() as f64 / DPI;
        let height = rect.height() as f64 / DPI;
        let metrics = self.shape_metrics(visio_id).unwrap_or(ShapeMetrics {
            loc_ratio_x: 0.5,
            loc_ratio_y: 0.5,
            text: None,
        });
        let pin_x = rect.min.x as f64 / DPI + metrics.loc_ratio_x * width;
        let pin_y =
            self.page_height_in + metrics.loc_ratio_y * height - height - rect.min.y as f64 / DPI;
        let mut patch = ShapeCellPatch::new(visio_id)
            .set("PinX", number(pin_x))
            .set("PinY", number(pin_y))
            .set("Width", number(width))
            .set("Height", number(height))
            .set("LocPinX", number(metrics.loc_ratio_x * width))
            .set("LocPinY", number(metrics.loc_ratio_y * height));
        if resized {
            if let (Some(text), Some(old)) = (metrics.text, self.doc_shape(visio_id)) {
                let old_w = (old.w / DPI).max(1e-9);
                let old_h = (old.h / DPI).max(1e-9);
                let scale_x = width / old_w;
                let scale_y = height / old_h;
                if scale_x.is_finite() && scale_y.is_finite() {
                    patch = patch
                        .set("TxtPinX", number(text.pin_x * scale_x))
                        .set("TxtPinY", number(text.pin_y * scale_y))
                        .set("TxtWidth", number(text.width * scale_x))
                        .set("TxtHeight", number(text.height * scale_y))
                        .set("TxtLocPinX", number(text.loc_x * scale_x))
                        .set("TxtLocPinY", number(text.loc_y * scale_y));
                }
            }
        }
        let patched = match self.engine.package.as_ref() {
            Some(package) => patch_page_cells(package, self.page_index, &[patch]).map_err(|error| error.to_string()),
            None => return,
        };
        let new_bytes = match patched {
            Ok(bytes) => bytes,
            Err(error) => {
                self.status = format!("Could not write {visio_id}: {error}");
                return;
            }
        };
        if let Err(error) = self.engine.commit(new_bytes) {
            self.status = format!("Could not re-import patched package: {error}");
            return;
        }
        self.dirty = self.has_changes();
        self.selected = Some(visio_id.to_string());
        self.build_page(ctx, false);
        self.status = format!("{verb} shape {visio_id}; save a copy to keep it.");
    }

    fn undo(&mut self, ctx: &egui::Context) {
        match self.engine.undo() {
            Ok(true) => {
                self.reimport(ctx);
                self.dirty = self.has_changes();
                self.status = "Undo".to_string();
            }
            Ok(false) => {}
            Err(error) => self.status = format!("Could not undo: {error}"),
        }
    }

    fn redo(&mut self, ctx: &egui::Context) {
        match self.engine.redo() {
            Ok(true) => {
                self.reimport(ctx);
                self.dirty = self.has_changes();
                self.status = "Redo".to_string();
            }
            Ok(false) => {}
            Err(error) => self.status = format!("Could not redo: {error}"),
        }
    }

    /// The engine already holds the new document: drop the drag and draw the page again.
    fn reimport(&mut self, ctx: &egui::Context) {
        self.drag = None;
        self.preview = None;
        self.build_page(ctx, false);
    }

    fn has_changes(&self) -> bool {
        let (Some(document), Some(baseline)) = (&self.engine.document, &self.baseline) else {
            return false;
        };
        if document.pages.len() != baseline.pages.len() {
            return true;
        }
        for (page, base_page) in document.pages.iter().zip(&baseline.pages) {
            if page.shapes.len() != base_page.shapes.len() {
                return true;
            }
            for shape in &page.shapes {
                let Some(id) = shape.visio_id.as_deref() else {
                    continue;
                };
                let Some(base) = base_page
                    .shapes
                    .iter()
                    .find(|base| base.visio_id.as_deref() == Some(id))
                else {
                    return true;
                };
                if (shape.x - base.x).abs() > 1e-6
                    || (shape.y - base.y).abs() > 1e-6
                    || (shape.w - base.w).abs() > 1e-6
                    || (shape.h - base.h).abs() > 1e-6
                    || shape.label != base.label
                {
                    return true;
                }
            }
        }
        false
    }

    fn save_copy(&mut self) {
        let (Some(bytes), Some(path)) = (self.engine.export().map(|bytes| bytes.to_vec()), self.loaded_path.clone())
        else {
            return;
        };
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("drawing");
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("vsdx");
        let output = path.with_file_name(format!("{stem}-edited.{extension}"));
        match fs::write(&output, bytes.clone()) {
            Ok(()) => {
                self.baseline = self.engine.document.clone();
                self.dirty = false;
                self.status = format!("Saved copy to {}", output.display());
            }
            Err(error) => {
                self.status = format!("Could not save {}: {error}", output.display());
            }
        }
    }

    fn shape_metrics(&self, visio_id: &str) -> Option<ShapeMetrics> {
        let package = self.engine.package.as_ref()?;
        let pages = package.pages().ok()?;
        let part = &pages.get(self.page_index)?.part;
        let shapes = package.resolve_page_shapes(part).ok()?;
        let mut metrics = None;
        for node in &shapes {
            node.walk(&mut |node| {
                if node.id != visio_id || metrics.is_some() {
                    return;
                }
                let number = |name: &str| {
                    node.cells
                        .get(name)
                        .and_then(|cell| cell.value.as_deref())
                        .and_then(|value| value.parse::<f64>().ok())
                };
                let width = number("Width").unwrap_or(0.0);
                let height = number("Height").unwrap_or(0.0);
                let loc_x = number("LocPinX").unwrap_or(width / 2.0);
                let loc_y = number("LocPinY").unwrap_or(height / 2.0);
                let ratio_x = if width > 1e-9 {
                    (loc_x / width).clamp(0.0, 1.0)
                } else {
                    0.5
                };
                let ratio_y = if height > 1e-9 {
                    (loc_y / height).clamp(0.0, 1.0)
                } else {
                    0.5
                };
                let text = number("TxtWidth").map(|text_width| TextMetrics {
                    pin_x: number("TxtPinX").unwrap_or(text_width / 2.0),
                    pin_y: number("TxtPinY").unwrap_or(height / 2.0),
                    width: text_width,
                    height: number("TxtHeight").unwrap_or(0.25),
                    loc_x: number("TxtLocPinX").unwrap_or(text_width / 2.0),
                    loc_y: number("TxtLocPinY").unwrap_or(0.125),
                });
                metrics = Some(ShapeMetrics {
                    loc_ratio_x: ratio_x,
                    loc_ratio_y: ratio_y,
                    text,
                });
            });
        }
        metrics
    }

    fn load_from_input(&mut self, ctx: &egui::Context) {
        let path = self.path_input.trim().trim_matches('"').to_string();
        if path.is_empty() {
            self.status = "Enter a VSDX/VSTX path first.".to_string();
        } else if self.dirty {
            self.pending_open = Some(PendingOpen::Load(path));
        } else {
            self.load(ctx, Path::new(&path));
        }
    }

    fn confirm_pending(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_open.take() else {
            return;
        };
        match pending {
            PendingOpen::New => self.new_drawing(ctx),
            PendingOpen::Reload => self.reload(ctx),
            PendingOpen::Load(path) => self.load(ctx, Path::new(&path)),
        }
    }
}

impl LayoutCraftApp {
    fn open_dialog(&mut self, ctx: &egui::Context) {
        let Some(path) = file_dialog::open() else {
            return;
        };
        self.path_input = path.display().to_string();
        if self.dirty {
            self.pending_open = Some(PendingOpen::Load(self.path_input.clone()));
        } else {
            self.load(ctx, &path);
        }
    }

    fn save_as_dialog(&mut self) {
        let Some(bytes) = self.engine.export().map(|bytes| bytes.to_vec()) else {
            return;
        };
        let default_name = self
            .loaded_path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("drawing.vsdx")
            .to_string();
        let Some(path) = file_dialog::save_as(&default_name) else {
            return;
        };
        match fs::write(&path, bytes) {
            Ok(()) => {
                self.loaded_path = Some(path.clone());
                self.path_input = path.display().to_string();
                self.baseline = self.engine.document.clone();
                self.dirty = false;
                self.status = format!("Saved {}", path.display());
            }
            Err(error) => {
                self.status = format!("Could not save {}: {error}", path.display());
            }
        }
    }

    fn open_text_editor(&mut self) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let Some(shape) = self.doc_shape(&id) else {
            return;
        };
        if shape.group.is_some() {
            self.status = "Grouped shapes are read-only in this slice.".to_string();
            return;
        }
        let text = self
            .scene_shape(&id)
            .and_then(shape_text)
            .unwrap_or_else(|| shape.label.clone());
        let (bold, italic, underline) = (shape.bold.unwrap_or(false), shape.italic.unwrap_or(false), shape.underline.unwrap_or(false));
        // the model keeps the size in px (96 per inch); the editor shows points
        let font_pt = (shape.font_size.unwrap_or(12.0 * 96.0 / 72.0) * 72.0 / 96.0 * 10.0).round() / 10.0;
        self.text_editor = Some(TextEditor { visio_id: id, text, bold, italic, underline, font_pt, initial: (bold, italic, underline, font_pt) });
    }

    /// Write the text (op 8) and, when given, the character format (op 10 style JSON) as ONE undo step.
    fn apply_text(&mut self, ctx: &egui::Context, visio_id: &str, text: &str, format: Option<&str>) {
        let page = self.page_index;
        let depth = self.engine.undo_depth();
        let mut result = self.engine.edit(page, 8, &format!("{visio_id}\n{text}"), 0.0, 0.0, 0.0, 0.0, 0);
        if result.is_ok() {
            if let Some(json) = format {
                result = self.engine.edit(page, 10, &format!("{visio_id}\n{json}"), 0.0, 0.0, 0.0, 0.0, 0);
            }
            self.engine.collapse_history(depth);
        }
        self.finish_edit(ctx, result, "Updated text", Some(visio_id.to_string()));
    }

    fn delete_selected(&mut self, ctx: &egui::Context) {
        let ids = self.all_selected();
        if ids.is_empty() {
            return;
        }
        if ids.iter().any(|id| self.doc_shape(id).is_some_and(|shape| shape.group.is_some())) {
            self.status = "A member of a group cannot be deleted alone: select the group.".to_string();
            return;
        }
        let page = self.page_index;
        // op 6 deletes several shapes as ONE undo step (the web's deleteShapes); op 4 is the single-shape form
        let result = if ids.len() == 1 { self.engine.edit(page, 4, &ids[0], 0.0, 0.0, 0.0, 0.0, 0) } else { self.engine.edit(page, 6, &ids.join("\n"), 0.0, 0.0, 0.0, 0.0, 0) };
        self.extra_selected.clear();
        self.finish_edit(ctx, result, if ids.len() == 1 { "Deleted shape" } else { "Deleted shapes" }, None);
    }

    /// Ctrl+A: every page-level shape (not a group member), the first one primary.
    fn select_all(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let mut ids: Vec<String> = Vec::new();
        for item in &scene.items {
            if let SceneItem::Shape(shape) = item {
                if let Some(id) = shape.visio_id.as_deref() {
                    if self.doc_shape(id).is_some_and(|doc| doc.group.is_none()) && !ids.iter().any(|other| other == id) {
                        ids.push(id.to_string());
                    }
                }
            }
        }
        let mut ids = ids.into_iter();
        self.selected = ids.next();
        self.extra_selected = ids.collect();
        self.status = format!("{} shape(s) selected", self.all_selected().len());
    }

    /// An arrow key: move the selection by (dx, dy) inches (y up), one undo step; the web's `moveMembers`.
    fn nudge_selected(&mut self, ctx: &egui::Context, dx: f64, dy: f64) {
        let ids = self.all_selected();
        if ids.is_empty() || ids.iter().any(|id| self.doc_shape(id).is_none_or(|shape| shape.group.is_some())) {
            return;
        }
        let page = self.page_index;
        let result = if ids.len() == 1 { self.engine.edit(page, 2, &ids[0], dx, dy, 0.0, 0.0, 0) } else { self.engine.edit(page, 5, &ids.join("\n"), dx, dy, 0.0, 0.0, 0) };
        self.finish_edit(ctx, result, "Moved", self.selected.clone());
    }

    /// The icon of the web ribbon (`web/assets/icons`) as an egui texture, drawn once at 36 px in the light text colour.
    fn icon_texture(&mut self, ctx: &egui::Context, name: &str) -> Option<egui::TextureHandle> {
        if let Some(texture) = self.ribbon_textures.get(name) {
            return Some(texture.clone());
        }
        let svg = ribbon::icon_svg(name, "#e6e6e6")?;
        let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).ok()?;
        let size = 36u32;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)?;
        let scale = size as f32 / tree.size().width().max(1.0);
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
        let image = egui::ColorImage::from_rgba_premultiplied([size as usize, size as usize], pixmap.data());
        let texture = ctx.load_texture(format!("ribbon-{name}"), image, egui::TextureOptions::LINEAR);
        self.ribbon_textures.insert(name.to_string(), texture.clone());
        Some(texture)
    }

    /// The ribbon: tab strip and the groups of the chosen tab, from the web ribbon's data (`ribbon.rs`). Commands the native app has are
    /// live, the others are greyed out with a tooltip.
    fn draw_ribbon(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        let ctx = ui.ctx().clone();
        // like the web: below 1750 px the buttons that have an icon and a text show only the icon
        self.ribbon_compact = ribbon::is_compact(ctx.content_rect().width());
        // Ctrl+F1 collapses / expands the ribbon (web/keyboard.js)
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::F1)) {
            self.ribbon_collapsed = !self.ribbon_collapsed;
            self.ribbon_peek = false;
        }
        // the quick access toolbar of the web ribbon: save, undo, redo
        ui.horizontal(|ui| {
            for id in ["save", "undo", "redo"] {
                self.ribbon_button(ui, &ctx, id, action);
            }
        });
        ui.horizontal(|ui| {
            for index in 0..self.ribbon.tabs.len() {
                let name = self.ribbon.tabs[index].name.clone();
                if ui.selectable_label(self.ribbon_tab == index, name).clicked() {
                    if self.ribbon_collapsed && self.ribbon_peek && self.ribbon_tab == index {
                        self.ribbon_peek = false;
                    } else {
                        self.ribbon_tab = index;
                        // a collapsed ribbon shows the panel of the clicked tab as an overlay until a command is used
                        self.ribbon_peek = self.ribbon_collapsed;
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (glyph, tip) = if self.ribbon_collapsed { ("\u{2304}", "Expand the ribbon (Ctrl+F1)") } else { ("\u{2303}", "Collapse the ribbon (Ctrl+F1)") };
                if ui.button(glyph).on_hover_text(tip).clicked() {
                    self.ribbon_collapsed = !self.ribbon_collapsed;
                    self.ribbon_peek = false;
                }
            });
        });
        let tab = self.ribbon.tabs[self.ribbon_tab.min(self.ribbon.tabs.len() - 1)].clone();
        if !self.ribbon_collapsed {
            egui::ScrollArea::horizontal().id_salt("ribbon-scroll").show(ui, |ui| self.draw_ribbon_groups(ui, &ctx, &tab, action));
        } else if self.ribbon_peek {
            let below_tabs = ui.cursor().min;
            let width = ctx.content_rect().width() - 16.0;
            let area = egui::Area::new(egui::Id::new("ribbon-peek")).order(egui::Order::Foreground).fixed_pos(below_tabs).show(&ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(width);
                    egui::ScrollArea::horizontal().id_salt("ribbon-peek-scroll").show(ui, |ui| self.draw_ribbon_groups(ui, &ctx, &tab, action));
                });
            });
            let rect = area.response.rect;
            let outside = ctx.input(|input| input.pointer.any_pressed() && input.pointer.interact_pos().is_some_and(|p| p.y > below_tabs.y && !rect.contains(p)));
            if action.is_some() || outside || ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.ribbon_peek = false;
            }
        }
        ui.add_space(2.0);
    }

    /// The groups of one tab side by side, each with its items in a grid of two rows (column after column, like the web's `grid-auto-flow: column`
    /// over two rows), a thin line between the groups and the group name under the items.
    fn draw_ribbon_groups(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, tab: &ribbon::Tab, action: &mut Option<Action>) {
        enum Cell {
            Select,
            Command(String),
            Menu(String, Vec<String>),
        }
        ui.horizontal_top(|ui| {
            if tab.groups.is_empty() {
                ui.label(egui::RichText::new(tab.note.clone().unwrap_or_default()).weak());
            }
            for group in &tab.groups {
                let mut cells: Vec<Cell> = Vec::new();
                if tab.name == "Home" && group.name == "Tools" {
                    cells.push(Cell::Select);
                }
                for item in &group.items {
                    match item {
                        ribbon::Item::Command { id } => cells.push(Cell::Command(id.clone())),
                        ribbon::Item::Menu { menu, ids } => cells.push(Cell::Menu(menu.clone(), ids.clone())),
                    }
                }
                let group_ui = ui.vertical(|ui| {
                    let items = ui.horizontal_top(|ui| {
                        for column in 0..ribbon::grid_columns(cells.len()) {
                            ui.vertical(|ui| {
                                for row in 0..2 {
                                    let Some(cell) = cells.get(ribbon::grid_index(column, row)) else { continue };
                                    match cell {
                                        Cell::Select => {
                                            let selecting = self.tool == Tool::Select;
                                            if ui.add(egui::Button::new("Select").selected(selecting)).on_hover_text("Select, move and resize shapes").clicked() {
                                                self.tool = Tool::Select;
                                            }
                                        }
                                        Cell::Command(id) => ui.horizontal(|ui| self.ribbon_button(ui, ctx, id, action)).inner,
                                        Cell::Menu(label, ids) => {
                                            // the Font popup stays open while A- / A+ and B / I / U are clicked again and again; the others close after a click
                                            let behavior = if label == "Font" { egui::PopupCloseBehavior::CloseOnClickOutside } else { egui::PopupCloseBehavior::CloseOnClick };
                                            egui::containers::menu::MenuButton::new({
                                                let title = egui::RichText::new(format!("{label} \u{25be}"));
                                                if label == "Font" && self.selected_font().is_some_and(|(_, replaced)| replaced.is_some()) { title.color(egui::Color32::from_rgb(255, 90, 90)) } else { title }
                                            }).config(egui::containers::menu::MenuConfig::new().close_behavior(behavior)).ui(ui, |ui| {
                                                // the popup lists the commands with their names
                                                let compact = std::mem::replace(&mut self.ribbon_compact, false);
                                                for id in ids {
                                                    ui.horizontal(|ui| self.ribbon_button(ui, ctx, id, action));
                                                }
                                                self.ribbon_compact = compact;
                                            });
                                        }
                                    }
                                }
                            });
                        }
                    });
                    // the group name centred under the items (the label takes the width of the items, not the whole row)
                    let width = items.response.rect.width().max(40.0);
                    ui.allocate_ui_with_layout(egui::vec2(width, 14.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(&group.name).small().weak());
                    });
                });
                // a thin line of the group's height (ui.separator() would fill the whole available height and grow every frame)
                let rect = group_ui.response.rect;
                ui.add_space(4.0);
                let x = ui.cursor().left();
                ui.painter().line_segment([egui::pos2(x, rect.top() + 2.0), egui::pos2(x, rect.bottom() - 2.0)], ui.visuals().widgets.noninteractive.bg_stroke);
                ui.add_space(5.0);
            }
            if tab.name == "File" {
                self.ribbon_file_group(ui, action);
            }
        });
    }

    /// The File tab's extra group: the path of a drawing to load, Reload, and the save commands (the web has them in its own File dialogs).
    fn ribbon_file_group(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        ui.group(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.path_input).hint_text(r"path\to\drawing.vsdx").desired_width(240.0));
                    if ui.button("Load").clicked() {
                        *action = Some(Action::Load);
                    }
                    if ui.add_enabled(self.loaded_path.is_some(), egui::Button::new("Reload")).clicked() {
                        *action = Some(Action::Reload);
                    }
                    if ui.add_enabled(self.loaded_path.is_some(), egui::Button::new("Save copy")).clicked() {
                        *action = Some(Action::SaveCopy);
                    }
                    if ui.add_enabled(self.engine.export().is_some(), egui::Button::new("Save as…")).clicked() {
                        *action = Some(Action::SaveAs);
                    }
                });
                ui.label(egui::RichText::new("Open path").small().weak());
            });
        });
    }

    fn ribbon_button(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, id: &str, action: &mut Option<Action>) {
        const DRAW_ICONS: [(&str, &str); 7] = [
            ("draw-rectangle", "Rectangle"),
            ("draw-rounded", "Rounded Rectangle"),
            ("draw-ellipse", "Ellipse"),
            ("draw-diamond", "Diamond"),
            ("draw-triangle", "Triangle"),
            ("draw-textbox", "Text Box"),
            ("draw-line", "Line"),
        ];
        if id == "drawTools" {
            for (kind, (icon, name)) in DRAW_ICONS.iter().enumerate() {
                let selected = self.tool == Tool::Draw(kind as i32);
                let response = match self.icon_texture(ctx, icon) {
                    Some(texture) => ui.add(egui::Button::image(egui::Image::new(egui::load::SizedTexture::new(texture.id(), egui::vec2(18.0, 18.0)))).selected(selected)),
                    None => ui.add(egui::Button::new(*name).selected(selected)),
                };
                if response.on_hover_text(*name).clicked() {
                    self.tool = if selected { Tool::Select } else { Tool::Draw(kind as i32) };
                }
            }
            return;
        }
        if id == "gridStep" {
            const STEPS: [(f64, &str); 4] = [(0.125, "1/8 in"), (0.25, "1/4 in"), (0.5, "1/2 in"), (1.0, "1 in")];
            let current = STEPS.iter().find(|(step, _)| (step - self.grid_step).abs() < 1e-9).map_or("custom", |(_, name)| *name);
            egui::ComboBox::from_id_salt("grid-step").selected_text(current).width(70.0).show_ui(ui, |ui| {
                for (step, name) in STEPS {
                    ui.selectable_value(&mut self.grid_step, step, name);
                }
            });
            return;
        }
        if id == "pageTools" {
            for page_id in ["pageAdd", "pageDup", "pageRename", "pageDel", "pageLeft", "pageRight"] {
                self.ribbon_button(ui, ctx, page_id, action);
            }
            return;
        }
        if matches!(id, "p:bold" | "p:strike" | "p:alignL" | "p:indLeft" | "p:lineSp" | "p:fontSize" | "p:fontFamily" | "p:fillColor" | "p:lineColor" | "p:shadow") {
            self.ribbon_format_controls(ui, id, action);
            return;
        }
        let native = ribbon::native(id);
        let mut enabled = native.is_some_and(|command| !command.needs_document() || self.texture.is_some());
        match native {
            Some(ribbon::Native::Undo) => enabled &= self.engine.undo_depth() > 0,
            Some(ribbon::Native::Redo) => enabled &= self.engine.can_redo(),
            _ => {}
        }
        let label = ribbon::label(id);
        let icon = self.ribbon.icons.get(id).cloned();
        let selected = (native == Some(ribbon::Native::Connector) && self.tool == Tool::Connector) || (native == Some(ribbon::Native::TextBox) && self.tool == Tool::Draw(5)) || (native == Some(ribbon::Native::Hand) && self.tool == Tool::Hand)
            || (native == Some(ribbon::Native::SnapGrid) && self.snap_grid)
            || (native == Some(ribbon::Native::SnapShapes) && self.snap_shapes)
            || (native == Some(ribbon::Native::SnapGuides) && self.snap_guides)
            || (native == Some(ribbon::Native::ShowGrid) && self.show_grid);
        let compact = self.ribbon_compact;
        let response = match icon.and_then(|name| self.icon_texture(ctx, &name)) {
            Some(texture) => {
                let image = egui::Image::new(egui::load::SizedTexture::new(texture.id(), egui::vec2(18.0, 18.0)));
                let button = if compact { egui::Button::image(image) } else { egui::Button::image_and_text(image, label.clone()) };
                ui.add_enabled(enabled, button.selected(selected))
            }
            None => ui.add_enabled(enabled, egui::Button::new(label.clone()).selected(selected)),
        };
        let response = if enabled {
            response.on_hover_text(label)
        } else {
            response.on_disabled_hover_text(format!("{label} (not in the native app yet)"))
        };
        if enabled && response.clicked() {
            if let Some(native) = native {
                match native {
                    ribbon::Native::Connector => self.tool = if selected { Tool::Select } else { Tool::Connector },
                    ribbon::Native::TextBox => self.tool = if selected { Tool::Select } else { Tool::Draw(5) },
                    ribbon::Native::Hand => self.tool = if selected { Tool::Select } else { Tool::Hand },
                    ribbon::Native::SnapGrid => self.snap_grid = !self.snap_grid,
                    ribbon::Native::SnapShapes => self.snap_shapes = !self.snap_shapes,
                    ribbon::Native::SnapGuides => self.snap_guides = !self.snap_guides,
                    ribbon::Native::ShowGrid => self.show_grid = !self.show_grid,
                    other => *action = Some(Self::action_for(other)),
                }
            }
        }
    }

    /// The Font and Shape Styles controls of the Home tab for the selected shapes: B / I / U toggles, A- / A+ (one point), and fill / line colour
    /// swatch menus; every click is one `Engine::edit` op 10 (see `style_selected`).
    fn ribbon_format_controls(&mut self, ui: &mut egui::Ui, id: &str, action: &mut Option<Action>) {
        let primary = self.selected.as_deref().and_then(|selected| self.doc_shape(selected)).cloned();
        let enabled = primary.is_some() && self.texture.is_some();
        match id {
            "p:bold" => {
                let current = primary.as_ref().map(|shape| (shape.bold.unwrap_or(false), shape.italic.unwrap_or(false), shape.underline.unwrap_or(false))).unwrap_or_default();
                for (label, key, on) in [("B", "bold", current.0), ("I", "italic", current.1), ("U", "underline", current.2)] {
                    let text = match key {
                        "bold" => egui::RichText::new(label).strong(),
                        "italic" => egui::RichText::new(label).italics(),
                        _ => egui::RichText::new(label).underline(),
                    };
                    if ui.add_enabled(enabled, egui::Button::new(text).selected(on)).on_hover_text(key).clicked() {
                        *action = Some(Action::StyleSelected(serde_json::json!({ key: !on }).to_string()));
                    }
                }
            }
            "p:strike" => {
                let on = enabled && self.selection_struck();
                if ui.add_enabled(enabled, egui::Button::new(egui::RichText::new("Strike").strikethrough()).selected(on)).on_hover_text("Strikethrough of the whole text").clicked() {
                    *action = Some(Action::FormatText(serde_json::json!({ "cells": { "Strikethru": if on { "0" } else { "1" } } })));
                }
            }
            "p:alignL" => {
                let current = primary.as_ref().and_then(|shape| shape.align.clone());
                for (label, model, cell, tip) in [("Left", "left", "0", "Align text left"), ("Centre", "center", "1", "Centre text"), ("Right", "right", "2", "Align text right"), ("Justify", "justify", "3", "Justify text")] {
                    if ui.add_enabled(enabled, egui::Button::new(label).selected(current.as_deref() == Some(model))).on_hover_text(tip).clicked() {
                        *action = Some(Action::FormatText(serde_json::json!({ "para": { "HorzAlign": cell } })));
                    }
                }
            }
            "p:indLeft" => {
                ui.add_enabled_ui(enabled, |ui| {
                    ui.menu_button("Indent \u{25be}", |ui| {
                        for inches in INDENTS_IN {
                            if ui.button(format!("{inches} in")).clicked() {
                                *action = Some(Action::FormatText(indent_spec(inches)));
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("Left indent of the whole text");
                });
            }
            "p:lineSp" => {
                ui.add_enabled_ui(enabled, |ui| {
                    ui.menu_button("Line spacing \u{25be}", |ui| {
                        for multiple in LINE_SPACINGS {
                            if ui.button(format!("{multiple}")).clicked() {
                                *action = Some(Action::FormatText(line_spacing_spec(multiple)));
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("Line spacing of the whole text, in lines");
                });
            }
            "p:shadow" => {
                let on = primary.as_ref().is_some_and(|shape| shape.shadow.is_some());
                if ui.add_enabled(enabled, egui::Button::new("Shadow").selected(on)).on_hover_text("Shadow of the selected shapes").clicked() {
                    *action = Some(Action::StyleSelected(shadow_style(!on)));
                }
            }
            "p:fontFamily" => {
                let (name, replaced) = self.selected_font().unwrap_or_default();
                let label = if name.is_empty() { "Font family".to_string() } else { name.clone() };
                let mut title = egui::RichText::new(format!("{label} \u{25be}"));
                if replaced.is_some() {
                    title = title.color(egui::Color32::from_rgb(255, 90, 90));
                }
                let tip = match &replaced {
                    Some(found) => format!("{} is not installed here: {} is drawn instead. The file keeps {}.", found.requested, found.used, found.requested),
                    None => "Font family of the selected shapes".to_string(),
                };
                let families = self.font_list.get_or_insert_with(installed_font_families).clone();
                let filter = &mut self.font_filter;
                ui.add_enabled_ui(enabled, |ui| {
                    ui.menu_button(title, |ui| {
                        ui.add(egui::TextEdit::singleline(filter).hint_text("Search fonts").desired_width(200.0));
                        let needle = filter.to_lowercase();
                        let shown: Vec<&String> = families.iter().filter(|family| needle.is_empty() || family.to_lowercase().contains(&needle)).collect();
                        egui::ScrollArea::vertical().max_height(280.0).show_rows(ui, 20.0, shown.len(), |ui, rows| {
                            for family in &shown[rows] {
                                let chosen = family.eq_ignore_ascii_case(&name);
                                if ui.selectable_label(chosen, family.as_str()).clicked() {
                                    *action = Some(Action::StyleSelected(serde_json::json!({ "fontFamily": family }).to_string()));
                                    ui.close();
                                }
                            }
                        });
                        if shown.is_empty() {
                            ui.label("No installed font matches.");
                        }
                    })
                    .response
                    .on_hover_text(tip);
                });
            }
            "p:fontSize" => {
                // the model keeps the size in px (96 per inch); the file and the engine speak points
                let points = primary.as_ref().and_then(|shape| shape.font_size).map(|px| (px * 72.0 / 96.0 * 10.0).round() / 10.0).unwrap_or(12.0);
                for (label, delta) in [("A-", -1.0), ("A+", 1.0)] {
                    if ui.add_enabled(enabled, egui::Button::new(label)).on_hover_text(format!("Font size {points} pt")).clicked() {
                        *action = Some(Action::StyleSelected(serde_json::json!({ "fontPt": (points + delta).max(1.0) }).to_string()));
                    }
                }
            }
            _ => {
                let (label, key) = if id == "p:fillColor" { ("Fill ▾", "fill") } else { ("Line ▾", "line") };
                ui.add_enabled_ui(enabled, |ui| {
                    ui.menu_button(label, |ui| {
                        const SWATCHES: [[u8; 3]; 12] = [
                            [0, 0, 0], [255, 255, 255], [128, 128, 128], [192, 0, 0], [255, 128, 0], [255, 192, 0],
                            [0, 160, 0], [0, 128, 128], [0, 120, 215], [0, 32, 96], [112, 48, 160], [240, 240, 240],
                        ];
                        ui.horizontal_wrapped(|ui| {
                            ui.set_max_width(150.0);
                            for [r, g, b] in SWATCHES {
                                let swatch = egui::Button::new("    ").fill(egui::Color32::from_rgb(r, g, b));
                                if ui.add(swatch).on_hover_text(hex([r, g, b])).clicked() {
                                    *action = Some(Action::StyleSelected(serde_json::json!({ key: hex([r, g, b]) }).to_string()));
                                    ui.close();
                                }
                            }
                            if ui.button("none").clicked() {
                                *action = Some(Action::StyleSelected(serde_json::json!({ key: "none" }).to_string()));
                                ui.close();
                            }
                        });
                    });
                });
            }
        }
    }

    /// `all_struck` of the selection, remembered until the page is rebuilt or the selection changes (it reads the file's XML).
    fn selection_struck(&mut self) -> bool {
        let ids = self.all_selected();
        let key = ids.join("\n");
        match &self.struck_cache {
            Some((known, struck)) if *known == key => *struck,
            _ => {
                let struck = all_struck(&self.engine, self.page_index, &ids);
                self.struck_cache = Some((key, struck));
                struck
            }
        }
    }

    /// Format the whole text of every selected shape (one undo step).
    fn format_selected_text(&mut self, ctx: &egui::Context, spec: &serde_json::Value) {
        let ids = self.all_selected();
        let Some(primary) = self.selected.clone() else {
            self.status = "Select a shape first.".to_string();
            return;
        };
        let page = self.page_index;
        let result = format_whole_text(&mut self.engine, page, &ids, spec).map(|_| ());
        self.finish_edit(ctx, result, "Formatted text", Some(primary));
    }

    /// Apply an `Engine::edit` op 10 style JSON to every selected shape (one undo step).
    fn style_selected(&mut self, ctx: &egui::Context, json: &str) {
        let ids = self.all_selected();
        let Some(primary) = self.selected.clone() else {
            self.status = "Select a shape first.".to_string();
            return;
        };
        let page = self.page_index;
        let result = self.engine.edit(page, 10, &format!("{}\n{json}", ids.join("\n")), 0.0, 0.0, 0.0, 0.0, 0);
        self.finish_edit(ctx, result, "Changed style", Some(primary));
    }

    fn action_for(native: ribbon::Native) -> Action {
        use ribbon::Native as N;
        match native {
            N::Undo => Action::Undo,
            N::Redo => Action::Redo,
            N::SaveAs => Action::SaveAs,
            N::Copy => Action::Copy,
            N::Cut => Action::Cut,
            N::Paste => Action::Paste,
            N::Duplicate => Action::Duplicate,
            N::Delete => Action::DeleteSelected,
            N::Group => Action::Arrange(ArrangeCmd::Group),
            N::Ungroup => Action::Arrange(ArrangeCmd::Ungroup),
            N::Order(op) => Action::Arrange(ArrangeCmd::Order(op)),
            N::RotateQuarter => Action::Arrange(ArrangeCmd::RotateQuarter),
            N::RotateLeft => Action::Arrange(ArrangeCmd::RotateLeft),
            N::Flip(vertical) => Action::Arrange(ArrangeCmd::Flip(vertical)),
            N::SameSize(width, height) => Action::Arrange(ArrangeCmd::SameSize(width, height)),
            N::ShapeOp(op) => Action::Arrange(ArrangeCmd::Shapes(op)),
            N::Align(mode) => Action::Arrange(ArrangeCmd::Align(mode)),
            N::Distribute(vertical) => Action::Arrange(ArrangeCmd::Distribute(vertical)),
            N::PageAdd => Action::PageAdd,
            N::PageDuplicate => Action::PageDuplicate,
            N::PageRename => Action::PageRename,
            N::PageDelete => Action::PageDelete,
            N::PageMove(direction) => Action::PageMove(direction),
            N::Fit => Action::Fit,
            N::ActualSize => Action::ActualSize,
            N::ZoomIn => Action::ZoomIn,
            N::ZoomOut => Action::ZoomOut,
            N::NewDrawing => Action::NewDrawing,
            N::OpenFile => Action::OpenDialog,
            N::DocProps => Action::DocProps,
            N::HelpShortcuts => Action::HelpShortcuts,
            N::Find => Action::Find,
            N::HelpAbout => Action::HelpAbout,
            N::HelpMcp => Action::HelpMcp,
            N::PageSetup => Action::PageSetup,
            N::Comments => Action::Comments,
            N::CheckDiagram => Action::CheckDiagram,
            N::Compare => Action::Compare,
            N::Reports => Action::Reports,
            N::Borders => Action::Borders,
            N::Export => Action::Export,
            N::ShapesPane => Action::ShapesPane,
            // the text box is a tool (handled by the caller)
            N::TextBox => Action::Fit,
            // the connector tool is a tool, not an action (handled by the caller)
            N::Connector => Action::Fit,
            // the Pan tool is a tool too
            N::Hand => Action::Fit,
            // the snap switches and the grid step are state, handled where the button is drawn
            N::SnapGrid | N::SnapShapes | N::SnapGuides | N::GridStep | N::ShowGrid => Action::Fit,
        }
    }

    /// The page names of the open drawing.
    fn page_names(&self) -> Vec<String> {
        self.engine.document().map(|document| document.pages.iter().map(|page| page.name.clone()).collect()).unwrap_or_default()
    }

    /// Review > Reports: the rows of the inventory (this page, or every page) and the Shape Data columns.
    fn refresh_report(&mut self) {
        let model = self.engine.json().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
        let names = self.page_names();
        if let Some(model) = model {
            let (rows, cols) = layoutcraft_engine::inventory::inventory_data(&model, &names, self.page_index, self.report_all);
            self.report_rows = rows;
            self.report_cols = cols;
        }
    }

    /// The report columns that are ticked: Shape ID, Text and the Shape Data columns by default; Page only when asked.
    fn report_columns(&self) -> Vec<String> {
        let mut keys: Vec<String> = vec!["id".to_string(), "text".to_string()];
        if self.report_all {
            keys.push("page".to_string());
        }
        keys.extend(self.report_cols.iter().map(|c| c.key.clone()));
        keys.into_iter().filter(|k| self.report_override.get(k).copied().unwrap_or(k != "page")).collect()
    }

    /// Review > Compare File: choose a drawing, compare it with the open one (the open drawing is "new", the file "old") and show the list.
    fn compare_dialog(&mut self) {
        let Some(path) = file_dialog::open() else {
            return;
        };
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let result = fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| {
            let mut other = layoutcraft_engine::Engine::new();
            other.open(bytes).map_err(|e| e.to_string())?;
            self.engine.compare_with(&other)
        });
        match result {
            Ok(diffs) => {
                self.compare_name = name;
                self.compare_diffs = diffs;
                self.compare_open = true;
            }
            Err(error) => self.status = format!("Cannot compare with {name}: {error}"),
        }
    }

    /// Remember a stencil the engine opened (`{index, masters: [{id, name, ..}]}`) under `name`.
    fn register_stencil(&mut self, name: &str, json: &str) -> Result<(), String> {
        let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let index = v["index"].as_u64().ok_or("no stencil index")? as usize;
        if self.stencils.iter().any(|s| s.index == index) {
            return Ok(());
        }
        let masters: Vec<(String, String)> = v["masters"]
            .as_array()
            .map_or_else(Vec::new, |list| {
                list.iter()
                    .map(|m| {
                        let id = match &m["id"] {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        (id, m["name"].as_str().unwrap_or("").to_string())
                    })
                    .collect()
            });
        self.stencils.push(StencilEntry { index, name: name.to_string(), masters, path: None });
        Ok(())
    }

    /// Write the paths of the opened stencil files, so the next session opens them again.
    fn remember_stencils(&self) {
        let Some(file) = stencil_memory_file() else { return };
        let text: String = self.stencils.iter().filter_map(|s| s.path.as_ref()).map(|p| format!("{}
", p.display())).collect();
        if let Some(dir) = file.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(file, text);
    }

    /// Open again the stencils of the last session (files that no longer exist are skipped).
    fn restore_stencils(&mut self) {
        let Some(file) = stencil_memory_file() else { return };
        let Ok(text) = fs::read_to_string(file) else { return };
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let path = std::path::PathBuf::from(line.trim());
            let name = path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Stencil".to_string());
            let opened = fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| self.engine.stencil_open(bytes)).and_then(|json| self.register_stencil(&name, &json));
            match opened {
                Ok(()) => {
                    if let Some(entry) = self.stencils.last_mut() {
                        entry.path = Some(path);
                    }
                }
                Err(error) => self.status = format!("Could not reopen the stencil {}: {error}", path.display()),
            }
        }
    }

    /// The engine's built-in "Basic shapes" stencil goes first in the pane.
    fn open_builtin_stencil(&mut self) {
        let result = self.engine.stencil_builtin().and_then(|json| self.register_stencil("Basic shapes", &json));
        if let Err(error) = result {
            self.status = format!("Could not open the built-in shapes: {error}");
        }
    }

    /// File dialog + `Engine::stencil_open` + the pane entry.
    fn open_stencil_dialog(&mut self) {
        let Some(path) = file_dialog::open_stencil() else {
            return;
        };
        let name = path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Stencil".to_string());
        let result = fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| self.engine.stencil_open(bytes)).and_then(|json| self.register_stencil(&name, &json));
        if result.is_ok() {
            if let Some(entry) = self.stencils.last_mut() {
                entry.path = Some(path.clone());
            }
            self.remember_stencils();
        }
        self.status = match result {
            Ok(()) => format!("Opened the stencil {name}: click a shape, then click on the page to place it."),
            Err(error) => format!("Could not open {}: {error}", path.display()),
        };
    }

    /// The thumbnail of a master as a texture (36 px was the ribbon; 56 px here), rendered once when the cell is first shown.
    fn stencil_thumb_texture(&mut self, ctx: &egui::Context, index: usize, master: &str) -> Option<egui::TextureHandle> {
        let key = (index, master.to_string());
        if let Some(t) = self.stencil_textures.get(&key) {
            return Some(t.clone());
        }
        let svg = self.engine.stencil_thumb(index, master).ok()?;
        let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).ok()?;
        let side = 56u32;
        let (w, h) = (tree.size().width().max(1.0), tree.size().height().max(1.0));
        let scale = (side as f32 / w).min(side as f32 / h);
        let (tx, ty) = ((side as f32 - w * scale) / 2.0, (side as f32 - h * scale) / 2.0);
        let mut pixmap = resvg::tiny_skia::Pixmap::new(side, side)?;
        resvg::render(&tree, resvg::tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, tx, ty), &mut pixmap.as_mut());
        let image = egui::ColorImage::from_rgba_premultiplied([side as usize, side as usize], pixmap.data());
        let texture = ctx.load_texture(format!("stencil-{index}-{master}"), image, egui::TextureOptions::LINEAR);
        self.stencil_textures.insert(key, texture.clone());
        Some(texture)
    }

    /// Run Check Diagram on every page (`Engine::check_diagram`) and keep the list for the window.
    fn refresh_check(&mut self) {
        self.check_issues = self.engine.check_diagram(self.check_alt).unwrap_or_default();
    }

    /// Repair one problem like the web (one undo step): "Delete connector" removes the connector, "Move into page" moves the shape just far enough in.
    fn apply_check_fix(&mut self, ctx: &egui::Context, issue: &layoutcraft_engine::check::Issue) {
        let page = issue.page.unwrap_or(self.page_index);
        let result = match issue.fix.as_deref() {
            Some("Delete connector") => self.engine.edit(page, 4, &issue.id, 0.0, 0.0, 0.0, 0.0, 0),
            Some("Move into page") => {
                let model = self.engine.json().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
                let shape = model.as_ref().and_then(|model| model["pages"][page]["shapes"].as_array()).and_then(|shapes| shapes.iter().find(|shape| shape["visioId"] == issue.id.as_str()));
                let size = self.engine.page_size_json(page).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
                match (shape, size) {
                    (Some(shape), Some(size)) => {
                        let (page_w, page_h) = (size["w"].as_f64().unwrap_or(11.0) * DPI, size["h"].as_f64().unwrap_or(8.5) * DPI);
                        let (x, y, w, h) = (shape["x"].as_f64().unwrap_or(0.0), shape["y"].as_f64().unwrap_or(0.0), shape["w"].as_f64().unwrap_or(0.0), shape["h"].as_f64().unwrap_or(0.0));
                        let dx = if x < 0.0 { -x } else if x + w > page_w { page_w - (x + w) } else { 0.0 };
                        let dy = if y < 0.0 { -y } else if y + h > page_h { page_h - (y + h) } else { 0.0 };
                        if dx == 0.0 && dy == 0.0 { Ok(()) } else { self.engine.edit(page, 2, &issue.id, dx / DPI, -dy / DPI, 0.0, 0.0, 0) }
                    }
                    _ => Err("the shape was not found".to_string()),
                }
            }
            _ => return,
        };
        if page != self.page_index && result.is_ok() {
            self.page_index = page;
        }
        self.finish_edit(ctx, result, "Fixed", None);
        self.refresh_check();
    }

    /// The selected shapes, primary first.
    fn all_selected(&self) -> Vec<String> {
        self.selected.iter().chain(self.extra_selected.iter()).cloned().collect()
    }

    /// Shift-click: add the shape to the selection or take it out.
    fn toggle_selected(&mut self, id: &str) {
        if self.selected.as_deref() == Some(id) {
            self.selected = if self.extra_selected.is_empty() { None } else { Some(self.extra_selected.remove(0)) };
        } else if let Some(at) = self.extra_selected.iter().position(|other| other == id) {
            self.extra_selected.remove(at);
        } else if self.selected.is_none() {
            self.selected = Some(id.to_string());
        } else {
            self.extra_selected.push(id.to_string());
        }
        self.status = format!("{} shape(s) selected", self.all_selected().len());
    }

    /// Group, order, align, distribute, rotate or flip the selected shapes through the engine (one undo step).
    fn arrange(&mut self, ctx: &egui::Context, command: ArrangeCmd) {
        let ids = self.all_selected();
        let Some(primary) = self.selected.clone() else {
            return;
        };
        let list = ids.join("\n");
        let page = self.page_index;
        let need = |count: usize| ids.len() >= count;
        // the shapes a shape operation made: all of them stay selected
        let mut pieces: Vec<String> = Vec::new();
        let (verb, result): (&str, Result<Option<String>, String>) = match command {
            ArrangeCmd::Group if !need(2) => ("Group", Err("select two or more shapes (shift-click)".to_string())),
            ArrangeCmd::Group => ("Grouped", self.engine.group_op(page, 0, &list).map(Some)),
            ArrangeCmd::Ungroup => ("Ungrouped", self.engine.group_op(page, 1, &primary).map(|freed| serde_json::from_str::<Vec<String>>(&freed).ok().and_then(|ids| ids.into_iter().next()))),
            ArrangeCmd::Order(op) => ("Changed the order", self.engine.group_op(page, op, &list).map(|_| None)),
            ArrangeCmd::Align(_) if !need(2) => ("Align", Err("select two or more shapes (shift-click)".to_string())),
            ArrangeCmd::Align(mode) => ("Aligned", self.engine.edit(page, 14, &list, mode as f64, 0.0, 0.0, 0.0, 0).map(|_| None)),
            ArrangeCmd::Distribute(_) if !need(3) => ("Spread", Err("select three or more shapes (shift-click)".to_string())),
            ArrangeCmd::Distribute(vertical) => ("Spread evenly", self.engine.edit(page, 15, &list, 0.0, 0.0, 0.0, 0.0, vertical as i32).map(|_| None)),
            // as the web ribbon: rotate right is -PI/2 (clockwise), rotate left +PI/2
            ArrangeCmd::RotateQuarter => ("Rotated right", self.engine.edit(page, 11, &list, -std::f64::consts::FRAC_PI_2, 0.0, 0.0, 0.0, 0).map(|_| None)),
            ArrangeCmd::RotateLeft => ("Rotated left", self.engine.edit(page, 11, &list, std::f64::consts::FRAC_PI_2, 0.0, 0.0, 0.0, 0).map(|_| None)),
            ArrangeCmd::Flip(vertical) => ("Flipped", self.engine.edit(page, 12, &list, 0.0, 0.0, 0.0, 0.0, vertical as i32).map(|_| None)),
            ArrangeCmd::SameSize(width, height) => ("Resized", self.make_same_size(&ids, width, height).map(|_| None)),
            ArrangeCmd::Shapes(op) if !need(shape_op_info(op).1) => (shape_op_info(op).0, Err("select two or more shapes (shift-click)".to_string())),
            ArrangeCmd::Shapes(op) => (
                shape_op_info(op).0,
                self.engine.group_op(page, op, &list).map(|made| {
                    pieces = serde_json::from_str(&made).unwrap_or_default();
                    pieces.first().cloned()
                }),
            ),
        };
        match result {
            Ok(new_primary) => {
                if new_primary.is_some() {
                    self.extra_selected.clear();
                }
                self.finish_edit(ctx, Ok(()), verb, new_primary.or(Some(primary)));
                if pieces.len() > 1 {
                    self.extra_selected = pieces.split_off(1);
                }
            }
            Err(error) => self.status = format!("{verb} failed: {error}"),
        }
    }

    /// Resize the selection to the first shape's width and / or height, about each centre, as one undo step.
    fn make_same_size(&mut self, ids: &[String], width: bool, height: bool) -> Result<(), String> {
        let page = self.page_index;
        let frames = self.engine.group_info_json(page)?;
        let plan = same_size_plan(&frames, ids, |id| self.is_connector(id), width, height)?;
        let depth = self.engine.undo_depth();
        let result = plan.iter().try_for_each(|(id, w, h)| self.engine.edit(page, 3, id, *w, *h, 0.5, 0.5, 0));
        self.engine.collapse_history(depth);
        // all or nothing: a failure part way takes the earlier resizes back
        if result.is_err() && self.engine.undo_depth() > depth {
            let _ = self.engine.undo();
        }
        result
    }

    /// (shape id, text) of the current page's shapes that are not on a hidden layer, in page order.
    fn searchable_shapes(&self) -> Vec<(String, String)> {
        let page = self.page_index;
        self
            .engine
            .document
            .as_ref()
            .and_then(|document| document.pages.get(page))
            .map(|page| {
                page.shapes
                    .iter()
                    .filter(|shape| !shape.layer_hidden)
                    .filter_map(|shape| {
                        let mut text = shape.label.clone();
                        for sub in &shape.sub_texts {
                            text.push('\n');
                            text.push_str(&sub.text);
                        }
                        Some((shape.visio_id.clone()?, text))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Replace in the last match (when it is on this page) or, with `all`, in every shape of the page; then the Find
    /// window says how many texts changed. One undo step.
    fn replace_text(&mut self, ctx: &egui::Context, all: bool) {
        let Some(find) = self.find.as_ref() else {
            return;
        };
        let (needle, with, match_case) = (find.text.clone(), find.replace_with.clone(), find.match_case);
        let page = self.page_index;
        let shapes = self.searchable_shapes();
        let current = find.last.filter(|(last_page, _)| *last_page == page).and_then(|(_, at)| shapes.get(at));
        let ids: Vec<String> = if all {
            shapes.iter().map(|(id, _)| id.clone()).collect()
        } else {
            current.map(|(id, _)| id.clone()).into_iter().collect()
        };
        let selected = self.selected.clone();
        match replace_in_shapes(&mut self.engine, page, &ids, &needle, &with, match_case) {
            Ok(0) => {}
            Ok(count) => {
                self.finish_edit(ctx, Ok(()), "Replaced text", selected);
                if let Some(find) = self.find.as_mut() {
                    find.status = format!("Replaced in {count} shape(s).");
                }
                if all {
                    return;
                }
            }
            Err(error) => {
                if let Some(find) = self.find.as_mut() {
                    find.status = format!("Could not replace: {error}");
                }
                return;
            }
        }
        // "Replace" moves on to the next match (and is "find" when nothing is selected yet)
        if !all {
            self.find_next();
        } else if let Some(find) = self.find.as_mut() {
            find.status = "Nothing to replace on this page.".to_string();
        }
    }

    /// Open the Find window (or return the keyboard focus to it).
    fn open_find(&mut self) {
        self.find.get_or_insert_with(Find::default).focus = true;
    }

    /// Select the next shape of the current page whose text contains the Find window's text. Shapes on hidden layers are
    /// not searched; a match inside a group selects the group.
    fn find_next(&mut self) {
        let Some(mut find) = self.find.take() else {
            return;
        };
        let page = self.page_index;
        let shapes = self.searchable_shapes();
        let texts: Vec<String> = shapes.iter().map(|(_, text)| text.clone()).collect();
        let after = find.last.filter(|(last_page, _)| *last_page == page).map(|(_, at)| at);
        let (count, next) = find_match(&texts, &find.text, find.match_case, after);
        find.status = match (find.text.is_empty(), count) {
            (true, _) => "Type the text to find.".to_string(),
            (false, 0) => "Not found on this page.".to_string(),
            (false, 1) => "1 shape matches.".to_string(),
            (false, count) => format!("{count} shapes match."),
        };
        if let Some(at) = next {
            find.last = Some((page, at));
            self.selected = Some(self.group_root(&shapes[at].0));
            self.extra_selected.clear();
        }
        self.find = Some(find);
    }

    /// The Find window: Enter or "Find next" selects the next match, Esc closes.
    fn draw_find(&mut self, ctx: &egui::Context) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let mut open = true;
        let mut search = false;
        // Some(true) = replace all
        let mut replace: Option<bool> = None;
        egui::Window::new("Find").open(&mut open).collapsible(false).show(ctx, |ui| {
            let label = ui.label("Text to find on this page");
            let edit = ui.text_edit_singleline(&mut find.text).labelled_by(label.id);
            if std::mem::take(&mut find.focus) {
                edit.request_focus();
            }
            if edit.changed() {
                find.last = None;
                find.status.clear();
            }
            if edit.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                search = true;
                edit.request_focus();
            }
            let with_label = ui.label("Replace with");
            ui.text_edit_singleline(&mut find.replace_with).labelled_by(with_label.id);
            if ui.checkbox(&mut find.match_case, "Match case").changed() {
                find.last = None;
            }
            ui.horizontal(|ui| {
                if ui.button("Find next").clicked() {
                    search = true;
                }
                if ui.button("Replace").clicked() {
                    replace = Some(false);
                }
                if ui.button("Replace all").clicked() {
                    replace = Some(true);
                }
            });
            ui.label(&find.status);
        });
        if !open || ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.find = None;
        } else if let Some(all) = replace {
            self.replace_text(ctx, all);
        } else if search {
            self.find_next();
        }
    }

    /// Copy the selected shape to the engine's clipboard (`Engine::copy`).
    fn copy_selected(&mut self) {
        let Some(id) = self.selected.clone() else {
            self.status = "Select a shape to copy.".to_string();
            return;
        };
        let page = self.page_index;
        match self.engine.copy(page, std::slice::from_ref(&id)) {
            Ok(count) => {
                self.paste_count = 0;
                self.status = format!("Copied {count} shape(s); Paste or Ctrl+V puts them on the page.");
            }
            Err(error) => self.status = format!("Could not copy: {error}"),
        }
    }

    /// Paste the clipboard onto the current page, a quarter inch further right and down for every paste; the new shape is selected (one undo step).
    fn paste_clipboard(&mut self, ctx: &egui::Context) {
        self.paste_count += 1;
        let step = 0.25 * self.paste_count as f64;
        let page = self.page_index;
        match self.engine.paste(page, step, -step) {
            Ok(ids) => {
                let first = ids.first().cloned();
                self.finish_edit(ctx, Ok(()), "Pasted", first);
            }
            Err(error) => {
                self.paste_count -= 1;
                self.status = format!("Could not paste: {error}");
            }
        }
    }

    /// A layer operation (`Engine::page_op` 7 add, 8 change, 9 delete) on the current page; the selection and the view stay.
    fn layer_op(&mut self, ctx: &egui::Context, op: i32, index: usize, text: &str) {
        let page = self.page_index;
        let result = self.engine.page_op(op, page, index, text).map(|_| ());
        let selected = self.selected.clone();
        self.finish_edit(ctx, result, "Changed layers", selected);
    }

    /// Put the selected shape on or off a layer (`Engine::edit` op 16).
    fn layer_shape(&mut self, ctx: &egui::Context, index: usize, on: bool) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let page = self.page_index;
        let result = self.engine.edit(page, 16, &id, index as f64, 0.0, 0.0, 0.0, on as i32);
        self.finish_edit(ctx, result, "Changed the shape's layers", Some(id));
    }

    /// A page operation of the engine (`Engine::page_op`, one undo step): afterwards the page it names is shown.
    fn run_page_op(&mut self, ctx: &egui::Context, op: i32, a: usize, b: usize, text: &str, verb: &str) {
        match self.engine.page_op(op, a, b, text) {
            Ok(show) => {
                self.page_index = show.trim().parse().unwrap_or(0);
                self.selected = None;
                self.drag = None;
                self.preview = None;
                self.dirty = self.has_changes();
                self.build_page(ctx, true);
                self.status = format!("{verb}; save as to keep it.");
            }
            Err(error) => self.status = format!("{verb} failed: {error}"),
        }
    }

    fn page_count(&self) -> usize {
        self.engine.page_count().unwrap_or(0)
    }

    fn delete_page(&mut self, ctx: &egui::Context) {
        if self.page_count() <= 1 {
            self.status = "A drawing needs at least one page.".to_string();
            return;
        }
        self.run_page_op(ctx, 2, self.page_index, 0, "", "Deleted page (Undo brings it back)");
    }

    fn open_page_rename(&mut self) {
        let names = self.engine.page_names_json().ok().and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok()).unwrap_or_default();
        self.page_rename = Some(names.get(self.page_index).cloned().unwrap_or_default());
    }

    fn move_page(&mut self, ctx: &egui::Context, direction: i32) {
        let target = self.page_index as i64 + direction as i64;
        if target < 0 || target >= self.page_count() as i64 {
            self.status = "The page is already at that end.".to_string();
            return;
        }
        self.run_page_op(ctx, 4, self.page_index, target as usize, "", "Moved page");
    }

    /// Write the changed style values of the inspector (Engine::edit op 10), one undo step.
    fn apply_style(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.style_draft.take() else {
            return;
        };
        let Some(json) = draft.changes() else {
            self.style_draft = Some(draft);
            return;
        };
        let page = self.page_index;
        let result = self.engine.edit(page, 10, &format!("{}\n{json}", draft.id), 0.0, 0.0, 0.0, 0.0, 0);
        self.finish_edit(ctx, result, "Changed style", Some(draft.id));
    }

    /// After an engine edit: redraw the page and update the selection and the status line.
    fn finish_edit(&mut self, ctx: &egui::Context, result: Result<(), String>, verb: &str, selected: Option<String>) {
        if let Err(error) = result {
            self.status = format!("{verb} failed: {error}");
            return;
        }
        self.dirty = self.has_changes();
        self.selected = selected;
        self.drag = None;
        self.preview = None;
        self.build_page(ctx, false);
        self.status = format!("{verb}; save as to keep it.");
    }
}

impl eframe::App for LayoutCraftApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let _frame_timer = Timer::new("frame", 16.0);
        let ctx = ui.ctx().clone();
        self.apply_raster(&ctx);
        self.settle_raster(&ctx);
        self.shift_down = ctx.input(|input| input.modifiers.shift);
        let mut action: Option<Action> = None;

        let dropped = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .next()
        });
        if let Some(path) = dropped {
            self.path_input = path.display().to_string();
            if self.dirty {
                self.pending_open = Some(PendingOpen::Load(self.path_input.clone()));
            } else {
                self.load(&ctx, &path);
            }
        }

        self.space_down = !ctx.egui_wants_keyboard_input() && ctx.input(|input| input.key_down(egui::Key::Space));
        if !ctx.egui_wants_keyboard_input() {
            let mut undo = false;
            let mut redo = false;
            ctx.input_mut(|input| {
                if input.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::Z)
                    || input.consume_key(egui::Modifiers::CTRL, egui::Key::Y)
                {
                    redo = true;
                } else if input.consume_key(egui::Modifiers::CTRL, egui::Key::Z) {
                    undo = true;
                }
            });
            if undo {
                self.undo(&ctx);
            }
            if redo {
                self.redo(&ctx);
            }
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::S)) {
                self.save_copy();
            }
            // egui turns Ctrl+C / Ctrl+V into Copy / Paste events; Ctrl+D (duplicate) is a plain key
            let (copy, paste, cut) = ctx.input(|input| {
                (
                    input.events.iter().any(|event| matches!(event, egui::Event::Copy)),
                    input.events.iter().any(|event| matches!(event, egui::Event::Paste(_))),
                    input.events.iter().any(|event| matches!(event, egui::Event::Cut)),
                )
            });
            if cut {
                action = Some(Action::Cut);
            } else if copy {
                action = Some(Action::Copy);
            } else if paste {
                action = Some(Action::Paste);
            } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::D)) {
                action = Some(Action::Duplicate);
            }
            if self.selected.is_some()
                && ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::E))
            {
                action = Some(Action::EditText);
            }
            if self.selected.is_some()
                && ctx.input(|input| {
                    input.key_pressed(egui::Key::Delete) || input.key_pressed(egui::Key::Backspace)
                })
            {
                action = Some(Action::DeleteSelected);
            }
            // arrange commands (ARRANGE_KEYS), then select all, zoom, F2 and the arrow nudge
            if let Some(command) = ctx.input_mut(consume_arrange_key) {
                action = Some(Action::Arrange(command));
            } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::A)) {
                self.select_all();
            } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Equals) || input.consume_key(egui::Modifiers::CTRL, egui::Key::Plus)) {
                action = Some(Action::ZoomIn);
            } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Minus)) {
                action = Some(Action::ZoomOut);
            }
            match ctx.input_mut(consume_view_key) {
                Some(ViewKey::Tool(tool)) => self.tool = tool,
                Some(ViewKey::FitPage) => action = Some(Action::Fit),
                Some(ViewKey::NextPage) => action = Some(Action::Page(self.page_index + 1)),
                Some(ViewKey::PreviousPage) => action = Some(Action::Page(self.page_index.saturating_sub(1))),
                Some(ViewKey::Find) => action = Some(Action::Find),
                None => {}
            }
            if self.selected.is_some() {
                if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F2)) {
                    action = Some(Action::EditText);
                }
                let nudge = ctx.input(|input| {
                    [egui::Key::ArrowLeft, egui::Key::ArrowRight, egui::Key::ArrowUp, egui::Key::ArrowDown]
                        .into_iter()
                        .find(|key| input.key_pressed(*key))
                        .and_then(|key| nudge_delta(key, input.modifiers.shift))
                });
                if let Some((dx, dy)) = nudge {
                    self.nudge_selected(&ctx, dx, dy);
                }
            }
            if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.selected = None;
                self.text_editor = None;
            }
        }

        egui::Panel::top("ribbon").show(ui, |ui| {
            self.draw_ribbon(ui, &mut action);
        });

        if self.pending_open.is_some() {
            egui::Panel::top("pending").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::from_rgb(255, 196, 0), "Unsaved changes.");
                    if ui.button("Discard and continue").clicked() {
                        action = Some(Action::ConfirmDiscard);
                    }
                    if ui.button("Cancel").clicked() {
                        action = Some(Action::CancelDiscard);
                    }
                });
            });
        }

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                let pages = self.page_count();
                if ui.add_enabled(self.page_index > 0 && pages > 0, egui::Button::new("<")).on_hover_text("Previous page").clicked() {
                    action = Some(Action::Page(self.page_index - 1));
                }
                ui.label(format!("page {} / {}", if pages == 0 { 0 } else { self.page_index + 1 }, pages));
                if ui.add_enabled(self.page_index + 1 < pages, egui::Button::new(">")).on_hover_text("Next page").clicked() {
                    action = Some(Action::Page(self.page_index + 1));
                }
                ui.separator();
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("zoom {:.0}%", self.zoom * 100.0));
                    if self.dirty {
                        ui.colored_label(egui::Color32::from_rgb(255, 196, 0), "modified");
                    }
                });
            });
            ui.add_space(2.0);
        });

        egui::Panel::right("inspector")
            .default_size(250.0)
            .resizable(true)
            .show(ui, |ui| {
                ui.heading("Document");
                if let Some(document) = &self.engine.document {
                    ui.label(format!("Pages: {}", document.pages.len()));
                    ui.label(format!("Media: {}", document.media.len()));
                    if let Some(page) = document.pages.get(self.page_index) {
                        ui.label(format!("Page: {}", page.name));
                        ui.label(format!(
                            "Shapes: {}  Connectors: {}",
                            page.shapes.len(),
                            page.connectors.len()
                        ));
                    }
                    for found in &self.font_warnings {
                        ui.colored_label(egui::Color32::from_rgb(255, 90, 90), format!("\u{26a0} Font {} is not installed: drawn with {}", found.requested, found.used))
                            .on_hover_text("The file keeps the font it names; only this screen uses the replacement, so text may look and wrap a little differently.");
                    }
                } else {
                    ui.label("Nothing loaded.");
                }
                ui.separator();
                ui.heading("Selection");
                if let Some(selected) = self.selected.clone() {
                    if let Some(shape) = self.scene_shape(&selected) {
                        ui.label(format!("Shape {selected}"));
                        ui.label(format!(
                            "Box: {:.1} x {:.1} at {:.1}, {:.1}",
                            shape.w, shape.h, shape.x, shape.y
                        ));
                        ui.label(format!("Parts: {}", shape.parts.len()));
                        // the shape's hyperlinks (read-only here; the web edits them in a dialog)
                        let key = (selected.clone(), self.page_index, self.engine.undo_depth());
                        let cached = self.links_cache.borrow().as_ref().filter(|(k, _)| *k == key).map(|(_, v)| v.clone());
                        let links = cached.unwrap_or_else(|| {
                            let parsed = self.engine.hyperlinks_json(self.page_index, &selected).map(|j| parse_links(&j)).unwrap_or_default();
                            *self.links_cache.borrow_mut() = Some((key, parsed.clone()));
                            parsed
                        });
                        for (name, address) in &links {
                            ui.label(egui::RichText::new(format!("Link: {} {}", if name.is_empty() { "(unnamed)" } else { name }, address)).small());
                        }
                        if let Some(text) = shape_text(shape) {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(text).monospace());
                        }
                    }
                    if let Some(shape) = self.doc_shape(&selected) {
                        ui.add_space(4.0);
                        ui.label(format!(
                            "Position: {:.3} x {:.3} in",
                            (shape.w / DPI) as f32,
                            (shape.h / DPI) as f32
                        ));
                        if shape.group.is_some() {
                            ui.colored_label(
                                egui::Color32::from_rgb(255, 196, 0),
                                "Grouped: read-only in this slice",
                            );
                        } else if shape.rot.abs() >= 1e-6 {
                            ui.label("Rotated: move only, resize disabled");
                        }
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Edit text").clicked() {
                            action = Some(Action::EditText);
                        }
                        if ui
                            .add_enabled(
                                self.doc_shape(&selected)
                                    .is_some_and(|shape| shape.group.is_none())
                                    || self.is_connector(&selected),
                                egui::Button::new("Delete"),
                            )
                            .clicked()
                        {
                            action = Some(Action::DeleteSelected);
                        }
                    });
                    if let Some(shape) = self.doc_shape(&selected).filter(|shape| shape.group.is_none()).cloned() {
                        if self.style_draft.as_ref().is_none_or(|draft| draft.id != selected) {
                            self.style_draft = Some(StyleDraft::from_shape(&selected, &shape));
                        }
                        if let Some(draft) = &mut self.style_draft {
                            ui.separator();
                            ui.label("Style");
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut draft.has_fill, "Fill");
                                ui.color_edit_button_srgb(&mut draft.fill);
                            });
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut draft.has_line, "Line");
                                ui.color_edit_button_srgb(&mut draft.line);
                                ui.add(egui::DragValue::new(&mut draft.weight_px).range(0.0..=40.0).speed(0.1).suffix(" px"));
                            });
                            egui::ComboBox::from_label("Pattern")
                                .selected_text(LINE_PATTERNS.iter().find(|(code, _)| *code == draft.pattern).map_or("Other", |(_, name)| *name))
                                .show_ui(ui, |ui| {
                                    for (code, name) in LINE_PATTERNS {
                                        ui.selectable_value(&mut draft.pattern, code, name);
                                    }
                                });
                            if ui.add_enabled(draft.changes().is_some(), egui::Button::new("Apply style")).clicked() {
                                action = Some(Action::ApplyStyle);
                            }
                        }
                    }
                } else {
                    ui.label("Click a shape on the canvas.");
                }
                if self.selected.is_some() {
                    ui.separator();
                    ui.label(format!("Arrange ({} selected; shift-click adds)", 1 + self.extra_selected.len()));
                    ui.horizontal_wrapped(|ui| {
                        for (label, command) in [
                            ("Group", ArrangeCmd::Group),
                            ("Ungroup", ArrangeCmd::Ungroup),
                            ("Front", ArrangeCmd::Order(2)),
                            ("Back", ArrangeCmd::Order(3)),
                            ("Fwd", ArrangeCmd::Order(4)),
                            ("Bwd", ArrangeCmd::Order(5)),
                            ("Rot R", ArrangeCmd::RotateQuarter),
                            ("Flip H", ArrangeCmd::Flip(false)),
                            ("Flip V", ArrangeCmd::Flip(true)),
                        ] {
                            if ui.small_button(label).clicked() {
                                action = Some(Action::Arrange(command));
                            }
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Align");
                        for (label, mode) in [("L", 0), ("C", 1), ("R", 2), ("T", 3), ("M", 4), ("B", 5)] {
                            if ui.small_button(label).clicked() {
                                action = Some(Action::Arrange(ArrangeCmd::Align(mode)));
                            }
                        }
                        ui.label("Spread");
                        if ui.small_button("H").clicked() {
                            action = Some(Action::Arrange(ArrangeCmd::Distribute(false)));
                        }
                        if ui.small_button("V").clicked() {
                            action = Some(Action::Arrange(ArrangeCmd::Distribute(true)));
                        }
                    });
                }
                ui.separator();
                ui.label("Layers");
                let layers = self
                    .engine
                    .document()
                    .and_then(|document| document.pages.get(self.page_index))
                    .map(|page| page.layers.clone())
                    .unwrap_or_default();
                let selected_layers: Vec<u32> = self
                    .selected
                    .as_deref()
                    .and_then(|id| self.doc_shape(id))
                    .map(|shape| shape.layers.clone())
                    .unwrap_or_default();
                let has_selection = self.selected.is_some();
                if layers.is_empty() {
                    ui.label(egui::RichText::new("This page has no layers.").small().weak());
                }
                for layer in &layers {
                    ui.horizontal(|ui| {
                        let (mut visible, mut print, mut locked) = (layer.visible, layer.print, layer.locked);
                        let a = ui.checkbox(&mut visible, "V").on_hover_text("Visible");
                        let b = ui.checkbox(&mut print, "P").on_hover_text("Prints");
                        let c = ui.checkbox(&mut locked, "L").on_hover_text("Locked");
                        if a.changed() || b.changed() || c.changed() {
                            action = Some(Action::LayerOp(8, layer.index as usize, format!("|{}|{}|{}", visible as u8, print as u8, locked as u8)));
                        }
                        let mut on_layer = selected_layers.contains(&layer.index);
                        let was = on_layer;
                        if ui.add_enabled(has_selection, egui::Checkbox::new(&mut on_layer, "Sel")).on_hover_text("The selected shape is on this layer").changed() && on_layer != was {
                            action = Some(Action::LayerShape(layer.index as usize, on_layer));
                        }
                        ui.label(&layer.name);
                        if ui.small_button("x").on_hover_text("Delete the layer (shapes stay)").clicked() {
                            action = Some(Action::LayerOp(9, layer.index as usize, String::new()));
                        }
                    });
                }
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.layer_name).desired_width(90.0).hint_text("Layer name"));
                    if ui.add_enabled(!self.layer_name.trim().is_empty() && self.texture.is_some(), egui::Button::new("+ Layer")).clicked() {
                        action = Some(Action::LayerOp(7, 0, self.layer_name.trim().to_string()));
                        self.layer_name.clear();
                    }
                });
                ui.separator();
                ui.label(
                    egui::RichText::new(
                        "Drag a shape to move. Drag a corner handle to resize.\n\
                         Ctrl+Z undo, Ctrl+Shift+Z redo, Ctrl+S save a copy.\n\
                         Drag empty canvas to pan; wheel zooms.",
                    )
                    .small()
                    .weak(),
                );
            });

        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) && self.armed_master.is_some() {
            self.armed_master = None;
            self.status = "Cancelled placing a shape.".to_string();
        }
        if self.shapes_open && !self.stencil_autoopen_done && self.texture.is_some() {
            self.stencil_autoopen_done = true;
            if self.stencils.is_empty() {
                self.open_builtin_stencil();
                self.restore_stencils();
            }
        }
        if self.shapes_open {
            let mut open_stencil = false;
            let mut builtin = false;
            let mut close = None;
            let mut replace: Option<(usize, String, String)> = None;
            let can_replace = self.selected.is_some();
            egui::Panel::left("shapes").default_size(250.0).resizable(true).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Shapes");
                    if ui.small_button("Open stencil…").clicked() {
                        open_stencil = true;
                    }
                    if ui.small_button("Basic shapes").clicked() {
                        builtin = true;
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut self.shape_search).hint_text("Search shapes").desired_width(f32::INFINITY));
                if let Some((_, _, name)) = &self.armed_master {
                    ui.add(egui::Label::new(egui::RichText::new(format!("Click on the page to place \"{name}\" (Esc cancels).")).small()).wrap());
                } else if self.stencils.is_empty() {
                    ui.label("No stencil open.");
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for si in 0..self.stencils.len() {
                        let (name, index) = (self.stencils[si].name.clone(), self.stencils[si].index);
                        let masters: Vec<(String, String)> = self.stencils[si].masters.iter().filter(|(_, mname)| master_matches(mname, &self.shape_search)).cloned().collect();
                        if masters.is_empty() && !self.shape_search.trim().is_empty() {
                            continue;
                        }
                        let header = egui::CollapsingHeader::new(format!("{name} ({})", masters.len())).id_salt(("stencil", index)).default_open(true);
                        let shown = header.show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                                for (id, mname) in &masters {
                                    let (rect, response) = ui.allocate_exact_size(egui::vec2(70.0, 84.0), egui::Sense::click_and_drag());
                                    if ui.is_rect_visible(rect) {
                                        let thumb = egui::Rect::from_min_size(rect.min + egui::vec2(7.0, 2.0), egui::vec2(56.0, 56.0));
                                        ui.painter().rect_filled(thumb, 3.0, egui::Color32::from_rgb(242, 242, 242));
                                        if let Some(texture) = self.stencil_thumb_texture(&ctx, index, id) {
                                            ui.painter().image(texture.id(), thumb, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                                        }
                                        let short: String = if mname.chars().count() > 11 { format!("{}…", mname.chars().take(10).collect::<String>()) } else { mname.clone() };
                                        ui.painter().text(egui::pos2(rect.center().x, thumb.bottom() + 3.0), egui::Align2::CENTER_TOP, short, egui::FontId::proportional(10.5), egui::Color32::from_gray(200));
                                        let armed = self.armed_master.as_ref().is_some_and(|(i, m, _)| *i == index && m == id);
                                        if armed || response.hovered() {
                                            ui.painter().rect_stroke(thumb.expand(2.0), 4.0, egui::Stroke::new(2.0, if armed { egui::Color32::from_rgb(255, 196, 0) } else { egui::Color32::from_rgb(90, 150, 220) }), egui::StrokeKind::Outside);
                                        }
                                    }
                                    let response = response.on_hover_text(mname.as_str());
                                    if response.clicked() {
                                        self.armed_master = Some((index, id.clone(), mname.clone()));
                                        self.status = format!("Click on the page to place \"{mname}\".");
                                    }
                                    // drag a master onto the page: the drop is placed by the canvas block (it knows the page rectangle)
                                    if response.drag_started() {
                                        self.pane_drag = Some(mname.clone());
                                    }
                                    if response.drag_stopped() {
                                        self.pane_drag = None;
                                        if let Some(at) = ctx.pointer_latest_pos() {
                                            self.pending_drop = Some((index, id.clone(), mname.clone(), at));
                                        }
                                    }
                                    // the web's Change Shape: the selected shape becomes an instance of this master
                                    response.context_menu(|ui| {
                                        if ui.button("Place on the page").clicked() {
                                            self.armed_master = Some((index, id.clone(), mname.clone()));
                                            ui.close();
                                        }
                                        if ui.add_enabled(can_replace, egui::Button::new("Replace the selected shape")).clicked() {
                                            replace = Some((index, id.clone(), mname.clone()));
                                            ui.close();
                                        }
                                    });
                                }
                            });
                        });
                        shown.header_response.context_menu(|ui| {
                            if ui.button("Close this stencil").clicked() {
                                close = Some(si);
                                ui.close();
                            }
                        });
                    }
                });
            });
            if let Some((index, master, name)) = replace {
                if let Some(shape) = self.selected.clone() {
                    let page = self.page_index;
                    let result = self.engine.replace_shape(page, index as i32, &format!("{shape}|{master}"));
                    match result {
                        Ok(new_id) => {
                            self.extra_selected.clear();
                            self.finish_edit(&ctx, Ok(()), &format!("Changed shape {shape} to \"{name}\""), Some(new_id));
                        }
                        Err(error) => self.status = format!("Could not change the shape: {error}"),
                    }
                }
            }
            if let Some(si) = close {
                let index = self.stencils[si].index;
                self.engine.stencil_close(index);
                self.stencils.remove(si);
                self.remember_stencils();
                self.stencil_textures.retain(|(i, _), _| *i != index);
            }
            if open_stencil {
                self.open_stencil_dialog();
            }
            if builtin {
                self.open_builtin_stencil();
            }
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let available = ui.available_size();
                if self.fit_pending {
                    self.fit_to_view(available);
                }
                let (viewport, response) =
                    ui.allocate_exact_size(available, egui::Sense::click_and_drag());
                let painter = ui.painter_at(viewport);
                painter.rect_filled(viewport, 0.0, egui::Color32::from_rgb(28, 30, 34));
                let Some(texture_id) = self.texture.as_ref().map(|texture| texture.id()) else {
                    painter.text(
                        viewport.center(),
                        egui::Align2::CENTER_CENTER,
                        "Open a VSDX file to begin.\nYou can also drag and drop one here.",
                        egui::FontId::proportional(18.0),
                        egui::Color32::from_gray(180),
                    );
                    return;
                };

                if response.dragged_by(egui::PointerButton::Middle) {
                    self.pan += response.drag_delta();
                }
                if response.hovered() {
                    let scroll = ui.input(|input| input.smooth_scroll_delta.y);
                    if scroll.abs() > 0.01 {
                        if let Some(pointer) = response.hover_pos() {
                            let center = viewport.center();
                            let before = (pointer - center - self.pan) / self.zoom;
                            let factor = (scroll * 0.0015).exp();
                            self.zoom = (self.zoom * factor).clamp(0.05, 16.0);
                            self.pan = pointer - center - before * self.zoom;
                        }
                    }
                }
                self.last_viewport = viewport;
                let image_rect = self.image_rect(viewport);
                painter.image(
                    texture_id,
                    image_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                if let Some(detail) = &self.detail {
                    let view = self.view_scale();
                    let r = detail.region;
                    let rect = egui::Rect::from_min_size(
                        image_rect.min + egui::vec2(r[0] as f32, r[1] as f32) * view,
                        egui::vec2(r[2] as f32, r[3] as f32) * view,
                    );
                    painter.image(detail.texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                }

                if response.drag_started_by(egui::PointerButton::Primary) {
                    self.panning = false;
                    let press = ctx
                        .input(|input| input.pointer.press_origin())
                        .or_else(|| response.interact_pointer_pos());
                    if let Some(pointer) = press {
                        self.begin_pointer(pointer, image_rect);
                    }
                }
                if response.dragged_by(egui::PointerButton::Primary) {
                    if self.drag.is_some() {
                        if let Some(pointer) = response.interact_pointer_pos() {
                            self.update_drag(pointer, image_rect);
                        }
                    } else if self.panning {
                        self.pan += response.drag_delta();
                    }
                }
                if response.drag_stopped_by(egui::PointerButton::Primary) {
                    self.finish_drag(&ctx);
                }
                // the right-click menus (web/contextmenu.js): a shape or the selection, or the empty page. A right click on a shape that is not
                // selected selects it first; on empty canvas it clears the selection.
                if response.secondary_clicked() {
                    if let Some(pointer) = response.interact_pointer_pos() {
                        let at = self.screen_to_scene(pointer, image_rect);
                        match self.pick_shape(at) {
                            Some(id) if !self.all_selected().contains(&id) => {
                                self.selected = Some(id);
                                self.extra_selected.clear();
                            }
                            None => {
                                self.selected = None;
                                self.extra_selected.clear();
                            }
                            _ => {}
                        }
                    }
                }
                let has_selection = self.selected.is_some();
                let can_paste = self.engine.clipboard_count() > 0;
                response.context_menu(|ui| {
                    if has_selection {
                        let mut pick = |ui: &mut egui::Ui, label: &str, enabled: bool, command: Action| {
                            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                                action = Some(command);
                                ui.close();
                            }
                        };
                        pick(ui, "Cut", true, Action::Cut);
                        pick(ui, "Copy", true, Action::Copy);
                        pick(ui, "Paste", can_paste, Action::Paste);
                        pick(ui, "Duplicate", true, Action::Duplicate);
                        pick(ui, "Delete", true, Action::DeleteSelected);
                        ui.separator();
                        pick(ui, "Group", true, Action::Arrange(ArrangeCmd::Group));
                        pick(ui, "Ungroup", true, Action::Arrange(ArrangeCmd::Ungroup));
                        ui.separator();
                        pick(ui, "Bring to front", true, Action::Arrange(ArrangeCmd::Order(2)));
                        pick(ui, "Bring forward", true, Action::Arrange(ArrangeCmd::Order(4)));
                        pick(ui, "Send backward", true, Action::Arrange(ArrangeCmd::Order(5)));
                        pick(ui, "Send to back", true, Action::Arrange(ArrangeCmd::Order(3)));
                        ui.separator();
                        pick(ui, "Rotate left", true, Action::Arrange(ArrangeCmd::RotateLeft));
                        pick(ui, "Rotate right", true, Action::Arrange(ArrangeCmd::RotateQuarter));
                        pick(ui, "Flip horizontal", true, Action::Arrange(ArrangeCmd::Flip(false)));
                        pick(ui, "Flip vertical", true, Action::Arrange(ArrangeCmd::Flip(true)));
                        ui.separator();
                        pick(ui, "Edit text", true, Action::EditText);
                    } else {
                        let mut pick = |ui: &mut egui::Ui, label: &str, enabled: bool, command: Action| {
                            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                                action = Some(command);
                                ui.close();
                            }
                        };
                        pick(ui, "Paste", can_paste, Action::Paste);
                        ui.separator();
                        pick(ui, "Page setup...", true, Action::PageSetup);
                        pick(ui, "Fit page to window", true, Action::Fit);
                        ui.separator();
                        ui.checkbox(&mut self.show_grid, "Grid");
                        ui.checkbox(&mut self.snap_grid, "Snap to grid");
                        ui.checkbox(&mut self.snap_shapes, "Snap to shapes");
                        ui.checkbox(&mut self.snap_guides, "Snap to guides");
                    }
                });
                let mut dropped_now = false;
                if let Some(label) = &self.pane_drag {
                    if let Some(at) = ctx.pointer_latest_pos() {
                        let ghost = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("pane-ghost")));
                        ghost.text(at + egui::vec2(12.0, 12.0), egui::Align2::LEFT_TOP, format!("+ {label}"), egui::FontId::proportional(13.0), egui::Color32::from_rgb(255, 196, 0));
                    }
                }
                if let Some((index, master, name, at_screen)) = self.pending_drop.take() {
                    if image_rect.contains(at_screen) {
                        let at = self.screen_to_scene(at_screen, image_rect);
                        let page = self.page_index;
                        let (x, y) = (at.x as f64 / DPI, self.page_height_in - at.y as f64 / DPI);
                        match self.engine.stencil_drop(page, index, &master, x, y) {
                            Ok(id) => self.finish_edit(&ctx, Ok(()), &format!("Placed \"{name}\""), Some(id)),
                            Err(error) => self.status = format!("Could not place \"{name}\": {error}"),
                        }
                        self.armed_master = None;
                        dropped_now = true;
                    }
                }
                if response.clicked() && self.armed_master.is_some() {
                    if let Some(pointer) = response.interact_pointer_pos() {
                        if image_rect.contains(pointer) {
                            let at = self.screen_to_scene(pointer, image_rect);
                            if let Some((index, master, name)) = self.armed_master.take() {
                                let page = self.page_index;
                                let (x, y) = (at.x as f64 / DPI, self.page_height_in - at.y as f64 / DPI);
                                match self.engine.stencil_drop(page, index, &master, x, y) {
                                    Ok(id) => self.finish_edit(&ctx, Ok(()), &format!("Placed \"{name}\""), Some(id)),
                                    Err(error) => self.status = format!("Could not place \"{name}\": {error}"),
                                }
                                dropped_now = true;
                            }
                        }
                    }
                }
                if response.clicked() && self.drag.is_none() && !self.panning && self.shift_down && !dropped_now {
                    if let Some(pointer) = response.interact_pointer_pos() {
                        if image_rect.contains(pointer) {
                            let scene_pointer = self.screen_to_scene(pointer, image_rect);
                            if let Some(id) = self.pick_shape(scene_pointer) {
                                self.toggle_selected(&id);
                            }
                        }
                    }
                } else if response.clicked() && self.drag.is_none() && !self.panning && !dropped_now {
                    self.extra_selected.clear();
                    if let Some(pointer) = response.interact_pointer_pos() {
                        if image_rect.contains(pointer) {
                            let scene_pointer = self.screen_to_scene(pointer, image_rect);
                            self.selected = self.pick_connector(scene_pointer).or_else(|| self.pick_shape(scene_pointer));
                        } else {
                            self.selected = None;
                        }
                    }
                }
                if response.double_clicked() && self.drag.is_none() {
                    if let Some(pointer) = response.interact_pointer_pos() {
                        if image_rect.contains(pointer) {
                            let scene_pointer = self.screen_to_scene(pointer, image_rect);
                            self.selected = self.pick_shape(scene_pointer);
                            if self.selected.is_some() {
                                action = Some(Action::EditText);
                            }
                        }
                    }
                }

                if let Some(selected) = self.selected.clone() {
                    if let Some((_, points)) = self.connector_routes().into_iter().find(|(id, _)| *id == selected) {
                        let screen: Vec<egui::Pos2> = points
                            .iter()
                            .map(|point| self.scene_rect_to_screen(egui::Rect::from_min_size(*point, egui::Vec2::ZERO), image_rect).min)
                            .collect();
                        // the two end handles
                        for end in [screen.first().copied(), screen.last().copied()].into_iter().flatten() {
                            painter.circle_filled(end, 5.5, egui::Color32::from_rgb(255, 246, 210));
                            painter.circle_stroke(end, 5.5, egui::Stroke::new(1.5, egui::Color32::from_rgb(90, 70, 0)));
                        }
                        painter.add(egui::Shape::line(screen, egui::Stroke::new(3.0, egui::Color32::from_rgb(255, 196, 0))));
                    }
                }
                // a connector end being dragged: a line from the other end to the pointer; connectors of moved shapes follow live
                if let (Some(state), Some(DragState { mode: DragMode::ConnEnd, end_pointer, .. })) = (&self.conn_end, &self.drag) {
                    let (from, to) = (self.scene_to_screen(state.fixed, image_rect), self.scene_to_screen(*end_pointer, image_rect));
                    painter.line_segment([from, to], egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 140, 0)));
                }
                // the snap guides (thin orange lines through the snapped position)
                if let Some(x) = self.snap_guide.0 {
                    let at = self.scene_to_screen(egui::pos2(x, 0.0), image_rect).x;
                    painter.line_segment([egui::pos2(at, image_rect.top()), egui::pos2(at, image_rect.bottom())], egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 120, 0)));
                }
                if let Some(y) = self.snap_guide.1 {
                    let at = self.scene_to_screen(egui::pos2(0.0, y), image_rect).y;
                    painter.line_segment([egui::pos2(image_rect.left(), at), egui::pos2(image_rect.right(), at)], egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 120, 0)));
                }
                for (_, points) in &self.live_routes {
                    let screen: Vec<egui::Pos2> = points.iter().map(|p| self.scene_to_screen(*p, image_rect)).collect();
                    painter.add(egui::Shape::line(screen, egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 140, 0))));
                }
                // the grid: a line every grid step (inches) over the page, as the web's Grid check box
                if self.show_grid {
                    let page_w = self.page_width_in;
                    let step = self.grid_step.max(0.05);
                    let stroke = egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(120, 150, 190, 90));
                    let mut x = step;
                    while x < page_w {
                        let a = self.scene_to_screen(egui::pos2((x * DPI) as f32, 0.0), image_rect);
                        let b = self.scene_to_screen(egui::pos2((x * DPI) as f32, (self.page_height_in * DPI) as f32), image_rect);
                        painter.line_segment([a, b], stroke);
                        x += step;
                    }
                    let mut y = step;
                    while y < self.page_height_in {
                        let a = self.scene_to_screen(egui::pos2(0.0, (y * DPI) as f32), image_rect);
                        let b = self.scene_to_screen(egui::pos2((page_w * DPI) as f32, (y * DPI) as f32), image_rect);
                        painter.line_segment([a, b], stroke);
                        y += step;
                    }
                }
                // the rubber band
                if let (Some(DragState { mode: DragMode::Marquee, .. }), Some(band)) = (&self.drag, self.preview) {
                    let band = self.scene_rect_to_screen(band, image_rect);
                    painter.rect_filled(band, 0.0, egui::Color32::from_rgba_unmultiplied(0, 160, 255, 28));
                    painter.rect_stroke(band, 0.0, egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 160, 255)), egui::StrokeKind::Inside);
                }
                // while several shapes are dragged the others follow the primary one
                let move_delta = match (&self.drag, self.preview) {
                    (Some(DragState { mode: DragMode::Move, start_rect, .. }), Some(now)) => now.min - start_rect.min,
                    _ => egui::Vec2::ZERO,
                };
                for id in self.extra_selected.clone() {
                    if let Some(shape) = self.scene_shape(&id) {
                        let rect = self.scene_rect_to_screen(Self::shape_scene_rect(shape).translate(move_delta), image_rect);
                        painter.rect_stroke(rect, 0.0, egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 140, 0)), egui::StrokeKind::Outside);
                    }
                }
                let mut hover_handle = None;
                if let Some(selected) = self.selected.clone() {
                    if let Some(shape) = self.scene_shape(&selected) {
                        let doc_shape = self.doc_shape(&selected);
                        // a shape whose frame is a hairline under its picture (1-D masters) cannot be resized by the outline of its picture
                        let resizable = doc_shape.is_some_and(|shape| shape.group.is_none() && shape.rot.abs() < 1e-6)
                            && Self::selection_scene_rect(shape) == Self::shape_scene_rect(shape);
                        let rect = self
                            .preview
                            .unwrap_or_else(|| Self::selection_scene_rect(shape));
                        let screen_rect = self.scene_rect_to_screen(rect, image_rect);
                        let turned = doc_shape.map_or(0.0, |shape| shape.rot.to_radians() as f32);
                        if turned.abs() > 1e-6 {
                            // a turned shape: its own (turned) outline instead of the upright frame
                            let (sin, cos) = turned.sin_cos();
                            let half = screen_rect.size() / 2.0;
                            let centre = screen_rect.center();
                            let corners: Vec<egui::Pos2> = [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                                .iter()
                                .map(|(sx, sy)| {
                                    let (x, y) = (sx * half.x, sy * half.y);
                                    centre + egui::vec2(x * cos - y * sin, x * sin + y * cos)
                                })
                                .collect();
                            painter.add(egui::Shape::closed_line(corners, egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 196, 0))));
                        } else {
                            painter.rect_stroke(
                                screen_rect,
                                0.0,
                                egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 196, 0)),
                                egui::StrokeKind::Outside,
                            );
                        }
                        if resizable && Self::handles_fit(screen_rect) {
                            for (handle, hit) in Self::handle_rects(screen_rect) {
                                if let Some(pointer) = response.hover_pos() {
                                    if hit.expand(2.0).contains(pointer) {
                                        hover_handle = Some(handle);
                                    }
                                }
                                painter.rect_filled(
                                    hit,
                                    1.0,
                                    egui::Color32::from_rgb(255, 246, 210),
                                );
                                painter.rect_stroke(
                                    hit,
                                    1.0,
                                    egui::Stroke::new(1.0, egui::Color32::from_rgb(90, 70, 0)),
                                    egui::StrokeKind::Inside,
                                );
                            }
                        }
                    }
                }

                // the rotate handle (a stem above the top edge) of the selected shape; while rotating, the turned outline
                let mut hover_rotate = false;
                if let Some(selected) = self.selected.clone() {
                    if let Some((top, handle, centre)) = self.rotate_handle_screen(&selected, image_rect) {
                        let accent = egui::Color32::from_rgb(255, 196, 0);
                        painter.line_segment([top, handle], egui::Stroke::new(1.5, accent));
                        painter.circle_filled(handle, 5.5, egui::Color32::from_rgb(255, 246, 210));
                        painter.circle_stroke(handle, 5.5, egui::Stroke::new(1.5, egui::Color32::from_rgb(90, 70, 0)));
                        if response.hover_pos().is_some_and(|p| (p - handle).length() <= 9.0) {
                            hover_rotate = true;
                        }
                        if let (Some(r), Some(scene)) = (&self.rotating, self.scene_shape(&selected)) {
                            let rect = self.scene_rect_to_screen(Self::shape_scene_rect(scene), image_rect);
                            let phi = (-r.theta) as f32;
                            let (sin, cos) = phi.sin_cos();
                            let half = rect.size() / 2.0;
                            let corners: Vec<egui::Pos2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                                .iter()
                                .map(|(sx, sy)| {
                                    let (x, y) = (sx * half.x, sy * half.y);
                                    centre + egui::vec2(x * cos - y * sin, x * sin + y * cos)
                                })
                                .collect();
                            painter.add(egui::Shape::closed_line(corners, egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 160, 255))));
                        }
                    }
                }
                // connection points: of the selected shape, and with the connector tool of the shape under the pointer and the shapes of the drag
                {
                    let mut site_ids: Vec<String> = Vec::new();
                    if let Some(selected) = &self.selected {
                        if self.scene_shape(selected).is_some() {
                            site_ids.push(selected.clone());
                        }
                    }
                    let pointer_scene = response.hover_pos().map(|p| self.screen_to_scene(p, image_rect));
                    if self.tool == Tool::Connector || matches!(&self.drag, Some(DragState { mode: DragMode::ConnEnd, .. })) {
                        let mut candidates: Vec<egui::Pos2> = Vec::new();
                        candidates.extend(pointer_scene);
                        if let Some(DragState { mode: DragMode::Connect, visio_id, end_pointer, .. }) = &self.drag {
                            if !site_ids.contains(visio_id) {
                                site_ids.push(visio_id.clone());
                            }
                            candidates.push(*end_pointer);
                        }
                        for at in candidates {
                            if let Some(id) = self.pick_shape_near(at) {
                                if !site_ids.contains(&id) {
                                    site_ids.push(id);
                                }
                            }
                        }
                    }
                    for id in site_ids {
                        let sites = self.sites_of(&id);
                        let reach = SITE_HIT_PX / self.view_scale().max(0.01);
                        for (_, site) in sites {
                            let at = self.scene_rect_to_screen(egui::Rect::from_min_size(site, egui::Vec2::ZERO), image_rect).min;
                            let hot = pointer_scene.is_some_and(|p| (p - site).length() <= reach);
                            let colour = if hot { egui::Color32::from_rgb(255, 120, 0) } else { egui::Color32::from_rgb(0, 120, 215) };
                            let arm = if hot { 5.0 } else { 3.5 };
                            painter.line_segment([at + egui::vec2(-arm, -arm), at + egui::vec2(arm, arm)], egui::Stroke::new(1.6, colour));
                            painter.line_segment([at + egui::vec2(-arm, arm), at + egui::vec2(arm, -arm)], egui::Stroke::new(1.6, colour));
                        }
                    }
                }
                if let (Some(DragState { mode: DragMode::Draw(kind), start_pointer, end_pointer, .. }), Some(rect)) = (&self.drag, self.preview) {
                    let stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 160, 255));
                    if *kind == 6 {
                        let (a, b) = (self.scene_rect_to_screen(egui::Rect::from_min_size(*start_pointer, egui::Vec2::ZERO), image_rect).min, self.scene_rect_to_screen(egui::Rect::from_min_size(*end_pointer, egui::Vec2::ZERO), image_rect).min);
                        painter.line_segment([a, b], stroke);
                    } else {
                        let screen_rect = self.scene_rect_to_screen(rect, image_rect);
                        match *kind {
                            2 => painter.circle_stroke(screen_rect.center(), screen_rect.width().min(screen_rect.height()) / 2.0, stroke),
                            _ => painter.rect_stroke(screen_rect, 0.0, stroke, egui::StrokeKind::Outside),
                        };
                    }
                }
                if let Some(DragState { mode: DragMode::Connect, start_pointer, end_pointer, .. }) = &self.drag {
                    let at = |p: egui::Pos2| self.scene_rect_to_screen(egui::Rect::from_min_size(p, egui::Vec2::ZERO), image_rect).min;
                    painter.line_segment([at(*start_pointer), at(*end_pointer)], egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 140, 0)));
                }
                if self.drag.is_some() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                } else if hover_rotate {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                } else if let Some(handle) = hover_handle {
                    ui.ctx().set_cursor_icon(handle.cursor());
                } else if response.hovered() {
                    if let Some(pointer) = response.hover_pos() {
                        let scene_pointer = self.screen_to_scene(pointer, image_rect);
                        if self.pick_shape(scene_pointer).is_some() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                    }
                }
            });

        let mut apply_text = None;
        let mut cancel_text = false;
        if let Some(editor) = &mut self.text_editor {
            egui::Window::new("Edit text")
                .collapsible(false)
                .resizable(true)
                .show(&ctx, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut editor.text)
                            .desired_width(360.0)
                            .desired_rows(4),
                    );
                    ui.horizontal(|ui| {
                        ui.toggle_value(&mut editor.bold, egui::RichText::new("B").strong());
                        ui.toggle_value(&mut editor.italic, egui::RichText::new("I").italics());
                        ui.toggle_value(&mut editor.underline, egui::RichText::new("U").underline());
                        ui.add(egui::DragValue::new(&mut editor.font_pt).range(1.0..=400.0).speed(0.5).suffix(" pt"));
                    });
                    ui.horizontal(|ui| {
                        let apply = ui.button("Apply").clicked()
                            || ui.input(|input| {
                                input.modifiers.ctrl && input.key_pressed(egui::Key::Enter)
                            });
                        if apply {
                            let (bold, italic, underline, pt) = editor.initial;
                            let mut format = serde_json::Map::new();
                            if editor.bold != bold {
                                format.insert("bold".to_string(), editor.bold.into());
                            }
                            if editor.italic != italic {
                                format.insert("italic".to_string(), editor.italic.into());
                            }
                            if editor.underline != underline {
                                format.insert("underline".to_string(), editor.underline.into());
                            }
                            if (editor.font_pt - pt).abs() > 1e-9 {
                                format.insert("fontPt".to_string(), editor.font_pt.into());
                            }
                            let format = (!format.is_empty()).then(|| serde_json::Value::Object(format).to_string());
                            apply_text = Some((editor.visio_id.clone(), editor.text.clone(), format));
                        }
                        if ui.button("Cancel").clicked() {
                            cancel_text = true;
                        }
                    });
                    ui.label(
                        egui::RichText::new("Ctrl+Enter applies, Esc closes.")
                            .small()
                            .weak(),
                    );
                });
        }
        if let Some((id, text, format)) = apply_text {
            self.text_editor = None;
            self.apply_text(&ctx, &id, &text, format.as_deref());
        } else if cancel_text {
            self.text_editor = None;
        }

        if self.export_open {
            let mut open = true;
            let mut go = false;
            const FORMATS: [(&str, &str, &str); 6] = [("DXF (CAD outlines, inches, release 12)", "dxf", "DXF files"), ("SVG (the page as a vector picture)", "svg", "SVG files"), ("Web page (one HTML file, a tab per page, live text and links)", "html", "HTML files"), ("Excalidraw (the current page as an editable scene)", "excalidraw", "Excalidraw files"), ("draw.io (every page, uncompressed .drawio)", "drawio", "draw.io files"), ("PDF (every page: vector where possible, a picture otherwise)", "pdf", "PDF files")];
            egui::Window::new("Export").open(&mut open).collapsible(false).show(&ctx, |ui| {
                ui.label("Export the current page as:");
                for (i, (text, _, _)) in FORMATS.iter().enumerate() {
                    ui.radio_value(&mut self.export_format, i, *text);
                }
                if ui.button("Export…").clicked() {
                    go = true;
                }
            });
            if go {
                let (_, ext, label) = FORMATS[self.export_format];
                let page = self.page_index;
                let stem = self.loaded_path.as_ref().and_then(|p| p.file_stem()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "drawing".to_string());
                let made: Result<(Vec<u8>, String), String> = if ext == "pdf" {
                    self.engine.export_pdf(2.0).map(|p| {
                        let what = format!("{} page(s), {} as vector pages{}", p.pages, p.vector_pages, if p.fallbacks.is_empty() { String::new() } else { format!(" ({})", p.fallbacks.join("; ")) });
                        (p.bytes, what)
                    })
                } else if ext == "drawio" {
                    self.engine.export_drawio().map(|xml| {
                        let n = xml.matches("<mxCell ").count();
                        (xml.into_bytes(), format!("{n} cells"))
                    })
                } else if ext == "excalidraw" {
                    self.engine.export_excalidraw(page).and_then(|scene| {
                        let n = scene["elements"].as_array().map_or(0, Vec::len);
                        serde_json::to_string_pretty(&scene).map(|text| (text.into_bytes(), format!("{n} elements"))).map_err(|e| e.to_string())
                    })
                } else if ext == "html" {
                    self.engine.export_webpage().map(|w| (w.html.clone().into_bytes(), format!("{} link(s) in a web page", w.links)))
                } else if ext == "dxf" {
                    self.engine.export_dxf(page).map(|d| (d.dxf.clone().into_bytes(), format!("{} polylines, {} circles, {} texts", d.polylines, d.circles, d.texts)))
                } else {
                    match self.engine.svg(page, true) {
                        Some(Ok(text)) => Ok((text.into_bytes(), "the page".to_string())),
                        Some(Err(error)) => Err(error),
                        None => Err("no document".to_string()),
                    }
                };
                match made {
                    Ok((text, what)) => {
                        if let Some(path) = file_dialog::save_ext(&format!("{stem}.{ext}"), label, ext) {
                            self.status = match fs::write(&path, text) {
                                Ok(()) => format!("Exported {what} to {}", path.display()),
                                Err(error) => format!("Could not save {}: {error}", path.display()),
                            };
                        }
                    }
                    Err(error) => self.status = format!("Cannot export: {error}"),
                }
            }
            self.export_open = open;
        }
        if self.borders_open {
            let mut open = true;
            let mut apply = false;
            let mut remove = false;
            let has_background = self.engine.page_has_background(self.page_index);
            egui::Window::new("Borders & Titles").open(&mut open).collapsible(false).show(&ctx, |ui| {
                egui::Grid::new("borders-grid").num_columns(2).show(ui, |ui| {
                    ui.label("Style");
                    egui::ComboBox::from_id_salt("border-style").selected_text(layoutcraft_engine::borders::BORDER_STYLES[self.border_style].0).show_ui(ui, |ui| {
                        for (i, (name, tip)) in layoutcraft_engine::borders::BORDER_STYLES.iter().enumerate() {
                            ui.selectable_value(&mut self.border_style, i, *name).on_hover_text(*tip);
                        }
                    });
                    ui.end_row();
                    ui.label("Title");
                    ui.add(egui::TextEdit::singleline(&mut self.border_title).desired_width(240.0));
                    ui.end_row();
                });
                ui.label(layoutcraft_engine::borders::BORDER_STYLES[self.border_style].1);
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        apply = true;
                    }
                    if ui.add_enabled(has_background, egui::Button::new("Remove")).clicked() {
                        remove = true;
                    }
                });
            });
            if apply {
                let style = layoutcraft_engine::borders::BORDER_STYLES[self.border_style].0;
                let page = self.page_index;
                let result = self.engine.apply_border(page, style, self.border_title.trim()).map(|_| ());
                let selected = self.selected.clone();
                self.finish_edit(&ctx, result, &format!("Added the border \"{style}\""), selected);
            }
            if remove {
                let page = self.page_index;
                let result = self.engine.remove_border(page);
                let selected = self.selected.clone();
                self.finish_edit(&ctx, result, "Took the background page off this page", selected);
            }
            self.borders_open = open;
        }
        if self.report_open {
            let mut open = true;
            let mut refresh = false;
            let mut save_csv = false;
            let mut keys: Vec<(String, String)> = vec![("id".into(), "Shape ID".into()), ("text".into(), "Text".into())];
            if self.report_all {
                keys.push(("page".into(), "Page".into()));
            }
            keys.extend(self.report_cols.iter().map(|c| (c.key.clone(), c.label.clone())));
            egui::Window::new("Inventory report").open(&mut open).collapsible(false).default_width(640.0).default_height(420.0).show(&ctx, |ui| {
                if ui.checkbox(&mut self.report_all, "All pages").changed() {
                    refresh = true;
                }
                ui.horizontal_wrapped(|ui| {
                    for (key, text) in &keys {
                        let mut on = self.report_override.get(key).copied().unwrap_or(key != "page");
                        if ui.checkbox(&mut on, text).changed() {
                            self.report_override.insert(key.clone(), on);
                        }
                    }
                });
                let columns = self.report_columns();
                ui.horizontal(|ui| {
                    ui.label(format!("{} shape{}", self.report_rows.len(), if self.report_rows.len() == 1 { "" } else { "s" }));
                    if ui.add_enabled(!columns.is_empty(), egui::Button::new("Save CSV…")).clicked() {
                        save_csv = true;
                    }
                });
                egui::ScrollArea::both().max_height(320.0).show(ui, |ui| {
                    egui::Grid::new("report-table").striped(true).show(ui, |ui| {
                        for key in &columns {
                            ui.strong(layoutcraft_engine::inventory::label(&self.report_cols, key));
                        }
                        ui.end_row();
                        // a drawing can have thousands of shapes: the window shows the first 300, the CSV has them all
                        for row in self.report_rows.iter().take(300) {
                            for key in &columns {
                                ui.label(layoutcraft_engine::inventory::cell(row, key));
                            }
                            ui.end_row();
                        }
                    });
                    if self.report_rows.len() > 300 {
                        ui.label(format!("... {} more rows in the CSV", self.report_rows.len() - 300));
                    }
                });
            });
            if refresh {
                self.refresh_report();
            }
            if save_csv {
                let columns = self.report_columns();
                let model = self.engine.json().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
                if let Some(model) = model {
                    let csv = format!("\u{feff}{}", layoutcraft_engine::inventory::inventory_csv(&model, &self.page_names(), self.page_index, &columns, self.report_all));
                    let base = self.loaded_path.as_ref().and_then(|p| p.file_stem()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "drawing".to_string());
                    if let Some(path) = file_dialog::save_csv(&format!("{base}-inventory.csv")) {
                        self.status = match fs::write(&path, csv) {
                            Ok(()) => format!("Saved the report to {}", path.display()),
                            Err(error) => format!("Could not save {}: {error}", path.display()),
                        };
                    }
                }
            }
            self.report_open = open;
        }
        if self.compare_open {
            let mut open = true;
            let mut goto: Option<(usize, String, bool)> = None;
            let mut save_csv = false;
            let count = |kind: &str| self.compare_diffs.iter().filter(|d| d.kind == kind).count();
            let summary = if self.compare_diffs.is_empty() {
                format!("{}: no differences found.", self.compare_name)
            } else {
                format!("{}: {} added, {} removed, {} changed (the open drawing against the file; added = only in the open drawing). Click one to go to it.", self.compare_name, count("added"), count("removed"), count("changed"))
            };
            egui::Window::new("Compare Documents").open(&mut open).collapsible(false).default_width(600.0).default_height(360.0).show(&ctx, |ui| {
                ui.add(egui::Label::new(summary).wrap());
                if ui.add_enabled(!self.compare_diffs.is_empty(), egui::Button::new("Save list…")).clicked() {
                    save_csv = true;
                }
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for d in &self.compare_diffs {
                        let text = format!("{} {}  {}: {} ({})", d.kind, d.what, d.page_name, d.label, d.detail);
                        if d.kind != "removed" && d.page >= 0 {
                            if ui.link(text).clicked() {
                                goto = Some((d.page as usize, d.id.clone(), d.what == "shape" && !d.id.is_empty()));
                            }
                        } else {
                            ui.label(egui::RichText::new(text).weak());
                        }
                    }
                });
            });
            if let Some((page, id, select)) = goto {
                self.go_to_page(&ctx, page);
                if select {
                    self.selected = Some(id);
                }
            }
            if save_csv {
                // a byte order mark first, as the web's file has, so spreadsheets read the text as UTF-8
                let csv = format!("\u{feff}{}", layoutcraft_engine::compare::diffs_to_csv(&self.compare_diffs));
                let base = self.loaded_path.as_ref().and_then(|p| p.file_stem()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "drawing".to_string());
                if let Some(path) = file_dialog::save_csv(&format!("{base}-compare.csv")) {
                    self.status = match fs::write(&path, csv) {
                        Ok(()) => format!("Saved {} difference(s) to {}", self.compare_diffs.len(), path.display()),
                        Err(error) => format!("Could not save {}: {error}", path.display()),
                    };
                }
            }
            self.compare_open = open;
        }
        if self.check_open {
            let mut open = true;
            let mut recheck = false;
            let mut fix: Option<layoutcraft_engine::check::Issue> = None;
            let mut goto: Option<(usize, String)> = None;
            let mut save_csv = false;
            let names: Vec<String> = self.engine.document().map(|document| document.pages.iter().map(|page| page.name.clone()).collect()).unwrap_or_default();
            egui::Window::new("Check Drawing").open(&mut open).collapsible(false).default_width(560.0).default_height(360.0).show(&ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut self.check_alt, "Also list shapes without text and alt text").changed() {
                        recheck = true;
                    }
                    if ui.button("Check again").clicked() {
                        recheck = true;
                    }
                    if ui.add_enabled(!self.check_issues.is_empty(), egui::Button::new("Save list…")).clicked() {
                        save_csv = true;
                    }
                });
                if self.check_issues.is_empty() {
                    ui.label("No problems found.");
                } else {
                    ui.label(format!("{} problem(s)", self.check_issues.len()));
                }
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for issue in &self.check_issues {
                        ui.horizontal(|ui| {
                            let page = issue.page.unwrap_or(0);
                            let page_name = names.get(page).cloned().unwrap_or_default();
                            if ui.link(format!("{}: {} ({page_name})", issue.kind, issue.message)).clicked() {
                                goto = Some((page, issue.id.clone()));
                            }
                            if let Some(label) = &issue.fix {
                                if ui.small_button(label).clicked() {
                                    fix = Some(issue.clone());
                                }
                            }
                        });
                    }
                });
            });
            if recheck {
                self.refresh_check();
            }
            if let Some((page, id)) = goto {
                self.go_to_page(&ctx, page);
                self.selected = Some(id);
            }
            if let Some(issue) = fix {
                self.apply_check_fix(&ctx, &issue);
            }
            if save_csv {
                let csv = layoutcraft_engine::check::issues_to_csv(&self.check_issues, &names);
                match file_dialog::save_csv("check-diagram.csv") {
                    Some(path) => match fs::write(&path, csv) {
                        Ok(()) => self.status = format!("Saved the list to {}", path.display()),
                        Err(error) => self.status = format!("Could not save {}: {error}", path.display()),
                    },
                    None => {}
                }
            }
            self.check_open = open;
        }
        if self.page_setup_open {
            let mut open = true;
            let mut apply = false;
            let info = self.engine.page_size_json(self.page_index).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
            egui::Window::new("Page setup").open(&mut open).collapsible(false).show(&ctx, |ui| {
                egui::Grid::new("page-setup").num_columns(2).show(ui, |ui| {
                    ui.label("Width");
                    ui.add(egui::DragValue::new(&mut self.page_setup_size.0).range(0.5..=200.0).speed(0.05).suffix(" in"));
                    ui.end_row();
                    ui.label("Height");
                    ui.add(egui::DragValue::new(&mut self.page_setup_size.1).range(0.5..=200.0).speed(0.05).suffix(" in"));
                    ui.end_row();
                    if let Some(info) = &info {
                        let orientation = if info["print"]["orientation"].as_u64() == Some(2) { "Landscape" } else { "Portrait" };
                        ui.label("Print orientation");
                        ui.label(orientation);
                        ui.end_row();
                        ui.label("Units");
                        ui.label(if info["metric"].as_bool() == Some(true) { "Metric" } else { "Inches" });
                        ui.end_row();
                    }
                });
                if ui.button("Apply").clicked() {
                    apply = true;
                }
            });
            if apply {
                let text = format!("{},{}", self.page_setup_size.0, self.page_setup_size.1);
                let page = self.page_index;
                let result = self.engine.page_op(6, page, 0, &text).map(|_| ());
                let selected = self.selected.clone();
                self.finish_edit(&ctx, result, "Page size changed", selected);
            }
            self.page_setup_open = open;
        }
        if self.comments_open {
            let mut open = true;
            let mut add = false;
            let list = self.engine.comments_json().ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()).and_then(|value| value.as_array().cloned()).unwrap_or_default();
            egui::Window::new("Comments").open(&mut open).collapsible(false).default_width(420.0).show(&ctx, |ui| {
                if list.is_empty() {
                    ui.label("No comments yet.");
                }
                for comment in &list {
                    let page = comment["page"].as_i64().map_or("-".to_string(), |page| (page + 1).to_string());
                    let shape = comment["shape"].as_str().map_or("the page".to_string(), |shape| format!("shape {shape}"));
                    ui.label(egui::RichText::new(format!("{} on {shape} (page {page}), {}", comment["author"].as_str().unwrap_or(""), comment["date"].as_str().unwrap_or(""))).small().weak());
                    ui.add(egui::Label::new(comment["text"].as_str().unwrap_or("")).wrap());
                    ui.separator();
                }
                ui.label(match &self.selected {
                    Some(id) => format!("New comment on shape {id}:"),
                    None => "New comment on the page:".to_string(),
                });
                ui.add(egui::TextEdit::multiline(&mut self.comment_text).desired_rows(3).desired_width(400.0));
                if ui.add_enabled(!self.comment_text.trim().is_empty(), egui::Button::new("Add comment")).clicked() {
                    add = true;
                }
            });
            if add {
                let shape = self.selected.clone().unwrap_or_default();
                let text = format!("{shape}|Me|ME|{}|{}", iso_now(), self.comment_text.trim().replace('|', "/"));
                let page = self.page_index;
                let result = self.engine.page_op(20, page, 0, &text).map(|_| ());
                if result.is_ok() {
                    self.comment_text.clear();
                }
                let selected = self.selected.clone();
                self.finish_edit(&ctx, result, "Comment added", selected);
            }
            self.comments_open = open;
        }
        self.draw_find(&ctx);
        if self.help_shortcuts_open {
            let mut open = true;
            egui::Window::new("Keyboard shortcuts").open(&mut open).collapsible(false).show(&ctx, |ui| {
                for (group, rows) in help::SHORTCUTS {
                    ui.strong(*group);
                    egui::Grid::new(format!("shortcuts-{group}")).num_columns(2).show(ui, |ui| {
                        for (keys, what) in *rows {
                            ui.monospace(*keys);
                            ui.label(*what);
                            ui.end_row();
                        }
                    });
                    ui.add_space(6.0);
                }
            });
            self.help_shortcuts_open = open;
        }
        if self.help_about_open {
            let mut open = true;
            egui::Window::new(format!("About {}", help::PRODUCT)).open(&mut open).collapsible(false).default_width(420.0).show(&ctx, |ui| {
                ui.heading(help::PRODUCT);
                ui.add(egui::Label::new(help::about_text()).wrap());
            });
            self.help_about_open = open;
        }
        if self.mcp_open {
            let mut open = true;
            let mut copy_text: Option<String> = None;
            egui::Window::new("AI assistant (MCP)").open(&mut open).collapsible(false).default_width(520.0).show(&ctx, |ui| {
                ui.add(egui::Label::new("An AI assistant (an MCP client such as Claude) can read and edit drawings through the LayoutCraft MCP server. The client starts the server itself; nothing listens on a network port. Put the configuration below into the client.").wrap());
                ui.add_space(4.0);
                let bundled = mcpinfo::bundled_server();
                if let Some(exe) = &bundled {
                    ui.add(egui::Label::new(egui::RichText::new(format!("Server: {} (bundled with this app, no node needed)", exe.display())).small()).wrap());
                }
                egui::Grid::new("mcp-inputs").num_columns(2).show(ui, |ui| {
                    if bundled.is_none() {
                        ui.label(format!("Server program ({})", mcpinfo::server_file_name()));
                        ui.add(egui::TextEdit::singleline(&mut self.mcp_server).desired_width(360.0));
                        ui.end_row();
                    }
                    ui.label("Drawings folder");
                    ui.add(egui::TextEdit::singleline(&mut self.mcp_root).desired_width(360.0));
                    ui.end_row();
                });
                let server = match &bundled {
                    Some(exe) => exe.to_string_lossy().into_owned(),
                    None => self.mcp_server.trim().to_string(),
                };
                let config = mcpinfo::mcp_config(&server, self.mcp_root.trim(), "layoutcraft");
                ui.label("Configuration for Claude Desktop and other clients that read a JSON file:");
                let mut json = config.json.clone();
                ui.add(egui::TextEdit::multiline(&mut json).font(egui::TextStyle::Monospace).desired_rows(9).desired_width(500.0).interactive(false));
                if ui.button("Copy configuration").clicked() {
                    copy_text = Some(config.json.clone());
                    self.mcp_message = "Configuration copied.".to_string();
                }
                ui.label("Command for Claude Code:");
                let mut command = config.claude_code.clone();
                ui.add(egui::TextEdit::singleline(&mut command).font(egui::TextStyle::Monospace).desired_width(500.0).interactive(false));
                if ui.button("Copy command").clicked() {
                    copy_text = Some(config.claude_code.clone());
                    self.mcp_message = "Command copied.".to_string();
                }
                if !self.mcp_message.is_empty() {
                    ui.label(&self.mcp_message);
                }
                ui.add_space(4.0);
                ui.add(egui::Label::new(egui::RichText::new("A connected assistant can read and change the files below the drawings folder. Choose a folder that holds only drawings you are willing to let it edit.").small().weak()).wrap());
            });
            if let Some(text) = copy_text {
                ctx.copy_text(text);
            }
            if !open {
                self.mcp_message.clear();
            }
            self.mcp_open = open;
        }
        if self.doc_props_open {
            let props = self.engine.doc_props_json().ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
            let mut open = true;
            egui::Window::new("Document properties").open(&mut open).collapsible(false).default_width(480.0).show(&ctx, |ui| {
                ui.set_max_width(480.0);
                egui::Grid::new("doc-props").num_columns(2).show(ui, |ui| {
                    for (label, key) in [("Title", "title"), ("Subject", "subject"), ("Creator", "creator"), ("Keywords", "keywords"), ("Description", "description"), ("Category", "category"), ("Manager", "manager"), ("Company", "company")] {
                        ui.label(label);
                        let value = props.as_ref().and_then(|props| props.get(key)).and_then(|value| value.as_str()).unwrap_or("");
                        ui.add(egui::Label::new(if value.is_empty() { "-" } else { value }).wrap());
                        ui.end_row();
                    }
                });
                ui.label(egui::RichText::new("Read-only in this version.").small().weak());
            });
            self.doc_props_open = open;
        }

        let mut rename_apply = None;
        let mut rename_cancel = false;
        if let Some(name) = &mut self.page_rename {
            egui::Window::new("Rename page").collapsible(false).show(&ctx, |ui| {
                let edit = ui.text_edit_singleline(name);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() || (edit.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))) {
                        rename_apply = Some(name.clone());
                    }
                    if ui.button("Cancel").clicked() {
                        rename_cancel = true;
                    }
                });
            });
        }
        if let Some(name) = rename_apply {
            self.page_rename = None;
            let page = self.page_index;
            self.run_page_op(&ctx, 3, page, 0, &name, "Renamed page");
        } else if rename_cancel {
            self.page_rename = None;
        }

        match action {
            Some(Action::Load) => self.load_from_input(&ctx),
            Some(Action::OpenDialog) => self.open_dialog(&ctx),
            Some(Action::SaveAs) => self.save_as_dialog(),
            Some(Action::DeleteSelected) => self.delete_selected(&ctx),
            Some(Action::EditText) => self.open_text_editor(),
            Some(Action::ApplyStyle) => self.apply_style(&ctx),
            Some(Action::Arrange(command)) => self.arrange(&ctx, command),
            Some(Action::NewDrawing) => {
                if self.dirty {
                    self.pending_open = Some(PendingOpen::New);
                } else {
                    self.new_drawing(&ctx);
                }
            }
            Some(Action::DocProps) => self.doc_props_open = true,
            Some(Action::HelpShortcuts) => self.help_shortcuts_open = true,
            Some(Action::Find) => self.open_find(),
            Some(Action::HelpAbout) => self.help_about_open = true,
            Some(Action::HelpMcp) => self.mcp_open = true,
            Some(Action::PageSetup) => {
                let size = self.engine.page_size_json(self.page_index).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
                if let Some(size) = size {
                    self.page_setup_size = (size["w"].as_f64().unwrap_or(11.0), size["h"].as_f64().unwrap_or(8.5));
                }
                self.page_setup_open = true;
            }
            Some(Action::Comments) => self.comments_open = true,
            Some(Action::Compare) => self.compare_dialog(),
            Some(Action::ShapesPane) => {
                self.shapes_open = !self.shapes_open;
                if self.shapes_open && self.stencils.is_empty() {
                    self.open_builtin_stencil();
                }
            }
            Some(Action::Export) => self.export_open = true,
            Some(Action::Borders) => self.borders_open = true,
            Some(Action::Reports) => {
                self.report_open = true;
                self.refresh_report();
            }
            Some(Action::CheckDiagram) => {
                self.check_open = true;
                self.refresh_check();
            }
            Some(Action::ZoomIn) => {
                self.zoom = (self.zoom * 1.25).min(16.0);
                self.fit_pending = false;
            }
            Some(Action::ZoomOut) => {
                self.zoom = (self.zoom / 1.25).max(0.05);
                self.fit_pending = false;
            }
            Some(Action::Copy) => self.copy_selected(),
            Some(Action::Cut) => {
                self.copy_selected();
                if self.engine.clipboard_count() > 0 {
                    self.delete_selected(&ctx);
                }
            }
            Some(Action::StyleSelected(ref json)) => self.style_selected(&ctx, json),
            Some(Action::FormatText(ref spec)) => self.format_selected_text(&ctx, spec),
            Some(Action::Paste) => self.paste_clipboard(&ctx),
            Some(Action::Duplicate) => {
                self.copy_selected();
                self.paste_clipboard(&ctx);
            }
            Some(Action::LayerOp(op, index, ref text)) => self.layer_op(&ctx, op, index, text),
            Some(Action::LayerShape(index, on)) => self.layer_shape(&ctx, index, on),
            Some(Action::PageAdd) => self.run_page_op(&ctx, 1, self.page_index, 0, "", "Added page"),
            Some(Action::PageDuplicate) => self.run_page_op(&ctx, 5, self.page_index, 0, "", "Duplicated page"),
            Some(Action::PageDelete) => self.delete_page(&ctx),
            Some(Action::PageRename) => self.open_page_rename(),
            Some(Action::PageMove(direction)) => self.move_page(&ctx, direction),
            Some(Action::Reload) => {
                if self.dirty {
                    self.pending_open = Some(PendingOpen::Reload);
                } else {
                    self.reload(&ctx);
                }
            }
            Some(Action::Page(page)) => self.go_to_page(&ctx, page),
            Some(Action::Fit) => self.fit_pending = true,
            Some(Action::ActualSize) => {
                self.zoom = 1.0;
                self.pan = egui::Vec2::ZERO;
            }
            Some(Action::Undo) => self.undo(&ctx),
            Some(Action::Redo) => self.redo(&ctx),
            Some(Action::SaveCopy) => self.save_copy(),
            Some(Action::ConfirmDiscard) => self.confirm_pending(&ctx),
            Some(Action::CancelDiscard) => self.pending_open = None,
            None => {}
        }
        if matches!(
            action,
            Some(
                Action::Page(_)
                    | Action::Undo
                    | Action::Redo
                    | Action::ConfirmDiscard
                    | Action::Load
                    | Action::Reload
            )
        ) {
            ctx.request_repaint();
        }
    }
}

enum Action {
    Find,
    /// Format the whole text of the selected shapes: `cells` and / or `para` of `Engine::edit` op 13.
    FormatText(serde_json::Value),
    Load,
    OpenDialog,
    Reload,
    SaveAs,
    DeleteSelected,
    EditText,
    ApplyStyle,
    PageAdd,
    PageDuplicate,
    PageDelete,
    PageRename,
    PageMove(i32),
    /// `Engine::page_op` on a layer: op 7 add (text = name), 8 change (text `name|visible|print|lock`), 9 delete; `index` is the layer index.
    LayerOp(i32, usize, String),
    /// Put the selected shape on (`true`) or take it off (`false`) the layer with this index.
    LayerShape(usize, bool),
    NewDrawing,
    DocProps,
    HelpShortcuts,
    HelpAbout,
    HelpMcp,
    PageSetup,
    Comments,
    CheckDiagram,
    Compare,
    Reports,
    Borders,
    Export,
    ShapesPane,
    ZoomIn,
    ZoomOut,
    Copy,
    Cut,
    Paste,
    Duplicate,
    /// An `Engine::edit` op 10 style JSON for every selected shape.
    StyleSelected(String),
    Arrange(ArrangeCmd),
    Page(usize),
    Fit,
    ActualSize,
    Undo,
    Redo,
    SaveCopy,
    ConfirmDiscard,
    CancelDiscard,
}

fn number(value: f64) -> String {
    format!("{value:.6}")
}

fn shape_text(shape: &SceneShape) -> Option<String> {
    let text = &shape.text.as_ref()?;
    let mut out = String::new();
    for paragraph in &text.paragraphs {
        for run in &paragraph.runs {
            out.push_str(&run.text);
        }
        out.push('\n');
    }
    let out = out.trim().to_string();
    if out.is_empty() {
        None
    } else {
        Some(out.chars().take(160).collect())
    }
}
