//! Native LayoutCraft shell: open a VSDX, render a page through the shared
//! SVG/`resvg` path, and move/resize shapes with patch-save and undo.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use eframe::egui;
use layoutcraft_engine::Engine;
use layoutcraft_model::{DataRow, Document, Shape};
use layoutcraft_render::{first_font_name, font_substitution, installed_font_families, render_region_rgba, render_rgba, scene_font_substitutions, scene_from_document_with_media, FontSubstitution, Scene, SceneItem, SceneShape};
use layoutcraft_vsdx::{
    patch_page_cells, ShapeCellPatch,
};

mod appearance;
mod connectors;
mod connpoints;
mod gallery;
mod styles;
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
/// A shape frame lower than this (scene px) may be a mere line under its picture (1-D masters).
const HAIRLINE_FRAME: f32 = 12.0;

/// The outline of a hairline-framed group: `own` (the frame and what the shape itself paints) widened to its `members`, when
/// they make it clearly higher than the frame (the picture of the "Direct QSFP" stencil part is 1.6 times its frame); a real
/// line (a group of lines) keeps its own outline.
fn hairline_outline(frame: egui::Rect, own: egui::Rect, members: impl Iterator<Item = egui::Rect>) -> egui::Rect {
    let all = members.fold(own, |all, member| all.union(member));
    if all.height() > frame.height() * 1.25 {
        all
    } else {
        own
    }
}

#[cfg(test)]
mod small_shape_tests {
    use super::{hairline_outline, Handle, LayoutCraftApp};
    use eframe::egui::{pos2, vec2, Rect};

    /// Stencil parts a few pixels high must stay movable: a press in their middle takes no resize handle, a press on a
    /// corner still does, and a shape of normal size keeps all its handles.
    #[test]
    fn the_middle_of_a_shape_is_never_a_resize_handle() {
        for size in [vec2(23.0, 8.0), vec2(28.0, 8.0), vec2(57.0, 16.0), vec2(30.0, 30.0), vec2(200.0, 6.0), vec2(6.0, 200.0)] {
            let rect = Rect::from_min_size(pos2(100.0, 100.0), size);
            assert_eq!(LayoutCraftApp::handle_at(rect, rect.center()), None, "{size:?}");
            assert_eq!(LayoutCraftApp::handle_at(rect, rect.center() + size / 5.0), None, "{size:?}");
            assert!(LayoutCraftApp::handle_at(rect, rect.max + vec2(1.0, 1.0)) == Some(Handle::Se), "{size:?}");
        }
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(120.0, 60.0));
        for (handle, hit) in LayoutCraftApp::handle_rects(rect) {
            assert!(LayoutCraftApp::handle_at(rect, hit.center()) == Some(handle));
        }
    }

    #[test]
    fn a_hairline_group_is_outlined_around_its_pictures() {
        let frame = Rect::from_min_size(pos2(10.0, 20.0), vec2(5.0, 2.0));
        let picture = Rect::from_min_size(pos2(9.0, 17.0), vec2(8.0, 7.0));
        let outline = hairline_outline(frame, frame, [picture].into_iter());
        assert!(outline.contains_rect(picture) && outline.contains_rect(frame));
        // a picture only somewhat higher than the frame counts too
        let low = Rect::from_min_size(pos2(10.0, 19.4), vec2(5.0, 3.2));
        assert!(hairline_outline(frame, frame, [low].into_iter()).contains_rect(low));
        // members that lie along the line do not widen it
        let along = Rect::from_min_size(pos2(0.0, 20.0), vec2(40.0, 2.0));
        assert_eq!(hairline_outline(frame, frame, [along].into_iter()), frame);
    }
}

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
/// egui ignores an extra Shift when it matches a shortcut, so a Shift binding comes before a plain one on the same key.
/// Plain letters are not shortcuts: typing them on a selected shape edits its text.
const ARRANGE_KEYS: &[(egui::Modifiers, egui::Key, ArrangeCmd)] = &[
    (CTRL_SHIFT, egui::Key::U, ArrangeCmd::Ungroup),
    (egui::Modifiers::CTRL, egui::Key::G, ArrangeCmd::Group),
    (CTRL_SHIFT, egui::Key::F, ArrangeCmd::Order(2)),
    (CTRL_SHIFT, egui::Key::B, ArrangeCmd::Order(3)),
    (egui::Modifiers::CTRL, egui::Key::L, ArrangeCmd::RotateLeft),
    (egui::Modifiers::CTRL, egui::Key::R, ArrangeCmd::RotateQuarter),
    (egui::Modifiers::CTRL, egui::Key::H, ArrangeCmd::Flip(false)),
    (egui::Modifiers::CTRL, egui::Key::J, ArrangeCmd::Flip(true)),
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
    fn turn_and_flip_take_the_ctrl_keys_only() {
        for (key, command) in [(Key::L, ArrangeCmd::RotateLeft), (Key::R, ArrangeCmd::RotateQuarter), (Key::H, ArrangeCmd::Flip(false)), (Key::J, ArrangeCmd::Flip(true))] {
            assert!(press(key, true, false) == Some(command));
        }
        // the letters and brackets alone are typing, not commands
        for key in [Key::H, Key::V, Key::OpenBracket, Key::CloseBracket] {
            assert!(press(key, false, false).is_none());
        }
        // Ctrl+V is paste, never a flip
        assert!(press(Key::V, true, false).is_none());
    }
}

/// What a tool or view shortcut asks for.
#[derive(Clone, Copy, PartialEq)]
enum ViewKey {
    /// Go to the inspector's Style settings.
    StyleSettings,
    /// Go to the inspector's align buttons.
    AlignButtons,
    Tool(Tool),
    FitPage,
    NextPage,
    PreviousPage,
    Find,
    Hyperlink,
    FullScreen,
    SnapDialog,
    Help,
    ZoomIn,
    ZoomOut,
}

/// Tool and view shortcuts for features the app has, with Visio's documented keys (`docs/43-ui-parity.md` 4.1). The draw
/// numbers are the positions in the ribbon's draw tools: 0 rectangle, 2 ellipse, 6 line.
const VIEW_KEYS: &[(egui::Modifiers, egui::Key, ViewKey)] = &[
    // before Ctrl+1: egui ignores an extra Shift; some keyboards report Shift+1 as "!"
    (CTRL_SHIFT, egui::Key::Num1, ViewKey::Tool(Tool::ConnectionPoint)),
    (CTRL_SHIFT, egui::Key::Exclamationmark, ViewKey::Tool(Tool::ConnectionPoint)),
    (egui::Modifiers::CTRL, egui::Key::Num1, ViewKey::Tool(Tool::Select)),
    // the text tool draws a text box
    (egui::Modifiers::CTRL, egui::Key::Num2, ViewKey::Tool(Tool::Draw(5))),
    (egui::Modifiers::CTRL, egui::Key::Num3, ViewKey::Tool(Tool::Connector)),
    (egui::Modifiers::CTRL, egui::Key::Num6, ViewKey::Tool(Tool::Draw(6))),
    (egui::Modifiers::CTRL, egui::Key::Num8, ViewKey::Tool(Tool::Draw(0))),
    (egui::Modifiers::CTRL, egui::Key::Num9, ViewKey::Tool(Tool::Draw(2))),
    (CTRL_SHIFT, egui::Key::W, ViewKey::FitPage),
    (egui::Modifiers::CTRL, egui::Key::PageDown, ViewKey::NextPage),
    (egui::Modifiers::CTRL, egui::Key::PageUp, ViewKey::PreviousPage),
    (egui::Modifiers::CTRL, egui::Key::F, ViewKey::Find),
    (egui::Modifiers::CTRL, egui::Key::K, ViewKey::Hyperlink),
    (egui::Modifiers::NONE, egui::Key::F3, ViewKey::StyleSettings),
    (egui::Modifiers::NONE, egui::Key::F5, ViewKey::FullScreen),
    (egui::Modifiers::NONE, egui::Key::F8, ViewKey::AlignButtons),
    (egui::Modifiers::ALT, egui::Key::F9, ViewKey::SnapDialog),
    // Ctrl+F1 (collapse the ribbon) is taken before these keys are read
    (egui::Modifiers::NONE, egui::Key::F1, ViewKey::Help),
    // the Shift binding first: egui ignores an extra Shift
    (ALT_SHIFT, egui::Key::F6, ViewKey::ZoomOut),
    (egui::Modifiers::ALT, egui::Key::F6, ViewKey::ZoomIn),
];

const ALT_SHIFT: egui::Modifiers = egui::Modifiers { alt: true, ctrl: false, shift: true, mac_cmd: false, command: false };

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
        assert!(press(Key::K, true, false) == Some(ViewKey::Hyperlink));
        assert!(press(Key::F5, false, false) == Some(ViewKey::FullScreen));
        // F9 opens the snap window only with Alt
        assert!(press(Key::F9, false, false).is_none());
        let mut input = egui::InputState::default();
        input.events.push(egui::Event::Key { key: Key::F9, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::ALT });
        assert!(consume_view_key(&mut input) == Some(ViewKey::SnapDialog));
        let alt = |key: Key, shift: bool| {
            let mut input = egui::InputState::default();
            input.events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers { shift, ..Modifiers::ALT } });
            consume_view_key(&mut input)
        };
        assert!(alt(Key::F6, false) == Some(ViewKey::ZoomIn) && alt(Key::F6, true) == Some(ViewKey::ZoomOut));
        assert!(press(Key::F6, false, false).is_none());
        // F1 is help; Ctrl+F1 belongs to the ribbon
        assert!(press(Key::F1, false, false) == Some(ViewKey::Help) && press(Key::F1, true, false).is_none());
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
    all_set(engine, page, ids, "Strikethru")
}

/// Whether the character cell `cell` is 1 over the whole text of every one of the shapes that has text.
fn all_set(engine: &Engine, page: usize, ids: &[String], cell: &str) -> bool {
    let targets = text_targets(engine, page, ids);
    !targets.is_empty()
        && targets.iter().all(|(owner, length)| {
            engine
                .text_state_json(page, owner, 0, *length)
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .is_some_and(|state| state["char"][cell].as_str() == Some("1"))
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

/// The selected shapes "Change Shape" can work on (`web/shell.js` `changeableShapes`): shapes of the page itself that are
/// editable and not locked, and are not connectors. `frames` is `Engine::group_info_json`.
fn changeable_shapes(frames: &str, ids: &[String], is_connector: impl Fn(&str) -> bool) -> Vec<String> {
    let frames: Vec<serde_json::Value> = serde_json::from_str(frames).unwrap_or_default();
    ids.iter()
        .filter(|id| !is_connector(id))
        .filter(|id| {
            frames.iter().any(|frame| {
                frame["id"].as_str() == Some(id.as_str()) && frame["parentId"].is_null() && frame["editable"].as_bool() == Some(true) && frame["locked"].as_bool() != Some(true)
            })
        })
        .cloned()
        .collect()
}

/// Replace each shape with an instance of the stencil's master (`Engine::replace_shape`) as one undo step, all or nothing.
/// Returns the new ids.
fn change_shapes(engine: &mut Engine, page: usize, ids: &[String], stencil: i32, master: &str) -> Result<Vec<String>, String> {
    if ids.is_empty() {
        return Err("select a shape of the page that is not a connector and not locked".to_string());
    }
    let depth = engine.undo_depth();
    let result: Result<Vec<String>, String> = ids.iter().map(|id| engine.replace_shape(page, stencil, &format!("{id}|{master}"))).collect();
    engine.collapse_history(depth);
    if result.is_err() && engine.undo_depth() > depth {
        let _ = engine.undo();
    }
    result
}

#[cfg(test)]
mod change_shape_tests {
    use super::{change_shapes, changeable_shapes};
    use layoutcraft_engine::Engine;

    #[test]
    fn connectors_members_and_locked_shapes_are_not_changed() {
        let frames = r#"[
            {"id":"1","parentId":null,"editable":true,"locked":false},
            {"id":"2","parentId":null,"editable":true,"locked":true},
            {"id":"3","parentId":"1","editable":true,"locked":false},
            {"id":"4","parentId":null,"editable":true,"locked":false}
        ]"#;
        let ids: Vec<String> = ["1", "2", "3", "4"].iter().map(|id| id.to_string()).collect();
        assert_eq!(changeable_shapes(frames, &ids, |id| id == "4"), ["1"]);
    }

    #[test]
    fn several_shapes_change_in_one_undo_step() {
        let mut engine = Engine::default();
        engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
        let ids: Vec<String> = [1.0, 5.0].iter().map(|x| engine.draw(0, 0, *x, 1.0, x + 2.0, 3.0).unwrap()).collect();
        let stencil: serde_json::Value = serde_json::from_str(&engine.stencil_builtin().unwrap()).unwrap();
        let index = stencil["index"].as_i64().unwrap() as i32;
        let master = &stencil["masters"][0]["id"];
        let master = master.as_str().map_or_else(|| master.to_string(), str::to_string);
        let depth = engine.undo_depth();
        let made = change_shapes(&mut engine, 0, &ids, index, &master).unwrap();
        assert_eq!(made.len(), 2);
        assert_eq!(engine.undo_depth(), depth + 1);
        // a shape that does not exist fails the whole change
        let both = [made[0].clone(), "999".to_string()];
        assert!(change_shapes(&mut engine, 0, &both, index, &master).is_err());
        assert_eq!(engine.undo_depth(), depth + 1);
    }
}

/// The paper sizes of the Design tab's Size menu: (name, short side, long side) in inches (`web/shell.html` `designSize`).
const PAGE_SIZES: [(&str, f64, f64); 6] = [("Letter", 8.5, 11.0), ("Legal", 8.5, 14.0), ("Tabloid", 11.0, 17.0), ("A5", 5.8268, 8.2677), ("A4", 8.2677, 11.6929), ("A3", 11.6929, 16.5354)];

/// (width, height) of the shown page from `Engine::page_size_json`.
fn page_dimensions(engine: &Engine, page: usize) -> Option<(f64, f64)> {
    let size: serde_json::Value = serde_json::from_str(&engine.page_size_json(page).ok()?).ok()?;
    Some((size["w"].as_f64()?, size["h"].as_f64()?))
}

/// The new page size when a paper size (short, long) and / or an orientation is picked; what is not picked is kept
/// (`web/shell.js` `applyPageShape`). `None` when nothing would change.
fn shaped_page(current: (f64, f64), paper: Option<(f64, f64)>, portrait: Option<bool>) -> Option<(f64, f64)> {
    let (short, long) = paper.unwrap_or((current.0.min(current.1), current.0.max(current.1)));
    let portrait = portrait.unwrap_or(current.0 <= current.1);
    let (w, h) = if portrait { (short, long) } else { (long, short) };
    ((w - current.0).abs() > 1e-3 || (h - current.1).abs() > 1e-3).then_some((w, h))
}

#[cfg(test)]
mod page_shape_tests {
    use super::{page_dimensions, shaped_page};
    use layoutcraft_engine::Engine;

    #[test]
    fn orientation_swaps_the_sides_and_a_paper_size_keeps_the_orientation() {
        assert_eq!(shaped_page((8.5, 11.0), None, Some(false)), Some((11.0, 8.5)));
        assert_eq!(shaped_page((11.0, 8.5), Some((8.5, 14.0)), None), Some((14.0, 8.5)));
        assert_eq!(shaped_page((8.5, 11.0), Some((8.5, 14.0)), None), Some((8.5, 14.0)));
        assert_eq!(shaped_page((8.5, 11.0), Some((8.5, 11.0)), None), None);
        assert_eq!(shaped_page((8.5, 11.0), None, Some(true)), None);
    }

    #[test]
    fn the_new_size_reaches_the_page() {
        let mut engine = Engine::default();
        engine.new_blank(8.5, 11.0, false, "Page-1").unwrap();
        let (w, h) = shaped_page(page_dimensions(&engine, 0).unwrap(), None, Some(false)).unwrap();
        engine.page_op(6, 0, 0, &format!("{w},{h}")).unwrap();
        let (w, h) = page_dimensions(&engine, 0).unwrap();
        assert!(w > h);
    }
}

/// The theme (name) and colour variant the page shows, from `Engine::themes_json`, as `web/shell.js` `buildThemeGallery`
/// reads it: when some page states a theme of its own, the page's own one counts (none if it states none), else the drawing's.
fn current_theme(info: &serde_json::Value, page: usize) -> (Option<String>, u64) {
    let pages = info["pages"].as_array().cloned().unwrap_or_default();
    if pages.iter().any(|entry| entry["theme"].as_u64().is_some_and(|index| index != 0)) {
        let own = pages.get(page).and_then(|entry| entry["theme"].as_u64());
        let name = info["themes"].as_array().and_then(|themes| themes.iter().find(|theme| theme["index"].as_u64() == own)).and_then(|theme| theme["name"].as_str());
        (name.map(str::to_string), pages.get(page).and_then(|entry| entry["variant"].as_u64()).unwrap_or(0))
    } else {
        (info["current"].as_str().map(str::to_string), info["variant"].as_u64().unwrap_or(0))
    }
}

/// A theme's colours as small squares, from the list of hex codes the engine gives; each has an outline, so that a pale one
/// shows on the menu.
fn theme_swatches(ui: &mut egui::Ui, colours: &serde_json::Value) {
    ui.spacing_mut().item_spacing.x = 2.0;
    for [r, g, b] in colours.as_array().into_iter().flatten().filter_map(|colour| typed_colour(colour.as_str()?)) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(11.0, 11.0), egui::Sense::hover());
        let outline = egui::Stroke::new(1.0, ui.visuals().weak_text_color());
        ui.painter().rect(rect, 0.0, egui::Color32::from_rgb(r, g, b), outline, egui::StrokeKind::Inside);
    }
}

/// A ribbon command with its icon over its name, as high as a group.
fn large_button(ui: &mut egui::Ui, icon: &egui::TextureHandle, label: &str, enabled: bool, selected: bool) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| {
        // names of more than one word may take two lines
        let mut job = egui::text::LayoutJob::simple(label.to_string(), egui::TextStyle::Button.resolve(ui.style()), egui::Color32::PLACEHOLDER, 74.0);
        job.halign = egui::Align::Center;
        let galley = ui.painter().layout_job(job);
        let width = (galley.size().x + 12.0).max(46.0);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, ribbon::ITEMS_HEIGHT), egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, selected, label));
        let visuals = ui.style().interact_selectable(&response, selected);
        if selected || response.hovered() || response.has_focus() {
            ui.painter().rect(rect, visuals.corner_radius, visuals.weak_bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
        }
        let image = egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.top() + 17.0), egui::vec2(28.0, 28.0));
        let tint = if enabled { egui::Color32::WHITE } else { egui::Color32::from_white_alpha(110) };
        ui.painter().image(icon.id(), image, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
        // centred lines are laid out around x = 0
        let at = egui::pos2(rect.center().x, rect.top() + 33.0);
        ui.painter().galley(at, galley, visuals.text_color());
        response
    })
    .inner
}

/// The `Engine::page_op` number that applies a theme (or, with `variant`, a colour variant) to every page or to one page.
fn theme_op(variant: bool, all_pages: bool) -> i32 {
    match (variant, all_pages) {
        (false, true) => 30,
        (true, true) => 31,
        (false, false) => 32,
        (true, false) => 33,
    }
}

#[cfg(test)]
mod theme_tests {
    use super::{current_theme, theme_op};
    use layoutcraft_engine::Engine;

    #[test]
    fn a_theme_and_a_variant_are_applied_and_marked() {
        for all_pages in [true, false] {
            let mut engine = Engine::default();
            engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
            let info = |engine: &Engine| serde_json::from_str::<serde_json::Value>(&engine.themes_json().unwrap()).unwrap();
            let name = info(&engine)["themes"][1]["name"].as_str().unwrap().to_string();
            assert_ne!(current_theme(&info(&engine), 0).0.as_deref(), Some(name.as_str()));
            engine.page_op(theme_op(false, all_pages), 0, 0, &name).unwrap();
            assert_eq!(current_theme(&info(&engine), 0).0.as_deref(), Some(name.as_str()));
            engine.page_op(theme_op(true, all_pages), 0, 0, "2").unwrap();
            assert_eq!(current_theme(&info(&engine), 0), (Some(name), 2));
        }
    }

    #[test]
    fn a_page_without_a_theme_of_its_own_shows_none_when_others_have_one() {
        let info = serde_json::json!({ "themes": [{ "index": 5, "name": "A" }], "current": "A", "variant": 1, "pages": [{ "theme": 5, "variant": 3 }, { "theme": null, "variant": 0 }] });
        assert_eq!(current_theme(&info, 0), (Some("A".to_string()), 3));
        assert_eq!(current_theme(&info, 1), (None, 0));
    }
}

/// The Hyperlink window: the shape's links as `Engine::hyperlinks_json` gives them, edited in place (fields the window
/// does not show are kept).
struct LinkDraft {
    page: usize,
    shape: String,
    links: Vec<serde_json::Value>,
    error: String,
    /// Put the keyboard focus in the first address field on the next frame.
    focus: bool,
}

/// Replace the shape's hyperlinks (`Engine::page_op` 19). Every link needs an address or a sub-address. `Ok(false)` when
/// nothing changed (no undo step).
fn save_links(engine: &mut Engine, page: usize, shape: &str, links: &[serde_json::Value]) -> Result<bool, String> {
    let blank = |link: &serde_json::Value, key: &str| link[key].as_str().unwrap_or("").trim().is_empty();
    if links.iter().any(|link| blank(link, "address") && blank(link, "subAddress")) {
        return Err("A hyperlink needs an address or a sub-address.".to_string());
    }
    let before: Vec<serde_json::Value> = serde_json::from_str(&engine.hyperlinks_json(page, shape)?).map_err(|error| error.to_string())?;
    if before == links {
        return Ok(false);
    }
    let json = serde_json::to_string(links).map_err(|error| error.to_string())?;
    engine.page_op(19, page, 0, &format!("{shape}|{json}"))?;
    Ok(true)
}

fn blank_link() -> serde_json::Value {
    serde_json::json!({ "name": "", "description": "", "address": "", "subAddress": "", "extraInfo": "", "frame": "", "newWindow": false, "default": false, "invisible": false })
}

#[cfg(test)]
mod link_edit_tests {
    use super::{blank_link, parse_links, save_links};
    use layoutcraft_engine::Engine;

    #[test]
    fn links_are_added_kept_and_removed() {
        let mut engine = Engine::default();
        engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
        let id = engine.draw(0, 0, 1.0, 1.0, 3.0, 3.0).unwrap();
        let depth = engine.undo_depth();
        assert!(save_links(&mut engine, 0, &id, &[blank_link()]).is_err());
        assert_eq!(engine.undo_depth(), depth);
        let mut link = blank_link();
        link["address"] = serde_json::json!("https://example.org/");
        assert_eq!(save_links(&mut engine, 0, &id, &[link]), Ok(true));
        let read: Vec<serde_json::Value> = serde_json::from_str(&engine.hyperlinks_json(0, &id).unwrap()).unwrap();
        assert_eq!(parse_links(&serde_json::to_string(&read).unwrap())[0].1, "https://example.org/");
        // saving what was read changes nothing
        assert_eq!(save_links(&mut engine, 0, &id, &read), Ok(false));
        assert_eq!(engine.undo_depth(), depth + 1);
        assert_eq!(save_links(&mut engine, 0, &id, &[]), Ok(true));
        assert!(parse_links(&engine.hyperlinks_json(0, &id).unwrap()).is_empty());
    }
}

/// The Shape Data window: a working copy of the selected shape's rows, read again after every edit or selection change.
#[derive(Default)]
struct DataPane {
    /// (page, shape, undo depth) the rows were read at.
    key: Option<(usize, String, usize)>,
    rows: Vec<DataRow>,
    new_label: String,
    /// The new field's type (an entry of `NEW_FIELD_TYPES`), prompt and, for a list, its choices.
    new_type: u32,
    new_prompt: String,
    new_choices: String,
    /// The name of the field whose type and prompt are being changed.
    changing: Option<String>,
    /// Whether that field is to be hidden.
    new_hidden: bool,
}

/// The types a new field can be given, with the numbers the file stores (`Prop.Type`).
const NEW_FIELD_TYPES: [(u32, &str); 3] = [(0, "Text"), (3, "Yes or no"), (1, "List")];

/// A new row given its type and prompt: a yes-or-no field starts as no, a list at its first choice. `choices` are separated
/// by ";"; a list without any is refused with what to type.
fn typed_data_row(mut row: DataRow, kind: u32, choices: &str, prompt: &str) -> Result<DataRow, String> {
    row.r#type = kind;
    row.prompt = prompt.trim().to_string();
    if kind == 3 {
        row.value = "0".to_string();
    } else if kind == 1 {
        let items: Vec<&str> = choices.split(';').map(str::trim).filter(|item| !item.is_empty()).collect();
        row.value = items.first().ok_or_else(|| "Type the choices of the list, separated by \";\".".to_string())?.to_string();
        row.format = items.join(";");
    }
    Ok(row)
}

/// Write one Shape Data row (`Engine::edit` op 17; the row's own name says which). All fields are sent as they are.
fn write_data_row(engine: &mut Engine, page: usize, shape: &str, row: &DataRow) -> Result<(), String> {
    let sort_key = row.extra.get("sortKey").and_then(|key| key.as_str()).unwrap_or("");
    let json = serde_json::json!({
        "name": row.name, "label": row.label, "value": row.value, "prompt": row.prompt, "format": row.format,
        "type": row.r#type, "invisible": row.invisible != 0, "sortKey": sort_key,
    });
    engine.edit(page, 17, &format!("{shape}\n{json}"), 0.0, 0.0, 0.0, 0.0, 0)
}

/// A new text row for `label`, with a row name made from it that no other row has (`web/shell.js` `dataAdd`).
fn new_data_row(label: &str, rows: &[DataRow]) -> Option<DataRow> {
    let label = label.trim();
    if label.is_empty() {
        return None;
    }
    let mut base: String = label.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if base.starts_with(|c: char| c.is_ascii_digit()) {
        base = format!("Row_{base}");
    }
    let taken = |name: &str| rows.iter().any(|row| row.name.eq_ignore_ascii_case(name));
    let mut name = base.clone();
    let mut number = 2;
    while taken(&name) {
        name = format!("{base}_{number}");
        number += 1;
    }
    serde_json::from_value(serde_json::json!({ "name": name, "label": label })).ok()
}

/// An existing row with another prompt and, when `kind` differs or it is a list, another type or choices. The value is kept
/// where the new type can hold it: yes or no becomes "1" or "0", and a list keeps a value that is one of its choices.
fn changed_data_row(row: &DataRow, kind: u32, choices: &str, prompt: &str) -> Result<DataRow, String> {
    let mut changed = if kind == 1 { typed_data_row(row.clone(), kind, choices, prompt)? } else { row.clone() };
    changed.prompt = prompt.trim().to_string();
    if kind == 1 {
        if changed.format.split(';').any(|item| item == row.value) {
            changed.value = row.value.clone();
        }
    } else if kind != row.r#type {
        changed.r#type = kind;
        changed.format.clear();
        if kind == 3 {
            let yes = row.value == "1" || row.value.eq_ignore_ascii_case("true") || row.value.eq_ignore_ascii_case("yes");
            changed.value = if yes { "1" } else { "0" }.to_string();
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod data_row_tests {
    use super::{changed_data_row, new_data_row, write_data_row};
    use layoutcraft_engine::Engine;
    use layoutcraft_model::DataRow;

    fn rows(engine: &Engine, id: &str) -> Vec<DataRow> {
        let page = &engine.document().unwrap().pages[0];
        page.shapes.iter().find(|shape| shape.visio_id.as_deref() == Some(id)).unwrap().data.clone()
    }

    #[test]
    fn a_new_row_gets_a_free_name() {
        let first = new_data_row("Serial no.", &[]).unwrap();
        assert!(first.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        assert_ne!(new_data_row("Serial no.", std::slice::from_ref(&first)).unwrap().name, first.name);
        assert!(new_data_row("1st", &[]).unwrap().name.starts_with(|c: char| c.is_ascii_alphabetic()));
        assert!(new_data_row("  ", &[]).is_none());
    }

    #[test]
    fn changing_a_type_keeps_what_the_new_type_can_hold_also_in_the_file() {
        let mut engine = Engine::default();
        engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
        let id = engine.draw(0, 0, 1.0, 1.0, 3.0, 3.0).unwrap();
        let mut row = new_data_row("Colour", &[]).unwrap();
        row.value = "Green".to_string();
        row.invisible = 1;
        write_data_row(&mut engine, 0, &id, &row).unwrap();
        let change = |engine: &mut Engine, kind: u32, choices: &str| {
            let now = rows(engine, &id).remove(0);
            write_data_row(engine, 0, &id, &changed_data_row(&now, kind, choices, " Which? ").unwrap()).unwrap();
            rows(engine, &id).remove(0)
        };
        let list = change(&mut engine, 1, "Red;Green");
        assert_eq!((list.r#type, list.format.as_str(), list.value.as_str(), list.prompt.as_str(), list.invisible != 0), (1, "Red;Green", "Green", "Which?", true));
        // a value that is no longer a choice gives way to the first one
        assert_eq!(change(&mut engine, 1, "Blue;Black").value, "Blue");
        // as text again the list of choices is gone from the file
        let plain = change(&mut engine, 0, "");
        assert_eq!((plain.r#type, plain.format.as_str(), plain.value.as_str()), (0, "", "Blue"));
        assert_eq!(change(&mut engine, 3, "").value, "0");
        assert!(changed_data_row(&plain, 1, ";", "").is_err());
    }

    #[test]
    fn editing_a_value_keeps_the_row_hidden_and_a_row_can_be_removed() {
        let mut engine = Engine::default();
        engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
        let id = engine.draw(0, 0, 1.0, 1.0, 3.0, 3.0).unwrap();
        let mut row = new_data_row("Owner", &[]).unwrap();
        row.invisible = 1;
        write_data_row(&mut engine, 0, &id, &row).unwrap();
        let mut row = rows(&engine, &id).remove(0);
        row.value = "Network team".to_string();
        write_data_row(&mut engine, 0, &id, &row).unwrap();
        let read = rows(&engine, &id);
        assert_eq!((read.len(), read[0].value.as_str(), read[0].invisible != 0), (1, "Network team", true));
        engine.edit(0, 18, &format!("{id}\n{}", read[0].name), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
        assert!(rows(&engine, &id).is_empty());
    }
}

/// The inspector's Name field: for which (page, shape, undo depth) the name was read, the text in the field and the name
/// the file has (empty when the shape only has its automatic name).
#[derive(Default)]
struct NameField {
    key: Option<(usize, String, usize)>,
    text: String,
    read: String,
}

/// The frame of a shape of the page as the Size and Position fields show it: pin (inches, y up), size, the pin inside the
/// box, and the angle (radians, counter-clockwise).
#[derive(Clone, Copy, Debug)]
struct SizeFrame {
    pin: (f64, f64),
    size: (f64, f64),
    loc: (f64, f64),
    angle: f64,
}

/// The labels of the Size and Position fields, in order.
const SIZE_FIELDS: [&str; 5] = ["X", "Y", "Width", "Height", "Angle"];
/// The smallest width and height the fields accept, inches (`web/shell.js` `applySizePos`).
const MIN_SIDE_IN: f64 = 0.05;

/// The frame of `id` from `Engine::group_info_json`, or why its size and position cannot be edited.
fn size_frame(frames: &str, id: &str) -> Result<SizeFrame, &'static str> {
    let frames: Vec<serde_json::Value> = serde_json::from_str(frames).map_err(|_| "The shape cannot be read.")?;
    let frame = frames.iter().find(|frame| frame["id"].as_str() == Some(id)).ok_or("The shape cannot be read.")?;
    if !frame["parentId"].is_null() {
        return Err("A shape inside a group cannot be edited here.");
    }
    if frame["locked"].as_bool() == Some(true) {
        return Err("The shape is locked.");
    }
    let pair = |key: &str| Some((frame[key][0].as_f64()?, frame[key][1].as_f64()?));
    match (frame["editable"].as_bool(), pair("pin"), pair("size"), pair("loc"), frame["angle"].as_f64()) {
        (Some(true), Some(pin), Some(size), Some(loc), Some(angle)) => Ok(SizeFrame { pin, size, loc, angle }),
        _ => Err("This shape cannot be edited (a flipped group)."),
    }
}

/// The text of the five fields; lengths in the page unit (`per_inch` of them make an inch), the angle in degrees 0..360.
fn size_texts(frame: SizeFrame, per_inch: f64) -> [String; 5] {
    let number = |value: f64| ((value * 10_000.0).round() / 10_000.0 + 0.0).to_string();
    let degrees = (frame.angle.to_degrees().rem_euclid(360.0) * 1000.0).round() / 1000.0;
    [frame.pin.0, frame.pin.1, frame.size.0, frame.size.1].map(|length| number(length * per_inch)).into_iter().chain([number(degrees % 360.0)]).collect::<Vec<_>>().try_into().unwrap_or_default()
}

/// Write one field (index into `SIZE_FIELDS`): X / Y move the pin there, width / height keep the pin where it is, the angle
/// turns the shape about its pin. One undo step.
fn apply_size_field(engine: &mut Engine, page: usize, id: &str, field: usize, value: f64, per_inch: f64) -> Result<(), String> {
    if !value.is_finite() {
        return Err(format!("{} must be a number", SIZE_FIELDS[field]));
    }
    let frame = size_frame(&engine.group_info_json(page)?, id)?;
    let length = value / per_inch;
    match field {
        0 => engine.edit(page, 2, id, length - frame.pin.0, 0.0, 0.0, 0.0, 0),
        1 => engine.edit(page, 2, id, 0.0, length - frame.pin.1, 0.0, 0.0, 0),
        2 | 3 => {
            if length < MIN_SIDE_IN {
                return Err("a shape must be at least 0.05 in wide and high".to_string());
            }
            let (w, h) = if field == 2 { (length, frame.size.1) } else { (frame.size.0, length) };
            let fraction = |at: f64, side: f64| if side.abs() > 1e-9 { at / side } else { 0.5 };
            engine.edit(page, 3, id, w, h, fraction(frame.loc.0, frame.size.0), fraction(frame.loc.1, frame.size.1), 0)
        }
        _ => engine.edit(page, 7, id, value.rem_euclid(360.0).to_radians(), 0.0, 0.0, 0.0, 0),
    }
}

/// The working copy of the Size and Position fields of the selected shape.
#[derive(Default)]
struct SizePos {
    /// (page, shape) the texts were read for; `None` reads them again.
    key: Option<(usize, String)>,
    /// What the fields hold, and what they held when read; or why there are none.
    fields: Option<Result<([String; 5], [String; 5]), &'static str>>,
    unit: &'static str,
    per_inch: f64,
    /// The field to give the keyboard focus on the next frame.
    focus: Option<usize>,
}

#[cfg(test)]
mod size_pos_tests {
    use super::{apply_size_field, size_frame, SizeFrame};
    use layoutcraft_engine::Engine;

    #[test]
    fn fields_move_resize_and_turn_about_the_pin() {
        let mut engine = Engine::default();
        engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
        let id = engine.draw(0, 0, 1.0, 1.0, 3.0, 2.0).unwrap();
        let frame = |engine: &Engine| -> SizeFrame { size_frame(&engine.group_info_json(0).unwrap(), &id).unwrap() };
        let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
        let start = frame(&engine);
        apply_size_field(&mut engine, 0, &id, 0, 5.0, 1.0).unwrap();
        let moved = frame(&engine);
        assert!(close(moved.pin.0, 5.0) && close(moved.pin.1, start.pin.1));
        // millimetres: 101.6 mm are 4 inches
        apply_size_field(&mut engine, 0, &id, 2, 101.6, 25.4).unwrap();
        let wider = frame(&engine);
        assert!(close(wider.size.0, 4.0) && close(wider.size.1, moved.size.1) && close(wider.pin.0, moved.pin.0));
        apply_size_field(&mut engine, 0, &id, 4, 90.0, 1.0).unwrap();
        let turned = frame(&engine);
        assert!(close(turned.angle, std::f64::consts::FRAC_PI_2) && close(turned.pin.1, wider.pin.1));
        let depth = engine.undo_depth();
        assert!(apply_size_field(&mut engine, 0, &id, 3, 0.01, 1.0).is_err());
        assert_eq!(engine.undo_depth(), depth);
    }

    #[test]
    fn locked_shapes_and_group_members_are_refused() {
        let frames = r#"[{"id":"1","parentId":null,"locked":true,"editable":true},{"id":"2","parentId":"1","locked":false,"editable":true}]"#;
        assert!(size_frame(frames, "1").is_err() && size_frame(frames, "2").is_err() && size_frame(frames, "3").is_err());
    }
}

/// How a text field of a window was left this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Left {
    /// Enter, Tab or a click elsewhere: what was typed counts.
    Apply,
    /// Esc: what was typed is thrown away.
    Revert,
}

fn field_left(ui: &egui::Ui, field: &egui::Response) -> Option<Left> {
    field.lost_focus().then(|| if ui.input(|input| input.key_pressed(egui::Key::Escape)) { Left::Revert } else { Left::Apply })
}

#[cfg(test)]
mod field_left_tests {
    use super::{field_left, Left};
    use eframe::egui::{self, Event, Key, Modifiers};

    /// One frame with a single focused-on-demand text field; what `field_left` says in it.
    fn frame(ctx: &egui::Context, text: &mut String, focus: bool, events: Vec<Event>) -> Option<Left> {
        let mut left = None;
        let output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            let field = ui.text_edit_singleline(text);
            if focus {
                field.request_focus();
            }
            left = field_left(ui, &field);
        });
        // nothing paints here: the texture changes of the frame are not applied, so the output must not be dropped
        std::mem::forget(output);
        left
    }

    fn press(key: Key) -> Vec<Event> {
        vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]
    }

    #[test]
    fn escape_reverts_and_enter_applies() {
        for (key, expected) in [(Key::Escape, Left::Revert), (Key::Enter, Left::Apply)] {
            let ctx = egui::Context::default();
            let mut text = "typed".to_string();
            assert_eq!(frame(&ctx, &mut text, true, Vec::new()), None);
            assert_eq!(frame(&ctx, &mut text, false, Vec::new()), None);
            assert_eq!(frame(&ctx, &mut text, false, press(key)), Some(expected));
        }
    }
}

/// The zoom and pan that make an image of `size` (at zoom 1) as wide as the view of `available`, its top at the top of
/// the view when it is taller than the view, else centred. The zoom stays within the app's zoom range.
fn width_view(size: egui::Vec2, available: egui::Vec2) -> (f32, egui::Vec2) {
    let zoom = (available.x / size.x * 0.98).clamp(0.05, 8.0);
    let margin = available.x * 0.01;
    let down = ((size.y * zoom - available.y) / 2.0 + margin).max(0.0);
    (zoom, egui::vec2(0.0, down))
}

#[cfg(test)]
mod width_view_tests {
    use super::width_view;
    use eframe::egui::vec2;

    #[test]
    fn a_tall_page_fills_the_width_and_shows_its_top() {
        let (size, view) = (vec2(800.0, 3000.0), vec2(1000.0, 600.0));
        let (zoom, pan) = width_view(size, view);
        let width = size.x * zoom;
        assert!(width <= view.x && width > view.x * 0.9);
        // top edge of the image relative to the top edge of the view: just inside it
        let top = view.y / 2.0 + pan.y - size.y * zoom / 2.0;
        assert!(top >= 0.0 && top < view.y * 0.1);
    }

    #[test]
    fn a_wide_page_stays_centred() {
        assert_eq!(width_view(vec2(1000.0, 200.0), vec2(1000.0, 600.0)).1, vec2(0.0, 0.0));
    }
}

/// The width of the left ruler and the height of the top ruler, logical px.
const RULER_BANDS: egui::Vec2 = egui::vec2(34.0, 20.0);
/// The least distance between two numbered marks of a ruler, logical px.
const RULER_SPACING: f64 = 50.0;

/// The unit the rulers count in: (units per inch of paper, name). Metric pages count in millimetres. A drawing with a
/// scale is still counted on the paper, and the name says so.
fn ruler_unit(page_size: &str) -> (f64, &'static str) {
    let size: serde_json::Value = serde_json::from_str(page_size).unwrap_or_default();
    let metric = size["metric"].as_bool() == Some(true);
    let scale = &size["scale"];
    let scaled = match (scale["paper"].as_f64(), scale["drawing"].as_f64()) {
        (Some(paper), Some(drawing)) => (paper - drawing).abs() > 1e-9 || scale["paperUnit"] != scale["drawingUnit"],
        _ => false,
    };
    match (metric, scaled) {
        (false, false) => (1.0, "in"),
        (false, true) => (1.0, "in*"),
        (true, false) => (25.4, "mm"),
        (true, true) => (25.4, "mm*"),
    }
}

/// The numbered marks between `from` and `to` (in the ruler's unit): multiples of the smallest step that keeps them
/// `RULER_SPACING` apart at `px_per_unit`. Inches step by halves of an inch below one, millimetres by 1, 2, 5.
fn ruler_marks(from: f64, to: f64, px_per_unit: f64, metric: bool) -> Vec<f64> {
    if !(px_per_unit > 0.0) || !from.is_finite() || !to.is_finite() || to < from {
        return Vec::new();
    }
    let steps: &[f64] = if metric { &[1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0] } else { &[0.125, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0] };
    let step = steps.iter().copied().find(|step| step * px_per_unit >= RULER_SPACING).unwrap_or_else(|| (RULER_SPACING / px_per_unit / 1000.0).ceil() * 1000.0);
    let first = (from / step).ceil() as i64;
    let last = (to / step).floor() as i64;
    (first..=last).take(1000).map(|at| at as f64 * step).collect()
}

#[cfg(test)]
mod ruler_tests {
    use super::{ruler_marks, ruler_unit, RULER_SPACING};

    #[test]
    fn marks_stay_apart_inside_the_range_and_get_finer_when_zoomed_in() {
        for metric in [false, true] {
            let mut previous = f64::MAX;
            for px_per_unit in [0.5, 4.0, 40.0, 400.0] {
                let marks = ruler_marks(-3.0, 900.0, px_per_unit, metric);
                assert!(marks.iter().all(|mark| (-3.0..=900.0).contains(mark)));
                assert!(marks.contains(&0.0), "the page's zero is marked");
                let step = marks[1] - marks[0];
                assert!(step * px_per_unit >= RULER_SPACING && step <= previous);
                previous = step;
            }
        }
        assert!(ruler_marks(0.0, 10.0, 0.0, false).is_empty());
    }

    #[test]
    fn the_unit_follows_the_page_and_marks_a_scaled_drawing() {
        let size = |metric: bool, drawing: f64| format!(r#"{{"metric":{metric},"scale":{{"paper":1.0,"paperUnit":"in","drawing":{drawing},"drawingUnit":"in"}}}}"#);
        assert_eq!(ruler_unit(&size(false, 1.0)).1, "in");
        assert!(ruler_unit(&size(true, 1.0)).0 > 25.0);
        assert!(ruler_unit(&size(false, 12.0)).1.ends_with('*'));
    }
}

/// A ruler guide as the canvas uses it: its id, whether it is vertical, and its position (x or y) in page inches.
type GuideLine = (String, bool, f64);

/// A guide being dragged: an existing one (with its id) or a new one pulled out of a ruler.
#[derive(Clone, Debug)]
struct GuideDrag {
    id: Option<String>,
    vertical: bool,
    at: f64,
}

/// The horizontal and vertical guides of a page (turned guides are not shown), from `Engine::guides_json`.
fn guide_lines(guides: &str) -> Vec<GuideLine> {
    let guides: Vec<serde_json::Value> = serde_json::from_str(guides).unwrap_or_default();
    guides
        .iter()
        .filter_map(|guide| {
            let vertical = guide["vertical"].as_bool()?;
            let at = guide[if vertical { "x" } else { "y" }].as_f64()?;
            Some((guide["id"].as_str()?.to_string(), vertical, at))
        })
        .collect()
}

/// The guide nearest to the page point (inches), if one is within `tolerance`.
fn guide_near(guides: &[GuideLine], x: f64, y: f64, tolerance: f64) -> Option<GuideLine> {
    let distance = |guide: &GuideLine| (if guide.1 { x } else { y } - guide.2).abs();
    guides.iter().filter(|guide| distance(guide) <= tolerance).min_by(|a, b| distance(a).total_cmp(&distance(b))).cloned()
}

/// Release a dragged guide (`Engine::page_op` 16 add, 17 move, 18 delete): on the page it is added or moved, off the page an
/// existing one is deleted. `None` when there is nothing to do; else the status verb.
fn drop_guide(engine: &mut Engine, page: usize, drag: &GuideDrag, on_page: bool) -> Option<Result<&'static str, String>> {
    let (op, text, verb) = match (&drag.id, on_page) {
        (None, true) => (16, drag.at.to_string(), "Added a guide"),
        (Some(id), true) => (17, format!("{id}|{}", drag.at), "Moved the guide"),
        (Some(id), false) => (18, id.clone(), "Deleted the guide"),
        (None, false) => return None,
    };
    Some(engine.page_op(op, page, usize::from(drag.vertical), &text).map(|_| verb))
}

#[cfg(test)]
mod guide_tests {
    use super::{drop_guide, guide_lines, guide_near, GuideDrag};
    use layoutcraft_engine::Engine;

    #[test]
    fn a_guide_is_added_moved_and_deleted_by_where_it_is_dropped() {
        let mut engine = Engine::default();
        engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
        let guides = |engine: &Engine| guide_lines(&engine.guides_json(0).unwrap());
        let drag = |id: Option<&str>, at: f64| GuideDrag { id: id.map(str::to_string), vertical: true, at };
        assert!(drop_guide(&mut engine, 0, &drag(None, 2.0), false).is_none());
        drop_guide(&mut engine, 0, &drag(None, 2.0), true).unwrap().unwrap();
        let (id, vertical, at) = guides(&engine).remove(0);
        assert!(vertical && (at - 2.0).abs() < 1e-6);
        drop_guide(&mut engine, 0, &drag(Some(&id), 3.0), true).unwrap().unwrap();
        assert!((guides(&engine)[0].2 - 3.0).abs() < 1e-6);
        drop_guide(&mut engine, 0, &drag(Some(&id), 3.0), false).unwrap().unwrap();
        assert!(guides(&engine).is_empty());
    }

    #[test]
    fn the_nearest_guide_within_reach_is_picked_on_its_own_axis() {
        let guides = vec![("v".to_string(), true, 2.0), ("h".to_string(), false, 5.0), ("w".to_string(), true, 2.3)];
        assert_eq!(guide_near(&guides, 2.2, 0.0, 0.25).unwrap().0, "w");
        assert_eq!(guide_near(&guides, 9.0, 5.1, 0.25).unwrap().0, "h");
        assert!(guide_near(&guides, 9.0, 0.0, 0.25).is_none());
    }
}

/// The zoom levels the Zoom menu offers, percent of actual size.
const ZOOM_PERCENTS: [u32; 6] = [50, 75, 100, 150, 200, 400];

/// The size the page is shown at, percent of actual size: `zoom` scales the page image, which has `raster_scale` pixels per
/// scene px (96 per inch).
fn zoom_percent(zoom: f32, raster_scale: f64) -> f64 {
    f64::from(zoom) * raster_scale * 100.0
}

/// The `zoom` that shows the page at `percent` of actual size, within the app's zoom range.
fn zoom_for_percent(percent: f64, raster_scale: f64) -> f32 {
    ((percent / 100.0 / raster_scale.max(1e-6)) as f32).clamp(0.05, 16.0)
}

/// The zoom window: what was typed, why it was refused, and whether the field still has to take the keyboard.
#[derive(Default)]
struct ZoomDraft {
    text: String,
    refused: Option<String>,
    focus: bool,
}

/// A typed zoom level ("150" or "150 %") as a percentage the app can show; otherwise what to type instead.
fn typed_zoom(text: &str, raster_scale: f64) -> Result<f64, String> {
    let (least, most) = (zoom_percent(0.05, raster_scale).ceil(), zoom_percent(16.0, raster_scale).floor());
    let number = text.trim().trim_end_matches('%').trim_end();
    match number.parse::<f64>() {
        Ok(percent) if (least..=most).contains(&percent) => Ok(percent),
        _ => Err(format!("Type a number from {least:.0} to {most:.0}.")),
    }
}

#[cfg(test)]
mod zoom_tests {
    use super::{typed_zoom, zoom_for_percent, zoom_percent, ZOOM_PERCENTS};

    #[test]
    fn a_typed_level_is_taken_only_when_the_app_can_show_it() {
        assert_eq!(typed_zoom(" 125 % ", 1.0), Ok(125.0));
        assert_eq!(typed_zoom("62.5", 1.0), Ok(62.5));
        for refused in ["", "%", "big", "0", "-50", "1000000", "NaN", "inf"] {
            assert!(typed_zoom(refused, 1.0).is_err(), "{refused:?}");
        }
        // what is accepted is shown as typed, not cut to the app's zoom range
        for raster_scale in [0.4, 2.5] {
            for percent in 1..5000 {
                if let Ok(percent) = typed_zoom(&percent.to_string(), raster_scale) {
                    assert!((zoom_percent(zoom_for_percent(percent, raster_scale), raster_scale) - percent).abs() < 0.01, "{percent}");
                }
            }
        }
    }

    #[test]
    fn a_percentage_means_the_same_size_whatever_the_image_resolution() {
        for raster_scale in [0.4, 1.0, 2.5] {
            for percent in ZOOM_PERCENTS {
                let zoom = zoom_for_percent(f64::from(percent), raster_scale);
                assert!((zoom_percent(zoom, raster_scale) - f64::from(percent)).abs() < 0.01);
            }
        }
        // a sharper image of the same view is zoomed less
        assert!(zoom_for_percent(100.0, 2.0) < zoom_for_percent(100.0, 1.0));
    }
}

/// The place a page tab dragged from `from` goes to when it is let go at `x`: the tab whose horizontal span holds `x`, or
/// the first / last place when `x` is left / right of the strip. `None` when nothing would move.
fn tab_drop(spans: &[(f32, f32)], from: usize, x: f32) -> Option<usize> {
    let to = if x < spans.first()?.0 {
        0
    } else if x > spans.last()?.1 {
        spans.len() - 1
    } else {
        spans.iter().position(|(left, right)| x >= *left && x <= *right)?
    };
    (to != from).then_some(to)
}

#[cfg(test)]
mod tab_tests {
    use super::tab_drop;
    use layoutcraft_engine::Engine;

    #[test]
    fn a_dragged_tab_goes_where_it_is_let_go() {
        let spans = [(0.0, 50.0), (60.0, 110.0), (120.0, 170.0)];
        assert_eq!(tab_drop(&spans, 0, 130.0), Some(2));
        assert_eq!(tab_drop(&spans, 2, -20.0), Some(0));
        assert_eq!(tab_drop(&spans, 0, 500.0), Some(2));
        // on itself, or in the gap between two tabs: nothing moves
        assert_eq!(tab_drop(&spans, 1, 80.0), None);
        assert_eq!(tab_drop(&spans, 0, 55.0), None);
        assert_eq!(tab_drop(&[], 0, 1.0), None);
    }

    #[test]
    fn the_move_reorders_the_pages() {
        let mut engine = Engine::default();
        engine.new_blank(11.0, 8.5, false, "First").unwrap();
        engine.page_op(1, 0, 0, "").unwrap();
        let names = |engine: &Engine| serde_json::from_str::<Vec<String>>(&engine.page_names_json().unwrap()).unwrap();
        let before = names(&engine);
        engine.page_op(4, 0, 1, "").unwrap();
        assert_eq!(names(&engine), [before[1].clone(), before[0].clone()]);
    }
}

/// The status bar's zoom slider over `percent` of actual size; the new percentage when the user moves it. A zoom outside
/// the slider's range (a very small fitted page) is left alone.
fn zoom_slider(ui: &mut egui::Ui, percent: f64) -> Option<u32> {
    let mut value = percent;
    let slider = egui::Slider::new(&mut value, 10.0..=800.0).logarithmic(true).show_value(false).clamping(egui::SliderClamping::Edits);
    let response = ui.add(slider).on_hover_text("Zoom");
    (response.changed() && (value - percent).abs() >= 0.5).then(|| value.round() as u32)
}

#[cfg(test)]
mod zoom_slider_tests {
    use super::zoom_slider;
    use eframe::egui::{self, Event, Key, Modifiers};

    /// One frame with the slider; what it asks for.
    fn frame(ctx: &egui::Context, percent: f64, events: Vec<Event>) -> Option<u32> {
        let mut asked = None;
        let output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            asked = zoom_slider(ui, percent);
        });
        // nothing paints here: the texture changes of the frame are not applied, so the output must not be dropped
        std::mem::forget(output);
        asked
    }

    fn press(key: Key) -> Vec<Event> {
        vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]
    }

    #[test]
    fn showing_the_slider_changes_nothing_also_outside_its_range() {
        for percent in [3.0, 100.0, 1200.0] {
            let ctx = egui::Context::default();
            assert_eq!(frame(&ctx, percent, Vec::new()), None);
            assert_eq!(frame(&ctx, percent, Vec::new()), None);
        }
    }

    #[test]
    fn tab_reaches_the_slider_and_an_arrow_key_zooms_in() {
        let ctx = egui::Context::default();
        frame(&ctx, 100.0, Vec::new());
        frame(&ctx, 100.0, press(Key::Tab));
        frame(&ctx, 100.0, Vec::new());
        assert!(frame(&ctx, 100.0, press(Key::ArrowRight)).is_some_and(|percent| percent > 100));
    }
}

/// The view (zoom, pan) after a wheel or pinch over the canvas: a zoom factor (Ctrl+wheel) zooms about the pointer, whose
/// position is given from the middle of the canvas; otherwise the scroll moves the page.
fn wheel_view(zoom: f32, pan: egui::Vec2, pointer: egui::Vec2, factor: f32, scroll: egui::Vec2) -> (f32, egui::Vec2) {
    if (factor - 1.0).abs() > 1e-4 {
        let under = (pointer - pan) / zoom;
        let zoom = (zoom * factor).clamp(0.05, 16.0);
        (zoom, pointer - under * zoom)
    } else {
        (zoom, pan + scroll)
    }
}

/// One click of the magnifier zooms by this factor.
const MAGNIFY_STEP: f32 = 1.5;

/// The view (zoom, pan) that shows a box dragged with the magnifier as large as the canvas allows, in its middle. The box
/// is given from the middle of the canvas.
fn magnified_view(zoom: f32, pan: egui::Vec2, area: egui::Rect, canvas: egui::Vec2) -> (f32, egui::Vec2) {
    let under = (area.center().to_vec2() - pan) / zoom;
    let factor = (canvas.x / area.width().max(1.0)).min(canvas.y / area.height().max(1.0));
    let zoom = (zoom * factor).clamp(0.05, 16.0);
    (zoom, -under * zoom)
}

#[cfg(test)]
mod magnifier_tests {
    use super::magnified_view;
    use eframe::egui::{pos2, vec2, Rect};

    #[test]
    fn the_dragged_box_ends_up_in_the_middle_and_inside_the_canvas() {
        let canvas = vec2(800.0, 600.0);
        for area in [Rect::from_min_max(pos2(-100.0, -40.0), pos2(60.0, 20.0)), Rect::from_min_max(pos2(120.0, -200.0), pos2(160.0, 100.0))] {
            let (zoom, pan) = (1.25, vec2(30.0, -15.0));
            let (new_zoom, new_pan) = magnified_view(zoom, pan, area, canvas);
            assert!(new_zoom > zoom);
            // where the corners of the box are afterwards
            let moved = |point: eframe::egui::Pos2| (point.to_vec2() - pan) / zoom * new_zoom + new_pan;
            let (low, high) = (moved(area.min), moved(area.max));
            assert!((low + high).length() < 0.01, "in the middle: {low:?} {high:?}");
            let size = high - low;
            assert!(size.x <= canvas.x + 0.01 && size.y <= canvas.y + 0.01);
            assert!((size.x - canvas.x).abs() < 0.01 || (size.y - canvas.y).abs() < 0.01, "as large as it fits: {size:?}");
        }
    }
}

/// What a wheel movement means when "Mouse wheel zooms" is on: a plain up or down movement becomes a zoom (up zooms in);
/// a sideways movement still moves the page, and Ctrl+wheel arrives as a zoom already.
fn wheel_zooming(on: bool, factor: f32, scroll: egui::Vec2) -> (f32, egui::Vec2) {
    if on && (factor - 1.0).abs() <= 1e-4 && scroll.y != 0.0 {
        ((scroll.y * 0.003).exp(), egui::vec2(scroll.x, 0.0))
    } else {
        (factor, scroll)
    }
}

#[cfg(test)]
mod wheel_tests {
    use super::{wheel_view, wheel_zooming};
    use eframe::egui::{self, vec2, Event, Modifiers, MouseWheelSource, MouseWheelUnit, TouchPhase};

    #[test]
    fn with_the_wheel_set_to_zoom_up_zooms_in_and_sideways_still_moves() {
        let (up, down) = (wheel_zooming(true, 1.0, vec2(0.0, 40.0)).0, wheel_zooming(true, 1.0, vec2(0.0, -40.0)).0);
        assert!(up > 1.01 && down < 0.99 && (up * down - 1.0).abs() < 1e-5);
        for (on, factor, scroll) in [(false, 1.0, vec2(0.0, 40.0)), (true, 1.0, vec2(25.0, 0.0)), (true, 1.4, vec2(0.0, 0.0))] {
            assert_eq!(wheel_zooming(on, factor, scroll), (factor, scroll));
        }
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer_and_scrolling_only_moves() {
        let (zoom, pan, pointer) = (1.0, vec2(20.0, -10.0), vec2(100.0, 50.0));
        let under = (pointer - pan) / zoom;
        for factor in [1.5, 0.5, 1000.0] {
            let (new_zoom, new_pan) = wheel_view(zoom, pan, pointer, factor, vec2(0.0, 40.0));
            assert!(new_zoom != zoom);
            assert!(((pointer - new_pan) / new_zoom - under).length() < 0.01);
        }
        assert_eq!(wheel_view(zoom, pan, pointer, 1.0, vec2(5.0, 40.0)), (zoom, pan + vec2(5.0, 40.0)));
    }

    /// What egui reports for one wheel notch: (zoom factor, scroll).
    fn notch(modifiers: Modifiers) -> (f32, egui::Vec2) {
        let ctx = egui::Context::default();
        let event = Event::MouseWheel { unit: MouseWheelUnit::Line, delta: vec2(0.0, 1.0), modifiers, phase: TouchPhase::Move, source: MouseWheelSource::Wheel };
        let (mut factor, mut scroll) = (1.0, egui::Vec2::ZERO);
        for events in [Vec::new(), vec![event]] {
            let output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                (factor, scroll) = ui.input(|input| (input.zoom_delta(), input.smooth_scroll_delta));
            });
            // nothing paints here: the texture changes of the frame are not applied, so the output must not be dropped
            std::mem::forget(output);
        }
        (factor, scroll)
    }

    #[test]
    fn the_toolkit_gives_ctrl_wheel_as_zoom_and_the_plain_wheel_as_scroll() {
        let (factor, scroll) = notch(Modifiers::NONE);
        assert!((factor - 1.0).abs() < 1e-4 && scroll.y > 0.0);
        let ctrl = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
        let (factor, scroll) = notch(ctrl);
        assert!(factor > 1.0 && scroll.y.abs() < 1e-4);
        let (factor, scroll) = notch(Modifiers { shift: true, ..Modifiers::NONE });
        assert!((factor - 1.0).abs() < 1e-4 && scroll.x > 0.0 && scroll.y.abs() < 1e-4);
    }
}

/// Takes the Tab presses (without Ctrl or Alt) out of a frame's events, so that the toolkit does not move its focus with
/// them. `Some(true)` for Shift+Tab (backwards), `Some(false)` for Tab.
fn take_shape_tab(events: &mut Vec<egui::Event>) -> Option<bool> {
    let mut taken = None;
    events.retain(|event| match event {
        egui::Event::Key { key: egui::Key::Tab, pressed: true, modifiers, .. } if !modifiers.ctrl && !modifiers.alt && !modifiers.command => {
            taken = Some(modifiers.shift);
            false
        }
        _ => true,
    });
    taken
}

/// The shape after (or before) `current` in `ids`, wrapping round; the first (or last) when `current` is not among them.
fn next_in_cycle(ids: &[String], current: Option<&str>, backwards: bool) -> Option<String> {
    let count = ids.len();
    if count == 0 {
        return None;
    }
    let at = match current.and_then(|current| ids.iter().position(|id| id == current)) {
        Some(at) if backwards => (at + count - 1) % count,
        Some(at) => (at + 1) % count,
        None if backwards => count - 1,
        None => 0,
    };
    Some(ids[at].clone())
}

#[cfg(test)]
mod shape_tab_tests {
    use super::{next_in_cycle, take_shape_tab};
    use eframe::egui::{Event, Key, Modifiers};

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    #[test]
    fn only_tab_and_shift_tab_are_taken() {
        let ctrl = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
        let mut events = vec![key(Key::Tab, ctrl), key(Key::A, Modifiers::NONE)];
        assert_eq!(take_shape_tab(&mut events), None);
        assert_eq!(events.len(), 2);
        events.push(key(Key::Tab, Modifiers::SHIFT));
        assert_eq!(take_shape_tab(&mut events), Some(true));
        assert_eq!(events.len(), 2);
        events.push(key(Key::Tab, Modifiers::NONE));
        assert_eq!(take_shape_tab(&mut events), Some(false));
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn the_cycle_goes_both_ways_and_wraps() {
        let ids: Vec<String> = ["a", "b", "c"].iter().map(|id| id.to_string()).collect();
        let step = |current: Option<&str>, backwards: bool| next_in_cycle(&ids, current, backwards).unwrap();
        assert_eq!((step(Some("a"), false), step(Some("c"), false)), ("b".to_string(), "a".to_string()));
        assert_eq!((step(Some("a"), true), step(Some("gone"), false), step(None, true)), ("c".to_string(), "a".to_string(), "c".to_string()));
        assert_eq!(next_in_cycle(&[], None, false), None);
    }
}

/// What a key that works on the selection asks for.
#[derive(Clone, Copy, PartialEq)]
enum EditKey {
    Arrange(ArrangeCmd),
    /// Text alignment: the `HorzAlign` cell value.
    Align(&'static str),
    /// Switch bold, italic or underline (the `Engine::edit` op 10 name).
    Style(&'static str),
    /// Make the font one point larger or smaller.
    Size(i8),
}

/// Font size keys: Ctrl+Shift with the `>` / `<` keys, which the toolkit reports as the period and comma keys.
const SIZE_KEYS: [(egui::Key, i8); 2] = [(egui::Key::Period, 1), (egui::Key::Comma, -1)];

/// The font sizes, in points, that can be typed into the ribbon.
const FONT_SIZES: std::ops::RangeInclusive<f64> = 1.0..=1000.0;

/// The font size in points one step from the model's size in px (96 per inch; 12 pt when the shape states none).
fn stepped_font_size(px: Option<f64>, delta: f64) -> f64 {
    let points = px.map_or(12.0, |px| (px * 72.0 / 96.0 * 10.0).round() / 10.0);
    (points + delta).max(1.0)
}

/// Text alignment keys. They are taken before the arrange keys: egui ignores an extra Shift when it matches a shortcut, so
/// Ctrl+J (flip) would otherwise swallow Ctrl+Shift+J.
const ALIGN_KEYS: [(egui::Key, &str); 4] = [(egui::Key::L, "0"), (egui::Key::C, "1"), (egui::Key::R, "2"), (egui::Key::J, "3")];
/// Text style keys. They are taken after the arrange keys, which own Ctrl+Shift+B and Ctrl+Shift+U.
const STYLE_KEYS: [(egui::Key, &str); 3] = [(egui::Key::B, "bold"), (egui::Key::I, "italic"), (egui::Key::U, "underline")];

/// Takes the first selection shortcut pressed in this frame out of the input.
fn consume_edit_key(input: &mut egui::InputState) -> Option<EditKey> {
    if let Some((_, cell)) = ALIGN_KEYS.iter().find(|(key, _)| input.consume_key(CTRL_SHIFT, *key)) {
        return Some(EditKey::Align(cell));
    }
    if let Some((_, delta)) = SIZE_KEYS.iter().find(|(key, _)| input.consume_key(CTRL_SHIFT, *key)) {
        return Some(EditKey::Size(*delta));
    }
    if let Some(command) = consume_arrange_key(input) {
        return Some(EditKey::Arrange(command));
    }
    STYLE_KEYS.iter().find(|(key, _)| input.consume_key(egui::Modifiers::CTRL, *key)).map(|(_, name)| EditKey::Style(name))
}

#[cfg(test)]
mod edit_key_tests {
    use super::{consume_edit_key, stepped_font_size, ArrangeCmd, EditKey};
    use eframe::egui::{self, Key, Modifiers};

    fn press(key: Key, shift: bool) -> Option<EditKey> {
        let modifiers = Modifiers { alt: false, ctrl: true, shift, mac_cmd: false, command: true };
        let mut input = egui::InputState::default();
        input.events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
        consume_edit_key(&mut input)
    }

    #[test]
    fn a_font_size_step_starts_from_the_shapes_size_and_stays_positive() {
        // 16 px are 12 pt
        assert!(stepped_font_size(Some(16.0), 1.0) > 12.5 && stepped_font_size(Some(16.0), -1.0) < 11.5);
        assert_eq!(stepped_font_size(None, 1.0), stepped_font_size(Some(16.0), 1.0));
        assert!(stepped_font_size(Some(1.0), -1.0) >= 1.0);
    }

    #[test]
    fn keys_that_differ_only_by_shift_do_not_swallow_each_other() {
        for (key, plain, shifted) in [
            (Key::J, EditKey::Arrange(ArrangeCmd::Flip(true)), EditKey::Align("3")),
            (Key::L, EditKey::Arrange(ArrangeCmd::RotateLeft), EditKey::Align("0")),
            (Key::R, EditKey::Arrange(ArrangeCmd::RotateQuarter), EditKey::Align("2")),
            (Key::B, EditKey::Style("bold"), EditKey::Arrange(ArrangeCmd::Order(3))),
            (Key::U, EditKey::Style("underline"), EditKey::Arrange(ArrangeCmd::Ungroup)),
        ] {
            assert!(press(key, false) == Some(plain));
            assert!(press(key, true) == Some(shifted));
        }
        assert!(press(Key::C, true) == Some(EditKey::Align("1")));
        assert!(press(Key::I, false) == Some(EditKey::Style("italic")));
        assert!(press(Key::Period, true) == Some(EditKey::Size(1)) && press(Key::Comma, true) == Some(EditKey::Size(-1)));
        assert!(press(Key::Period, false).is_none());
    }
}

/// The status bar's geometry text of the selected shape, from the Size and Position fields: only when they were read for
/// that shape on that page and it can be edited.
fn status_geometry(size_pos: &SizePos, page: usize, selected: Option<&str>) -> Option<String> {
    let (read_page, shape) = size_pos.key.as_ref()?;
    if *read_page != page || Some(shape.as_str()) != selected {
        return None;
    }
    let (_, read) = size_pos.fields.as_ref()?.as_ref().ok()?;
    let unit = size_pos.unit;
    Some(format!("Width {} {unit}  Height {} {unit}  Angle {} deg", read[2], read[3], read[4]))
}

#[cfg(test)]
mod status_geometry_tests {
    use super::{status_geometry, SizePos};

    #[test]
    fn only_the_selected_editable_shape_is_described() {
        let texts = ["1", "2", "3.5", "4.5", "90"].map(str::to_string);
        let mut size_pos = SizePos { key: Some((0, "7".to_string())), fields: Some(Ok((texts.clone(), texts))), unit: "mm", per_inch: 25.4, focus: None };
        let text = status_geometry(&size_pos, 0, Some("7")).unwrap();
        assert!(text.contains("3.5 mm") && text.contains("4.5 mm") && text.contains("90 deg"));
        // another shape, another page, nothing selected, or fields about to be read again: nothing stale is shown
        assert!(status_geometry(&size_pos, 0, Some("8")).is_none() && status_geometry(&size_pos, 1, Some("7")).is_none() && status_geometry(&size_pos, 0, None).is_none());
        size_pos.fields = Some(Err("The shape is locked."));
        assert!(status_geometry(&size_pos, 0, Some("7")).is_none());
        size_pos.key = None;
        assert!(status_geometry(&size_pos, 0, Some("7")).is_none());
    }
}

/// One part of the Shape Formulas table: its title and rows of (cell, value, formula).
#[derive(Debug, Clone, PartialEq)]
struct SheetGroup {
    title: String,
    rows: Vec<(String, String, String)>,
}

/// `Engine::shape_sheet_json` as the table shows it: the shape's own cells first, then every section; a section the
/// shape takes from its master says so. A value carries its unit.
fn sheet_groups(json: &str) -> Vec<SheetGroup> {
    let sheet: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let row = |prefix: &str, cell: &serde_json::Value| {
        let text = |key: &str| cell[key].as_str().unwrap_or("").to_string();
        let (value, unit) = (text("value"), text("unit"));
        let value = if unit.is_empty() { value } else { format!("{value} {unit}") };
        (format!("{prefix}{}", text("name")), value, text("formula"))
    };
    let list = |value: &serde_json::Value| value.as_array().cloned().unwrap_or_default();
    let mut groups = Vec::new();
    let cells = list(&sheet["cells"]);
    if !cells.is_empty() {
        groups.push(SheetGroup { title: "Shape".to_string(), rows: cells.iter().map(|cell| row("", cell)).collect() });
    }
    for section in list(&sheet["sections"]) {
        let mut title = section["name"].as_str().unwrap_or("Section").to_string();
        if let Some(index) = section["index"].as_u64() {
            title.push_str(&format!(" {index}"));
        }
        if section["fromMaster"].as_bool() == Some(true) {
            title.push_str(" (from the master)");
        }
        let mut rows: Vec<_> = list(&section["cells"]).iter().map(|cell| row("", cell)).collect();
        for entry in list(&section["rows"]) {
            let name = entry["name"].as_str().map(str::to_string).or_else(|| entry["index"].as_u64().map(|index| index.to_string())).unwrap_or_default();
            let kind = entry["type"].as_str().map(|kind| format!(" {kind}")).unwrap_or_default();
            rows.extend(list(&entry["cells"]).iter().map(|cell| row(&format!("{name}{kind} / "), cell)));
        }
        groups.push(SheetGroup { title, rows });
    }
    groups
}

/// The Shape Formulas window (read only) and what it last read.
#[derive(Default)]
struct SheetPane {
    /// (page, shape, undo depth) the groups were read at.
    key: Option<(usize, String, usize)>,
    groups: Vec<SheetGroup>,
}

#[cfg(test)]
mod sheet_tests {
    use super::sheet_groups;
    use layoutcraft_engine::Engine;

    #[test]
    fn a_drawn_shape_lists_its_size_cells_and_sections() {
        let mut engine = Engine::default();
        engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
        let id = engine.draw(0, 0, 1.0, 1.0, 3.0, 2.0).unwrap();
        let groups = sheet_groups(&engine.shape_sheet_json(0, &id).unwrap());
        let width = groups[0].rows.iter().find(|row| row.0 == "Width").expect("the width cell");
        assert!(width.1.starts_with('2'), "{width:?}");
        // the outline of a drawn rectangle is a section with rows
        assert!(groups.iter().skip(1).any(|group| group.title.starts_with("Geometry") && !group.rows.is_empty()), "{groups:?}");
    }

    #[test]
    fn units_and_inherited_sections_are_said() {
        let json = r#"{"cells":[{"name":"Angle","value":"1.5","unit":"RAD","formula":null}],"sections":[{"name":"User","index":null,"fromMaster":true,"cells":[],"rows":[{"name":"A","index":0,"type":null,"cells":[{"name":"Value","value":"3","formula":"1+2"}]}]}]}"#;
        let groups = sheet_groups(json);
        assert_eq!(groups[0].rows[0], ("Angle".to_string(), "1.5 RAD".to_string(), String::new()));
        assert!(groups[1].title.contains("from the master"));
        assert_eq!(groups[1].rows[0], ("A / Value".to_string(), "3".to_string(), "1+2".to_string()));
        assert!(sheet_groups("nonsense").is_empty());
    }
}

/// A click on the header of stencil `clicked` in the Shapes panel, where only one stencil is open at a time: it opens
/// (closing the other), or closes when it was the open one.
fn stencil_clicked(open: Option<usize>, clicked: usize) -> Option<usize> {
    (open != Some(clicked)).then_some(clicked)
}

#[cfg(test)]
mod stencil_accordion_tests {
    use super::stencil_clicked;

    #[test]
    fn one_stencil_is_open_at_a_time_and_it_can_be_closed() {
        assert_eq!(stencil_clicked(None, 2), Some(2));
        assert_eq!(stencil_clicked(Some(2), 5), Some(5));
        assert_eq!(stencil_clicked(Some(5), 5), None);
    }
}

/// A move held to one axis (Shift): the axis the pointer has gone further along (`raw`) keeps its snapped distance and
/// snap line, the other axis does not move.
fn axis_locked(raw: egui::Vec2, snapped: egui::Vec2, lines: (Option<f32>, Option<f32>)) -> (egui::Vec2, (Option<f32>, Option<f32>)) {
    if raw.x.abs() >= raw.y.abs() {
        (egui::vec2(snapped.x, 0.0), (lines.0, None))
    } else {
        (egui::vec2(0.0, snapped.y), (None, lines.1))
    }
}

#[cfg(test)]
mod axis_lock_tests {
    use super::axis_locked;
    use eframe::egui::vec2;

    #[test]
    fn the_longer_direction_wins_and_only_its_snap_line_stays() {
        // mostly sideways: no vertical movement, even when snapping would have pulled it
        assert_eq!(axis_locked(vec2(-40.0, 9.0), vec2(-38.0, 12.0), (Some(1.0), Some(2.0))), (vec2(-38.0, 0.0), (Some(1.0), None)));
        assert_eq!(axis_locked(vec2(5.0, 30.0), vec2(8.0, 32.0), (Some(1.0), Some(2.0))), (vec2(0.0, 32.0), (None, Some(2.0))));
    }
}

/// Whether a click or a band drag with these keys held adds to (or takes out of) the selection: Shift or Ctrl.
fn adds_to_selection(modifiers: egui::Modifiers) -> bool {
    modifiers.shift || modifiers.command
}

#[cfg(test)]
mod selection_modifier_tests {
    use super::adds_to_selection;
    use eframe::egui::Modifiers;

    #[test]
    fn shift_and_ctrl_add_to_the_selection_and_alt_does_not() {
        assert!(adds_to_selection(Modifiers::SHIFT) && adds_to_selection(Modifiers { ctrl: true, command: true, ..Modifiers::NONE }));
        assert!(!adds_to_selection(Modifiers::NONE) && !adds_to_selection(Modifiers::ALT));
    }
}

/// In the full-screen view, Page Down and Page Up alone change the page: +1 or -1.
fn consume_full_screen_page(input: &mut egui::InputState) -> Option<i32> {
    if input.consume_key(egui::Modifiers::NONE, egui::Key::PageDown) {
        Some(1)
    } else {
        input.consume_key(egui::Modifiers::NONE, egui::Key::PageUp).then_some(-1)
    }
}

#[cfg(test)]
mod full_screen_page_tests {
    use super::consume_full_screen_page;
    use eframe::egui::{self, Key, Modifiers};

    #[test]
    fn page_keys_alone_step_through_the_pages() {
        let press = |key: Key, modifiers: Modifiers| {
            let mut input = egui::InputState::default();
            input.events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
            consume_full_screen_page(&mut input)
        };
        assert_eq!((press(Key::PageDown, Modifiers::NONE), press(Key::PageUp, Modifiers::NONE)), (Some(1), Some(-1)));
        // Ctrl+Page Down is the ordinary page key, read elsewhere
        assert_eq!(press(Key::PageDown, Modifiers { ctrl: true, command: true, ..Modifiers::NONE }), None);
        assert_eq!(press(Key::ArrowDown, Modifiers::NONE), None);
    }
}

/// How many recently opened drawings are remembered.
const RECENT_LIMIT: usize = 8;

/// `%APPDATA%\LayoutCraft\recent.txt` (one path per line, newest first), beside the list of opened stencils.
fn recent_file() -> Option<std::path::PathBuf> {
    stencil_memory_file().map(|file| file.with_file_name("recent.txt"))
}

fn read_recent(file: &Path) -> Vec<std::path::PathBuf> {
    fs::read_to_string(file).unwrap_or_default().lines().filter(|line| !line.trim().is_empty()).take(RECENT_LIMIT).map(std::path::PathBuf::from).collect()
}

/// Put `opened` at the top of the list kept in `file`, once.
fn remember_recent(file: &Path, opened: &Path) -> std::io::Result<()> {
    let mut list = read_recent(file);
    list.retain(|path| path != opened);
    list.insert(0, opened.to_path_buf());
    list.truncate(RECENT_LIMIT);
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder)?;
    }
    fs::write(file, list.iter().map(|path| format!("{}\n", path.display())).collect::<String>())
}

#[cfg(test)]
mod recent_tests {
    use super::{read_recent, remember_recent, RECENT_LIMIT};
    use std::path::PathBuf;

    #[test]
    fn the_newest_is_first_listed_once_and_the_list_stays_short() {
        let folder = std::env::temp_dir().join(format!("layoutcraft-recent-{}", std::process::id()));
        let file = folder.join("recent.txt");
        assert!(read_recent(&file).is_empty());
        for number in 0..RECENT_LIMIT + 3 {
            remember_recent(&file, &PathBuf::from(format!("drawing {number}.vsdx"))).unwrap();
        }
        let again = PathBuf::from("drawing 5.vsdx");
        remember_recent(&file, &again).unwrap();
        let list = read_recent(&file);
        assert_eq!(list.len(), RECENT_LIMIT);
        assert_eq!(list[0], again);
        assert_eq!(list.iter().filter(|path| **path == again).count(), 1);
        std::fs::remove_dir_all(&folder).unwrap();
    }
}

/// The snap strength a session starts with (screen px), and the strengths the Snap and Attach window offers.
const SNAP_REACH_PX: f32 = 8.0;
const SNAP_REACHES: std::ops::RangeInclusive<f32> = 2.0..=30.0;

/// The colour of the marks across equal gaps: a darker orange than the snap guides, readable on white.
const SPACING_MARK: egui::Color32 = egui::Color32::from_rgb(184, 80, 0);

/// A range (low, high) along one axis of the page, inches.
type Span = (f64, f64);

/// The pull towards equal spacing along one axis: the moved box (`along` that axis, `across` the other one) between its
/// nearest neighbour on each side, among the `others` (along, across) that it passes beside. Returns the shift that makes
/// both gaps equal, when it is within `threshold` and the box fits, and the box's centre after the shift.
fn equal_gap(along: Span, across: Span, others: &[(Span, Span)], threshold: f64) -> Option<(f64, f64)> {
    let centre = (along.0 + along.1) / 2.0;
    let beside = || others.iter().filter(|(_, other)| other.0 < across.1 && other.1 > across.0).map(|(span, _)| *span);
    let before = beside().filter(|span| (span.0 + span.1) / 2.0 < centre).map(|span| span.1).max_by(f64::total_cmp)?;
    let after = beside().filter(|span| (span.0 + span.1) / 2.0 > centre).map(|span| span.0).min_by(f64::total_cmp)?;
    let width = along.1 - along.0;
    if after - before < width {
        return None;
    }
    let middle = (before + after) / 2.0;
    let shift = middle - centre;
    (shift.abs() <= threshold).then_some((shift, middle))
}

/// The two equal gaps of a box between its nearest neighbour on each side along one axis, as the ranges from the one
/// neighbour to the box and from the box to the other; `None` unless both gaps are there and match.
fn equal_gap_marks(along: Span, across: Span, others: &[(Span, Span)]) -> Option<[Span; 2]> {
    let centre = (along.0 + along.1) / 2.0;
    let beside = || others.iter().filter(|(_, other)| other.0 < across.1 && other.1 > across.0).map(|(span, _)| *span);
    let before = beside().filter(|span| (span.0 + span.1) / 2.0 < centre).map(|span| span.1).max_by(f64::total_cmp)?;
    let after = beside().filter(|span| (span.0 + span.1) / 2.0 > centre).map(|span| span.0).min_by(f64::total_cmp)?;
    let (first, second) = (along.0 - before, after - along.1);
    // a thousandth of an inch: the box is kept in single precision pixels
    (first > 0.0 && (first - second).abs() < 1e-3).then_some([(before, along.0), (along.1, after)])
}

#[cfg(test)]
mod equal_gap_tests {
    use super::{equal_gap, equal_gap_marks};

    #[test]
    fn equal_gaps_are_marked_and_unequal_or_missing_ones_are_not() {
        let others = [((0.0, 1.0), (0.0, 1.0)), ((5.0, 6.0), (0.5, 1.5))];
        let across = (0.2, 1.2);
        let (shift, _) = equal_gap((2.45, 3.45), across, &others, 0.1).unwrap();
        let [left, right] = equal_gap_marks((2.45 + shift, 3.45 + shift), across, &others).unwrap();
        assert!(left.0 == 1.0 && right.1 == 5.0 && ((left.1 - left.0) - (right.1 - right.0)).abs() < 1e-9);
        // not equal, passing above both, one neighbour only, or touching both without a gap
        assert!(equal_gap_marks((2.45, 3.45), across, &others).is_none());
        assert!(equal_gap_marks((2.5, 3.5), (7.0, 8.0), &others).is_none());
        assert!(equal_gap_marks((2.5, 3.5), across, &others[..1]).is_none());
        assert!(equal_gap_marks((1.0, 5.0), across, &others).is_none());
    }

    #[test]
    fn a_box_between_two_others_is_pulled_to_equal_gaps() {
        let others = [((0.0, 1.0), (0.0, 1.0)), ((5.0, 6.0), (0.5, 1.5))];
        let along = (2.45, 3.45);
        let (shift, centre) = equal_gap(along, (0.2, 1.2), &others, 0.1).unwrap();
        let (left, right) = (along.0 + shift - 1.0, 5.0 - (along.1 + shift));
        assert!((left - right).abs() < 1e-9 && (centre - 3.0).abs() < 1e-9);
        // out of reach, passing above both, only one neighbour, or no room: no pull
        assert!(equal_gap((1.2, 2.2), (0.2, 1.2), &others, 0.1).is_none());
        assert!(equal_gap(along, (7.0, 8.0), &others, 0.1).is_none());
        assert!(equal_gap(along, (0.2, 1.2), &others[..1], 0.1).is_none());
        assert!(equal_gap((0.5, 5.6), (0.2, 1.2), &others, 10.0).is_none());
    }
}

/// The picture "Copy as Picture" puts on the clipboard: the page drawn at twice its size (less for a very large page).
fn page_picture(scene: &Scene) -> Result<egui::ColorImage, String> {
    let (width, height) = scene.raster_size();
    let scale = LayoutCraftApp::raster_scale_for(width as f64, height as f64, 2.0);
    let (width, height, rgba) = render_rgba(scene, scale).map_err(|error| error.to_string())?;
    Ok(egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba))
}

#[cfg(test)]
mod page_picture_tests {
    use super::page_picture;
    use layoutcraft_engine::Engine;

    #[test]
    fn the_picture_is_sharper_than_the_page_and_shows_its_shapes() {
        let mut engine = Engine::default();
        engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
        engine.draw(0, 0, 1.0, 1.0, 3.0, 2.0).unwrap();
        let scene = layoutcraft_render::scene_from_document(engine.document().unwrap(), 0).unwrap();
        let picture = page_picture(&scene).unwrap();
        assert!(picture.size[0] > scene.raster_size().0 as usize);
        assert!(picture.pixels.iter().any(|pixel| *pixel != picture.pixels[0]), "the picture is not one flat colour");
    }
}

/// The whole app, driven without a window: real frames, key presses and pointer drags. Nothing here opens or writes the
/// user's settings (no file is loaded and the stencil panel does not open itself).
#[cfg(test)]
mod app_frame_tests {
    use super::{DataPane, Find, LayoutCraftApp, SheetPane};
    use eframe::egui::{self, pos2, vec2, Event, Key, Modifiers, PointerButton};
    use eframe::App as _;

    struct Driver {
        ctx: egui::Context,
        app: LayoutCraftApp,
        frame: eframe::Frame,
        time: f64,
        modifiers: Modifiers,
        /// What the last frame drew.
        drawn: Vec<egui::epaint::ClippedShape>,
    }

    impl Driver {
        /// A new drawing with two rectangles, shown in a 1400 x 900 window.
        fn new() -> (Self, Vec<String>) {
            let ctx = egui::Context::default();
            let mut app = LayoutCraftApp::new(&eframe::CreationContext::_new_kittest(ctx.clone()), None);
            app.stencil_autoopen_done = true;
            app.engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
            let ids: Vec<String> = [1.0, 6.0].iter().map(|x| app.engine.draw(0, 0, *x, 3.0, x + 2.0, 5.0).unwrap()).collect();
            for id in &ids {
                app.engine.edit(0, 8, &format!("{id}
label"), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
            }
            app.baseline = app.engine.document().cloned();
            app.build_page(&ctx, true);
            let mut driver = Driver { ctx, app, frame: eframe::Frame::_new_kittest(), time: 0.0, modifiers: Modifiers::NONE, drawn: Vec::new() };
            driver.frames(3);
            (driver, ids)
        }

        fn run(&mut self, mut events: Vec<Event>) {
            self.time += 0.1;
            events.insert(0, Event::ModifiersChanged(self.modifiers));
            let mut raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 900.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.app.raw_input_hook(&self.ctx, &mut raw);
            let (app, frame) = (&mut self.app, &mut self.frame);
            let output = self.ctx.run_ui(raw, |ui| app.ui(ui, frame));
            self.drawn = output.shapes.clone();
            // nothing paints here: the texture changes of the frame are not applied, so the output must not be dropped
            std::mem::forget(output);
        }

        /// The boxes of every drawn text that reads exactly `wanted`.
        fn texts(&self, wanted: &str) -> Vec<egui::Rect> {
            self.drawn
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(shown) if shown.galley.text() == wanted => Some(shown.galley.rect.translate(shown.pos.to_vec2())),
                    _ => None,
                })
                .collect()
        }

        /// Where the page's lower left corner is on the screen.
        fn page_corner(&self) -> egui::Pos2 {
            let image = self.app.image_rect(self.app.last_viewport);
            self.app.scene_to_screen(pos2(0.0, (self.app.page_height_in * super::DPI) as f32), image)
        }

        fn frames(&mut self, count: usize) {
            for _ in 0..count {
                self.run(Vec::new());
            }
        }

        fn key(&mut self, key: Key, modifiers: Modifiers) {
            self.modifiers = modifiers;
            self.run(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
            self.run(vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers }]);
            self.modifiers = Modifiers::NONE;
            self.frames(2);
        }

        fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
            let button = |at, pressed, modifiers| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers };
            self.run(vec![Event::PointerMoved(from)]);
            self.run(vec![button(from, true, self.modifiers)]);
            for step in 1..=6 {
                self.run(vec![Event::PointerMoved(from + (to - from) * (step as f32 / 6.0))]);
            }
            self.run(vec![button(to, false, self.modifiers)]);
            self.frames(3);
        }

        /// A press and the start of a drag, then `held` keys going down while it goes on, and the release. (Keys that are
        /// down before the pointer has moved count for the press: Shift would then add to the selection.)
        fn drag_holding(&mut self, from: egui::Pos2, to: egui::Pos2, held: Modifiers) {
            let button = |at, pressed, modifiers| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers };
            self.run(vec![Event::PointerMoved(from)]);
            self.run(vec![button(from, true, Modifiers::NONE)]);
            self.run(vec![Event::PointerMoved(from + (to - from) * (1.0 / 6.0))]);
            self.modifiers = held;
            for step in 2..=6 {
                self.run(vec![Event::PointerMoved(from + (to - from) * (step as f32 / 6.0))]);
            }
            self.run(vec![button(to, false, held)]);
            self.modifiers = Modifiers::NONE;
            self.frames(3);
        }

        fn click(&mut self, at: egui::Pos2, modifiers: Modifiers) {
            self.modifiers = modifiers;
            let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers };
            self.run(vec![Event::PointerMoved(at)]);
            self.run(vec![button(true)]);
            self.run(vec![button(false)]);
            self.modifiers = Modifiers::NONE;
            // long enough that the next click is not read as a double click
            self.time += 1.0;
            self.frames(2);
        }

        /// Typed characters, as the keyboard sends them to a focused field.
        fn write(&mut self, typed: &str) {
            self.run(vec![Event::Text(typed.to_string())]);
            self.frames(1);
        }

        fn width_in(&self, id: &str) -> f64 {
            self.app.doc_shape(id).expect("the shape exists").w / super::DPI
        }

        /// Where the middle of a shape is on the screen.
        fn middle_of(&self, id: &str) -> egui::Pos2 {
            let image = self.app.image_rect(self.app.last_viewport);
            let shape = self.app.scene_shape(id).expect("the shape is drawn");
            self.app.scene_to_screen(LayoutCraftApp::shape_scene_rect(shape).center(), image)
        }

        /// The shape's position in the model (px, y down).
        fn at(&self, id: &str) -> (f64, f64) {
            let shape = self.app.doc_shape(id).expect("the shape exists");
            (shape.x, shape.y)
        }
    }

    const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

    #[test]
    fn every_window_and_view_draws_with_a_shape_selected() {
        let (mut driver, ids) = Driver::new();
        driver.app.selected = Some(ids[0].clone());
        driver.app.find = Some(Find { text: "a".to_string(), ..Find::default() });
        driver.app.data_pane = Some(DataPane::default());
        driver.app.sheet_pane = Some(SheetPane::default());
        driver.app.layers_open = true;
        driver.app.snap_open = true;
        driver.app.help_shortcuts_open = true;
        driver.app.open_links();
        driver.frames(4);
        assert!(driver.app.link_draft.is_some() && driver.app.sheet_pane.as_ref().is_some_and(|pane| !pane.groups.is_empty()));
        driver.app.shapes_minimised = true;
        driver.frames(2);
        driver.app.full_screen = true;
        driver.frames(2);
        // every ribbon tab
        driver.app.full_screen = false;
        for tab in 0..driver.app.ribbon.tabs.len() {
            driver.app.ribbon_tab = tab;
            driver.frames(2);
        }
    }

    #[test]
    fn keys_reach_the_selection_and_the_view() {
        let (mut driver, ids) = Driver::new();
        driver.key(Key::A, CTRL);
        assert_eq!(driver.app.all_selected().len(), 2);
        driver.key(Key::B, CTRL);
        assert!(ids.iter().all(|id| driver.app.doc_shape(id).is_some_and(|shape| shape.bold == Some(true))), "{}", driver.app.status);
        // Tab steps through the shapes after select all
        let first = driver.app.selected.clone();
        driver.key(Key::Tab, Modifiers::NONE);
        assert!(driver.app.selected.is_some() && driver.app.selected != first && driver.app.all_selected().len() == 1);
        driver.key(Key::F5, Modifiers::NONE);
        assert!(driver.app.full_screen);
        driver.key(Key::Escape, Modifiers::NONE);
        assert!(!driver.app.full_screen && driver.app.selected.is_some(), "the first Esc only leaves full screen");
        driver.key(Key::Num3, CTRL);
        assert!(driver.app.tool == super::Tool::Connector);
    }

    #[test]
    fn a_guide_is_dragged_out_of_the_ruler_and_back() {
        let (mut driver, _) = Driver::new();
        let canvas = driver.app.last_viewport;
        assert!(canvas.width() > 100.0, "the canvas was drawn");
        // from the left ruler to an empty part of the canvas: a vertical guide
        let ruler = pos2(canvas.left() - 10.0, canvas.top() + 40.0);
        let inside = pos2(canvas.left() + 40.0, canvas.top() + 40.0);
        driver.drag(ruler, inside);
        assert_eq!(driver.app.guides.len(), 1, "{}", driver.app.status);
        assert!(driver.app.guides[0].1, "vertical");
        // and back onto the ruler: gone
        driver.drag(inside, ruler);
        assert!(driver.app.guides.is_empty(), "{}", driver.app.status);
    }

    #[test]
    fn shift_pressed_during_a_drag_keeps_the_move_on_one_axis() {
        let (mut driver, ids) = Driver::new();
        // free movement: nothing to snap to may bend the result
        (driver.app.snap_grid, driver.app.snap_shapes, driver.app.snap_guides) = (false, false, false);
        let start = driver.at(&ids[0]);
        let from = driver.middle_of(&ids[0]);
        driver.drag_holding(from, from + vec2(90.0, 30.0), Modifiers::SHIFT);
        let straight = driver.at(&ids[0]);
        assert!(straight.0 > start.0 + 10.0 && (straight.1 - start.1).abs() < 0.01, "{start:?} {straight:?} {}", driver.app.status);
        let from = driver.middle_of(&ids[0]);
        driver.drag_holding(from, from + vec2(60.0, 40.0), Modifiers::NONE);
        let free = driver.at(&ids[0]);
        assert!(free.0 > straight.0 + 10.0 && free.1 > straight.1 + 10.0, "{straight:?} {free:?}");
    }

    #[test]
    fn ctrl_click_adds_a_shape_and_takes_it_out_again() {
        let (mut driver, ids) = Driver::new();
        let (first, second) = (driver.middle_of(&ids[0]), driver.middle_of(&ids[1]));
        driver.click(first, Modifiers::NONE);
        assert_eq!(driver.app.all_selected(), [ids[0].clone()], "{}", driver.app.status);
        driver.click(second, CTRL);
        assert_eq!(driver.app.all_selected().len(), 2);
        driver.click(second, CTRL);
        assert_eq!(driver.app.all_selected(), [ids[0].clone()]);
    }

    #[test]
    fn a_clicked_guide_is_deleted_by_the_delete_key() {
        let (mut driver, ids) = Driver::new();
        let canvas = driver.app.last_viewport;
        let inside = pos2(canvas.left() + 40.0, canvas.top() + 40.0);
        driver.drag(pos2(canvas.left() - 10.0, canvas.top() + 40.0), inside);
        assert_eq!(driver.app.guides.len(), 1);
        driver.click(inside, Modifiers::NONE);
        assert!(driver.app.selected_guide.is_some(), "{}", driver.app.status);
        driver.key(Key::Delete, Modifiers::NONE);
        assert!(driver.app.guides.is_empty(), "{}", driver.app.status);
        assert!(ids.iter().all(|id| driver.app.doc_shape(id).is_some()), "no shape went with it");
    }

    #[test]
    fn page_keys_turn_pages_in_full_screen() {
        let (mut driver, _) = Driver::new();
        driver.app.engine.page_op(1, 0, 0, "").unwrap();
        let ctx = driver.ctx.clone();
        driver.app.build_page(&ctx, true);
        driver.frames(2);
        driver.key(Key::F5, Modifiers::NONE);
        driver.key(Key::PageDown, Modifiers::NONE);
        assert_eq!(driver.app.page_index, 1, "{}", driver.app.status);
        driver.key(Key::PageUp, Modifiers::NONE);
        assert_eq!(driver.app.page_index, 0);
    }

    #[test]
    fn find_selects_match_after_match_and_escape_closes_it() {
        let (mut driver, ids) = Driver::new();
        driver.key(Key::F, CTRL);
        assert!(driver.app.find.is_some());
        driver.write("label");
        driver.key(Key::Enter, Modifiers::NONE);
        let first = driver.app.selected.clone().unwrap_or_default();
        driver.key(Key::Enter, Modifiers::NONE);
        let second = driver.app.selected.clone().unwrap_or_default();
        assert!(ids.contains(&first) && ids.contains(&second) && first != second, "{first} {second}");
        driver.key(Key::Escape, Modifiers::NONE);
        assert!(driver.app.find.is_none());
        assert_eq!(driver.app.selected, Some(second), "closing the window keeps the shape that was found");
    }

    #[test]
    fn a_size_field_applies_on_enter_and_escape_only_leaves_it() {
        let (mut driver, ids) = Driver::new();
        driver.app.selected = Some(ids[0].clone());
        driver.frames(3);
        // what a click on the status bar's size does: the Width field takes the keyboard
        driver.app.size_pos.focus = Some(2);
        driver.frames(2);
        driver.key(Key::A, CTRL);
        driver.write("3");
        driver.key(Key::Enter, Modifiers::NONE);
        assert!((driver.width_in(&ids[0]) - 3.0).abs() < 0.01, "{} {}", driver.width_in(&ids[0]), driver.app.status);
        driver.app.size_pos.focus = Some(2);
        driver.frames(2);
        driver.key(Key::A, CTRL);
        driver.write("9");
        driver.key(Key::Escape, Modifiers::NONE);
        assert!((driver.width_in(&ids[0]) - 3.0).abs() < 0.01, "{}", driver.width_in(&ids[0]));
        assert_eq!(driver.app.selected, Some(ids[0].clone()), "Esc in a field does not deselect the shape");
    }

    #[test]
    fn the_text_editor_takes_typing_at_once_and_ctrl_enter_applies_it() {
        let (mut driver, ids) = Driver::new();
        driver.app.selected = Some(ids[0].clone());
        driver.frames(2);
        driver.key(Key::F2, Modifiers::NONE);
        // no click: the characters go straight into the editor
        driver.write("typed");
        assert!(driver.app.text_editor.as_ref().is_some_and(|editor| editor.text.contains("typed")), "{}", driver.app.status);
        driver.key(Key::Enter, CTRL);
        assert!(driver.app.text_editor.is_none());
        assert!(driver.app.doc_shape(&ids[0]).is_some_and(|shape| shape.label.contains("typed")), "{}", driver.app.status);
    }

    #[test]
    fn typing_on_a_selected_shape_edits_its_text() {
        let (mut driver, ids) = Driver::new();
        let first = driver.middle_of(&ids[0]);
        driver.click(first, Modifiers::NONE);
        driver.write("h");
        assert!(driver.app.text_editor.is_some(), "{}", driver.app.status);
        driver.write("ij");
        // the typing carries on where it started, after the text that was there
        assert_eq!(driver.app.text_editor.as_ref().map(|editor| editor.text.clone()), Some("labelhij".to_string()));
        driver.key(Key::Enter, CTRL);
        assert!(driver.app.doc_shape(&ids[0]).is_some_and(|shape| shape.label == "labelhij"), "{}", driver.app.status);
    }

    #[test]
    fn typing_without_a_shape_or_with_two_does_nothing() {
        let (mut driver, ids) = Driver::new();
        let canvas = driver.app.last_viewport;
        driver.click(pos2(canvas.left() + 30.0, canvas.top() + 30.0), Modifiers::NONE);
        driver.write("x");
        assert!(driver.app.text_editor.is_none());
        driver.key(Key::A, CTRL);
        driver.write("x");
        assert!(driver.app.text_editor.is_none() && ids.iter().all(|id| driver.app.doc_shape(id).is_some_and(|shape| shape.label == "label")));
    }

    #[test]
    fn f2_goes_into_the_text_and_back_keeping_it() {
        let (mut driver, ids) = Driver::new();
        driver.app.selected = Some(ids[0].clone());
        driver.frames(2);
        driver.key(Key::F2, Modifiers::NONE);
        driver.write("kept");
        driver.key(Key::F2, Modifiers::NONE);
        assert!(driver.app.text_editor.is_none(), "{}", driver.app.status);
        assert!(driver.app.doc_shape(&ids[0]).is_some_and(|shape| shape.label.contains("kept")), "{}", driver.app.status);
        assert_eq!(driver.app.selected, Some(ids[0].clone()));
    }

    /// The "0" of the top ruler stands over the page's left edge and the "0" of the left ruler beside its bottom edge,
    /// at any zoom and after the page was moved.
    #[test]
    fn the_ruler_zeroes_sit_at_the_lower_left_corner_of_the_page() {
        let (mut driver, _) = Driver::new();
        // fitted, then zoomed in and out with the corner moved to a chosen place inside the canvas
        for (zoom, inset) in [(None, vec2(0.0, 0.0)), (Some(100), vec2(120.0, 90.0)), (Some(50), vec2(310.0, 40.0))] {
            if let Some(percent) = zoom {
                driver.app.zoom = super::zoom_for_percent(f64::from(percent), driver.app.raster_scale);
                driver.app.fit_pending = false;
                driver.app.pan = vec2(0.0, 0.0);
                driver.frames(2);
                let canvas = driver.app.last_viewport;
                let target = pos2(canvas.left() + inset.x, canvas.bottom() - inset.y);
                driver.app.pan = target - driver.page_corner();
            }
            driver.frames(4);
            let corner = driver.page_corner();
            let canvas = driver.app.last_viewport;
            assert!(canvas.contains(corner), "{zoom:?}: the corner {corner:?} is in view {canvas:?}");
            let zeroes = driver.texts("0");
            let top = zeroes.iter().find(|text| text.center().y < canvas.top()).expect("a 0 on the top ruler");
            let left = zeroes.iter().find(|text| text.center().x < canvas.left()).expect("a 0 on the left ruler");
            // a number is written 2 px right of its mark on the top ruler, and 1 px above it on the left one
            assert!((top.left() - 2.0 - corner.x).abs() < 1.5, "{zoom:?}: top {top:?}, corner {corner:?}");
            assert!((left.bottom() + 1.0 - corner.y).abs() < 1.5, "{zoom:?}: left {left:?}, corner {corner:?}");
        }
    }

    #[test]
    fn a_zoom_level_can_be_typed_in_the_zoom_window() {
        let (mut driver, ids) = Driver::new();
        driver.app.selected = Some(ids[0].clone());
        let open = |driver: &mut Driver| {
            driver.app.zoom_draft = Some(super::ZoomDraft { focus: true, ..super::ZoomDraft::default() });
            driver.frames(3);
        };
        let shown = |driver: &Driver| super::zoom_percent(driver.app.zoom, driver.app.raster_scale);
        open(&mut driver);
        driver.write("250");
        driver.key(Key::Enter, Modifiers::NONE);
        assert!(driver.app.zoom_draft.is_none() && (shown(&driver) - 250.0).abs() < 0.01, "{}", shown(&driver));
        // a level that cannot be shown is refused, and said so
        open(&mut driver);
        driver.write("0");
        driver.key(Key::Enter, Modifiers::NONE);
        assert!(driver.app.zoom_draft.as_ref().is_some_and(|draft| draft.refused.is_some()));
        assert!((shown(&driver) - 250.0).abs() < 0.01);
        // Esc closes the window and nothing else
        driver.key(Key::Escape, Modifiers::NONE);
        assert!(driver.app.zoom_draft.is_none() && driver.app.selected == Some(ids[0].clone()));
        assert!(driver.app.doc_shape(&ids[0]).is_some_and(|shape| !shape.label.contains("250")));
    }

    #[test]
    fn a_font_size_can_be_typed_on_the_ribbon() {
        let (mut driver, ids) = Driver::new();
        driver.app.selected = Some(ids[0].clone());
        driver.frames(3);
        let size = |driver: &Driver| driver.app.doc_shape(&ids[0]).and_then(|shape| shape.font_size).map(|px| (px * 72.0 / 96.0).round());
        let enter = |driver: &mut Driver, typed: &str, end: Key| {
            let shown = size(driver).unwrap_or(12.0).to_string();
            // in a window this narrow the font controls are in the drop-down of their group
            if driver.texts(" pt").is_empty() {
                let group = *driver.texts("Font \u{25be}").first().expect("the Font group");
                driver.click(group.center(), Modifiers::NONE);
                driver.frames(2);
            }
            let unit = *driver.texts(" pt").first().expect("the size field");
            let field = *driver.texts(&shown).iter().find(|number| (number.center().y - unit.center().y).abs() < 2.0).unwrap_or_else(|| panic!("the field does not show {shown}"));
            driver.click(field.center(), Modifiers::NONE);
            driver.frames(2);
            driver.write(typed);
            driver.key(end, Modifiers::NONE);
            driver.frames(2);
        };
        enter(&mut driver, "24", Key::Enter);
        assert_eq!(size(&driver), Some(24.0), "{}", driver.app.status);
        // Esc leaves the size, the selection and the text alone
        enter(&mut driver, "48", Key::Escape);
        assert_eq!(size(&driver), Some(24.0), "{}", driver.app.status);
        assert_eq!(driver.app.selected, Some(ids[0].clone()));
        assert!(driver.app.text_editor.is_none() && driver.app.doc_shape(&ids[0]).is_some_and(|shape| !shape.label.contains("48")));
        // a size that cannot be set is refused, and said so
        enter(&mut driver, "0", Key::Enter);
        assert_eq!(size(&driver), Some(24.0));
        assert!(driver.app.status.contains("font size"), "{}", driver.app.status);
    }

    #[test]
    fn ctrl_shift_magnifies_without_touching_the_drawing() {
        let (mut driver, ids) = Driver::new();
        driver.app.selected = Some(ids[1].clone());
        driver.frames(3);
        let held = Modifiers { shift: true, ..CTRL };
        let canvas = driver.app.last_viewport;
        let at = canvas.center() + vec2(60.0, 30.0);
        // the page point under a screen position
        let under = |driver: &Driver, at: egui::Pos2| driver.app.screen_to_scene(at, driver.app.image_rect(driver.app.last_viewport));
        let before = (driver.app.zoom, under(&driver, at), driver.app.engine.undo_depth());
        driver.click(at, held);
        assert!(driver.app.zoom > before.0);
        assert!((under(&driver, at) - before.1).length() < 0.5, "the clicked point stays put");
        assert_eq!(driver.app.selected, Some(ids[1].clone()));
        assert!(driver.app.extra_selected.is_empty());
        // right click: out again, and no menu
        let zoomed = driver.app.zoom;
        driver.modifiers = held;
        let right = |pressed| Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed, modifiers: held };
        driver.run(vec![Event::PointerMoved(at)]);
        driver.run(vec![right(true)]);
        driver.run(vec![right(false)]);
        driver.frames(3);
        assert!(driver.app.zoom < zoomed);
        assert!(driver.texts("Cut").is_empty() && driver.texts("Page setup...").is_empty(), "no menu opened");
        // a dragged box is brought to the middle of the view
        let (from, to) = (canvas.center() - vec2(120.0, 80.0), canvas.center() - vec2(20.0, 20.0));
        let middle = under(&driver, from + (to - from) / 2.0);
        let zoomed = driver.app.zoom;
        driver.drag(from, to);
        driver.modifiers = Modifiers::NONE;
        driver.frames(2);
        assert!(driver.app.zoom > zoomed);
        assert!((under(&driver, canvas.center()) - middle).length() < 1.0);
        assert_eq!(driver.app.engine.undo_depth(), before.2, "nothing was moved or drawn");
        assert_eq!(driver.app.selected, Some(ids[1].clone()));
    }

    #[test]
    fn ctrl_shift_pressed_during_a_drag_leaves_it_a_drag() {
        let (mut driver, ids) = Driver::new();
        (driver.app.snap_grid, driver.app.snap_shapes, driver.app.snap_guides) = (false, false, false);
        let (start, zoom) = (driver.at(&ids[0]), driver.app.zoom);
        let from = driver.middle_of(&ids[0]);
        driver.drag_holding(from, from + vec2(90.0, 30.0), Modifiers { shift: true, ..CTRL });
        let moved = driver.at(&ids[0]);
        assert!(moved.0 > start.0 + 10.0, "{start:?} {moved:?} {}", driver.app.status);
        assert_eq!(driver.app.zoom, zoom);
        assert!(driver.app.engine.undo_depth() > 0 && driver.app.magnify_from.is_none());
        // and the next click is an ordinary one
        let middle = driver.middle_of(&ids[1]);
        driver.click(middle, Modifiers::NONE);
        assert_eq!(driver.app.selected, Some(ids[1].clone()));
    }

    #[test]
    fn a_shape_dragged_to_equal_spacing_shows_both_gaps() {
        let (mut driver, _) = Driver::new();
        // a third shape between the two, off centre
        let id = driver.app.engine.draw(0, 0, 3.2, 3.5, 4.2, 4.5).unwrap();
        driver.app.build_page(&driver.ctx.clone(), false);
        (driver.app.snap_grid, driver.app.snap_guides, driver.app.snap_shapes) = (false, false, true);
        driver.frames(3);
        let marks = |driver: &Driver| {
            driver
                .drawn
                .iter()
                .filter(|clipped| matches!(&clipped.shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == super::SPACING_MARK))
                .count()
        };
        assert_eq!(marks(&driver), 0);
        let from = driver.middle_of(&id);
        let inch = super::DPI as f32 * driver.app.view_scale();
        let to = from + vec2(0.78 * inch, 0.0);
        let button = |at, pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        driver.run(vec![Event::PointerMoved(from)]);
        driver.run(vec![button(from, true)]);
        for step in 1..=6 {
            driver.run(vec![Event::PointerMoved(from + (to - from) * (step as f32 / 6.0))]);
        }
        driver.frames(2);
        // two gaps, each a line and its two bars
        assert_eq!(marks(&driver), 6, "{:?} {}", driver.app.preview, driver.app.status);
        let gaps = driver.app.spacing_marks(driver.app.image_rect(driver.app.last_viewport));
        assert!(gaps.len() == 2 && (gaps[0][0].distance(gaps[0][1]) - gaps[1][0].distance(gaps[1][1])).abs() < 0.5, "{gaps:?}");
        driver.run(vec![button(to, false)]);
        driver.frames(3);
        assert_eq!(marks(&driver), 0, "gone after the drop");
    }

    #[test]
    fn the_view_tab_goes_to_size_and_position() {
        let (mut driver, ids) = Driver::new();
        let choose = |driver: &mut Driver| {
            let tab = *driver.texts("View").first().expect("the View tab");
            driver.click(tab.center(), Modifiers::NONE);
            driver.frames(2);
            let top = driver.app.last_viewport.top();
            let in_ribbon = |driver: &Driver| driver.texts("Size and Position").into_iter().find(|text| text.center().y < top);
            // in a narrow window the command may be inside the drop-down of its group
            if in_ribbon(driver).is_none() {
                let group = *driver.texts("Show \u{25be}").first().expect("the Show group");
                driver.click(group.center(), Modifiers::NONE);
                driver.frames(2);
            }
            let command = driver.texts("Size and Position").into_iter().min_by(|a, b| a.left().total_cmp(&b.left())).expect("the command");
            driver.click(command.center(), Modifiers::NONE);
            driver.frames(3);
        };
        // nothing selected: said, and no field is waiting to take the keyboard later
        choose(&mut driver);
        assert!(driver.app.status.contains("Select a shape") && driver.app.size_pos.focus.is_none(), "{}", driver.app.status);
        driver.app.selected = Some(ids[0].clone());
        driver.frames(3);
        choose(&mut driver);
        let before = driver.at(&ids[0]);
        driver.key(Key::A, CTRL);
        driver.write("4");
        driver.key(Key::Enter, Modifiers::NONE);
        let after = driver.at(&ids[0]);
        assert!((after.0 - before.0).abs() > 1.0 && (after.1 - before.1).abs() < 0.01, "the X field took the typing: {before:?} {after:?} {}", driver.app.status);
    }

    #[test]
    fn a_connect_arrow_offers_shapes_of_the_open_stencil() {
        let (mut driver, ids) = Driver::new();
        driver.app.open_builtin_stencil();
        driver.app.shapes_open = false;
        driver.frames(3);
        let names: Vec<String> = driver.app.stencils[0].masters.iter().map(|master| master.1.clone()).collect();
        assert!(names.len() > 4, "{names:?}");
        // the names over the canvas (the ribbon names some of the same shapes)
        let over_canvas = |driver: &Driver, name: &str| driver.texts(name).into_iter().find(|text| driver.app.last_viewport.contains(text.center()));
        let shown = |driver: &Driver| names.iter().filter(|name| over_canvas(driver, name).is_some()).count();
        assert_eq!(shown(&driver), 0);
        let page = |driver: &Driver| {
            let page = &driver.app.engine.document().unwrap().pages[0];
            (page.shapes.len(), page.connectors.len())
        };
        let before = page(&driver);
        // over the shape the arrows appear; over its right arrow, the list
        let image = driver.app.image_rect(driver.app.last_viewport);
        let frame = driver.app.scene_rect_to_screen(LayoutCraftApp::selection_scene_rect(driver.app.scene_shape(&ids[0]).unwrap()), image);
        driver.run(vec![Event::PointerMoved(frame.center())]);
        driver.frames(2);
        let arrow = pos2(frame.right() + 16.0, frame.center().y);
        driver.run(vec![Event::PointerMoved(arrow)]);
        driver.frames(3);
        assert_eq!(shown(&driver), 4, "the first four shapes of the stencil");
        assert!(over_canvas(&driver, &names[4]).is_none());
        // the pointer can travel to the list, and a click there places that shape and connects it
        let entry = over_canvas(&driver, &names[1]).expect("the second shape").center();
        for step in 1..=6 {
            driver.run(vec![Event::PointerMoved(arrow + (entry - arrow) * (step as f32 / 6.0))]);
        }
        driver.frames(2);
        assert_eq!(shown(&driver), 4, "still there");
        driver.click(entry, Modifiers::NONE);
        assert_eq!(page(&driver), (before.0 + 1, before.1 + 1), "{}", driver.app.status);
        let made = driver.app.selected.clone().expect("the new shape is selected");
        let size = |id: &str| driver.app.doc_shape(id).map(|shape| (shape.w, shape.h)).unwrap();
        assert!(made != ids[0] && size(&made) != size(&ids[0]), "the chosen shape, not a copy");
        // away from the arrows the list goes
        driver.run(vec![Event::PointerMoved(pos2(frame.center().x, driver.app.last_viewport.bottom() - 5.0))]);
        driver.frames(3);
        assert_eq!(shown(&driver), 0);
    }

    #[test]
    fn a_shape_dragged_from_the_panel_onto_a_connect_arrow_is_connected() {
        let (mut driver, ids) = Driver::new();
        driver.app.open_builtin_stencil();
        driver.frames(3);
        // a name short enough to be written out in the panel
        let name = driver.app.stencils[0].masters[2].1.clone();
        let counts = |driver: &Driver| {
            let page = &driver.app.engine.document().unwrap().pages[0];
            (page.shapes.len(), page.connectors.len())
        };
        // press on the shape in the panel, move over a shape of the page, then to `end`, and let go
        let drag_to = |driver: &mut Driver, end: &dyn Fn(egui::Rect) -> egui::Pos2| {
            let canvas = driver.app.last_viewport;
            let entry = driver.texts(&name).into_iter().find(|text| text.center().x < canvas.left()).expect("the shape in the panel").center();
            let image = driver.app.image_rect(canvas);
            let frame = driver.app.scene_rect_to_screen(LayoutCraftApp::selection_scene_rect(driver.app.scene_shape(&ids[0]).unwrap()), image);
            let end = end(frame);
            let button = |at, pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            driver.run(vec![Event::PointerMoved(entry)]);
            driver.run(vec![button(entry, true)]);
            for (from, to) in [(entry, frame.center()), (frame.center(), end)] {
                for step in 1..=6 {
                    driver.run(vec![Event::PointerMoved(from + (to - from) * (step as f32 / 6.0))]);
                }
            }
            driver.frames(2);
            driver.run(vec![button(end, false)]);
            driver.frames(3);
        };
        let before = counts(&driver);
        drag_to(&mut driver, &|frame| pos2(frame.right() + 16.0, frame.center().y));
        assert_eq!(counts(&driver), (before.0 + 1, before.1 + 1), "{}", driver.app.status);
        let made = driver.app.selected.clone().expect("the new shape is selected");
        assert!(driver.at(&made).0 > driver.at(&ids[0]).0, "on the right of the shape");
        // let go beside the arrows: placed, not connected
        let before = counts(&driver);
        drag_to(&mut driver, &|frame| pos2(frame.center().x, frame.bottom() + 120.0));
        assert_eq!(counts(&driver), (before.0 + 1, before.1), "{}", driver.app.status);
    }

    #[test]
    fn a_new_field_takes_a_type_and_a_prompt() {
        let (mut driver, ids) = Driver::new();
        driver.app.selected = Some(ids[0].clone());
        driver.app.data_pane = Some(DataPane::default());
        driver.frames(4);
        let add = |driver: &mut Driver, label: &str, kind: u32, choices: &str| {
            let pane = driver.app.data_pane.as_mut().unwrap();
            (pane.new_label, pane.new_type, pane.new_choices, pane.new_prompt) = (label.to_string(), kind, choices.to_string(), " Who? ".to_string());
            driver.frames(3);
            let button = *driver.texts("Add").first().expect("the Add button");
            driver.click(button.center(), Modifiers::NONE);
            driver.frames(3);
        };
        let rows = |driver: &Driver| driver.app.doc_shape(&ids[0]).map(|shape| shape.data.clone()).unwrap_or_default();
        add(&mut driver, "Approved", 3, "");
        let first = rows(&driver);
        assert_eq!((first.len(), first[0].r#type, first[0].value.as_str(), first[0].prompt.as_str()), (1, 3, "0", "Who?"), "{}", driver.app.status);
        // a list needs choices; without them nothing is written and the typing stays
        add(&mut driver, "Colour", 1, " ; ");
        assert_eq!(rows(&driver).len(), 1);
        assert!(driver.app.status.contains("choices") && driver.app.data_pane.as_ref().unwrap().new_label == "Colour", "{}", driver.app.status);
        add(&mut driver, "Colour", 1, "Red; Green ;");
        let all = rows(&driver);
        let colour = all.iter().find(|row| row.label == "Colour").expect("the list field");
        assert_eq!((colour.r#type, colour.format.as_str(), colour.value.as_str()), (1, "Red;Green", "Red"));
    }

    #[test]
    fn the_home_tab_picks_the_pointer_reverses_a_connector_and_sets_its_arrowhead() {
        let (mut driver, ids) = Driver::new();
        let connector = driver.app.engine.add_connector(0, &ids[0], None, &ids[1], None).unwrap();
        driver.app.build_page(&driver.ctx.clone(), false);
        driver.frames(3);
        // click a command of the ribbon by what it says, opening the drop-downs on the way
        let choose = |driver: &mut Driver, path: &[&str]| {
            for label in path {
                let top = driver.app.last_viewport.top();
                let found = driver.texts(label).into_iter().filter(|text| text.left() >= 0.0).min_by(|a, b| a.top().total_cmp(&b.top()));
                let at = found.unwrap_or_else(|| panic!("nothing says {label:?} (canvas from {top})"));
                driver.click(at.center(), Modifiers::NONE);
                driver.frames(3);
            }
        };
        driver.app.tool = super::Tool::Draw(0);
        driver.frames(2);
        choose(&mut driver, &["Select"]);
        assert!(driver.app.tool == super::Tool::Select);

        driver.app.selected = Some(connector.clone());
        driver.frames(3);
        let ends = driver.app.connector_end_points(&connector).expect("the connector is drawn");
        choose(&mut driver, &["Rotate \u{25be}", "Reverse Ends"]);
        let turned = driver.app.connector_end_points(&connector).expect("still drawn");
        assert!(turned.0.distance(ends.1) < 1.0 && turned.1.distance(ends.0) < 1.0, "{ends:?} {turned:?} {}", driver.app.status);

        let arrow = |driver: &Driver| serde_json::from_str::<serde_json::Value>(&driver.app.engine.style_info_json(0, &connector).unwrap()).unwrap()["endArrow"].as_f64();
        assert_ne!(arrow(&driver), Some(14.0));
        choose(&mut driver, &["Styles \u{25be}", "Line ends \u{25be}", "End arrowhead", "Hollow triangle"]);
        assert_eq!(arrow(&driver), Some(14.0), "{}", driver.app.status);
    }

    #[test]
    fn another_fill_colour_can_be_typed() {
        let (mut driver, ids) = Driver::new();
        driver.app.selected = Some(ids[0].clone());
        driver.frames(3);
        for label in ["Styles \u{25be}", "Fill \u{25be}", "More colours..."] {
            let at = driver.texts(label).into_iter().min_by(|a, b| a.top().total_cmp(&b.top())).unwrap_or_else(|| panic!("nothing says {label:?}"));
            driver.click(at.center(), Modifiers::NONE);
            driver.frames(3);
        }
        assert!(driver.app.colour_draft.is_some());
        let fill = |driver: &Driver| driver.app.doc_shape(&ids[0]).and_then(|shape| shape.fill.clone()).map(|fill| fill.to_lowercase());
        let (before, depth) = (fill(&driver), driver.app.engine.undo_depth());
        // refused: nothing changes and the window stays
        driver.write("orange");
        driver.key(Key::Enter, Modifiers::NONE);
        assert!(driver.app.colour_draft.as_ref().is_some_and(|draft| draft.refused));
        assert_eq!((fill(&driver), driver.app.engine.undo_depth()), (before, depth));
        driver.key(Key::A, CTRL);
        driver.write("3a7bd5");
        driver.key(Key::Enter, Modifiers::NONE);
        assert!(driver.app.colour_draft.is_none(), "{}", driver.app.status);
        assert_eq!(fill(&driver).as_deref(), Some("#3a7bd5"), "{}", driver.app.status);
        assert_eq!(driver.app.engine.undo_depth(), depth + 1);
        assert_eq!(driver.app.selected, Some(ids[0].clone()));
        assert!(driver.app.extra_selected.is_empty(), "Ctrl+A stayed in the field");
    }

    /// A shape moved so that its edge stops 14 screen px short of another's is pulled onto it only by a strength that
    /// reaches that far.
    #[test]
    fn the_snap_strength_sets_how_far_an_edge_is_pulled() {
        let (mut driver, ids) = Driver::new();
        (driver.app.snap_grid, driver.app.snap_guides, driver.app.snap_shapes) = (false, false, true);
        driver.app.selected = Some(ids[0].clone());
        driver.app.snap_open = true;
        driver.frames(3);
        assert!(!driver.texts("Snap strength").is_empty(), "the setting is in the Snap and Attach window");
        let rect = |driver: &Driver, id: &str| LayoutCraftApp::selection_scene_rect(driver.app.scene_shape(id).unwrap());
        let (moved, other) = (rect(&driver, &ids[0]), rect(&driver, &ids[1]));
        let short = 14.0 / driver.app.view_scale();
        let delta = vec2(other.left() - moved.right() - short, 0.0);
        driver.app.snap_reach_px = 8.0;
        let weak = driver.app.snapped_move(moved, delta).0;
        assert!((weak.x - delta.x).abs() < 0.01, "{weak:?} {delta:?}");
        driver.app.snap_reach_px = 20.0;
        let strong = driver.app.snapped_move(moved, delta).0;
        assert!((moved.right() + strong.x - other.left()).abs() < 0.01, "{strong:?} {delta:?}");
    }

    #[test]
    fn a_fields_prompt_and_type_can_be_changed_in_the_window() {
        let (mut driver, ids) = Driver::new();
        driver.app.selected = Some(ids[0].clone());
        driver.app.data_pane = Some(DataPane { new_label: "Owner".to_string(), ..DataPane::default() });
        driver.frames(4);
        let press = |driver: &mut Driver, label: &str| {
            let button = *driver.texts(label).first().unwrap_or_else(|| panic!("no {label:?} button"));
            driver.click(button.center(), Modifiers::NONE);
            driver.frames(3);
        };
        press(&mut driver, "Add");
        let row = |driver: &Driver| driver.app.doc_shape(&ids[0]).and_then(|shape| shape.data.first().cloned()).expect("the field");
        let depth = driver.app.engine.undo_depth();
        press(&mut driver, "Change");
        // saving without a change writes nothing
        press(&mut driver, "Save field");
        assert_eq!(driver.app.engine.undo_depth(), depth);
        press(&mut driver, "Change");
        driver.app.data_pane.as_mut().unwrap().new_prompt = "Who looks after it?".to_string();
        driver.frames(2);
        press(&mut driver, "Save field");
        assert_eq!((row(&driver).prompt.as_str(), driver.app.engine.undo_depth()), ("Who looks after it?", depth + 1), "{}", driver.app.status);
        assert!(driver.app.data_pane.as_ref().is_some_and(|pane| pane.changing.is_none() && pane.new_prompt.is_empty()));
        // hiding the field, and showing it again
        for hidden in [true, false] {
            press(&mut driver, "Change");
            press(&mut driver, "Hidden");
            press(&mut driver, "Save field");
            assert_eq!(row(&driver).invisible != 0, hidden, "{}", driver.app.status);
            assert_eq!(driver.texts("Owner (hidden)").is_empty(), !hidden);
        }
        assert_eq!(row(&driver).prompt, "Who looks after it?");
        // the form does not follow to another shape
        press(&mut driver, "Change");
        driver.app.selected = Some(ids[1].clone());
        driver.frames(3);
        assert!(driver.app.data_pane.as_ref().is_some_and(|pane| pane.changing.is_none() && pane.new_prompt.is_empty()));
    }

    #[test]
    fn the_wheel_can_be_set_to_zoom_from_the_view_tab() {
        let (mut driver, _) = Driver::new();
        let wheel = |driver: &mut Driver| {
            let at = driver.app.last_viewport.center();
            driver.run(vec![Event::PointerMoved(at)]);
            driver.run(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: vec2(0.0, 1.0), modifiers: Modifiers::NONE, phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel }]);
            // the toolkit spreads one notch over several frames
            driver.frames(40);
        };
        let (zoom, pan) = (driver.app.zoom, driver.app.pan);
        wheel(&mut driver);
        assert!(driver.app.zoom == zoom && driver.app.pan.y != pan.y, "by itself the wheel scrolls");
        let click = |driver: &mut Driver, found: &dyn Fn(&str) -> bool| {
            let at = driver
                .drawn
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(shown) if found(shown.galley.text()) => Some(egui::Rect::from_min_size(shown.pos, shown.galley.size()).center()),
                    _ => None,
                })
                .expect("the control");
            driver.click(at, Modifiers::NONE);
            driver.frames(3);
        };
        click(&mut driver, &|text| text == "View");
        // the Zoom menu says the percentage shown
        click(&mut driver, &|text| text.ends_with("% \u{25be}"));
        click(&mut driver, &|text| text == "Mouse wheel zooms");
        assert!(driver.app.wheel_zooms, "{}", driver.app.status);
        let (zoom, pan) = (driver.app.zoom, driver.app.pan);
        wheel(&mut driver);
        assert!(driver.app.zoom > zoom * 1.01, "{zoom} {} {pan:?}", driver.app.zoom);
    }

    #[test]
    fn ctrl_2_takes_the_text_tool_and_a_drag_makes_a_text_box() {
        let (mut driver, _) = Driver::new();
        let count = |driver: &Driver| driver.app.engine.document().unwrap().pages[0].shapes.len();
        let before = count(&driver);
        driver.key(Key::Num2, CTRL);
        assert!(driver.app.tool == super::Tool::Draw(5), "{}", driver.app.status);
        let canvas = driver.app.last_viewport;
        let from = canvas.center() + vec2(-60.0, 120.0);
        driver.drag(from, from + vec2(120.0, 50.0));
        assert_eq!(count(&driver), before + 1, "{}", driver.app.status);
    }

    #[test]
    fn the_connection_point_tool_adds_chooses_and_deletes_points() {
        let (mut driver, ids) = Driver::new();
        driver.app.selected = Some(ids[0].clone());
        driver.key(Key::Num1, Modifiers { shift: true, ..CTRL });
        assert!(driver.app.tool == super::Tool::ConnectionPoint && driver.app.selected.is_none(), "{}", driver.app.status);
        let points = |driver: &mut Driver| driver.app.sites_of(&ids[0]);
        let before = points(&mut driver);
        // a place a fifth of the way in from the left and a third down from the top
        let rect = LayoutCraftApp::selection_scene_rect(driver.app.scene_shape(&ids[0]).unwrap());
        let wanted = pos2(rect.left() + rect.width() * 0.2, rect.top() + rect.height() / 3.0);
        let at = driver.app.scene_to_screen(wanted, driver.app.image_rect(driver.app.last_viewport));
        driver.click(at, Modifiers::NONE);
        let added = points(&mut driver);
        assert_eq!(added.len(), before.len() + 1, "{}", driver.app.status);
        let new = added.iter().find(|point| !before.contains(point)).expect("a new point");
        assert!(new.1.distance(wanted) < 1.5, "where it was clicked: {:?} {wanted:?}", new.1);
        assert!(driver.app.selected.is_none());
        // Delete without a chosen point takes nothing away, also not a selected shape
        driver.app.selected = Some(ids[1].clone());
        driver.key(Key::Delete, Modifiers::NONE);
        assert!(driver.app.doc_shape(&ids[1]).is_some() && points(&mut driver).len() == added.len());
        // choose the point, then delete it
        driver.click(at, Modifiers::NONE);
        assert_eq!(driver.app.chosen_connection_point(), Some((ids[0].as_str(), new.0)), "{}", driver.app.status);
        assert_eq!(points(&mut driver).len(), added.len(), "choosing adds nothing");
        driver.key(Key::Delete, Modifiers::NONE);
        assert_eq!(points(&mut driver).len(), before.len(), "{}", driver.app.status);
        // a point added again and dragged somewhere else keeps its number
        driver.click(at, Modifiers::NONE);
        let depth = driver.app.engine.undo_depth();
        let place = pos2(rect.left() + rect.width() * 0.7, rect.top() + rect.height() * 0.8);
        let to = driver.app.scene_to_screen(place, driver.app.image_rect(driver.app.last_viewport));
        driver.drag(at, to);
        let moved = points(&mut driver);
        assert_eq!((moved.len(), driver.app.engine.undo_depth()), (before.len() + 1, depth + 1), "{}", driver.app.status);
        assert!(moved.iter().any(|point| point.0 == new.0 && point.1.distance(place) < 1.5), "{moved:?} {place:?}");
        // a drag that begins away from every point moves nothing
        let empty = driver.app.scene_to_screen(pos2(rect.center().x, rect.top() + rect.height() * 0.55), driver.app.image_rect(driver.app.last_viewport));
        driver.drag(empty, at);
        assert_eq!((points(&mut driver), driver.app.engine.undo_depth()), (moved, depth + 1), "{}", driver.app.status);
        driver.key(Key::Escape, Modifiers::NONE);
        assert!(driver.app.tool == super::Tool::Select);
        // a turned shape is refused, and said so
        driver.app.selected = Some(ids[0].clone());
        driver.key(Key::R, CTRL);
        driver.key(Key::Num1, Modifiers { shift: true, ..CTRL });
        let depth = driver.app.engine.undo_depth();
        let middle = driver.middle_of(&ids[0]);
        driver.click(middle + vec2(7.0, 11.0), Modifiers::NONE);
        assert_eq!(driver.app.engine.undo_depth(), depth);
        assert!(driver.app.status.contains("turned"), "{}", driver.app.status);
        assert!(driver.app.moving_point.is_none());
    }

    #[test]
    fn a_comment_can_be_added_changed_and_deleted() {
        let (mut driver, _) = Driver::new();
        driver.app.comments_open = true;
        driver.app.comment_text = "Either | or".to_string();
        driver.frames(4);
        let press = |driver: &mut Driver, label: &str| {
            let button = *driver.texts(label).first().unwrap_or_else(|| panic!("no {label:?} button"));
            driver.click(button.center(), Modifiers::NONE);
            driver.frames(3);
        };
        let texts = |driver: &Driver| {
            let list: serde_json::Value = serde_json::from_str(&driver.app.engine.comments_json().unwrap()).unwrap();
            list.as_array().unwrap().iter().map(|comment| comment["text"].as_str().unwrap_or("").to_string()).collect::<Vec<_>>()
        };
        press(&mut driver, "Add comment");
        assert_eq!(texts(&driver), ["Either | or"], "{}", driver.app.status);
        let depth = driver.app.engine.undo_depth();
        // unchanged: nothing is written
        press(&mut driver, "Edit comment");
        press(&mut driver, "Save comment");
        assert_eq!(driver.app.engine.undo_depth(), depth);
        // a new comment being typed survives changing an old one
        driver.app.comment_text = "half written".to_string();
        press(&mut driver, "Edit comment");
        driver.app.comment_edit.as_mut().expect("being changed").1 = "Neither".to_string();
        driver.frames(2);
        press(&mut driver, "Save comment");
        assert_eq!((texts(&driver), driver.app.engine.undo_depth()), (vec!["Neither".to_string()], depth + 1), "{}", driver.app.status);
        assert_eq!(driver.app.comment_text, "half written");
        press(&mut driver, "Delete comment");
        assert!(texts(&driver).is_empty(), "{}", driver.app.status);
        driver.key(Key::Escape, Modifiers::NONE);
        assert!(!driver.app.comments_open);
    }

    #[test]
    fn a_shape_can_be_named_in_the_inspector() {
        let (mut driver, ids) = Driver::new();
        let name = |driver: &Driver, id: &str| serde_json::from_str::<serde_json::Value>(&driver.app.engine.shape_names_json(0).unwrap()).unwrap()[id].as_str().unwrap_or("").to_string();
        let enter = |driver: &mut Driver, id: &str, typed: &str, end: Key| {
            driver.app.selected = Some(id.to_string());
            driver.frames(3);
            // the field stands right of its label
            let label = *driver.texts("Name").first().expect("the Name field");
            driver.click(pos2(label.right() + 40.0, label.center().y), Modifiers::NONE);
            driver.frames(2);
            driver.key(Key::A, CTRL);
            driver.write(typed);
            driver.key(end, Modifiers::NONE);
            driver.frames(2);
        };
        enter(&mut driver, &ids[0], "Router", Key::Enter);
        assert_eq!(name(&driver, &ids[0]), "Router", "{}", driver.app.status);
        // Esc leaves the name and the selection alone
        let depth = driver.app.engine.undo_depth();
        enter(&mut driver, &ids[0], "Switch", Key::Escape);
        assert_eq!((name(&driver, &ids[0]).as_str(), driver.app.engine.undo_depth()), ("Router", depth));
        assert_eq!(driver.app.selected, Some(ids[0].clone()));
        // a taken name is made unique, and the status says what it became
        enter(&mut driver, &ids[1], "Router", Key::Enter);
        let second = name(&driver, &ids[1]);
        assert!(second != "Router" && second.starts_with("Router") && driver.app.status.contains(&second), "{second} {}", driver.app.status);
    }

    #[test]
    fn a_style_of_the_drawing_can_be_applied_to_the_selection() {
        let (mut driver, ids) = Driver::new();
        driver.app.engine.page_op(37, 0, 0, r##"{"name":"Alarm","fillColor":"#c00000","lineColor":"#101010"}"##).unwrap();
        driver.app.build_page(&driver.ctx.clone(), false);
        driver.app.selected = Some(ids[0].clone());
        driver.frames(3);
        let press = |driver: &mut Driver, label: &str| {
            let at = driver.texts(label).into_iter().min_by(|a, b| a.top().total_cmp(&b.top())).unwrap_or_else(|| panic!("nothing says {label:?}"));
            driver.click(at.center(), Modifiers::NONE);
            driver.frames(3);
        };
        // what Home > Editing > Styles does (an icon without text in a window this narrow)
        driver.app.style_pick = Some(Default::default());
        driver.frames(3);
        let fill = |driver: &Driver| driver.app.doc_shape(&ids[0]).and_then(|shape| shape.fill.clone()).map(|fill| fill.to_lowercase());
        let (before, other, depth) = (fill(&driver), driver.app.doc_shape(&ids[1]).and_then(|shape| shape.fill.clone()), driver.app.engine.undo_depth());
        press(&mut driver, "Choose a style");
        press(&mut driver, "Alarm");
        press(&mut driver, "Apply style to selection");
        assert_ne!(fill(&driver), before, "{}", driver.app.status);
        assert_eq!(fill(&driver).as_deref(), Some("#c00000"), "{}", driver.app.status);
        assert_eq!(driver.app.engine.undo_depth(), depth + 1);
        assert_eq!(driver.app.doc_shape(&ids[1]).and_then(|shape| shape.fill.clone()), other, "only the selection");
        // the drawing's own style can be deleted, a built-in one cannot
        let names = |driver: &Driver| {
            let list: serde_json::Value = serde_json::from_str(&driver.app.engine.styles_list_json().unwrap()).unwrap();
            list.as_array().unwrap().iter().filter_map(|style| style["name"].as_str().map(str::to_string)).collect::<Vec<_>>()
        };
        let built_in = names(&driver).into_iter().find(|name| name != "Alarm").expect("a built-in style");
        let count = names(&driver).len();
        press(&mut driver, "Alarm");
        press(&mut driver, &built_in);
        let depth = driver.app.engine.undo_depth();
        press(&mut driver, "Delete style");
        assert_eq!((names(&driver).len(), driver.app.engine.undo_depth()), (count, depth), "{built_in}: {}", driver.app.status);
        press(&mut driver, &built_in);
        press(&mut driver, "Alarm");
        press(&mut driver, "Delete style");
        assert!(!names(&driver).contains(&"Alarm".to_string()) && names(&driver).len() == count - 1, "{}", driver.app.status);
        driver.key(Key::Escape, Modifiers::NONE);
        assert!(driver.app.style_pick.is_none());
    }

    #[test]
    fn page_setup_chooses_the_background_page() {
        let (mut driver, _) = Driver::new();
        let info = |driver: &Driver| serde_json::from_str::<serde_json::Value>(&driver.app.engine.background_info_json().unwrap()).unwrap();
        // a second page, made a background page
        driver.app.engine.page_op(1, 0, 0, "Letterhead").unwrap();
        let tabs = driver.app.engine.document().unwrap().pages.len();
        let at = driver.app.engine.document().unwrap().pages.iter().position(|page| page.name == "Letterhead").expect("the new page");
        driver.app.engine.page_op(13, at, 0, "").unwrap();
        driver.app.build_page(&driver.ctx.clone(), false);
        assert_eq!(info(&driver)["backgrounds"].as_array().map(Vec::len), Some(1), "{tabs}");
        // a background page has no tab: the page numbers the window uses are those of the pages that have one
        assert_eq!(driver.app.engine.document().unwrap().pages.len(), info(&driver)["assigned"].as_array().map_or(0, Vec::len));
        assert_eq!(driver.app.page_index, 0);
        driver.app.page_setup_open = true;
        driver.frames(4);
        let press = |driver: &mut Driver, label: &str, last: bool| {
            let mut found = driver.texts(label);
            found.sort_by(|a, b| a.top().total_cmp(&b.top()));
            let at = if last { found.pop() } else { found.first().copied() }.unwrap_or_else(|| panic!("nothing says {label:?}"));
            driver.click(at.center(), Modifiers::NONE);
            driver.frames(3);
        };
        let depth = driver.app.engine.undo_depth();
        press(&mut driver, "None", false);
        press(&mut driver, "Letterhead", true);
        assert_eq!(info(&driver)["assigned"][0], info(&driver)["backgrounds"][0]["id"], "{}", driver.app.status);
        assert_eq!(driver.app.engine.undo_depth(), depth + 1);
        // and none again
        press(&mut driver, "Letterhead", true);
        press(&mut driver, "None", false);
        assert!(info(&driver)["assigned"][0].is_null(), "{}", driver.app.status);
    }

    #[test]
    fn the_theme_and_variant_menus_show_each_choice_s_colours() {
        let (mut driver, _) = Driver::new();
        let info: serde_json::Value = serde_json::from_str(&driver.app.engine.themes_json().unwrap()).unwrap();
        let theme = &info["themes"][1];
        let name = theme["name"].as_str().unwrap().to_string();
        let colour = |hex: &serde_json::Value| super::typed_colour(hex.as_str().unwrap()).map(|[r, g, b]| egui::Color32::from_rgb(r, g, b)).unwrap();
        let painted = |driver: &Driver, wanted: egui::Color32| {
            driver.drawn.iter().any(|clipped| matches!(&clipped.shape, egui::Shape::Rect(square) if square.fill == wanted && (square.rect.width() - 11.0).abs() < 0.5))
        };
        let press = |driver: &mut Driver, label: &str| {
            let at = driver.texts(label).into_iter().min_by(|a, b| a.top().total_cmp(&b.top())).unwrap_or_else(|| panic!("nothing says {label:?}"));
            driver.click(at.center(), Modifiers::NONE);
            driver.frames(3);
        };
        // in a window this narrow the menu sits in the drop-down of its group, under a button of the same name
        let open_menu = |driver: &mut Driver, label: &str| {
            press(driver, label);
            let inner = driver.texts(label).into_iter().max_by(|a, b| a.top().total_cmp(&b.top())).expect("the menu");
            if driver.texts(label).len() > 1 {
                driver.click(inner.center(), Modifiers::NONE);
                driver.frames(3);
            }
        };
        press(&mut driver, "Design");
        open_menu(&mut driver, "Themes \u{25be}");
        for accent in theme["accents"].as_array().unwrap() {
            assert!(painted(&driver, colour(accent)), "{name} {accent}");
        }
        press(&mut driver, &name);
        let now: serde_json::Value = serde_json::from_str(&driver.app.engine.themes_json().unwrap()).unwrap();
        assert_eq!(super::current_theme(&now, 0).0.as_deref(), Some(name.as_str()), "the name still applies: {}", driver.app.status);
        open_menu(&mut driver, "Variants \u{25be}");
        let variants = theme["variants"].as_array().unwrap();
        assert!(!variants.is_empty());
        for variant in variants {
            for hex in variant.as_array().unwrap() {
                assert!(painted(&driver, colour(hex)), "{name} {hex}");
            }
        }
    }

    #[test]
    fn the_design_tab_shows_themes_and_variants_as_pictures_that_apply() {
        let (mut driver, _) = Driver::new();
        let tab = driver.texts("Design")[0];
        driver.click(tab.center(), Modifiers::NONE);
        driver.frames(40);
        // where the picture of a theme's variant was drawn, if it is on screen
        let tile = |driver: &Driver, theme: &str, variant: u32| {
            let id = driver.app.theme_pictures.get(&(theme.to_string(), variant))?.as_ref()?.id();
            driver.drawn.iter().find_map(|clipped| match &clipped.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id == id => Some(clipped.shape.visual_bounding_rect()),
                egui::Shape::Rect(shape) if shape.brush.as_ref().is_some_and(|brush| brush.fill_texture_id == id) => Some(shape.rect),
                _ => None,
            })
        };
        let info: serde_json::Value = serde_json::from_str(&driver.app.engine.themes_json().unwrap()).unwrap();
        let names: Vec<String> = info["themes"].as_array().unwrap().iter().map(|theme| theme["name"].as_str().unwrap().to_string()).collect();
        let current = |driver: &Driver| super::current_theme(&serde_json::from_str(&driver.app.engine.themes_json().unwrap()).unwrap(), 0);
        let second = tile(&driver, &names[1], 0).expect("a picture of the second theme");
        let last = names.last().unwrap();
        // the public build has few themes: they all fit, a build with more of them shows the first ones
        let all_fit = super::gallery::themes_in_view(1400.0) >= names.len();
        assert_eq!(tile(&driver, last, 0).is_some(), all_fit, "{names:?}");
        driver.click(second.center(), Modifiers::NONE);
        driver.frames(40);
        assert_eq!(current(&driver).0.as_deref(), Some(names[1].as_str()), "{}", driver.app.status);
        let third = tile(&driver, &names[1], 2).expect("a picture of its third variant");
        driver.click(third.center(), Modifiers::NONE);
        driver.frames(5);
        assert_eq!(current(&driver), (Some(names[1].clone()), 2), "{}", driver.app.status);
        for _ in 0..4 {
            let next = driver.texts("\u{203a}")[0];
            driver.click(next.center(), Modifiers::NONE);
            driver.frames(20);
        }
        assert!(tile(&driver, last, 0).is_some(), "the arrow reaches the last theme");
    }

    #[test]
    fn resting_on_a_theme_picture_shows_it_on_the_page_until_the_pointer_leaves() {
        let (mut driver, _) = Driver::new();
        let tab = driver.texts("Design")[0];
        driver.click(tab.center(), Modifiers::NONE);
        driver.frames(40);
        let drawn = |driver: &Driver, id: egui::TextureId| {
            driver.drawn.iter().find_map(|clipped| match &clipped.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id == id => Some(clipped.shape.visual_bounding_rect()),
                egui::Shape::Rect(shape) if shape.brush.as_ref().is_some_and(|brush| brush.fill_texture_id == id) => Some(shape.rect),
                _ => None,
            })
        };
        let info: serde_json::Value = serde_json::from_str(&driver.app.engine.themes_json().unwrap()).unwrap();
        let name = info["themes"][1]["name"].as_str().unwrap().to_string();
        let picture = driver.app.theme_pictures[&(name.clone(), 0)].as_ref().unwrap().id();
        let at = drawn(&driver, picture).expect("the picture").center();
        let before = (driver.app.engine.export().unwrap().to_vec(), driver.app.engine.undo_depth());
        let page = driver.app.texture.as_ref().unwrap().id();
        assert!(drawn(&driver, page).is_some() && driver.app.theme_preview.is_none());
        driver.run(vec![Event::PointerMoved(at)]);
        driver.frames(6);
        let preview = driver.app.theme_preview.as_ref().expect("a preview").texture.id();
        assert!(drawn(&driver, preview).is_some() && drawn(&driver, page).is_none(), "the preview takes the page's place");
        assert_eq!((driver.app.engine.export().unwrap().to_vec(), driver.app.engine.undo_depth()), before, "nothing changed");
        driver.run(vec![Event::PointerMoved(pos2(700.0, 600.0))]);
        driver.frames(3);
        assert!(driver.app.theme_preview.is_none() && drawn(&driver, page).is_some());
        // the keyboard: focus on a picture previews it too
        let mut reached = false;
        for _ in 0..60 {
            driver.key(Key::Tab, Modifiers::NONE);
            driver.frames(4);
            reached |= driver.app.theme_preview.is_some();
            if reached {
                break;
            }
        }
        assert!(reached, "Tab reaches a theme picture");
        assert_eq!(driver.app.engine.undo_depth(), before.1);
        // the page changes under a picture that still has the focus: it is drawn again for the new page
        driver.app.build_page(&driver.ctx.clone(), false);
        driver.frames(6);
        assert!(driver.app.theme_preview.is_some());
        // choosing it is one step, and the preview gives way to the real page
        driver.run(vec![Event::PointerMoved(at)]);
        driver.frames(6);
        driver.click(at, Modifiers::NONE);
        driver.frames(6);
        assert_eq!(driver.app.engine.undo_depth(), before.1 + 1, "{}", driver.app.status);
        assert!(driver.app.theme_preview.is_none());
    }

    #[test]
    fn the_main_command_of_a_group_is_large_and_the_group_names_stand_on_one_line() {
        let (mut driver, _) = Driver::new();
        let names = ["Clipboard", "Tools", "Editing"];
        let tops: Vec<f32> = names.iter().map(|name| driver.texts(name).last().unwrap_or_else(|| panic!("{name}")).top()).collect();
        assert!(tops.iter().all(|top| (top - tops[0]).abs() < 1.0), "{tops:?}");
        // Paste is large: its name stands lower than the name of the small command beside it, and clicking its icon works
        let paste = driver.texts("Paste")[0];
        let copy = driver.texts("Copy").first().copied();
        assert!(copy.is_none_or(|copy| paste.top() > copy.top() + 10.0), "{paste:?} {copy:?}");
        driver.app.status.clear();
        driver.click(pos2(paste.center().x, paste.top() - 14.0), Modifiers::NONE);
        driver.frames(3);
        assert!(!driver.app.status.is_empty(), "a click on the picture reaches the command");
    }

    #[test]
    fn the_font_group_sets_and_removes_all_capitals() {
        let (mut driver, ids) = Driver::new();
        driver.app.selected = Some(ids[0].clone());
        driver.frames(3);
        let drawn = |driver: &Driver| layoutcraft_render::to_svg(&layoutcraft_render::scene_from_document(driver.app.engine.document().unwrap(), 0).unwrap());
        assert!(!drawn(&driver).contains("LABEL"));
        let press = |driver: &mut Driver, label: &str| {
            if driver.texts(label).is_empty() {
                let group = *driver.texts("Font \u{25be}").first().expect("the Font group");
                driver.click(group.center(), Modifiers::NONE);
                driver.frames(2);
            }
            let at = *driver.texts(label).first().unwrap_or_else(|| panic!("no {label:?}"));
            driver.click(at.center(), Modifiers::NONE);
            driver.frames(3);
        };
        let depth = driver.app.engine.undo_depth();
        press(&mut driver, "Caps");
        assert!(drawn(&driver).contains("LABEL"), "{}", driver.app.status);
        assert_eq!((driver.app.engine.undo_depth(), driver.app.selection_text_flags()), (depth + 1, [false, true]));
        press(&mut driver, "Caps");
        assert!(!drawn(&driver).contains("LABEL") && drawn(&driver).contains("label"), "{}", driver.app.status);
    }

    #[test]
    fn f3_goes_to_the_style_settings_of_the_selected_shape() {
        let (mut driver, ids) = Driver::new();
        driver.key(Key::F3, Modifiers::NONE);
        assert!(!driver.app.style_focus && driver.app.status.contains("Select one shape"), "{}", driver.app.status);
        driver.app.selected = Some(ids[0].clone());
        driver.frames(3);
        let fill = |driver: &Driver| driver.app.style_draft.as_ref().map(|draft| draft.has_fill).expect("the style settings");
        let before = fill(&driver);
        driver.key(Key::F3, Modifiers::NONE);
        // the Fill box has the keyboard: Space ticks or clears it
        driver.key(Key::Space, Modifiers::NONE);
        assert_ne!(fill(&driver), before, "{}", driver.app.status);
        assert_eq!(driver.app.selected, Some(ids[0].clone()));
    }

    #[test]
    fn f8_goes_to_the_align_buttons_when_two_shapes_are_selected() {
        let (mut driver, ids) = Driver::new();
        driver.app.selected = Some(ids[0].clone());
        driver.frames(2);
        driver.key(Key::F8, Modifiers::NONE);
        assert!(!driver.app.align_focus && driver.app.status.contains("two or more"), "{}", driver.app.status);
        driver.app.extra_selected = vec![ids[1].clone()];
        driver.frames(3);
        assert!((driver.at(&ids[0]).0 - driver.at(&ids[1]).0).abs() > 1.0);
        driver.key(Key::F8, Modifiers::NONE);
        // the first button, align left edges, has the keyboard
        driver.key(Key::Space, Modifiers::NONE);
        driver.frames(2);
        assert!((driver.at(&ids[0]).0 - driver.at(&ids[1]).0).abs() < 0.01, "{:?} {:?} {}", driver.at(&ids[0]), driver.at(&ids[1]), driver.app.status);
    }

    #[test]
    fn the_font_group_colours_the_text() {
        let (mut driver, ids) = Driver::new();
        driver.app.selected = Some(ids[0].clone());
        driver.frames(3);
        for label in ["Font \u{25be}", "Text colour..."] {
            let at = *driver.texts(label).first().unwrap_or_else(|| panic!("nothing says {label:?}"));
            driver.click(at.center(), Modifiers::NONE);
            driver.frames(3);
        }
        assert!(driver.app.colour_draft.as_ref().is_some_and(|draft| draft.key == "textColor"));
        assert!(!driver.texts("Text colour").is_empty(), "the window says what it sets");
        let look = |driver: &Driver| driver.app.doc_shape(&ids[0]).map(|shape| (shape.text_color.clone().map(|c| c.to_lowercase()), shape.fill.clone(), shape.stroke.clone())).unwrap();
        let before = look(&driver);
        driver.write("#a01080");
        driver.key(Key::Enter, Modifiers::NONE);
        let after = look(&driver);
        assert_eq!(after.0.as_deref(), Some("#a01080"), "{}", driver.app.status);
        assert_eq!((after.1, after.2), (before.1, before.2), "fill and line stay");
        assert!(driver.app.colour_draft.is_none());
    }

    #[test]
    fn a_page_can_become_a_background_page_and_come_back() {
        let (mut driver, ids) = Driver::new();
        driver.app.engine.page_op(1, 0, 0, "Letterhead").unwrap();
        driver.app.build_page(&driver.ctx.clone(), false);
        driver.frames(3);
        let names = |driver: &Driver| driver.app.engine.document().unwrap().pages.iter().map(|page| page.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&driver), ["Page-1", "Letterhead"]);
        // the tab's menu
        let tab = *driver.texts("Letterhead").first().expect("the tab");
        let at = tab.center();
        let right = |pressed| Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE };
        driver.run(vec![Event::PointerMoved(at)]);
        driver.run(vec![right(true)]);
        driver.run(vec![right(false)]);
        driver.frames(3);
        let press = |driver: &mut Driver, label: &str| {
            let at = *driver.texts(label).first().unwrap_or_else(|| panic!("nothing says {label:?}"));
            driver.click(at.center(), Modifiers::NONE);
            driver.frames(3);
        };
        press(&mut driver, "Make a background page");
        assert_eq!(names(&driver), ["Page-1"], "{}", driver.app.status);
        assert!(driver.app.page_index == 0 && driver.app.doc_shape(&ids[0]).is_some(), "a page with a tab is shown");
        driver.app.page_setup_open = true;
        driver.frames(4);
        press(&mut driver, "Make Letterhead an ordinary page");
        assert!(names(&driver).contains(&"Letterhead".to_string()), "{} {:?}", driver.app.status, names(&driver));
        assert_eq!(driver.app.engine.document().unwrap().pages[driver.app.page_index].name, "Letterhead", "and shown");
    }

    #[test]
    fn a_layer_can_be_renamed() {
        let (mut driver, _) = Driver::new();
        driver.app.engine.page_op(7, 0, 0, "Wiring").unwrap();
        driver.app.build_page(&driver.ctx.clone(), false);
        driver.app.layers_open = true;
        driver.frames(4);
        let layer = |driver: &Driver| driver.app.engine.document().unwrap().pages[0].layers[0].clone();
        let before = layer(&driver);
        let press = |driver: &mut Driver, label: &str| {
            // the window lies over the inspector, which lists the layers too: its controls are the leftmost ones
            let at = driver.texts(label).into_iter().min_by(|a, b| a.left().total_cmp(&b.left())).unwrap_or_else(|| panic!("nothing says {label:?}"));
            driver.click(at.center(), Modifiers::NONE);
            driver.frames(3);
        };
        let depth = driver.app.engine.undo_depth();
        for (typed, renamed) in [("a|b", false), ("Power", true)] {
            press(&mut driver, "Rename");
            driver.app.layer_rename.as_mut().expect("the form").1 = typed.to_string();
            driver.frames(2);
            press(&mut driver, "Save name");
            assert_eq!(layer(&driver).name == typed, renamed, "{} {}", layer(&driver).name, driver.app.status);
        }
        let after = layer(&driver);
        assert_eq!((after.visible, after.print, after.locked, after.index), (before.visible, before.print, before.locked, before.index));
        assert_eq!(driver.app.engine.undo_depth(), depth + 1);
        assert!(driver.app.layer_rename.is_none() || driver.app.layer_rename.as_ref().is_some_and(|form| form.0 .2 == depth + 1));
    }

    #[test]
    fn a_layer_can_be_given_a_colour_and_lose_it() {
        let (mut driver, _) = Driver::new();
        driver.app.engine.page_op(7, 0, 0, "Wiring").unwrap();
        driver.app.build_page(&driver.ctx.clone(), false);
        driver.app.layers_open = true;
        driver.frames(4);
        let colour = |driver: &Driver| driver.app.engine.document().unwrap().pages[0].layers[0].color.clone().and_then(|colour| super::typed_colour(&colour));
        let press = |driver: &mut Driver, label: &str| {
            let at = driver.texts(label).into_iter().min_by(|a, b| a.left().total_cmp(&b.left())).unwrap_or_else(|| panic!("nothing says {label:?}"));
            driver.click(at.center(), Modifiers::NONE);
            driver.frames(3);
        };
        assert_eq!(colour(&driver), None);
        let depth = driver.app.engine.undo_depth();
        for (typed, wanted) in [("blue", None), ("#2060c0", Some([0x20, 0x60, 0xc0]))] {
            press(&mut driver, "Colour");
            driver.app.layer_colour.as_mut().expect("the form").1 = typed.to_string();
            driver.frames(2);
            press(&mut driver, "Apply colour");
            assert_eq!(colour(&driver), wanted, "{}", driver.app.status);
        }
        press(&mut driver, "Colour");
        press(&mut driver, "No colour");
        assert_eq!((colour(&driver), driver.app.engine.undo_depth()), (None, depth + 2), "{}", driver.app.status);
    }

    #[test]
    fn a_new_style_can_be_described_saved_and_applied() {
        let (mut driver, ids) = Driver::new();
        driver.app.selected = Some(ids[0].clone());
        driver.app.style_pick = Some(Default::default());
        driver.frames(3);
        let press = |driver: &mut Driver, label: &str| {
            let at = driver.texts(label).into_iter().min_by(|a, b| a.top().total_cmp(&b.top())).unwrap_or_else(|| panic!("nothing says {label:?}"));
            driver.click(at.center(), Modifiers::NONE);
            // long enough for a section to unfold
            driver.frames(12);
        };
        press(&mut driver, "New style");
        let styles = |driver: &Driver| driver.app.engine.styles_list_json().unwrap().matches("\"id\"").count();
        let (count, depth) = (styles(&driver), driver.app.engine.undo_depth());
        {
            let new = &mut driver.app.style_pick.as_mut().unwrap().new;
            (new.name, new.fill, new.weight) = ("Warning".to_string(), "yellow".to_string(), "3".to_string());
        }
        driver.frames(2);
        press(&mut driver, "Save style");
        assert_eq!((styles(&driver), driver.app.engine.undo_depth()), (count, depth), "{}", driver.app.status);
        assert!(driver.app.status.contains("fill colour"), "{}", driver.app.status);
        driver.app.style_pick.as_mut().unwrap().new.fill = "#e8c000".to_string();
        driver.frames(2);
        press(&mut driver, "Save style");
        assert_eq!((styles(&driver), driver.app.engine.undo_depth()), (count + 1, depth + 1), "{}", driver.app.status);
        press(&mut driver, "Apply style to selection");
        let fill = driver.app.doc_shape(&ids[0]).and_then(|shape| shape.fill.clone()).map(|fill| fill.to_lowercase());
        assert_eq!(fill.as_deref(), Some("#e8c000"), "the new style was chosen: {}", driver.app.status);
    }

    #[test]
    fn a_shapes_menu_leads_to_its_style_settings_and_fields() {
        let (mut driver, ids) = Driver::new();
        let choose = |driver: &mut Driver, entry: &str| {
            let at = driver.middle_of(&ids[0]);
            let right = |pressed| Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE };
            driver.run(vec![Event::PointerMoved(at)]);
            driver.run(vec![right(true)]);
            driver.run(vec![right(false)]);
            driver.frames(3);
            let item = driver.texts(entry).into_iter().find(|text| driver.app.last_viewport.contains(text.center())).unwrap_or_else(|| panic!("no {entry:?} in the menu"));
            driver.click(item.center(), Modifiers::NONE);
            driver.frames(3);
        };
        choose(&mut driver, "Style settings");
        assert_eq!(driver.app.selected, Some(ids[0].clone()));
        let fill = |driver: &Driver| driver.app.style_draft.as_ref().map(|draft| draft.has_fill).expect("the style settings");
        let before = fill(&driver);
        driver.key(Key::Space, Modifiers::NONE);
        assert_ne!(fill(&driver), before, "the Fill box has the keyboard: {}", driver.app.status);
        for _ in 0..2 {
            choose(&mut driver, "Shape Fields");
            assert!(driver.app.data_pane.is_some(), "opening it twice leaves it open");
        }
    }

    /// Near the window's edge the list must not be pushed back over the arrow: a click there is the arrow's.
    #[test]
    fn the_shape_list_never_covers_its_connect_arrow() {
        let (mut driver, ids) = Driver::new();
        driver.app.open_builtin_stencil();
        driver.app.shapes_open = false;
        driver.frames(3);
        let frame = |driver: &Driver| {
            let image = driver.app.image_rect(driver.app.last_viewport);
            driver.app.scene_rect_to_screen(LayoutCraftApp::selection_scene_rect(driver.app.scene_shape(&ids[0]).unwrap()), image)
        };
        // move the page so that the shape's lower edge is just above the bottom of the canvas: no room for a list below it
        let canvas = driver.app.last_viewport;
        driver.app.fit_pending = false;
        driver.app.pan.y += canvas.bottom() - 22.0 - frame(&driver).bottom();
        driver.frames(3);
        let at = frame(&driver);
        driver.run(vec![Event::PointerMoved(at.center())]);
        driver.frames(2);
        let arrow = pos2(at.center().x, at.bottom() + 16.0);
        driver.run(vec![Event::PointerMoved(arrow)]);
        driver.frames(6);
        driver.click(arrow, Modifiers::NONE);
        let made = driver.app.selected.clone().expect("the arrow placed a shape");
        let size = |id: &str| driver.app.doc_shape(id).map(|shape| (shape.w, shape.h)).unwrap();
        assert!(made != ids[0] && size(&made) == size(&ids[0]), "a copy, as the arrow gives: {:?} {:?} {}", size(&made), size(&ids[0]), driver.app.status);
    }

    /// The window follows the page: its fields are those of the page that is shown, so Apply cannot give one page another's size.
    #[test]
    fn page_setup_left_open_takes_the_size_of_the_page_shown() {
        let (mut driver, _) = Driver::new();
        driver.app.engine.page_op(1, 0, 0, "Wide").unwrap();
        driver.app.engine.page_op(6, 1, 0, "20,5").unwrap();
        driver.app.build_page(&driver.ctx.clone(), false);
        driver.frames(3);
        let sizes = |driver: &Driver| (0..2).map(|page| super::page_dimensions(&driver.app.engine, page).unwrap()).collect::<Vec<_>>();
        let before = sizes(&driver);
        assert_ne!(before[0], before[1]);
        let press = |driver: &mut Driver, label: &str| {
            let at = *driver.texts(label).first().unwrap_or_else(|| panic!("nothing says {label:?}"));
            driver.click(at.center(), Modifiers::NONE);
            driver.frames(3);
        };
        // opened on the first page, as the Page setup command does
        driver.app.page_setup_size = before[0];
        driver.app.page_setup_open = true;
        driver.frames(4);
        press(&mut driver, "Wide");
        assert_eq!(driver.app.page_index, 1);
        press(&mut driver, "Apply");
        assert_eq!(sizes(&driver), before, "{}", driver.app.status);
        // typed values are kept while the page stays
        driver.app.page_setup_size = (7.0, 6.0);
        driver.frames(3);
        press(&mut driver, "Apply");
        assert_eq!(sizes(&driver), vec![before[0], (7.0, 6.0)], "{}", driver.app.status);
    }

    /// Another drawing can have the same page, shape ID and undo depth: nothing read from the one before may be shown for
    /// it, and no half-made change may be written to it.
    #[test]
    fn panels_forget_the_drawing_that_was_open_before() {
        let (mut driver, ids) = Driver::new();
        driver.app.engine.page_op(40, 0, 0, &format!("{}|Router", ids[0])).unwrap();
        let mut row = super::new_data_row("Owner", &[]).unwrap();
        row.value = "Network team".to_string();
        super::write_data_row(&mut driver.app.engine, 0, &ids[0], &row).unwrap();
        driver.app.engine.page_op(20, 0, 0, "|Me|ME|2026-01-01T10:00:00|first drawing").unwrap();
        driver.app.build_page(&driver.ctx.clone(), false);
        driver.app.selected = Some(ids[0].clone());
        driver.app.data_pane = Some(DataPane::default());
        driver.frames(4);
        let depth = driver.app.engine.undo_depth();
        assert_eq!(driver.app.name_field.text, "Router");
        driver.app.comment_edit = Some(("0".to_string(), "changed".to_string()));
        driver.app.layer_rename = Some(((0, 0, depth), "Wiring".to_string()));
        driver.app.layer_colour = Some(((0, 0, depth), "#102030".to_string()));
        // another drawing with the same shape IDs, brought to the same undo depth
        let ctx = driver.ctx.clone();
        driver.app.new_drawing(&ctx);
        let others: Vec<String> = [1.0, 6.0].iter().map(|x| driver.app.engine.draw(0, 0, *x, 3.0, x + 2.0, 5.0).unwrap()).collect();
        assert_eq!(others, ids);
        while driver.app.engine.undo_depth() < depth {
            driver.app.engine.edit(0, 2, &others[1], 0.1, 0.0, 0.0, 0.0, 0).unwrap();
        }
        assert_eq!(driver.app.engine.undo_depth(), depth);
        driver.app.build_page(&ctx, false);
        assert!(driver.app.comment_edit.is_none() && driver.app.layer_rename.is_none() && driver.app.layer_colour.is_none());
        driver.app.selected = Some(others[0].clone());
        driver.frames(4);
        assert_ne!(driver.app.name_field.text, "Router", "the name is this drawing's");
        assert!(driver.app.data_pane.as_ref().is_some_and(|pane| pane.rows.is_empty()), "the fields are this drawing's");
    }

    /// A size typed for one shape goes to that shape, also when the field is left by clicking another one.
    #[test]
    fn a_typed_font_size_stays_with_the_shape_it_was_typed_for() {
        for other_size in [None, Some(20.0)] {
            let (mut driver, ids) = Driver::new();
            let size = |driver: &Driver, id: &str| driver.app.doc_shape(id).and_then(|shape| shape.font_size).map(|px| (px * 72.0 / 96.0).round());
            if let Some(points) = other_size {
                driver.app.engine.edit(0, 10, &format!("{}\n{{\"fontPt\":{points}}}", ids[1]), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
                driver.app.build_page(&driver.ctx.clone(), false);
            }
            driver.app.selected = Some(ids[0].clone());
            driver.frames(3);
            let before = size(&driver, &ids[1]);
            let group = *driver.texts("Font \u{25be}").first().expect("the Font group");
            driver.click(group.center(), Modifiers::NONE);
            driver.frames(2);
            let unit = *driver.texts(" pt").first().expect("the size field");
            driver.click(pos2(unit.left() - 6.0, unit.center().y), Modifiers::NONE);
            driver.frames(2);
            driver.write("31");
            let other = driver.middle_of(&ids[1]);
            driver.click(other, Modifiers::NONE);
            driver.frames(4);
            assert_eq!(size(&driver, &ids[1]), before, "{other_size:?}: the clicked shape keeps its size: {}", driver.app.status);
            assert_eq!(size(&driver, &ids[0]), Some(31.0), "{other_size:?}: {}", driver.app.status);
            assert_eq!(driver.app.selected, Some(ids[1].clone()));
        }
    }

    /// What was typed into a shape's field is written to that shape when another shape, or nothing, is clicked; Esc still
    /// throws it away.
    #[test]
    fn a_typed_shape_field_is_kept_when_another_shape_is_clicked() {
        for (leave, kept) in [("shape", true), ("empty page", true), ("Esc", false)] {
            let (mut driver, ids) = Driver::new();
            for id in &ids {
                let row = super::new_data_row("Owner", &[]).unwrap();
                super::write_data_row(&mut driver.app.engine, 0, id, &row).unwrap();
            }
            driver.app.build_page(&driver.ctx.clone(), false);
            driver.app.selected = Some(ids[0].clone());
            driver.app.data_pane = Some(DataPane::default());
            driver.frames(4);
            let label = *driver.texts("Owner").first().expect("the field");
            driver.click(pos2(label.right() + 40.0, label.center().y), Modifiers::NONE);
            driver.frames(2);
            let depth = driver.app.engine.undo_depth();
            driver.write("Typed");
            match leave {
                "shape" => {
                    let other = driver.middle_of(&ids[1]);
                    driver.click(other, Modifiers::NONE);
                }
                "empty page" => {
                    let below = driver.middle_of(&ids[1]) + vec2(0.0, 150.0);
                    driver.click(below, Modifiers::NONE);
                }
                _ => driver.key(Key::Escape, Modifiers::NONE),
            }
            driver.frames(4);
            let values: Vec<String> = ids.iter().map(|id| driver.app.doc_shape(id).unwrap().data[0].value.clone()).collect();
            assert_eq!(values, [if kept { "Typed" } else { "" }, ""], "{leave}: {}", driver.app.status);
            assert_eq!(driver.app.engine.undo_depth(), depth + usize::from(kept), "{leave}");
            if leave == "shape" {
                assert_eq!(driver.app.selected, Some(ids[1].clone()), "the click still selects");
            }
        }
    }

    /// A selection belongs to the page it was made on: another page can have shapes with the same IDs.
    #[test]
    fn showing_another_page_leaves_nothing_selected() {
        let (mut driver, ids) = Driver::new();
        // the copy of the page has the same shape IDs
        driver.app.engine.page_op(5, 0, 0, "").unwrap();
        driver.app.build_page(&driver.ctx.clone(), false);
        driver.frames(3);
        driver.key(Key::A, CTRL);
        assert_eq!(driver.app.all_selected().len(), ids.len());
        driver.key(Key::PageDown, CTRL);
        driver.frames(3);
        assert_eq!(driver.app.page_index, 1, "{}", driver.app.status);
        assert!(driver.app.all_selected().is_empty(), "{:?}", driver.app.all_selected());
        // so a command on the selection touches nothing here
        let shapes = |driver: &Driver| driver.app.engine.document().unwrap().pages[1].shapes.len();
        let (count, depth) = (shapes(&driver), driver.app.engine.undo_depth());
        let ctx = driver.ctx.clone();
        driver.app.delete_selected(&ctx);
        assert_eq!((shapes(&driver), driver.app.engine.undo_depth()), (count, depth), "{}", driver.app.status);
    }

    /// A command that selects something else (here: a shape added by a connect arrow) leaves only that selected.
    #[test]
    fn a_newly_made_shape_is_the_whole_selection() {
        let (mut driver, ids) = Driver::new();
        driver.key(Key::A, CTRL);
        assert_eq!(driver.app.all_selected().len(), 2);
        let image = driver.app.image_rect(driver.app.last_viewport);
        let frame = driver.app.scene_rect_to_screen(LayoutCraftApp::selection_scene_rect(driver.app.scene_shape(&ids[0]).unwrap()), image);
        driver.run(vec![Event::PointerMoved(frame.center())]);
        driver.frames(2);
        let arrow = pos2(frame.center().x, frame.bottom() + 16.0);
        driver.run(vec![Event::PointerMoved(arrow)]);
        driver.frames(2);
        driver.click(arrow, Modifiers::NONE);
        let made = driver.app.selected.clone().expect("the new shape");
        assert!(!ids.contains(&made), "{}", driver.app.status);
        assert_eq!(driver.app.all_selected(), [made], "{}", driver.app.status);
        // a command that keeps the first shape keeps the others
        driver.key(Key::A, CTRL);
        let chosen = driver.app.all_selected();
        driver.key(Key::B, CTRL);
        assert_eq!(driver.app.all_selected(), chosen, "{}", driver.app.status);
        // so does moving them together by the second one
        let (second, depth) = (chosen[1].clone(), driver.app.engine.undo_depth());
        let from = driver.middle_of(&second);
        driver.drag(from, from + vec2(0.0, -60.0));
        assert!(driver.app.engine.undo_depth() > depth, "moved: {}", driver.app.status);
        // the one dragged may become the first; the same shapes stay selected
        let sorted = |mut ids: Vec<String>| {
            ids.sort();
            ids
        };
        assert_eq!(sorted(driver.app.all_selected()), sorted(chosen), "{}", driver.app.status);
    }

    /// Undo can take a selected shape away: it leaves the selection, so the next command works on what is there.
    #[test]
    fn undo_drops_shapes_that_are_gone_from_the_selection() {
        let (mut driver, ids) = Driver::new();
        let third = driver.app.engine.draw(0, 0, 4.0, 1.0, 5.0, 2.0).unwrap();
        driver.app.build_page(&driver.ctx.clone(), false);
        driver.frames(3);
        driver.key(Key::A, CTRL);
        assert!(driver.app.all_selected().contains(&third));
        driver.key(Key::Z, CTRL);
        assert!(driver.app.doc_shape(&third).is_none(), "undone: {}", driver.app.status);
        let mut selected = driver.app.all_selected();
        selected.sort();
        assert_eq!(selected, ids, "{}", driver.app.status);
        driver.key(Key::Delete, Modifiers::NONE);
        assert!(driver.app.engine.document().unwrap().pages[0].shapes.is_empty(), "{}", driver.app.status);
    }

    /// Leaving a field of the inspector by clicking a page tab writes the field and shows that page, with the one click.
    #[test]
    fn one_click_on_a_page_tab_writes_the_field_left_and_changes_page() {
        for field in ["name", "width"] {
            let (mut driver, ids) = Driver::new();
            driver.app.engine.page_op(5, 0, 0, "").unwrap();
            driver.app.build_page(&driver.ctx.clone(), false);
            driver.app.selected = Some(ids[0].clone());
            driver.frames(4);
            let other = driver.app.engine.document().unwrap().pages[1].name.clone();
            let tab = *driver.texts(&other).first().expect("the second page's tab");
            let read = |driver: &Driver, page: usize| {
                let names: serde_json::Value = serde_json::from_str(&driver.app.engine.shape_names_json(page).unwrap()).unwrap();
                let shape = driver.app.engine.document().unwrap().pages[page].shapes.iter().find(|shape| shape.visio_id.as_deref() == Some(ids[0].as_str())).unwrap();
                (names[&ids[0]].as_str().unwrap_or("").to_string(), shape.w)
            };
            let before = (read(&driver, 0), read(&driver, 1));
            if field == "name" {
                let label = *driver.texts("Name").first().expect("the Name field");
                driver.click(pos2(label.right() + 40.0, label.center().y), Modifiers::NONE);
            } else {
                driver.app.size_pos.focus = Some(2);
            }
            driver.frames(2);
            driver.key(Key::A, CTRL);
            driver.write("3");
            driver.click(tab.center(), Modifiers::NONE);
            driver.frames(4);
            assert_eq!(driver.app.page_index, 1, "{field}: {}", driver.app.status);
            let after = (read(&driver, 0), read(&driver, 1));
            assert_ne!(after.0, before.0, "{field}: written to the shape it was typed for");
            assert_eq!(after.1, before.1, "{field}: and nothing on the page that is shown now");
        }
    }

    #[test]
    fn escape_still_closes_the_text_editor() {
        let (mut driver, ids) = Driver::new();
        driver.app.selected = Some(ids[0].clone());
        driver.frames(2);
        driver.key(Key::F2, Modifiers::NONE);
        assert!(driver.app.text_editor.is_some(), "{}", driver.app.status);
        driver.frames(2);
        driver.key(Key::Escape, Modifiers::NONE);
        assert!(driver.app.text_editor.is_none());
        assert_eq!(driver.app.selected, Some(ids[0].clone()), "the shape stays selected");
    }
}

/// The characters typed in a frame that start a text edit: what the keyboard sent, if it is more than blank space (the
/// space bar moves the view).
fn typed_text(events: &[egui::Event]) -> Option<String> {
    let typed: String = events.iter().filter_map(|event| if let egui::Event::Text(text) = event { Some(text.as_str()) } else { None }).collect();
    (!typed.trim().is_empty()).then_some(typed)
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
            "Visio, Excalidraw and draw.io files (*.vsdx;*.vstx;*.vsdm;*.vstm;*.vsd;*.vss;*.vst;*.vdx;*.vsx;*.vtx;*.excalidraw;*.drawio) *.vsdx;*.vstx;*.vsdm;*.vstm;*.vsd;*.vss;*.vst;*.vdx;*.vsx;*.vtx;*.excalidraw;*.drawio All files (*.*) *.*  "
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
        run_ext(false, "Open stencil", None, "Visio stencils", "vssx;*.vss;*.vsx")
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

    const DRAWINGS: &[&str] = &["vsdx", "vstx", "vsdm", "vstm", "vsd", "vss", "vst", "vdx", "vsx", "vtx", "excalidraw", "drawio"];

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
        FileDialog::new().set_title("Open stencil").add_filter("Visio stencils", &["vssx", "vss", "vsx"]).pick_file()
    }
}

/// `path` with the extension `.vsdx` in place of an old Visio format's, for saving; other names are returned unchanged.
fn vsdx_name(path: &Path) -> PathBuf {
    let old = path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| {
        ["vsd", "vss", "vst", "vdx", "vsx", "vtx"].iter().any(|old| extension.eq_ignore_ascii_case(old))
    });
    if old {
        path.with_extension("vsdx")
    } else {
        path.to_path_buf()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    /// Adds, chooses and deletes connection points.
    ConnectionPoint,
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

/// A typed colour, "#rrggbb" with or without the "#".
fn typed_colour(text: &str) -> Option<[u8; 3]> {
    let digits = text.trim().trim_start_matches('#');
    (digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| parse_hex(&format!("#{digits}"))).flatten()
}

/// The window for a colour that is not among the ribbon's swatches: which style it sets ("fill", "line" or "textColor"), the
/// code typed or picked, why it was refused, and whether the field still has to take the keyboard.
struct ColourDraft {
    key: &'static str,
    text: String,
    refused: bool,
    focus: bool,
}

#[cfg(test)]
mod typed_colour_tests {
    use super::{hex, typed_colour};

    #[test]
    fn a_colour_code_is_six_hex_digits() {
        assert_eq!(typed_colour(" #1a2B3c "), Some([0x1a, 0x2b, 0x3c]));
        assert_eq!(typed_colour(&hex([200, 3, 99])), Some([200, 3, 99]));
        assert_eq!(typed_colour("ff8000"), Some([255, 128, 0]));
        for refused in ["", "#", "red", "#12345", "#1234567", "#+f+f+f", "#a\u{e9}bcd", "#\u{e9}\u{e9}\u{e9}"] {
            assert_eq!(typed_colour(refused), None, "{refused:?}");
        }
    }
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
    /// Give the text field the keyboard on the next frame (once, when the editor opens).
    focus: bool,
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
    /// "Page Width" was chosen: apply it when the view size is known.
    fit_width_pending: bool,
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
    /// The layer being renamed: (page, layer number, undo depth when it was chosen) and the name in the field. The depth
    /// drops the form after any edit, as layer numbers may have moved.
    layer_rename: Option<((usize, u32, usize), String)>,
    /// The layer whose colour is being set, keyed as `layer_rename`, and the colour code in the field.
    layer_colour: Option<((usize, u32, usize), String)>,
    /// Pastes since the last copy (each lands a step further from the original).
    paste_count: u32,
    ribbon: ribbon::Ribbon,
    ribbon_tab: usize,
    ribbon_collapsed: bool,
    ribbon_peek: bool,
    ribbon_compact: bool,
    /// The command being drawn is the large one of its group.
    ribbon_large: bool,
    /// Installed font families for the picker (listed on first use) and the picker's search text.
    font_list: Option<Vec<String>>,
    font_filter: String,
    /// The fonts of the open page that are not installed here, with what is drawn instead.
    font_warnings: Vec<FontSubstitution>,
    doc_props_open: bool,
    help_shortcuts_open: bool,
    /// A widget had the keyboard when this frame began.
    focus_at_frame_start: bool,
    /// The Snap and Attach window is open.
    snap_open: bool,
    /// A plain mouse wheel zooms instead of scrolling.
    wheel_zooms: bool,
    /// The zoom window, while it is open.
    zoom_draft: Option<ZoomDraft>,
    /// The window for another fill or line colour, while it is open.
    colour_draft: Option<ColourDraft>,
    /// Where a box being dragged with the magnifier (Ctrl+Shift) began.
    magnify_from: Option<egui::Pos2>,
    /// The Shapes panel is narrowed to a column.
    shapes_minimised: bool,
    /// The one stencil of the Shapes panel that is open (its engine slot).
    open_stencil_shown: Option<usize>,
    /// The Shape Formulas window, while it is open.
    sheet_pane: Option<SheetPane>,
    /// The last press was on the canvas (or all shapes were selected by key); a Tab to apply to the shapes.
    canvas_keys: bool,
    tab_cycle: Option<bool>,
    /// The ruler guides of the page: shown or not, the lines (read when the page is built), the selected and the dragged one.
    show_guides: bool,
    guides: Vec<GuideLine>,
    selected_guide: Option<String>,
    guide_drag: Option<GuideDrag>,
    /// Rulers beside the canvas, and what they count in (read when the page is built).
    show_rulers: bool,
    ruler_unit: (f64, &'static str),
    /// The Size and Position fields of the inspector.
    size_pos: SizePos,
    name_field: NameField,
    /// The shapes the ribbon's font size field was entered for, while it has the keyboard.
    font_target: Option<Vec<String>>,
    /// The first Style control of the inspector is to take the keyboard (F3).
    style_focus: bool,
    /// The inspector's first align button is to take the keyboard (F8).
    align_focus: bool,
    /// The Styles window, while it is open.
    style_pick: Option<styles::StylePick>,
    /// The Layers window is open.
    layers_open: bool,
    /// The full-screen view is on.
    full_screen: bool,
    /// The Shape Data window, while it is open.
    data_pane: Option<DataPane>,
    /// The Hyperlink window, while it is open.
    link_draft: Option<LinkDraft>,
    /// The Themes menus: apply to every page instead of the shown one; and the gallery, until the page is rebuilt.
    theme_all_pages: bool,
    theme_cache: Option<serde_json::Value>,
    /// The Design tab's pictures by theme name and variant (`None` = it could not be drawn), and the first theme in the row.
    theme_pictures: BTreeMap<(String, u32), Option<egui::TextureHandle>>,
    theme_first: usize,
    /// The theme operation and value whose picture the pointer (or keyboard focus) is on in this frame, since when it has
    /// been, and the page drawn as that choice would leave it.
    theme_hover: Option<(i32, String)>,
    theme_resting: Option<((i32, String), f64)>,
    theme_preview: Option<gallery::Preview>,
    /// "Change Shape" is waiting for a click on a shape of the Shapes panel.
    change_shape: bool,
    /// The Find window, while it is open.
    find: Option<Find>,
    /// (selected ids, whether all their text is struck through) for the Strike button.
    struck_cache: Option<(String, [bool; 2])>,
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
    /// The snap strength: how near on screen (px) an edge must come to be pulled.
    snap_reach_px: f32,
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
    /// View > Add and Connect Arrows, the shape whose arrows are shown, and the line ends read for the menu.
    connect_arrows: bool,
    /// The connection point the Connection Point tool has chosen.
    chosen_point: Option<connpoints::ChosenPoint>,
    /// The connection point being dragged.
    moving_point: Option<connpoints::ChosenPoint>,
    /// The list of shapes shown beside the connect arrow under the pointer.
    arrow_picker: Option<connectors::ArrowPicker>,
    arrow_shape: Option<String>,
    line_ends_read: Option<connectors::LineEndsRead>,
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
    /// The page, and its size then, that the Page setup fields were last filled from.
    page_setup_read: Option<(usize, (f64, f64))>,
    comments_open: bool,
    comment_text: String,
    /// The comment whose text is being changed: its ID and the text in the field.
    comment_edit: Option<(String, String)>,
    help_about_open: bool,
    mcp_open: bool,
    mcp_server: String,
    mcp_root: String,
    mcp_message: String,
    ribbon_textures: BTreeMap<String, egui::TextureHandle>,
    /// Shift was held at the start of the frame (read once per frame).
    shift_down: bool,
    /// Shift or Ctrl is held: clicks and bands add to the selection.
    add_down: bool,
    status: String,
}

impl LayoutCraftApp {
    fn new(cc: &eframe::CreationContext<'_>, initial: Option<String>) -> Self {
        appearance::restore(&cc.egui_ctx);
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
            fit_width_pending: false,
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
            layer_rename: None,
            layer_colour: None,
            paste_count: 0,
            ribbon: ribbon::Ribbon::load(),
            ribbon_tab: 1,
            ribbon_collapsed: false,
            ribbon_peek: false,
            ribbon_compact: false,
            ribbon_large: false,
            font_list: None,
            font_filter: String::new(),
            font_warnings: Vec::new(),
            doc_props_open: false,
            help_shortcuts_open: false,
            focus_at_frame_start: false,
            snap_open: false,
            wheel_zooms: false,
            zoom_draft: None,
            colour_draft: None,
            magnify_from: None,
            shapes_minimised: false,
            open_stencil_shown: None,
            sheet_pane: None,
            canvas_keys: false,
            tab_cycle: None,
            show_guides: true,
            connect_arrows: true,
            chosen_point: None,
            moving_point: None,
            arrow_picker: None,
            arrow_shape: None,
            line_ends_read: None,
            guides: Vec::new(),
            selected_guide: None,
            guide_drag: None,
            show_rulers: true,
            ruler_unit: (1.0, "in"),
            size_pos: SizePos::default(),
            name_field: NameField::default(),
            font_target: None,
            style_focus: false,
            align_focus: false,
            style_pick: None,
            layers_open: false,
            full_screen: false,
            data_pane: None,
            link_draft: None,
            theme_all_pages: false,
            theme_cache: None,
            theme_pictures: BTreeMap::new(),
            theme_first: 0,
            theme_hover: None,
            theme_resting: None,
            theme_preview: None,
            change_shape: false,
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
            snap_reach_px: SNAP_REACH_PX,
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
            page_setup_read: None,
            comments_open: false,
            comment_text: String::new(),
            comment_edit: None,
            help_about_open: false,
            mcp_open: false,
            mcp_server: mcpinfo::server_placeholder().to_string(),
            mcp_root: "/path/to/your/drawings".to_string(),
            mcp_message: String::new(),
            ribbon_textures: BTreeMap::new(),
            shift_down: false,
            add_down: false,
            status: "Open a Visio drawing (.vsdx, or the older .vsd and .vdx) or drop one on the window.".to_string(),
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
        self.forget_drawing_state();
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
        // a list that cannot be written only means the drawing is not offered under Recent
        if let Some(file) = recent_file() {
            let _ = remember_recent(&file, path);
        }
        let pages = self
            .engine
            .document
            .as_ref()
            .map_or(0, |document| document.pages.len());
        self.status = if is_scene {
            format!("Imported {} as a new drawing; use Save as to keep it", path.display())
        } else if let Some(conversion) = &self.engine.conversion {
            format!("{} - {pages} page(s). {}", path.display(), conversion.message())
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
        self.forget_drawing_state();
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

    /// Another drawing is open: drop what panels and forms hold of the one before. Its page numbers, shape IDs and undo
    /// depth can all match the new one's.
    fn forget_drawing_state(&mut self) {
        self.name_field = NameField::default();
        self.font_target = None;
        if self.data_pane.is_some() {
            self.data_pane = Some(DataPane::default());
        }
        self.comment_edit = None;
        self.comment_text.clear();
        self.layer_rename = None;
        self.layer_colour = None;
        self.chosen_point = None;
        self.moving_point = None;
        self.link_draft = None;
        self.size_pos.key = None;
        self.text_editor = None;
        if self.style_pick.is_some() {
            self.style_pick = Some(styles::StylePick::default());
        }
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

    /// The scene of the shown page of `document` (the open drawing, or a preview of it), spanning at least the whole page.
    fn page_scene(&self, document: &Document) -> Result<Scene, String> {
        let package = self.engine.package.as_ref().ok_or("no drawing is open")?;
        let mut media: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for (id, record) in &document.media {
            // media_bytes converts what the renderer cannot draw itself (BMP to PNG, EMF / WMF to SVG): the raw part bytes of a stencil picture are an EMF
            if let Some(bytes) = layoutcraft_vsdx::media_bytes(package, record) {
                media.insert(id.clone(), bytes);
            }
        }
        let mut scene = scene_from_document_with_media(document, self.page_index, &media).map_err(|error| error.to_string())?;
        // An empty template page has no content bounds; show the actual page
        // rectangle instead of a 1x1 white square.
        // Likewise a page with shapes shows the WHOLE page (a shape that sticks out of it widens the view), not the bounding box of its
        // shapes: creating the first shape must not shrink the canvas to that shape.
        {
            // the sheet's PageWidth / PageHeight are in drawing units; the scene's shapes are already scaled to paper (page.scale, e.g. 1:50)
            let paper = document.pages.get(self.page_index).map_or(1.0, |page| page.scale).max(1e-6);
            let (page_w, page_h) = ((self.page_width_in * paper * DPI).max(1.0), (self.page_height_in * paper * DPI).max(1.0));
            let (min_x, min_y) = if scene.items.is_empty() { (0.0, 0.0) } else { (scene.min_x.min(0.0), scene.min_y.min(0.0)) };
            let (max_x, max_y) = if scene.items.is_empty() { (page_w, page_h) } else { ((scene.min_x + scene.width).max(page_w), (scene.min_y + scene.height).max(page_h)) };
            scene.min_x = min_x;
            scene.min_y = min_y;
            scene.width = max_x - min_x;
            scene.height = max_y - min_y;
        }
        Ok(scene)
    }

    fn build_page(&mut self, ctx: &egui::Context, reset_view: bool) {
        let _timer = Timer::new("build_page (whole)", 0.0);
        self.sites_cache.clear();
        self.struck_cache = None;
        if let Some(pane) = self.sheet_pane.as_mut() {
            pane.key = None;
        }
        self.guides = self.engine.guides_json(self.page_index).map(|json| guide_lines(&json)).unwrap_or_default();
        self.selected_guide = None;
        self.guide_drag = None;
        if let Ok(size) = self.engine.page_size_json(self.page_index) {
            self.ruler_unit = ruler_unit(&size);
        }
        self.size_pos.key = None;
        self.theme_cache = None;
        (self.theme_preview, self.theme_resting) = (None, None);
        *self.routes_cache.borrow_mut() = None;
        let (Some(package), Some(document)) = (self.engine.package.as_ref(), self.engine.document.as_ref())
        else {
            return;
        };
        if let Ok(pages) = package.pages() {
            if let Some(page) = pages.get(self.page_index) {
                self.page_height_in = page.height.unwrap_or(11.0);
                self.page_width_in = page.width.unwrap_or(8.5);
            }
        }
        let scene_timer = Timer::new("  scene_from_document", 0.0);
        let scene = match self.page_scene(document) {
            Ok(scene) => scene,
            Err(error) => {
                self.status = format!("Could not build page scene: {error}");
                return;
            }
        };
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
            // another page or drawing: none of the selection carries over (its shape IDs can be in use there)
            self.selected = None;
            self.extra_selected.clear();
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
        if shape.rot.abs() > 1e-9 || frame.height() >= HAIRLINE_FRAME {
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

    /// `selection_scene_rect` of the shape `id`, and for a group whose frame is a hairline under pictures that are its members (the SFP
    /// modules of the HPE stencils) what the members cover: the frame alone would put the resize handles across the picture.
    fn selection_rect(&self, id: &str) -> Option<egui::Rect> {
        let shape = self.scene_shape(id)?;
        let own = Self::selection_scene_rect(shape);
        let frame = Self::shape_scene_rect(shape);
        if shape.rot.abs() > 1e-6 || frame.height() >= HAIRLINE_FRAME {
            return Some(own);
        }
        let members = self.scene.iter().flat_map(|scene| scene.items.iter()).filter_map(|item| match item {
            SceneItem::Shape(member) => {
                let member_id = member.visio_id.as_deref()?;
                (member_id != id && self.group_root(member_id) == id).then(|| Self::selection_scene_rect(member))
            }
            _ => None,
        });
        Some(hairline_outline(frame, own, members))
    }

    /// The resize handle a press at `pointer` takes on a shape shown at `rect`. The middle half of a shape in each direction is
    /// always left for moving it: on a small shape the handles would otherwise cover (almost) all of it.
    fn handle_at(rect: egui::Rect, pointer: egui::Pos2) -> Option<Handle> {
        if !Self::handles_fit(rect) || egui::Rect::from_center_size(rect.center(), rect.size() / 2.0).contains(pointer) {
            return None;
        }
        // where handles overlap on a small shape, the nearest one
        Self::handle_rects(rect)
            .into_iter()
            .filter(|(_, hit)| hit.expand(2.0).contains(pointer))
            .min_by(|a, b| a.1.center().distance(pointer).total_cmp(&b.1.center().distance(pointer)))
            .map(|(handle, _)| handle)
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
        if self.snap_shapes {
            for ((x0, x1), (y0, y1)) in self.other_boxes() {
                xs.extend([x0, (x0 + x1) / 2.0, x1]);
                ys.extend([y0, (y0 + y1) / 2.0, y1]);
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

    /// The boxes (x span, y span; page inches, y up) of the page's shapes that are not being moved.
    fn other_boxes(&self) -> Vec<(Span, Span)> {
        let page_h = self.page_height_in;
        let moving = self.all_selected();
        let Some(scene) = self.scene.as_ref() else {
            return Vec::new();
        };
        scene
            .items
            .iter()
            .filter_map(|item| {
                let SceneItem::Shape(shape) = item else { return None };
                let id = shape.visio_id.as_deref()?;
                if moving.iter().any(|m| m == id) || self.doc_shape(id).is_none_or(|doc| doc.group.is_some()) {
                    return None;
                }
                let r = Self::selection_scene_rect(shape);
                let x = (f64::from(r.min.x) / DPI, f64::from(r.max.x) / DPI);
                let y = (page_h - f64::from(r.max.y) / DPI, page_h - f64::from(r.min.y) / DPI);
                Some((x, y))
            })
            .collect()
    }

    /// How near (page inches) something must come to be pulled: the snap strength, which is a distance on screen.
    fn snap_threshold(&self) -> f64 {
        f64::from(self.snap_reach_px) / (DPI * f64::from(self.view_scale().max(0.01)))
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
        let threshold = self.snap_threshold();
        let grid = if self.snap_grid { self.grid_step } else { 0.0 };
        let raw = (f64::from(delta.x) / DPI, -f64::from(delta.y) / DPI);
        let (mut dx, mut dy, mut gx, mut gy) = snap_move(box_in, raw.0, raw.1, grid, &xs, &ys, threshold);
        // equal spacing between two neighbours wins over the grid and edges when it is within reach
        if self.snap_shapes {
            let others = self.other_boxes();
            let (x, y) = ((box_in.0 + raw.0, box_in.2 + raw.0), (box_in.1 + raw.1, box_in.3 + raw.1));
            if let Some((shift, centre)) = equal_gap(x, y, &others, threshold) {
                (dx, gx) = (raw.0 + shift, Some(centre));
            }
            let swapped: Vec<(Span, Span)> = others.iter().map(|(x, y)| (*y, *x)).collect();
            if let Some((shift, centre)) = equal_gap(y, x, &swapped, threshold) {
                (dy, gy) = (raw.1 + shift, Some(centre));
            }
        }
        (egui::vec2((dx * DPI) as f32, (-dy * DPI) as f32), (gx.map(|x| (x * DPI) as f32), gy.map(|y| ((page_h - y) * DPI) as f32)))
    }

    /// While shapes are moved: a line (screen px) across each of two equal gaps between the moved box and its neighbours,
    /// left and right of it or above and below.
    fn spacing_marks(&self, image_rect: egui::Rect) -> Vec<[egui::Pos2; 2]> {
        let (Some(DragState { mode: DragMode::Move, .. }), Some(rect), true) = (&self.drag, self.preview, self.snap_shapes) else {
            return Vec::new();
        };
        let page_h = self.page_height_in;
        let x = (f64::from(rect.min.x) / DPI, f64::from(rect.max.x) / DPI);
        let y = (page_h - f64::from(rect.max.y) / DPI, page_h - f64::from(rect.min.y) / DPI);
        let others = self.other_boxes();
        let swapped: Vec<(Span, Span)> = others.iter().map(|(x, y)| (*y, *x)).collect();
        // page inches (y up) to the screen
        let at = |x: f64, y: f64| self.scene_to_screen(egui::pos2((x * DPI) as f32, ((page_h - y) * DPI) as f32), image_rect);
        let mut marks = Vec::new();
        if let Some(gaps) = equal_gap_marks(x, y, &others) {
            let middle = (y.0 + y.1) / 2.0;
            marks.extend(gaps.map(|gap| [at(gap.0, middle), at(gap.1, middle)]));
        }
        if let Some(gaps) = equal_gap_marks(y, x, &swapped) {
            let middle = (x.0 + x.1) / 2.0;
            marks.extend(gaps.map(|gap| [at(middle, gap.0), at(middle, gap.1)]));
        }
        marks
    }

    /// A point (scene px) pulled to the grid and the snap targets: the corners of a shape being drawn.
    fn snapped_point(&self, point: egui::Pos2) -> egui::Pos2 {
        if !self.snapping_on() {
            return point;
        }
        let (xs, ys) = self.snap_targets();
        let threshold = self.snap_threshold();
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
        let threshold = self.snap_threshold();
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
        let rect = self.scene_rect_to_screen(self.selection_rect(id).unwrap_or_else(|| Self::selection_scene_rect(scene)), image_rect);
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
        let shift = self.add_down;
        if self.tool == Tool::Hand || self.space_down {
            // the Hand tool and Space + drag move the view and touch nothing else
            self.drag = None;
            self.preview = None;
            self.panning = true;
            return;
        }
        if self.tool == Tool::ConnectionPoint {
            self.drag = None;
            self.preview = None;
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
                // from the empty page: the start is loose
                None => {
                    let at = self.snapped_point(scene_pointer);
                    let start = egui::Rect::from_min_size(at, egui::Vec2::ZERO);
                    self.connect_from_site = None;
                    self.drag = Some(DragState { visio_id: String::new(), mode: DragMode::Connect, start_pointer: at, end_pointer: at, start_rect: start });
                    self.preview = Some(start);
                    self.status = "Release on a shape to connect to it, or on the empty page.".to_string();
                }
            }
            return;
        }
        if self.press_connect_arrow(pointer, image_rect) {
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
                let frame_is_shape = self.scene_shape(&selected_id).is_some_and(|scene| self.selection_rect(&selected_id) == Some(Self::shape_scene_rect(scene)));
                if shape.group.is_none() && shape.rot.abs() < 1e-6 && frame_is_shape {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(shape.x as f32, shape.y as f32),
                        egui::vec2(shape.w as f32, shape.h as f32),
                    );
                    let screen_rect = self.scene_rect_to_screen(rect, image_rect);
                    if let Some(handle) = Self::handle_at(screen_rect, pointer) {
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
            let rect = self.selection_rect(&id).unwrap_or(frame);
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
                let raw = pointer - drag.start_pointer;
                let (mut delta, mut found) = self.snapped_move(drag.start_rect, raw);
                // Shift held during the drag keeps the move straight
                if self.shift_down {
                    (delta, found) = axis_locked(raw, delta, found);
                }
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
            self.commit_connector(ctx, &drag.visio_id, drag.start_pointer, drag.end_pointer);
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
                    if self.doc_shape(&id).is_none_or(|doc| doc.group.is_some()) {
                        return None;
                    }
                    let rect = self.selection_rect(&id).unwrap_or_else(|| Self::selection_scene_rect(shape));
                    Some((id, rect))
                }
                _ => None,
            })
            .collect();
        let found = enclosed_ids(&candidates, band);
        let mut chosen: Vec<String> = self.all_selected();
        if !self.add_down {
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
            if let Some(old) = self.selection_rect(visio_id) {
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
        // an undo or redo can take selected shapes away: those that are gone leave the selection
        let mut kept = self.all_selected();
        kept.retain(|id| self.doc_shape(id).is_some() || self.is_connector(id));
        let mut kept = kept.into_iter();
        self.selected = kept.next();
        self.extra_selected = kept.collect();
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
        // A drawing converted from an old format is a .vsdx package now.
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .filter(|_| self.engine.conversion.is_none())
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
            .map(|path| vsdx_name(path))
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("drawing.vsdx")
            .to_string();
        let Some(path) = file_dialog::save_as(&default_name) else {
            return;
        };
        // What is written is a .vsdx package: never under an old format's name, which could be the file that was opened.
        let path = vsdx_name(&path);
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
        self.text_editor = Some(TextEditor { visio_id: id, text, bold, italic, underline, font_pt, initial: (bold, italic, underline, font_pt), focus: true });
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
        self.canvas_keys = true;
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
        let dark = ctx.global_style().visuals.dark_mode;
        let key = format!("{name} {dark}");
        if let Some(texture) = self.ribbon_textures.get(&key) {
            return Some(texture.clone());
        }
        let svg = ribbon::icon_svg(name, appearance::chrome(dark).icon)?;
        let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).ok()?;
        let size = 36u32;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)?;
        let scale = size as f32 / tree.size().width().max(1.0);
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
        let image = egui::ColorImage::from_rgba_premultiplied([size as usize, size as usize], pixmap.data());
        let texture = ctx.load_texture(format!("ribbon-{key}"), image, egui::TextureOptions::LINEAR);
        self.ribbon_textures.insert(key, texture.clone());
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
                    let large: Vec<bool> = cells.iter().map(|cell| matches!(cell, Cell::Command(id) if ribbon::is_large(id))).collect();
                    let items = ui.horizontal_top(|ui| {
                        // every group is as high as its tallest, so the names under them stand on one line
                        ui.set_min_height(ribbon::ITEMS_HEIGHT);
                        ui.spacing_mut().interact_size.y = (ribbon::ITEMS_HEIGHT - ui.spacing().item_spacing.y) / 2.0;
                        for column in ribbon::columns(&large) {
                            ui.vertical(|ui| {
                                for index in column {
                                    let cell = &cells[index];
                                    match cell {
                                        Cell::Select => {
                                            let selecting = self.tool == Tool::Select;
                                            if ui.add(egui::Button::new("Select").selected(selecting)).on_hover_text("Select, move and resize shapes").clicked() {
                                                self.tool = Tool::Select;
                                            }
                                        }
                                        Cell::Command(id) => {
                                            ui.horizontal(|ui| {
                                                self.ribbon_large = large[index];
                                                self.ribbon_button(ui, ctx, id, action);
                                                self.ribbon_large = false;
                                            });
                                        }
                                        // the two galleries stand in the ribbon as pictures, with the list of names after them
                                        Cell::Menu(_, ids) if matches!(ids.as_slice(), [id] if id == "themeGallery" || id == "variantGallery") => {
                                            ui.horizontal(|ui| {
                                                let variants = ids[0] == "variantGallery";
                                                if variants {
                                                    self.variant_strip(ui, ctx, action);
                                                } else {
                                                    self.theme_strip(ui, ctx, action);
                                                }
                                                self.ribbon_themes(ui, variants, action);
                                            });
                                        }
                                        Cell::Menu(label, ids) => {
                                            // the Font popup stays open while A- / A+ and B / I / U are clicked again and again; the others close after a click
                                            let behavior = if label == "Font" { egui::PopupCloseBehavior::CloseOnClickOutside } else { egui::PopupCloseBehavior::CloseOnClick };
                                            egui::containers::menu::MenuButton::new({
                                                let title = egui::RichText::new(format!("{label} \u{25be}"));
                                                if label == "Font" && self.selected_font().is_some_and(|(_, replaced)| replaced.is_some()) { title.color(appearance::chrome(ui.visuals().dark_mode).error) } else { title }
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
        if id == "themeGallery" || id == "variantGallery" {
            self.ribbon_themes(ui, id == "variantGallery", action);
            return;
        }
        if id == "paletteSel" {
            self.ribbon_palette(ui);
            return;
        }
        // the recently opened drawings stand beside Open
        if id == "file" {
            ui.menu_button("Recent \u{25be}", |ui| {
                let list = recent_file().map(|file| read_recent(&file)).unwrap_or_default();
                if list.is_empty() {
                    ui.label("No drawings opened yet.");
                }
                for path in list {
                    let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
                    if ui.button(name).on_hover_text(path.display().to_string()).clicked() {
                        *action = Some(Action::OpenRecent(path));
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Drawings opened lately");
        }
        if id == "zl" {
            self.ribbon_zoom(ui, action);
            return;
        }
        if id == "designOrientation" || id == "designSize" {
            self.ribbon_page_shape(ui, id, action);
            return;
        }
        if id == "pageTools" {
            // the new page large, the other page commands two to a column beside it
            self.ribbon_large = true;
            self.ribbon_button(ui, ctx, "pageAdd", action);
            self.ribbon_large = false;
            for pair in [["pageDup", "pageRename"], ["pageDel", "pageLeft"], ["pageRight", ""]] {
                ui.vertical(|ui| {
                    for page_id in pair.into_iter().filter(|page_id| !page_id.is_empty()) {
                        ui.horizontal(|ui| self.ribbon_button(ui, ctx, page_id, action));
                    }
                });
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
            Some(ribbon::Native::ReverseEnds) => enabled &= !self.selected_connectors().is_empty(),
            Some(ribbon::Native::Redo) => enabled &= self.engine.can_redo(),
            _ => {}
        }
        let label = ribbon::label(id);
        let icon = self.ribbon.icons.get(id).cloned();
        let selected = (native == Some(ribbon::Native::Connector) && self.tool == Tool::Connector) || (native == Some(ribbon::Native::TextBox) && self.tool == Tool::Draw(5)) || (native == Some(ribbon::Native::Hand) && self.tool == Tool::Hand)
            || (native == Some(ribbon::Native::ChangeShape) && self.change_shape)
            || (native == Some(ribbon::Native::ConnectionPoint) && self.tool == Tool::ConnectionPoint)
            || (native == Some(ribbon::Native::SnapGrid) && self.snap_grid)
            || (native == Some(ribbon::Native::SnapShapes) && self.snap_shapes)
            || (native == Some(ribbon::Native::SnapGuides) && self.snap_guides)
            || (native == Some(ribbon::Native::ShowGrid) && self.show_grid)
            || (native == Some(ribbon::Native::ShowRulers) && self.show_rulers)
            || (native == Some(ribbon::Native::ShowGuides) && self.show_guides)
            || (native == Some(ribbon::Native::ConnectArrows) && self.connect_arrows);
        let compact = self.ribbon_compact;
        let large = std::mem::take(&mut self.ribbon_large);
        let response = match icon.and_then(|name| self.icon_texture(ctx, &name)) {
            Some(texture) if large => large_button(ui, &texture, &label, enabled, selected),
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
                    ribbon::Native::ConnectionPoint => self.toggle_connection_point_tool(),
                    ribbon::Native::ChangeShape => {
                        self.change_shape = !self.change_shape && self.selected.is_some();
                        if self.change_shape {
                            self.shapes_open = true;
                            self.shapes_minimised = false;
                            self.armed_master = None;
                            self.status = "Change Shape: click a shape in the Shapes panel (Esc cancels).".to_string();
                        } else if self.selected.is_none() {
                            self.status = "Select the shape to change first.".to_string();
                        }
                    }
                    ribbon::Native::SnapGrid => self.snap_grid = !self.snap_grid,
                    ribbon::Native::SnapShapes => self.snap_shapes = !self.snap_shapes,
                    ribbon::Native::SnapGuides => self.snap_guides = !self.snap_guides,
                    ribbon::Native::ShowGrid => self.show_grid = !self.show_grid,
                    ribbon::Native::ConnectArrows => self.connect_arrows = !self.connect_arrows,
                    ribbon::Native::ShowRulers => self.show_rulers = !self.show_rulers,
                    ribbon::Native::ShowGuides => {
                        self.show_guides = !self.show_guides;
                        self.selected_guide = None;
                    }
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
                let [struck, capitals] = self.selection_text_flags();
                let on = enabled && struck;
                if ui.add_enabled(enabled, egui::Button::new(egui::RichText::new("Strike").strikethrough()).selected(on)).on_hover_text("Strikethrough of the whole text").clicked() {
                    *action = Some(Action::FormatText(serde_json::json!({ "cells": { "Strikethru": if on { "0" } else { "1" } } })));
                }
                let on = enabled && capitals;
                if ui.add_enabled(enabled, egui::Button::new("Caps").selected(on)).on_hover_text("All capitals for the whole text").clicked() {
                    *action = Some(Action::FormatText(serde_json::json!({ "cells": { "Case": if on { "0" } else { "1" } } })));
                }
                if ui.add_enabled(enabled, egui::Button::new("Text colour...")).on_hover_text("Colour of the text of the selected shapes").clicked() {
                    *action = Some(Action::ColourDialog("textColor"));
                    ui.close();
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
                    title = title.color(appearance::chrome(ui.visuals().dark_mode).error);
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
                let px = primary.as_ref().and_then(|shape| shape.font_size);
                let in_points = |px: Option<f64>| px.map_or(12.0, |px| (px * 72.0 / 96.0 * 10.0).round() / 10.0);
                let points = in_points(px);
                let step = |ui: &mut egui::Ui, label: &str, delta: f64, action: &mut Option<Action>| {
                    if ui.add_enabled(enabled, egui::Button::new(label)).on_hover_text(format!("Font size {points} pt")).clicked() {
                        *action = Some(Action::StyleSelected(serde_json::json!({ "fontPt": stepped_font_size(px, delta) }).to_string()));
                    }
                };
                step(ui, "A-", -1.0, action);
                // typed only (no dragging, which would make an edit at every pointer move); Enter applies, Esc does not
                // while the field is being typed in it belongs to the shapes it was entered for: leaving it by clicking another
                // shape changes the selection before the field hands over what was typed
                let held = self.font_target.clone().filter(|ids| ids.iter().all(|id| self.doc_shape(id).is_some()));
                let shown = held.as_ref().and_then(|ids| ids.first()).map_or(points, |id| in_points(self.doc_shape(id).and_then(|shape| shape.font_size)));
                let mut typed = shown;
                let field = ui
                    .add_enabled(enabled, egui::DragValue::new(&mut typed).speed(0.0).custom_formatter(|points, _| points.to_string()).suffix(" pt").update_while_editing(false))
                    .on_hover_text("Font size: click and type");
                ui.ctx().accesskit_node_builder(field.id, |node| node.set_label("Font size in points"));
                if field.changed() && typed != shown {
                    if FONT_SIZES.contains(&typed) {
                        let target = held.unwrap_or_else(|| self.all_selected());
                        *action = Some(Action::StyleShapes(target, serde_json::json!({ "fontPt": typed }).to_string()));
                    } else {
                        *action = Some(Action::Say(format!("Type a font size from {} to {} points.", FONT_SIZES.start(), FONT_SIZES.end())));
                    }
                }
                if !field.has_focus() {
                    self.font_target = None;
                } else if self.font_target.is_none() {
                    self.font_target = Some(self.all_selected());
                }
                step(ui, "A+", 1.0, action);
            }
            _ => {
                let (label, key) = if id == "p:fillColor" { ("Fill ▾", "fill") } else { ("Line ▾", "line") };
                // a connector has a line (and line ends) but no fill
                let enabled = enabled || (key == "line" && self.can_set_line_ends());
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
                        if ui.button("More colours...").clicked() {
                            *action = Some(Action::ColourDialog(key));
                            ui.close();
                        }
                    });
                });
                if key == "line" {
                    self.line_ends_menu(ui, action);
                }
            }
        }
    }

    /// Whether the whole text of the selection is struck through, and whether it is in all capitals; remembered until the
    /// page is rebuilt or the selection changes (it reads the file's XML).
    fn selection_text_flags(&mut self) -> [bool; 2] {
        let ids = self.all_selected();
        let key = ids.join("\n");
        match &self.struck_cache {
            Some((known, flags)) if *known == key => *flags,
            _ => {
                let flags = [all_struck(&self.engine, self.page_index, &ids), all_set(&self.engine, self.page_index, &ids, "Case")];
                self.struck_cache = Some((key, flags));
                flags
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
    /// Style the shapes `ids`, which need not be the selected ones; the selection stays.
    fn style_shapes(&mut self, ctx: &egui::Context, ids: &[String], json: &str) {
        if ids.is_empty() {
            self.status = "Select a shape first.".to_string();
            return;
        }
        let result = self.engine.edit(self.page_index, 10, &format!("{}\n{json}", ids.join("\n")), 0.0, 0.0, 0.0, 0.0, 0);
        let selected = self.selected.clone();
        self.finish_edit(ctx, result, "Changed style", selected);
    }

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
            N::FitWidth => Action::FitWidth,
            N::ActualSize => Action::ActualSize,
            N::ZoomIn => Action::ZoomIn,
            N::ZoomOut => Action::ZoomOut,
            N::NewDrawing => Action::NewDrawing,
            N::OpenFile => Action::OpenDialog,
            N::DocProps => Action::DocProps,
            N::HelpShortcuts => Action::HelpShortcuts,
            N::Hyperlink => Action::Hyperlink,
            N::Layers => Action::Layers,
            N::CopyPicture => Action::CopyPicture,
            N::ShapeFormulas => Action::ShapeFormulas,
            N::Styles => Action::Styles,
            N::SizePosition => Action::SizePosition,
            N::FullScreen => Action::FullScreen,
            N::ShapeData => Action::ShapeData,
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
            // "Change Shape" is a mode, handled where the button is drawn
            N::ChangeShape => Action::Fit,
            // the Pan tool is a tool too
            N::Hand => Action::Fit,
            // the tool is taken where the button is drawn
            N::ConnectionPoint => Action::Fit,
            // the snap switches and the grid step are state, handled where the button is drawn
            N::SnapGrid | N::SnapShapes | N::SnapGuides | N::GridStep | N::ShowGrid | N::ShowRulers | N::ShowGuides | N::ConnectArrows => Action::Fit,
            N::ReverseEnds => Action::ReverseEnds,
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
        // the newest stencil is the open one
        self.open_stencil_shown = Some(index);
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
        let mut converted = String::new();
        let result = fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| self.engine.stencil_open(bytes)).and_then(|json| {
            let answer: Option<serde_json::Value> = serde_json::from_str(&json).ok();
            converted = answer.and_then(|answer| answer["converted"].as_str().map(str::to_string)).unwrap_or_default();
            self.register_stencil(&name, &json)
        });
        if result.is_ok() {
            if let Some(entry) = self.stencils.last_mut() {
                entry.path = Some(path.clone());
            }
            self.remember_stencils();
        }
        self.status = match result {
            Ok(()) => format!("Opened the stencil {name}: click a shape, then click on the page to place it. {converted}"),
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

    /// Turn the selected shapes into instances of a stencil master; the new shapes stay selected.
    fn change_selected_shapes(&mut self, ctx: &egui::Context, stencil: i32, master: &str, name: &str) {
        let page = self.page_index;
        let selection = self.all_selected();
        let result = self.engine.group_info_json(page).and_then(|frames| {
            let ids = changeable_shapes(&frames, &selection, |id| self.is_connector(id));
            change_shapes(&mut self.engine, page, &ids, stencil, master)
        });
        match result {
            Ok(mut made) => {
                let rest = made.split_off(made.len().min(1));
                let count = rest.len() + made.len();
                self.finish_edit(ctx, Ok(()), &format!("Changed {count} shape(s) to \"{name}\""), made.pop());
                self.extra_selected = rest;
            }
            Err(error) => self.status = format!("Could not change the shape: {error}"),
        }
    }

    /// `Engine::themes_json`, read once per page build (it parses every theme of the gallery).
    fn theme_info(&mut self) -> Option<serde_json::Value> {
        if self.theme_cache.is_none() {
            self.theme_cache = self.engine.themes_json().ok().and_then(|text| serde_json::from_str(&text).ok());
        }
        self.theme_cache.clone()
    }

    /// The Design tab's Themes and Variants menus: for the shown page, or for all pages when the box is ticked.
    fn ribbon_themes(&mut self, ui: &mut egui::Ui, variants: bool, action: &mut Option<Action>) {
        let enabled = self.texture.is_some();
        ui.add_enabled_ui(enabled, |ui| {
            ui.menu_button(if variants { "Variants \u{25be}" } else { "Themes \u{25be}" }, |ui| {
                let Some(info) = self.theme_info() else {
                    ui.label("The themes cannot be read.");
                    return;
                };
                let (current, variant) = current_theme(&info, self.page_index);
                ui.checkbox(&mut self.theme_all_pages, "Apply to all pages");
                let op = theme_op(variants, self.theme_all_pages);
                if !variants {
                    for theme in info["themes"].as_array().into_iter().flatten() {
                        let Some(name) = theme["name"].as_str() else { continue };
                        let chosen = ui
                            .horizontal(|ui| {
                                // the colours first, as in a list with a picture beside each name
                                theme_swatches(ui, &theme["accents"]);
                                ui.add_space(4.0);
                                ui.selectable_label(current.as_deref() == Some(name), name).clicked()
                            })
                            .inner;
                        if chosen {
                            *action = Some(Action::Theme(op, name.to_string()));
                            ui.close();
                        }
                    }
                } else if current.is_none() {
                    ui.label("Apply a theme first (Design > Themes).");
                } else {
                    // each variant of the shown theme by its first two colours
                    let shown = info["themes"].as_array().into_iter().flatten().find(|theme| theme["name"].as_str() == current.as_deref());
                    for choice in 0..4u64 {
                        let colours = shown.map_or(serde_json::Value::Null, |theme| theme["variants"][choice as usize].clone());
                        let chosen = ui
                            .horizontal(|ui| {
                                theme_swatches(ui, &colours);
                                ui.add_space(4.0);
                                ui.selectable_label(choice == variant, format!("Variant {}", choice + 1)).clicked()
                            })
                            .inner;
                        if chosen {
                            *action = Some(Action::Theme(op, choice.to_string()));
                            ui.close();
                        }
                    }
                }
            });
        });
    }

    /// Apply a theme or variant (`theme_op`) with the shown page as the target.
    fn apply_theme(&mut self, ctx: &egui::Context, op: i32, value: &str) {
        let page = self.page_index;
        let result = self.engine.page_op(op, page, 0, value).map(|_| ());
        let selected = self.selected.clone();
        self.finish_edit(ctx, result, "Changed the theme", selected);
    }

    /// The Design tab's Orientation and Size menus; the current choice is marked.
    fn ribbon_page_shape(&mut self, ui: &mut egui::Ui, id: &str, action: &mut Option<Action>) {
        let (engine, page, enabled) = (&self.engine, self.page_index, self.texture.is_some());
        let title = if id == "designSize" { "Size \u{25be}" } else { "Orientation \u{25be}" };
        ui.add_enabled_ui(enabled, |ui| {
            ui.menu_button(title, |ui| {
                let Some(current) = page_dimensions(engine, page) else {
                    ui.label("The page size cannot be read.");
                    return;
                };
                let choices: Vec<(&str, Option<(f64, f64)>, Option<bool>)> = if id == "designSize" {
                    PAGE_SIZES.iter().map(|(name, short, long)| (*name, Some((*short, *long)), None)).collect()
                } else {
                    vec![("Portrait", None, Some(true)), ("Landscape", None, Some(false))]
                };
                for (name, paper, portrait) in choices {
                    let next = shaped_page(current, paper, portrait);
                    if ui.selectable_label(next.is_none(), name).clicked() {
                        if let Some((w, h)) = next {
                            *action = Some(Action::PageShape(w, h));
                        }
                        ui.close();
                    }
                }
            });
        });
    }

    /// Set the size of the shown page (the shapes keep their positions) and fit it to the window.
    fn set_page_shape(&mut self, ctx: &egui::Context, w: f64, h: f64) {
        let page = self.page_index;
        let result = self.engine.page_op(6, page, 0, &format!("{w},{h}")).map(|_| ());
        let changed = result.is_ok();
        let selected = self.selected.clone();
        self.finish_edit(ctx, result, "Page size changed", selected);
        self.fit_pending |= changed;
    }

    /// Open the Hyperlink window for the one selected shape.
    fn open_links(&mut self) {
        let [shape] = &self.all_selected()[..] else {
            self.status = "Select one shape to give it a hyperlink.".to_string();
            return;
        };
        let shape = shape.clone();
        let page = self.page_index;
        match self.engine.hyperlinks_json(page, &shape).and_then(|json| serde_json::from_str::<Vec<serde_json::Value>>(&json).map_err(|error| error.to_string())) {
            Ok(mut links) => {
                if links.is_empty() {
                    links.push(blank_link());
                }
                self.link_draft = Some(LinkDraft { page, shape, links, error: String::new(), focus: true });
            }
            Err(error) => self.status = format!("Could not read the hyperlinks: {error}"),
        }
    }

    /// The Hyperlink window: OK saves (one undo step), Cancel or Esc closes.
    fn draw_links(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.link_draft.as_mut() else {
            return;
        };
        let mut open = true;
        let (mut save, mut cancel) = (false, false);
        egui::Window::new("Hyperlinks").open(&mut open).collapsible(false).default_width(420.0).show(ctx, |ui| {
            ui.label(format!("Shape {}", draft.shape));
            let mut remove = None;
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for (at, link) in draft.links.iter_mut().enumerate() {
                    ui.group(|ui| {
                        egui::Grid::new(("link", at)).num_columns(2).show(ui, |ui| {
                            for (label, key) in [("Address", "address"), ("Sub-address (page or bookmark)", "subAddress"), ("Description", "description")] {
                                let name = ui.label(label);
                                let mut value = link[key].as_str().unwrap_or("").to_string();
                                let edit = ui.text_edit_singleline(&mut value).labelled_by(name.id);
                                if at == 0 && key == "address" && std::mem::take(&mut draft.focus) {
                                    edit.request_focus();
                                }
                                if edit.changed() {
                                    link[key] = serde_json::Value::String(value);
                                    draft.error.clear();
                                }
                                ui.end_row();
                            }
                        });
                        if ui.button("Remove this link").clicked() {
                            remove = Some(at);
                        }
                    });
                }
            });
            if let Some(at) = remove {
                draft.links.remove(at);
            }
            if ui.button("Add a link").clicked() {
                draft.links.push(blank_link());
            }
            if !draft.error.is_empty() {
                ui.label(&draft.error);
            }
            ui.horizontal(|ui| {
                save = ui.button("OK").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if cancel || !open || ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.link_draft = None;
        } else if save {
            self.apply_links(ctx);
        }
    }

    fn apply_links(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.link_draft.take() else {
            return;
        };
        match save_links(&mut self.engine, draft.page, &draft.shape, &draft.links) {
            Ok(true) => {
                let selected = self.selected.clone();
                self.finish_edit(ctx, Ok(()), "Changed hyperlinks", selected);
            }
            Ok(false) => {}
            Err(error) => {
                draft.error = error;
                self.link_draft = Some(draft);
            }
        }
    }

    /// The Shape Data window follows the selection. A field is written when it is left (or Enter is pressed) with a new
    /// content; each write is one undo step.
    fn draw_data_pane(&mut self, ctx: &egui::Context) {
        let Some(mut pane) = self.data_pane.take() else {
            return;
        };
        let page = self.page_index;
        let selection = self.all_selected();
        let shape = match &selection[..] {
            [one] => Some(one.clone()),
            _ => None,
        };
        let key = shape.as_ref().map(|shape| (page, shape.clone(), self.engine.undo_depth()));
        // Another shape (or none) is shown now: a value typed for the one before, which its field had not handed over yet, is
        // written to it first. After any other edit the rows are only read again.
        let mut left_behind: Vec<(usize, String, DataRow)> = Vec::new();
        if let Some((old_page, old_shape, depth)) = pane.key.as_ref().filter(|old| key.as_ref().is_none_or(|new| (old.0, &old.1) != (new.0, &new.1))) {
            let before = self.engine.document().and_then(|document| document.pages.get(*old_page)).and_then(|page| page.shapes.iter().find(|shape| shape.visio_id.as_ref() == Some(old_shape)));
            if let Some(before) = before.filter(|_| *depth == self.engine.undo_depth()) {
                left_behind.extend(pane.rows.iter().filter(|row| before.data.iter().any(|old| old.name == row.name && old.value != row.value)).map(|row| (*old_page, old_shape.clone(), row.clone())));
            }
        }
        for (old_page, old_shape, row) in left_behind {
            let result = write_data_row(&mut self.engine, old_page, &old_shape, &row);
            let selected = self.selected.clone();
            self.finish_edit(ctx, result, "Changed shape fields", selected);
        }
        let key = shape.as_ref().map(|shape| (page, shape.clone(), self.engine.undo_depth()));
        if pane.key != key {
            // the form for changing a field belongs to the shape it was opened on
            let shown = |key: &Option<(usize, String, usize)>| key.as_ref().map(|(page, shape, _)| (*page, shape.clone()));
            if shown(&pane.key) != shown(&key) && pane.changing.take().is_some() {
                pane.new_type = 0;
                pane.new_prompt.clear();
                pane.new_choices.clear();
            }
            pane.rows = shape.as_deref().and_then(|id| self.doc_shape(id)).map(|shape| shape.data.clone()).unwrap_or_default();
            pane.key = key;
        }
        let mut open = true;
        // the row to write, or the name of the row to remove
        let mut write: Option<DataRow> = None;
        let mut reread = false;
        let mut remove: Option<String> = None;
        let mut refused: Option<String> = None;
        // the row whose type and prompt are to be changed
        let mut change: Option<DataRow> = None;
        egui::Window::new("Shape Fields").open(&mut open).collapsible(true).default_width(360.0).show(ctx, |ui| {
            if shape.is_none() {
                ui.label("Select one shape to see its fields.");
                return;
            }
            if pane.rows.is_empty() {
                ui.label("This shape has no fields.");
            }
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                egui::Grid::new("shape-data").num_columns(4).show(ui, |ui| {
                    for row in &mut pane.rows {
                        let title = if row.label.is_empty() { row.name.clone() } else { row.label.clone() };
                        let shown = if row.invisible != 0 { format!("{title} (hidden)") } else { title.clone() };
                        let label = ui.label(shown);
                        if !row.prompt.is_empty() {
                            label.clone().on_hover_text(&row.prompt);
                        }
                        if row.r#type == 3 {
                            let mut on = row.value == "1" || row.value.eq_ignore_ascii_case("true");
                            if ui.checkbox(&mut on, "Yes").labelled_by(label.id).changed() {
                                row.value = if on { "1" } else { "0" }.to_string();
                                write = Some(row.clone());
                            }
                        } else if row.r#type == 1 || row.r#type == 4 {
                            let before = row.value.clone();
                            egui::ComboBox::from_id_salt(("data", &row.name)).selected_text(row.value.clone()).show_ui(ui, |ui| {
                                for item in row.format.split(';').filter(|item| !item.is_empty()) {
                                    ui.selectable_value(&mut row.value, item.to_string(), item);
                                }
                            })
                            .response
                            .labelled_by(label.id);
                            if row.value != before {
                                write = Some(row.clone());
                            }
                        } else {
                            let field = ui.text_edit_singleline(&mut row.value).labelled_by(label.id);
                            match field_left(ui, &field) {
                                Some(Left::Apply) => write = Some(row.clone()),
                                Some(Left::Revert) => reread = true,
                                None => {}
                            }
                        }
                        if ui.button("Change").on_hover_text(format!("Change the type and prompt of the field {title}")).clicked() {
                            change = Some(row.clone());
                        }
                        if ui.button("Remove").on_hover_text(format!("Remove the field {title}")).clicked() {
                            remove = Some(row.name.clone());
                        }
                        ui.end_row();
                    }
                });
            });
            ui.separator();
            let changing = pane.changing.as_ref().and_then(|name| pane.rows.iter().find(|row| &row.name == name).cloned());
            if let Some(row) = &changing {
                ui.label(format!("Change the field {}", if row.label.is_empty() { &row.name } else { &row.label }));
            } else {
                ui.horizontal(|ui| {
                    let label = ui.label("New field");
                    let edit = ui.text_edit_singleline(&mut pane.new_label).labelled_by(label.id);
                    let entered = edit.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    if ui.button("Add").clicked() || entered {
                        match new_data_row(&pane.new_label, &pane.rows).map(|row| typed_data_row(row, pane.new_type, &pane.new_choices, &pane.new_prompt)) {
                            Some(Ok(row)) => {
                                write = Some(row);
                                pane.new_label.clear();
                                pane.new_prompt.clear();
                                pane.new_choices.clear();
                            }
                            Some(Err(why)) => refused = Some(why),
                            None => {}
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                // a field of a type that is not offered here (a number, a date) keeps it
                if changing.as_ref().is_some_and(|row| NEW_FIELD_TYPES.iter().all(|kind| kind.0 != row.r#type)) {
                    ui.label("Its type is kept.");
                } else {
                    let label = ui.label("Type");
                    let shown = NEW_FIELD_TYPES.iter().find(|kind| kind.0 == pane.new_type).map_or("", |kind| kind.1);
                    egui::ComboBox::from_id_salt("data-new-type")
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            for (kind, name) in NEW_FIELD_TYPES {
                                ui.selectable_value(&mut pane.new_type, kind, name);
                            }
                        })
                        .response
                        .labelled_by(label.id);
                }
                let label = ui.label("Prompt");
                ui.add(egui::TextEdit::singleline(&mut pane.new_prompt).desired_width(140.0)).labelled_by(label.id).on_hover_text("Shown when the pointer rests on the field's name");
            });
            if pane.new_type == 1 {
                ui.horizontal(|ui| {
                    let label = ui.label("Choices, separated by ;");
                    ui.text_edit_singleline(&mut pane.new_choices).labelled_by(label.id);
                });
            }
            if let Some(row) = changing {
                ui.checkbox(&mut pane.new_hidden, "Hidden").on_hover_text("A hidden field is marked in the file for programs not to show it; it stays listed here, marked as hidden");
                let mut done = false;
                ui.horizontal(|ui| {
                    if ui.button("Save field").clicked() {
                        match changed_data_row(&row, pane.new_type, &pane.new_choices, &pane.new_prompt) {
                            Ok(mut changed) => {
                                // a file may say "hidden" with any number but 0: keep it while the choice stays
                                if (changed.invisible != 0) != pane.new_hidden {
                                    changed.invisible = u32::from(pane.new_hidden);
                                }
                                write = Some(changed);
                                done = true;
                            }
                            Err(why) => refused = Some(why),
                        }
                    }
                    done |= ui.button("Cancel").clicked();
                });
                if done {
                    pane.changing = None;
                    pane.new_type = 0;
                    pane.new_prompt.clear();
                    pane.new_choices.clear();
                }
            }
        });
        if let Some(row) = change {
            pane.new_type = row.r#type;
            pane.new_prompt = row.prompt.clone();
            pane.new_hidden = row.invisible != 0;
            pane.new_choices = if row.r#type == 1 { row.format.clone() } else { String::new() };
            pane.changing = Some(row.name);
        }
        if let Some(why) = refused {
            self.status = why;
        }
        if let Some(shape) = shape {
            // leaving a field without a change writes nothing
            let write = write.filter(|row| {
                let before = self.doc_shape(&shape).and_then(|shape| shape.data.iter().find(|old| old.name == row.name));
                before.is_none_or(|old| (&old.value, old.r#type, &old.prompt, &old.format, old.invisible) != (&row.value, row.r#type, &row.prompt, &row.format, row.invisible))
            });
            let result = if let Some(name) = remove {
                Some(self.engine.edit(page, 18, &format!("{shape}\n{name}"), 0.0, 0.0, 0.0, 0.0, 0))
            } else {
                write.map(|row| write_data_row(&mut self.engine, page, &shape, &row))
            };
            if let Some(result) = result {
                // read the rows again, also after a refused value
                pane.key = None;
                self.finish_edit(ctx, result, "Changed shape fields", Some(shape));
            }
        }
        if reread {
            pane.key = None;
        }
        if open {
            self.data_pane = Some(pane);
        }
    }

    /// Enter or leave the full-screen view.
    fn toggle_full_screen(&mut self, ctx: &egui::Context) {
        self.full_screen = !self.full_screen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.full_screen));
        self.fit_pending = true;
        self.status = if self.full_screen { "Full screen: Esc or F5 leaves it.".to_string() } else { "Left full screen.".to_string() };
    }

    /// The layers of the shown page: visible / prints / locked, the selected shape's membership, delete and add.
    fn layer_controls(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
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
        let (page, depth) = (self.page_index, self.engine.undo_depth());
        if self.layer_rename.as_ref().is_some_and(|((at, _, then), _)| *at != page || *then != depth) {
            self.layer_rename = None;
        }
        if self.layer_colour.as_ref().is_some_and(|((at, _, then), _)| *at != page || *then != depth) {
            self.layer_colour = None;
        }
        for layer in &layers {
            // wrapped: a long layer name must not widen the inspector
            ui.horizontal_wrapped(|ui| {
                let (mut visible, mut print, mut locked) = (layer.visible, layer.print, layer.locked);
                let a = ui.checkbox(&mut visible, "V").on_hover_text("Visible");
                let b = ui.checkbox(&mut print, "P").on_hover_text("Prints");
                let c = ui.checkbox(&mut locked, "L").on_hover_text("Locked");
                if a.changed() || b.changed() || c.changed() {
                    *action = Some(Action::LayerOp(8, layer.index as usize, format!("|{}|{}|{}", visible as u8, print as u8, locked as u8)));
                }
                let mut on_layer = selected_layers.contains(&layer.index);
                let was = on_layer;
                if ui.add_enabled(has_selection, egui::Checkbox::new(&mut on_layer, "Sel")).on_hover_text("The selected shape is on this layer").changed() && on_layer != was {
                    *action = Some(Action::LayerShape(layer.index as usize, on_layer));
                }
                if let Some(colour) = layer.color.as_ref().filter(|colour| typed_colour(colour).is_some()) {
                    // the layer's colour as a square; its code on hover, so that it does not rest on colour
                    let [r, g, b] = typed_colour(colour).unwrap_or_default();
                    let (rect, square) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect(rect, 0.0, egui::Color32::from_rgb(r, g, b), egui::Stroke::new(1.0, ui.visuals().weak_text_color()), egui::StrokeKind::Inside);
                    square.on_hover_text(format!("Layer colour {colour}"));
                }
                ui.label(&layer.name);
                if ui.small_button("Colour").on_hover_text(format!("Colour of the shapes on the layer {}", layer.name)).clicked() {
                    self.layer_colour = Some(((page, layer.index, depth), layer.color.clone().filter(|colour| typed_colour(colour).is_some()).unwrap_or_default()));
                }
                if ui.small_button("Rename").on_hover_text(format!("Rename the layer {}", layer.name)).clicked() {
                    self.layer_rename = Some(((page, layer.index, depth), layer.name.clone()));
                }
                if ui.small_button("x").on_hover_text("Delete the layer (shapes stay)").clicked() {
                    *action = Some(Action::LayerOp(9, layer.index as usize, String::new()));
                }
            });
            if let Some((_, code)) = self.layer_colour.as_mut().filter(|((_, index, _), _)| *index == layer.index) {
                let (mut apply, mut none, mut cancel) = (false, false, false);
                ui.horizontal(|ui| {
                    let label = ui.label("Colour code");
                    let field = ui.add(egui::TextEdit::singleline(code).hint_text("#rrggbb").desired_width(70.0)).labelled_by(label.id);
                    apply = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    let mut picked = typed_colour(code).unwrap_or_default();
                    if ui.color_edit_button_srgb(&mut picked).changed() {
                        *code = hex(picked);
                    }
                });
                ui.horizontal(|ui| {
                    apply |= ui.small_button("Apply colour").clicked();
                    none = ui.small_button("No colour").clicked();
                    cancel = ui.small_button("Cancel").clicked();
                });
                if none {
                    *action = Some(Action::LayerOp(10, layer.index as usize, "none".to_string()));
                } else if apply {
                    match typed_colour(code) {
                        Some(colour) => *action = Some(Action::LayerOp(10, layer.index as usize, hex(colour))),
                        None => *action = Some(Action::Say("Type a colour as #rrggbb, or pick one.".to_string())),
                    }
                }
                if cancel {
                    self.layer_colour = None;
                }
            }
            let renaming = self.layer_rename.as_mut().filter(|((_, index, _), _)| *index == layer.index);
            if let Some((_, name)) = renaming {
                let (mut save, mut cancel) = (false, false);
                ui.horizontal(|ui| {
                    let label = ui.label("New name");
                    let field = ui.add(egui::TextEdit::singleline(name).desired_width(110.0)).labelled_by(label.id);
                    save = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    save |= ui.small_button("Save name").clicked();
                    cancel = ui.small_button("Cancel").clicked();
                });
                if save {
                    let wanted = name.trim().to_string();
                    // "|" separates the parts of the engine's request
                    if wanted.is_empty() || wanted.contains('|') {
                        *action = Some(Action::Say("A layer name needs at least one character, and cannot contain \"|\".".to_string()));
                    } else if wanted == layer.name {
                        cancel = true;
                    } else {
                        *action = Some(Action::LayerOp(8, layer.index as usize, format!("{wanted}|||")));
                    }
                }
                if cancel {
                    self.layer_rename = None;
                }
            }
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.layer_name).desired_width(90.0).hint_text("Layer name"));
            if ui.add_enabled(!self.layer_name.trim().is_empty() && self.texture.is_some(), egui::Button::new("+ Layer")).clicked() {
                *action = Some(Action::LayerOp(7, 0, self.layer_name.trim().to_string()));
                self.layer_name.clear();
            }
        });
    }

    /// The Size and Position fields of the selected shape. A field is applied when it is left (or Enter is pressed) with
    /// a new content.
    /// The shape's name. It is written when the field is left (or Enter is pressed) with another content; Esc gives the old
    /// name back. An empty field returns the shape to its automatic name.
    fn shape_name_controls(&mut self, ui: &mut egui::Ui, id: &str, action: &mut Option<Action>) {
        let key = (self.page_index, id.to_string(), self.engine.undo_depth());
        if self.name_field.key.as_ref() != Some(&key) {
            let names = self.engine.shape_names_json(self.page_index).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()).unwrap_or_default();
            self.name_field.read = names[id].as_str().unwrap_or("").to_string();
            self.name_field.text = self.name_field.read.clone();
            self.name_field.key = Some(key);
        }
        ui.horizontal(|ui| {
            let label = ui.label("Name");
            let edit = ui.add(egui::TextEdit::singleline(&mut self.name_field.text).hint_text("automatic").desired_width(150.0)).labelled_by(label.id);
            match field_left(ui, &edit) {
                Some(Left::Revert) => self.name_field.text = self.name_field.read.clone(),
                Some(Left::Apply) if self.name_field.text.trim() != self.name_field.read => *action = Some(Action::ShapeName(id.to_string(), self.name_field.text.trim().to_string())),
                _ => {}
            }
        });
    }

    fn set_shape_name(&mut self, ctx: &egui::Context, id: &str, wanted: &str) {
        let page = self.page_index;
        let result = self.engine.page_op(40, page, 0, &format!("{id}|{wanted}")).map(|_| ());
        // read the field again, also after a refused name
        self.name_field.key = None;
        let selected = self.selected.clone();
        // a name another shape has is made unique by the engine: say what the shape is called now
        let given = self.engine.shape_names_json(page).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()).and_then(|names| names[id].as_str().map(str::to_string)).unwrap_or_default();
        let done = if given.is_empty() { "The shape has its automatic name again".to_string() } else { format!("The shape is named {given}") };
        self.finish_edit(ctx, result, &done, selected);
    }

    /// Give the keyboard to the inspector's first Style setting, or say what to select.
    fn go_to_style_settings(&mut self) {
        // only when the inspector is there to show them, so that no later selection takes the keyboard
        if !self.full_screen && self.selected.as_deref().and_then(|id| self.doc_shape(id)).is_some_and(|shape| shape.group.is_none()) {
            self.style_focus = true;
        } else {
            self.status = "Select one shape to edit its style in the inspector.".to_string();
        }
    }

    fn size_pos_controls(&mut self, ui: &mut egui::Ui, id: &str, action: &mut Option<Action>) {
        let key = (self.page_index, id.to_string());
        if self.size_pos.key.as_ref() != Some(&key) {
            let metric = self.engine.page_size_json(self.page_index).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()).is_some_and(|size| size["metric"].as_bool() == Some(true));
            (self.size_pos.unit, self.size_pos.per_inch) = if metric { ("mm", 25.4) } else { ("in", 1.0) };
            let per_inch = self.size_pos.per_inch;
            self.size_pos.fields = Some(if self.is_connector(id) {
                Err("A connector is edited by its ends.")
            } else {
                self.engine
                    .group_info_json(self.page_index)
                    .map_err(|_| "The shape cannot be read.")
                    .and_then(|frames| size_frame(&frames, id))
                    .map(|frame| (size_texts(frame, per_inch), size_texts(frame, per_inch)))
            });
            self.size_pos.key = Some(key);
        }
        ui.add_space(4.0);
        ui.label("Size and Position");
        let unit = self.size_pos.unit;
        let per_inch = self.size_pos.per_inch;
        match self.size_pos.fields.as_mut() {
            Some(Ok((texts, read))) => {
                egui::Grid::new("size-pos").num_columns(3).show(ui, |ui| {
                    for (field, name) in SIZE_FIELDS.iter().enumerate() {
                        let label = ui.label(*name);
                        let edit = ui.add(egui::TextEdit::singleline(&mut texts[field]).desired_width(80.0)).labelled_by(label.id);
                        if self.size_pos.focus == Some(field) {
                            edit.request_focus();
                            edit.scroll_to_me(None);
                            self.size_pos.focus = None;
                        }
                        ui.label(if field == 4 { "deg" } else { unit });
                        ui.end_row();
                        match field_left(ui, &edit) {
                            // Esc gives the old value back instead of applying what was typed
                            Some(Left::Revert) => texts[field] = read[field].clone(),
                            Some(Left::Apply) if texts[field] != read[field] => match texts[field].trim().parse::<f64>() {
                                Ok(value) => *action = Some(Action::SizeField(id.to_string(), field, value, per_inch)),
                                Err(_) => texts[field] = read[field].clone(),
                            },
                            _ => {}
                        }
                    }
                });
            }
            Some(Err(reason)) => {
                ui.label(egui::RichText::new(*reason).small());
            }
            None => {}
        }
    }

    fn set_size_field(&mut self, ctx: &egui::Context, id: &str, field: usize, value: f64, per_inch: f64) {
        let page = self.page_index;
        let result = apply_size_field(&mut self.engine, page, id, field, value, per_inch);
        // read the fields again, also after a refused value
        self.size_pos.key = None;
        let selected = self.selected.clone();
        self.finish_edit(ctx, result, "Changed size and position", selected);
    }

    /// The rulers beside the canvas, counting from the page's lower left corner (y up).
    fn draw_rulers(&self, ui: &egui::Ui, area: egui::Rect, canvas: egui::Rect, image_rect: egui::Rect) {
        let (per_inch, unit) = self.ruler_unit;
        let metric = per_inch > 1.0;
        let origin = self.scene_origin();
        let scale = f64::from(self.view_scale());
        // screen position of a ruler value, and the ruler value at a screen position
        let page_h = self.page_height_in;
        let x_at = |value: f64| f64::from(image_rect.min.x) + (value / per_inch * DPI - f64::from(origin.x)) * scale;
        let y_at = |value: f64| f64::from(image_rect.min.y) + ((page_h - value / per_inch) * DPI - f64::from(origin.y)) * scale;
        let x_value = |screen: f32| ((f64::from(screen - image_rect.min.x) / scale + f64::from(origin.x)) / DPI) * per_inch;
        let y_value = |screen: f32| (page_h - (f64::from(screen - image_rect.min.y) / scale + f64::from(origin.y)) / DPI) * per_inch;
        let px_per_unit = DPI * scale / per_inch;
        let colours = appearance::chrome(ui.visuals().dark_mode);
        let (band, ink) = (colours.ruler, colours.ruler_ink);
        let font = egui::FontId::proportional(10.0);
        let top = egui::Rect::from_min_max(egui::pos2(canvas.min.x, area.min.y), egui::pos2(canvas.max.x, canvas.min.y));
        let left = egui::Rect::from_min_max(egui::pos2(area.min.x, canvas.min.y), egui::pos2(canvas.min.x, canvas.max.y));
        ui.painter().rect_filled(egui::Rect::from_min_max(area.min, canvas.min), 0.0, band);
        ui.painter().text(area.min + RULER_BANDS / 2.0, egui::Align2::CENTER_CENTER, unit, font.clone(), ink);
        let counts = if metric { "Millimetres" } else { "Inches" };
        let scaled = if unit.ends_with('*') { " on the printed page: the drawing scale is not applied" } else { " from the page's lower left corner" };
        ui.interact(egui::Rect::from_min_max(area.min, canvas.min), egui::Id::unique("ruler-corner"), egui::Sense::hover()).on_hover_text(format!("{counts}{scaled}"));
        let painter = ui.painter_at(top);
        painter.rect_filled(top, 0.0, band);
        for mark in ruler_marks(x_value(top.min.x), x_value(top.max.x), px_per_unit, metric) {
            let x = x_at(mark) as f32;
            painter.vline(x, top.max.y - 6.0..=top.max.y, egui::Stroke::new(1.0, ink));
            painter.text(egui::pos2(x + 2.0, top.min.y + 2.0), egui::Align2::LEFT_TOP, mark.to_string(), font.clone(), ink);
        }
        let painter = ui.painter_at(left);
        painter.rect_filled(left, 0.0, band);
        for mark in ruler_marks(y_value(left.max.y), y_value(left.min.y), px_per_unit, metric) {
            let y = y_at(mark) as f32;
            painter.hline(left.max.x - 6.0..=left.max.x, y, egui::Stroke::new(1.0, ink));
            painter.text(egui::pos2(left.min.x + 2.0, y - 1.0), egui::Align2::LEFT_BOTTOM, mark.to_string(), font.clone(), ink);
        }
    }

    /// The page point (inches, y up) under a screen point.
    fn page_point(&self, pointer: egui::Pos2, image_rect: egui::Rect) -> (f64, f64) {
        let scene = self.screen_to_scene(pointer, image_rect);
        (f64::from(scene.x) / DPI, self.page_height_in - f64::from(scene.y) / DPI)
    }

    /// The guide within a few screen px of the pointer, when guides are shown.
    fn guide_at(&self, pointer: egui::Pos2, image_rect: egui::Rect) -> Option<GuideLine> {
        if !self.show_guides {
            return None;
        }
        let (x, y) = self.page_point(pointer, image_rect);
        guide_near(&self.guides, x, y, 4.0 / (DPI * f64::from(self.view_scale())))
    }

    /// Follow the pointer with the dragged guide (to the grid when snapping to it).
    fn move_guide_drag(&mut self, pointer: egui::Pos2, image_rect: egui::Rect) {
        let (x, y) = self.page_point(pointer, image_rect);
        let (grid, step) = (self.snap_grid, self.grid_step);
        if let Some(drag) = self.guide_drag.as_mut() {
            let at = if drag.vertical { x } else { y };
            drag.at = if grid && step > 0.0 { (at / step).round() * step } else { at };
        }
    }

    /// End a guide drag: over the canvas the guide stays there, anywhere else (a ruler) it goes.
    fn finish_guide_drag(&mut self, ctx: &egui::Context, on_page: bool) {
        let Some(drag) = self.guide_drag.take() else {
            return;
        };
        let page = self.page_index;
        if let Some(result) = drop_guide(&mut self.engine, page, &drag, on_page) {
            self.selected_guide = None;
            let selected = self.selected.clone();
            match result {
                Ok(verb) => self.finish_edit(ctx, Ok(()), verb, selected),
                Err(error) => self.status = format!("Could not change the guide: {error}"),
            }
        }
    }

    fn delete_selected_guide(&mut self, ctx: &egui::Context) {
        let Some(id) = self.selected_guide.take() else {
            return;
        };
        if let Some((id, vertical, at)) = self.guides.iter().find(|guide| guide.0 == id).cloned() {
            self.guide_drag = Some(GuideDrag { id: Some(id), vertical, at });
            self.finish_guide_drag(ctx, false);
        }
    }

    /// The guides as dashed lines across the canvas; the selected one is thicker, the dragged one follows the pointer.
    fn draw_guides(&self, painter: &egui::Painter, canvas: egui::Rect, image_rect: egui::Rect) {
        if !self.show_guides {
            return;
        }
        let dragged = self.guide_drag.as_ref();
        let resting = self.guides.iter().filter(|guide| dragged.and_then(|drag| drag.id.as_ref()) != Some(&guide.0)).map(|guide| (guide.1, guide.2, self.selected_guide.as_ref() == Some(&guide.0)));
        for (vertical, at, selected) in resting.chain(dragged.map(|drag| (drag.vertical, drag.at, true))) {
            let scene = egui::pos2((at * DPI) as f32, ((self.page_height_in - at) * DPI) as f32);
            let screen = self.scene_to_screen(scene, image_rect);
            let ends = if vertical { [egui::pos2(screen.x, canvas.top()), egui::pos2(screen.x, canvas.bottom())] } else { [egui::pos2(canvas.left(), screen.y), egui::pos2(canvas.right(), screen.y)] };
            let stroke = egui::Stroke::new(if selected { 2.5 } else { 1.0 }, egui::Color32::from_rgb(20, 110, 220));
            painter.extend(egui::Shape::dashed_line(&ends, stroke, 7.0, 4.0));
        }
    }

    /// The View tab's Zoom menu: the shown percentage; fixed levels, the whole page or its width.
    fn ribbon_zoom(&self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        let percent = zoom_percent(self.zoom, self.raster_scale);
        ui.add_enabled_ui(self.texture.is_some(), |ui| {
            ui.menu_button(format!("{percent:.0}% \u{25be}"), |ui| {
                for level in ZOOM_PERCENTS {
                    if ui.selectable_label((percent - f64::from(level)).abs() < 0.5, format!("{level}%")).clicked() {
                        *action = Some(Action::ZoomTo(level));
                        ui.close();
                    }
                }
                if ui.button("Custom zoom...").clicked() {
                    *action = Some(Action::ZoomDialog);
                    ui.close();
                }
                let mut wheel = self.wheel_zooms;
                if ui.checkbox(&mut wheel, "Mouse wheel zooms").on_hover_text("The plain wheel zooms at the pointer (Ctrl+wheel always does)").changed() {
                    *action = Some(Action::WheelZooms(wheel));
                    ui.close();
                }
                ui.separator();
                for (name, command) in [("Fit page to window", Action::Fit), ("Page width", Action::FitWidth)] {
                    if ui.button(name).clicked() {
                        *action = Some(command);
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Zoom");
        });
    }

    /// The page tabs under the drawing: click to show a page, right-click for the page commands, drag to reorder.
    fn page_tabs(&self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        let Some(document) = self.engine.document.as_ref() else {
            return;
        };
        let count = document.pages.len();
        egui::ScrollArea::horizontal().id_salt("page-tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut spans = Vec::with_capacity(count);
                let mut dropped = None;
                for (at, page) in document.pages.iter().enumerate() {
                    let tab = ui.add(egui::Button::new(page.name.as_str()).selected(at == self.page_index).sense(egui::Sense::click_and_drag()));
                    spans.push((tab.rect.left(), tab.rect.right()));
                    // the menu's commands work on the shown page, so a right click shows the tab's page first
                    if tab.clicked() || (tab.secondary_clicked() && at != self.page_index) {
                        *action = Some(Action::Page(at));
                    }
                    if tab.drag_stopped_by(egui::PointerButton::Primary) {
                        dropped = ui.ctx().pointer_latest_pos().map(|pointer| (at, pointer));
                    }
                    tab.context_menu(|ui| {
                        for (name, enabled, command) in [
                            ("Add a page", true, Action::PageAdd),
                            ("Duplicate the page", true, Action::PageDuplicate),
                            ("Rename the page", true, Action::PageRename),
                            ("Move left", at > 0, Action::PageMove(-1)),
                            ("Move right", at + 1 < count, Action::PageMove(1)),
                            ("Delete the page", count > 1, Action::PageDelete),
                            ("Make a background page", count > 1, Action::PageToBackground),
                        ] {
                            if ui.add_enabled(enabled, egui::Button::new(name)).clicked() {
                                *action = Some(command);
                                ui.close();
                            }
                        }
                    });
                }
                // let go over the strip (not somewhere else in the window)
                if let Some((from, pointer)) = dropped.filter(|(_, pointer)| ui.max_rect().y_range().contains(pointer.y)) {
                    if let Some(to) = tab_drop(&spans, from, pointer.x) {
                        *action = Some(Action::PageReorder(from, to));
                    }
                }
                if ui.button("+").on_hover_text("Add a page").clicked() {
                    *action = Some(Action::PageAdd);
                }
            });
        });
    }

    /// Tab / Shift+Tab: select the next or previous shape of the page (a group as one shape), in drawing order.
    fn cycle_shape(&mut self, backwards: bool) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let mut ids: Vec<String> = Vec::new();
        for item in &scene.items {
            if let SceneItem::Shape(shape) = item {
                if let Some(id) = shape.visio_id.as_deref() {
                    let root = self.group_root(id);
                    if self.doc_shape(&root).is_some_and(|doc| !doc.layer_hidden) && !ids.contains(&root) {
                        ids.push(root);
                    }
                }
            }
        }
        if let Some(next) = next_in_cycle(&ids, self.selected.as_deref(), backwards) {
            let at = ids.iter().position(|id| *id == next).map_or(0, |at| at + 1);
            self.status = format!("Shape {next} selected ({at} of {}); Esc leaves the shapes.", ids.len());
            self.selected = Some(next);
            self.extra_selected.clear();
        }
    }

    /// The Shape Formulas window: the cells of the one selected shape with their values and formulas. Read only.
    fn draw_sheet_pane(&mut self, ctx: &egui::Context) {
        let Some(mut pane) = self.sheet_pane.take() else {
            return;
        };
        let page = self.page_index;
        let selection = self.all_selected();
        let key = match &selection[..] {
            [one] => Some((page, one.clone(), self.engine.undo_depth())),
            _ => None,
        };
        if pane.key != key {
            pane.groups = key.as_ref().and_then(|(page, shape, _)| self.engine.shape_sheet_json(*page, shape).ok()).map(|json| sheet_groups(&json)).unwrap_or_default();
            pane.key = key;
        }
        let mut open = true;
        egui::Window::new("Shape Formulas").open(&mut open).collapsible(true).default_width(460.0).show(ctx, |ui| {
            let Some((_, shape, _)) = &pane.key else {
                ui.label("Select one shape to see its formulas.");
                return;
            };
            ui.label(format!("Shape {shape} (read only)"));
            if pane.groups.is_empty() {
                ui.label("The cells of this shape cannot be read.");
            }
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                for (at, group) in pane.groups.iter().enumerate() {
                    egui::CollapsingHeader::new(&group.title).id_salt(("sheet", at)).default_open(at == 0).show(ui, |ui| {
                        egui::Grid::new(("sheet-grid", at)).num_columns(3).striped(true).show(ui, |ui| {
                            for heading in ["Cell", "Value", "Formula"] {
                                ui.strong(heading);
                            }
                            ui.end_row();
                            for (cell, value, formula) in &group.rows {
                                ui.label(cell);
                                ui.label(value);
                                ui.label(formula);
                                ui.end_row();
                            }
                        });
                    });
                }
            });
        });
        if open {
            self.sheet_pane = Some(pane);
        }
    }

    /// The window for another colour: type its code or pick it; Enter or Apply gives it to the selection (one undo step),
    /// Esc or Cancel changes nothing.
    fn draw_colour_window(&mut self, ctx: &egui::Context, action: &mut Option<Action>) {
        // what the picker shows before anything is typed
        let current = self.selected.as_deref().and_then(|id| self.doc_shape(id).map(|shape| StyleDraft::from_shape(id, shape)));
        let lettering = self.selected.as_deref().and_then(|id| self.doc_shape(id)).and_then(|shape| shape.text_color.as_deref()).and_then(parse_hex);
        let Some(draft) = self.colour_draft.as_mut() else {
            return;
        };
        let current = match draft.key {
            "textColor" => lettering.unwrap_or([0, 0, 0]),
            key => current.map_or([0, 0, 0], |style| if key == "fill" { style.fill } else { style.line }),
        };
        let (mut open, mut apply, mut cancel) = (true, false, false);
        let title = match draft.key {
            "fill" => "Fill colour",
            "textColor" => "Text colour",
            _ => "Line colour",
        };
        egui::Window::new(title).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            let label = ui.label("Colour code (#rrggbb)");
            let field = ui.add(egui::TextEdit::singleline(&mut draft.text).hint_text(hex(current)).desired_width(100.0)).labelled_by(label.id);
            if std::mem::take(&mut draft.focus) {
                field.request_focus();
            }
            apply = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let [r, g, b] = typed_colour(&draft.text).unwrap_or(current);
            let mut picked = egui::Color32::from_rgb(r, g, b);
            if egui::widgets::color_picker::color_picker_color32(ui, &mut picked, egui::widgets::color_picker::Alpha::Opaque) {
                draft.text = hex([picked.r(), picked.g(), picked.b()]);
            }
            if draft.refused {
                ui.colored_label(appearance::chrome(ui.visuals().dark_mode).warning, "Type a colour as #rrggbb, or pick one.");
            }
            ui.horizontal(|ui| {
                apply |= ui.button("Apply").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if apply {
            match typed_colour(&draft.text) {
                Some(colour) => {
                    *action = Some(Action::StyleSelected(serde_json::json!({ draft.key: hex(colour) }).to_string()));
                    cancel = true;
                }
                None => {
                    draft.refused = true;
                    draft.focus = true;
                }
            }
        }
        if cancel || !open || ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.colour_draft = None;
        }
    }

    /// The zoom window: type a percentage; Enter or Apply shows the page at it, Esc or Cancel leaves the view as it is.
    fn draw_zoom_window(&mut self, ctx: &egui::Context) {
        let now = zoom_percent(self.zoom, self.raster_scale);
        let raster_scale = self.raster_scale;
        let Some(draft) = self.zoom_draft.as_mut() else {
            return;
        };
        let (mut open, mut apply, mut cancel) = (true, false, false);
        egui::Window::new("Zoom").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            let label = ui.label("Percent of actual size");
            let field = ui.add(egui::TextEdit::singleline(&mut draft.text).hint_text(format!("{now:.0}")).desired_width(100.0)).labelled_by(label.id);
            if std::mem::take(&mut draft.focus) {
                field.request_focus();
            }
            apply = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if let Some(refused) = &draft.refused {
                ui.colored_label(appearance::chrome(ui.visuals().dark_mode).warning, refused);
            }
            ui.horizontal(|ui| {
                apply |= ui.button("Apply").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if apply {
            match typed_zoom(&draft.text, raster_scale) {
                Ok(percent) => {
                    self.zoom = zoom_for_percent(percent, raster_scale);
                    self.fit_pending = false;
                    self.fit_width_pending = false;
                    cancel = true;
                }
                Err(refused) => {
                    draft.refused = Some(refused);
                    draft.focus = true;
                }
            }
        }
        if cancel || !open || ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.zoom_draft = None;
        }
    }

    /// The Snap and Attach window: what dragged shapes snap to, and the grid. Changes apply at once; Esc closes.
    fn draw_snap_window(&mut self, ctx: &egui::Context) {
        if !self.snap_open {
            return;
        }
        let mut open = true;
        egui::Window::new("Snap and Attach").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label("Dragged and drawn shapes snap to:");
            ui.checkbox(&mut self.snap_grid, "The grid");
            ui.checkbox(&mut self.snap_shapes, "Edges and centres of other shapes");
            ui.checkbox(&mut self.snap_guides, "Ruler guides");
            ui.add(egui::Slider::new(&mut self.snap_reach_px, SNAP_REACHES).integer().suffix(" px").text("Snap strength"))
                .on_hover_text("How near on screen an edge must come before it snaps");
            ui.separator();
            ui.checkbox(&mut self.show_grid, "Show the grid");
            ui.horizontal(|ui| {
                let label = ui.label("Grid spacing");
                egui::ComboBox::from_id_salt("snap-grid-step")
                    .selected_text(format!("{} in", self.grid_step))
                    .show_ui(ui, |ui| {
                        for step in [0.125, 0.25, 0.5, 1.0] {
                            ui.selectable_value(&mut self.grid_step, step, format!("{step} in"));
                        }
                    })
                    .response
                    .labelled_by(label.id);
            });
            ui.label(egui::RichText::new("Connectors always attach to the shape or connection point they are dropped on.").small());
        });
        self.snap_open = open && !(!self.focus_at_frame_start && ctx.input(|input| input.key_pressed(egui::Key::Escape)));
    }

    /// Send a picture of the shown page to the system clipboard. The toolkit does not report whether the clipboard took it.
    fn copy_page_picture(&mut self, ctx: &egui::Context) {
        let Some(scene) = self.scene.as_ref() else {
            self.status = "Open a drawing first.".to_string();
            return;
        };
        match page_picture(scene) {
            Ok(picture) => {
                let [width, height] = picture.size;
                ctx.copy_image(picture);
                self.status = format!("Sent a {width} x {height} picture of the page to the clipboard.");
            }
            Err(error) => self.status = format!("Could not draw the picture: {error}"),
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
                // making a page a background page can leave the number past the last page that has a tab
                let last = self.engine.document().map_or(0, |document| document.pages.len().saturating_sub(1));
                self.page_index = show.trim().parse().unwrap_or(0).min(last);
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
        // a command that selects something that was not selected replaces the whole selection; one that names a selected shape
        // keeps the others
        match &selected {
            Some(id) if self.all_selected().contains(id) => self.extra_selected.retain(|other| other != id),
            _ => self.extra_selected.clear(),
        }
        self.selected = selected;
        self.drag = None;
        self.preview = None;
        self.build_page(ctx, false);
        self.status = format!("{verb}; save as to keep it.");
    }
}

impl eframe::App for LayoutCraftApp {
    /// While the canvas has the keyboard and a shape is selected, Tab steps through the shapes instead of through the
    /// buttons; Esc clears the selection and gives Tab back.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // read here, before the toolkit takes the focus away for an Esc: such an Esc is for the field alone
        self.focus_at_frame_start = ctx.memory(|memory| memory.focused()).is_some();
        if self.canvas_keys && self.selected.is_some() && ctx.memory(|memory| memory.focused()).is_none() {
            self.tab_cycle = take_shape_tab(&mut raw_input.events).or(self.tab_cycle);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let _frame_timer = Timer::new("frame", 16.0);
        let ctx = ui.ctx().clone();
        self.apply_raster(&ctx);
        self.settle_raster(&ctx);
        self.shift_down = ctx.input(|input| input.modifiers.shift);
        self.add_down = ctx.input(|input| adds_to_selection(input.modifiers));
        let mut action: Option<Action> = None;
        // what an inspector field that was just left asks for: kept apart, so that it does not take the place of the click that
        // left the field (a page tab)
        let mut field_commit: Option<Action> = None;
        if let Some(backwards) = self.tab_cycle.take() {
            self.cycle_shape(backwards);
        }

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
            if self.tool == Tool::ConnectionPoint {
                // Delete is the tool's: it never removes shapes while it is held
                if ctx.input(|input| input.key_pressed(egui::Key::Delete) || input.key_pressed(egui::Key::Backspace)) {
                    self.delete_connection_point(&ctx);
                }
            } else if self.selected.is_some()
                && ctx.input(|input| {
                    input.key_pressed(egui::Key::Delete) || input.key_pressed(egui::Key::Backspace)
                })
            {
                action = Some(Action::DeleteSelected);
            } else if self.selected.is_none()
                && self.selected_guide.is_some()
                && ctx.input(|input| input.key_pressed(egui::Key::Delete) || input.key_pressed(egui::Key::Backspace))
            {
                action = Some(Action::DeleteGuide);
            }
            // text and arrange keys (`consume_edit_key`), then select all, zoom, F2 and the arrow nudge
            if let Some(command) = ctx.input_mut(consume_edit_key) {
                action = Some(match command {
                    EditKey::Arrange(command) => Action::Arrange(command),
                    EditKey::Size(delta) => {
                        let px = self.selected.as_deref().and_then(|id| self.doc_shape(id)).and_then(|shape| shape.font_size);
                        Action::StyleSelected(serde_json::json!({ "fontPt": stepped_font_size(px, f64::from(delta)) }).to_string())
                    }
                    EditKey::Align(cell) => Action::FormatText(serde_json::json!({ "para": { "HorzAlign": cell } })),
                    EditKey::Style(name) => {
                        // switched off only when the first selected shape has it
                        let shape = self.selected.as_deref().and_then(|id| self.doc_shape(id));
                        let on = shape.is_some_and(|shape| match name {
                            "bold" => shape.bold == Some(true),
                            "italic" => shape.italic == Some(true),
                            _ => shape.underline == Some(true),
                        });
                        Action::StyleSelected(serde_json::json!({ name: !on }).to_string())
                    }
                });
            } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::A)) {
                self.select_all();
            } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Equals) || input.consume_key(egui::Modifiers::CTRL, egui::Key::Plus)) {
                action = Some(Action::ZoomIn);
            } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Minus)) {
                action = Some(Action::ZoomOut);
            }
            // typing on one selected shape of the canvas opens its text editor, with what was typed added to the text
            if self.canvas_keys && !self.focus_at_frame_start && self.text_editor.is_none() && self.extra_selected.is_empty() && self.selected.is_some() && self.tool == Tool::Select {
                if let Some(typed) = ctx.input(|input| typed_text(&input.events)) {
                    self.open_text_editor();
                    if let Some(editor) = self.text_editor.as_mut() {
                        editor.text.push_str(&typed);
                    }
                }
            }
            match ctx.input_mut(consume_view_key) {
                Some(ViewKey::Tool(tool)) => self.take_tool(tool),
                Some(ViewKey::AlignButtons) => {
                    if !self.full_screen && self.selected.is_some() && !self.extra_selected.is_empty() {
                        self.align_focus = true;
                    } else {
                        self.status = "Select two or more shapes to align them.".to_string();
                    }
                }
                Some(ViewKey::StyleSettings) => self.go_to_style_settings(),
                Some(ViewKey::FitPage) => action = Some(Action::Fit),
                Some(ViewKey::NextPage) => action = Some(Action::Page(self.page_index + 1)),
                Some(ViewKey::PreviousPage) => action = Some(Action::Page(self.page_index.saturating_sub(1))),
                Some(ViewKey::Find) => action = Some(Action::Find),
                Some(ViewKey::Hyperlink) => action = Some(Action::Hyperlink),
                Some(ViewKey::FullScreen) => action = Some(Action::FullScreen),
                Some(ViewKey::SnapDialog) => self.snap_open = !self.snap_open,
                Some(ViewKey::Help) => action = Some(Action::HelpShortcuts),
                Some(ViewKey::ZoomIn) => action = Some(Action::ZoomIn),
                Some(ViewKey::ZoomOut) => action = Some(Action::ZoomOut),
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
            if self.full_screen {
                match ctx.input_mut(consume_full_screen_page) {
                    // `go_to_page` keeps the number inside the drawing
                    Some(1) => action = Some(Action::Page(self.page_index + 1)),
                    Some(_) => action = Some(Action::Page(self.page_index.saturating_sub(1))),
                    None => {}
                }
            }
            if self.full_screen && !self.focus_at_frame_start && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                action = Some(Action::FullScreen);
            }
            if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.text_editor = None;
                // an Esc that left a text field (or a focused button) does not also drop the selection
                if !self.focus_at_frame_start {
                    if self.tool == Tool::ConnectionPoint {
                        self.take_tool(Tool::Select);
                    }
                    self.selected_guide = None;
                    self.guide_drag = None;
                    self.selected = None;
                }
            }
        }

        // the full-screen view shows the drawing and the status bar only
        self.theme_hover = None;
        if !self.full_screen {
            egui::Panel::top("ribbon").show(ui, |ui| {
                self.draw_ribbon(ui, &mut action);
            });
        }
        self.settle_theme_preview(&ctx);

        if self.pending_open.is_some() {
            egui::Panel::top("pending").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(appearance::chrome(ui.visuals().dark_mode).warning, "Unsaved changes.");
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
                // the selected shape's size; a click goes to the fields of the inspector
                if let Some(geometry) = status_geometry(&self.size_pos, self.page_index, self.selected.as_deref()).filter(|_| !self.full_screen) {
                    ui.separator();
                    if ui.button(geometry).on_hover_text("Edit in Size and Position").clicked() {
                        self.size_pos.focus = Some(2);
                    }
                }
                ui.separator();
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(self.texture.is_some(), egui::Button::new("Fit")).on_hover_text("Fit the page to the window (Ctrl+Shift+W)").clicked() {
                        action = Some(Action::Fit);
                    }
                    let full_screen = if self.full_screen { "Leave Full Screen" } else { "Full Screen" };
                    if ui.add_enabled(self.texture.is_some() || self.full_screen, egui::Button::new(full_screen)).on_hover_text("F5").clicked() {
                        action = Some(Action::FullScreen);
                    }
                    let percent = zoom_percent(self.zoom, self.raster_scale);
                    ui.label(format!("zoom {percent:.0}%"));
                    ui.add_enabled_ui(self.texture.is_some(), |ui| {
                        if let Some(level) = zoom_slider(ui, percent) {
                            action = Some(Action::ZoomTo(level));
                        }
                    });
                    if self.dirty {
                        ui.colored_label(appearance::chrome(ui.visuals().dark_mode).warning, "modified");
                    }
                });
            });
            ui.add_space(2.0);
        });

        if !self.full_screen && self.texture.is_some() {
            egui::Panel::bottom("page-tabs").show(ui, |ui| {
                self.page_tabs(ui, &mut action);
            });
        }

        if !self.full_screen {
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
                            ui.colored_label(appearance::chrome(ui.visuals().dark_mode).error, format!("\u{26a0} Font {} is not installed: drawn with {}", found.requested, found.used))
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
                            ui.label(format!("Parts: {}", shape.parts.len()));
                            // the shape's hyperlinks (edited in the Hyperlink window, Ctrl+K)
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
                            if shape.group.is_some() {
                                ui.colored_label(
                                    appearance::chrome(ui.visuals().dark_mode).warning,
                                    "Grouped: read-only in this slice",
                                );
                            } else if shape.rot.abs() >= 1e-6 {
                                ui.label("Rotated: move only, resize disabled");
                            }
                        }
                        self.shape_name_controls(ui, &selected, &mut field_commit);
                        self.size_pos_controls(ui, &selected, &mut field_commit);
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
                                    let first = ui.checkbox(&mut draft.has_fill, "Fill");
                                    if std::mem::take(&mut self.style_focus) {
                                        first.request_focus();
                                        first.scroll_to_me(None);
                                    }
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
                            for (label, mode, name) in [("L", 0, "Align left edges"), ("C", 1, "Align centres, left to right"), ("R", 2, "Align right edges"), ("T", 3, "Align top edges"), ("M", 4, "Align middles, top to bottom"), ("B", 5, "Align bottom edges")] {
                                let button = ui.small_button(label).on_hover_text(name);
                                ui.ctx().accesskit_node_builder(button.id, |node| node.set_label(name));
                                if mode == 0 && std::mem::take(&mut self.align_focus) {
                                    button.request_focus();
                                    button.scroll_to_me(None);
                                }
                                if button.clicked() {
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
                    self.layer_controls(ui, &mut action);
                    ui.separator();
                    ui.label(
                        egui::RichText::new(
                            "Drag a shape to move. Drag a corner handle to resize.\n\
                             Ctrl+Z undo, Ctrl+Shift+Z redo, Ctrl+S save a copy.\n\
                             Wheel scrolls, Ctrl+wheel zooms; drag empty canvas to pan.",
                        )
                        .small()
                        .weak(),
                    );
                });
        }

        if self.layers_open {
            let mut open = true;
            egui::Window::new("Layers").open(&mut open).collapsible(true).default_width(300.0).show(&ctx, |ui| {
                self.layer_controls(ui, &mut action);
            });
            // Esc closes it, unless a text field has the keyboard (then Esc only leaves the field)
            self.layers_open = open && !(!self.focus_at_frame_start && ctx.input(|input| input.key_pressed(egui::Key::Escape)));
        }

        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) && self.armed_master.is_some() {
            self.armed_master = None;
            self.status = "Cancelled placing a shape.".to_string();
        }
        if self.change_shape && (self.selected.is_none() || !self.shapes_open || self.shapes_minimised || ctx.input(|input| input.key_pressed(egui::Key::Escape))) {
            self.change_shape = false;
            self.status = "Change Shape cancelled.".to_string();
        }
        if self.shapes_open && !self.stencil_autoopen_done && self.texture.is_some() {
            self.stencil_autoopen_done = true;
            if self.stencils.is_empty() {
                self.open_builtin_stencil();
                self.restore_stencils();
            }
        }
        // minimised, the panel is a narrow column with the button that brings it back
        if self.shapes_open && self.shapes_minimised && !self.full_screen {
            egui::Panel::left("shapes-minimised").resizable(false).default_size(80.0).show(ui, |ui| {
                if ui.button("Shapes \u{bb}").on_hover_text("Show the Shapes panel").clicked() {
                    self.shapes_minimised = false;
                }
            });
        }
        if self.shapes_open && !self.shapes_minimised && !self.full_screen {
            let mut open_stencil = false;
            let mut builtin = false;
            let mut close = None;
            let mut replace: Option<(usize, String, String)> = None;
            let can_replace = self.selected.is_some();
            egui::Panel::left("shapes").default_size(250.0).resizable(true).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Shapes");
                    if ui.small_button("\u{ab} Minimise").on_hover_text("Narrow the Shapes panel to a column").clicked() {
                        self.shapes_minimised = true;
                    }
                    if ui.small_button("Open stencil…").clicked() {
                        open_stencil = true;
                    }
                    if ui.small_button("Basic shapes").clicked() {
                        builtin = true;
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut self.shape_search).hint_text("Search shapes").desired_width(f32::INFINITY));
                if self.change_shape {
                    ui.add(egui::Label::new(egui::RichText::new("Change Shape: click the new shape (Esc cancels).").small()).wrap());
                } else if let Some((_, _, name)) = &self.armed_master {
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
                        // one stencil is open at a time; a search shows the matches of all of them
                        let searching = !self.shape_search.trim().is_empty();
                        let header = egui::CollapsingHeader::new(format!("{name} ({})", masters.len())).id_salt(("stencil", index)).open(Some(searching || self.open_stencil_shown == Some(index)));
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
                                        ui.painter().text(egui::pos2(rect.center().x, thumb.bottom() + 3.0), egui::Align2::CENTER_TOP, short, egui::FontId::proportional(10.5), appearance::chrome(ui.visuals().dark_mode).caption);
                                        let armed = self.armed_master.as_ref().is_some_and(|(i, m, _)| *i == index && m == id);
                                        if armed || response.hovered() {
                                            ui.painter().rect_stroke(thumb.expand(2.0), 4.0, egui::Stroke::new(2.0, if armed { appearance::chrome(ui.visuals().dark_mode).warning } else { egui::Color32::from_rgb(60, 125, 200) }), egui::StrokeKind::Outside);
                                        }
                                    }
                                    let response = response.on_hover_text(mname.as_str());
                                    if response.clicked() && self.change_shape {
                                        replace = Some((index, id.clone(), mname.clone()));
                                    } else if response.clicked() {
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
                        if shown.header_response.clicked() {
                            self.open_stencil_shown = stencil_clicked(self.open_stencil_shown, index);
                        }
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
                self.change_shape = false;
                self.change_selected_shapes(&ctx, index as i32, &master, &name);
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
                // the rulers take a band above and to the left of the canvas, so a click on them does not reach the page
                let area = ui.available_rect_before_wrap();
                let rulers = self.show_rulers && !self.full_screen && self.texture.is_some();
                let canvas = egui::Rect::from_min_max(area.min + if rulers { RULER_BANDS } else { egui::Vec2::ZERO }, area.max);
                let available = canvas.size();
                if self.fit_pending {
                    self.fit_to_view(available);
                }
                if self.fit_width_pending && self.texture.is_some() && self.texture_size.x > 0.0 && available.x > 1.0 {
                    (self.zoom, self.pan) = width_view(self.texture_size, available);
                    self.fit_pending = false;
                    self.fit_width_pending = false;
                }
                let response = ui.allocate_rect(canvas, egui::Sense::click_and_drag());
                let viewport = response.rect;
                if ctx.input(|input| input.pointer.any_pressed()) {
                    self.canvas_keys = response.contains_pointer();
                }
                let painter = ui.painter_at(viewport);
                let colours = appearance::chrome(ui.visuals().dark_mode);
                painter.rect_filled(viewport, 0.0, colours.canvas);
                // a theme that is only being looked at takes the page picture's place
                let shown = self.theme_preview.as_ref().map(|preview| &preview.texture).or(self.texture.as_ref());
                let Some(texture_id) = shown.map(|texture| texture.id()) else {
                    painter.text(
                        viewport.center(),
                        egui::Align2::CENTER_CENTER,
                        "Open a VSDX file to begin.\nYou can also drag and drop one here.",
                        egui::FontId::proportional(18.0),
                        colours.canvas_text,
                    );
                    return;
                };

                if response.dragged_by(egui::PointerButton::Middle) {
                    self.pan += response.drag_delta();
                }
                if response.hovered() {
                    // Ctrl+wheel (or a pinch) zooms at the pointer; the wheel alone scrolls, sideways with Shift
                    let (factor, scroll) = ui.input(|input| (input.zoom_delta(), input.smooth_scroll_delta));
                    let (factor, scroll) = wheel_zooming(self.wheel_zooms, factor, scroll);
                    if let Some(pointer) = response.hover_pos().filter(|_| (factor - 1.0).abs() > 1e-4 || scroll != egui::Vec2::ZERO) {
                        (self.zoom, self.pan) = wheel_view(self.zoom, self.pan, pointer - viewport.center(), factor, scroll);
                        self.fit_pending = false;
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
                if let Some(detail) = self.detail.as_ref().filter(|_| self.theme_preview.is_none()) {
                    let view = self.view_scale();
                    let r = detail.region;
                    let rect = egui::Rect::from_min_size(
                        image_rect.min + egui::vec2(r[0] as f32, r[1] as f32) * view,
                        egui::vec2(r[2] as f32, r[3] as f32) * view,
                    );
                    painter.image(detail.texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                }
                if rulers {
                    self.draw_rulers(ui, area, viewport, image_rect);
                    // a guide pulled out of a ruler: the top one gives a horizontal guide, the left one a vertical guide
                    let top = egui::Rect::from_min_max(egui::pos2(viewport.min.x, area.min.y), egui::pos2(viewport.max.x, viewport.min.y));
                    let left = egui::Rect::from_min_max(egui::pos2(area.min.x, viewport.min.y), egui::pos2(viewport.min.x, viewport.max.y));
                    for (band, vertical, name) in [(top, false, "ruler-top"), (left, true, "ruler-left")] {
                        let ruler = ui.interact(band, egui::Id::unique(name), egui::Sense::drag()).on_hover_cursor(if vertical { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
                        if ruler.drag_started_by(egui::PointerButton::Primary) {
                            self.show_guides = true;
                            self.guide_drag = Some(GuideDrag { id: None, vertical, at: 0.0 });
                        }
                        if let Some(pointer) = ruler.interact_pointer_pos().filter(|_| ruler.dragged()) {
                            self.move_guide_drag(pointer, image_rect);
                        }
                        if ruler.drag_stopped() {
                            let on_page = ctx.pointer_latest_pos().is_some_and(|pointer| viewport.contains(pointer));
                            self.finish_guide_drag(&ctx, on_page);
                        }
                    }
                }
                self.draw_guides(&painter, viewport, image_rect);

                // The magnifier: with Ctrl+Shift a click zooms in, a right click zooms out, a dragged box fills the view and a
                // right drag moves the page. The pointer then does nothing else.
                // a drag that was already going on stays what it is when the keys come down during it
                let primary = egui::PointerButton::Primary;
                let ordinary_drag = self.drag.is_some()
                    || self.guide_drag.is_some()
                    || (self.magnify_from.is_none() && (response.drag_stopped_by(primary) || (response.dragged_by(primary) && !response.drag_started_by(primary))));
                let held = ctx.input(|input| input.modifiers.command && input.modifiers.shift);
                let magnifier = self.magnify_from.is_some() || (held && !ordinary_drag);
                if magnifier {
                    let from_middle = |at: egui::Pos2| (at - viewport.center()).to_pos2();
                    let factor = if response.clicked() { Some(MAGNIFY_STEP) } else if response.secondary_clicked() { Some(MAGNIFY_STEP.recip()) } else { None };
                    if let Some((factor, at)) = factor.zip(response.interact_pointer_pos()) {
                        (self.zoom, self.pan) = wheel_view(self.zoom, self.pan, at - viewport.center(), factor, egui::Vec2::ZERO);
                        self.fit_pending = false;
                    }
                    if response.dragged_by(egui::PointerButton::Secondary) {
                        self.pan += response.drag_delta();
                        self.fit_pending = false;
                    }
                    if response.drag_started_by(egui::PointerButton::Primary) {
                        self.magnify_from = ctx.input(|input| input.pointer.press_origin());
                    }
                    if let Some((from, to)) = self.magnify_from.zip(ctx.input(|input| input.pointer.latest_pos())) {
                        let area = egui::Rect::from_two_pos(from, viewport.clamp(to));
                        if response.drag_stopped_by(egui::PointerButton::Primary) {
                            // a box too small to have been meant is ignored
                            if area.width() > 8.0 && area.height() > 8.0 {
                                let area = egui::Rect::from_min_max(from_middle(area.min), from_middle(area.max));
                                (self.zoom, self.pan) = magnified_view(self.zoom, self.pan, area, viewport.size());
                                self.fit_pending = false;
                            }
                            self.magnify_from = None;
                        } else {
                            painter.rect_stroke(area, 0.0, egui::Stroke::new(1.5, egui::Color32::from_rgb(20, 110, 220)), egui::StrokeKind::Middle);
                        }
                    }
                    if !ctx.input(|input| input.pointer.primary_down()) {
                        self.magnify_from = None;
                    }
                }

                if !magnifier && response.drag_started_by(egui::PointerButton::Primary) {
                    self.panning = false;
                    let press = ctx
                        .input(|input| input.pointer.press_origin())
                        .or_else(|| response.interact_pointer_pos());
                    if let Some(pointer) = press {
                        // a guide is taken when no shape, connector or handle of the selection is under the pointer
                        let scene = self.screen_to_scene(pointer, image_rect);
                        let free = self.tool == Tool::Select && self.armed_master.is_none() && self.selected.is_none() && self.pick_connector(scene).is_none() && self.pick_shape(scene).is_none();
                        match self.guide_at(pointer, image_rect).filter(|_| free) {
                            Some((id, vertical, at)) => {
                                self.selected_guide = Some(id.clone());
                                self.guide_drag = Some(GuideDrag { id: Some(id), vertical, at });
                            }
                            None => self.begin_pointer(pointer, image_rect),
                        }
                    }
                }
                if !magnifier && response.dragged_by(egui::PointerButton::Primary) {
                    if self.guide_drag.is_some() {
                        if let Some(pointer) = response.interact_pointer_pos() {
                            self.move_guide_drag(pointer, image_rect);
                        }
                    } else if self.drag.is_some() {
                        if let Some(pointer) = response.interact_pointer_pos() {
                            self.update_drag(pointer, image_rect);
                        }
                    } else if self.panning {
                        self.pan += response.drag_delta();
                    }
                }
                if !magnifier && response.drag_stopped_by(egui::PointerButton::Primary) {
                    if self.guide_drag.is_some() {
                        let on_page = ctx.pointer_latest_pos().is_some_and(|pointer| viewport.contains(pointer));
                        self.finish_guide_drag(&ctx, on_page);
                    } else {
                        self.finish_drag(&ctx);
                    }
                }
                // the right-click menus (web/contextmenu.js): a shape or the selection, or the empty page. A right click on a shape that is not
                // selected selects it first; on empty canvas it clears the selection.
                if !magnifier && response.secondary_clicked() {
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
                if !magnifier {
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
                            ui.separator();
                            pick(ui, "Style settings", true, Action::StyleSettings);
                            // opens the window; it stays as it is when it is open already
                            if ui.button("Shape Fields").clicked() {
                                if self.data_pane.is_none() {
                                    action = Some(Action::ShapeData);
                                }
                                ui.close();
                            }
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
                            if ui.button("Snap and Attach...").clicked() {
                                self.snap_open = true;
                                ui.close();
                            }
                        }
                    });
                }
                let mut dropped_now = false;
                if let Some(label) = &self.pane_drag {
                    if let Some(at) = ctx.pointer_latest_pos() {
                        let ghost = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("pane-ghost")));
                        ghost.text(at + egui::vec2(12.0, 12.0), egui::Align2::LEFT_TOP, format!("+ {label}"), egui::FontId::proportional(13.0), appearance::chrome(ctx.global_style().visuals.dark_mode).warning);
                    }
                }
                if let Some((index, master, name, at_screen)) = self.pending_drop.take() {
                    // let go on a connect arrow: the shape goes to that side and is connected
                    if let Some((id, direction)) = self.connect_arrow_at(at_screen, image_rect) {
                        self.place_neighbour(&ctx, &id, direction, Some((index, &master)));
                        self.armed_master = None;
                        dropped_now = true;
                    } else if image_rect.contains(at_screen) {
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
                // an arrow beside a shape takes the click before the picked master is dropped on the page
                if !magnifier && response.clicked() && self.armed_master.is_some() && response.interact_pointer_pos().is_none_or(|pointer| self.connect_arrow_at(pointer, image_rect).is_none()) {
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
                if !magnifier && response.clicked() && !self.panning && !dropped_now {
                    if let Some(pointer) = response.interact_pointer_pos() {
                        dropped_now = self.click_connect_arrow(&ctx, pointer, image_rect);
                    }
                }
                // a magnifier click selects nothing
                dropped_now |= magnifier;
                if self.tool == Tool::ConnectionPoint {
                    if !dropped_now && !self.space_down && response.clicked() {
                        if let Some(pointer) = response.interact_pointer_pos() {
                            let at = self.screen_to_scene(pointer, image_rect);
                            self.click_connection_point(&ctx, at);
                        }
                    }
                    dropped_now = true;
                }
                if response.clicked() && self.drag.is_none() && !self.panning && self.add_down && !dropped_now {
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
                        let guide = self.guide_at(pointer, image_rect).filter(|_| self.selected.is_none());
                        self.selected_guide = guide.map(|guide| guide.0);
                    }
                }
                if !magnifier && self.tool != Tool::ConnectionPoint && response.double_clicked() && self.drag.is_none() {
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
                // equal spacing: each of the two gaps gets a line with a bar at both ends
                for [from, to] in self.spacing_marks(image_rect) {
                    let stroke = egui::Stroke::new(1.5, SPACING_MARK);
                    let bar = (to - from).normalized().rot90() * 5.0;
                    painter.line_segment([from, to], stroke);
                    painter.line_segment([from - bar, from + bar], stroke);
                    painter.line_segment([to - bar, to + bar], stroke);
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
                            && self.selection_rect(&selected) == Some(Self::shape_scene_rect(shape));
                        let rect = self
                            .preview
                            .unwrap_or_else(|| self.selection_rect(&selected).unwrap_or_else(|| Self::selection_scene_rect(shape)));
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
                            hover_handle = response.hover_pos().and_then(|pointer| Self::handle_at(screen_rect, pointer));
                            for (_, hit) in Self::handle_rects(screen_rect) {
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
                self.show_connect_arrows(&response, &painter, image_rect);
                self.show_connection_points(&response, &painter, image_rect);
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
                if magnifier && response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ZoomIn);
                } else if self.drag.is_some() {
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
                    let field = ui.add(
                        egui::TextEdit::multiline(&mut editor.text)
                            .desired_width(360.0)
                            .desired_rows(4),
                    );
                    if std::mem::take(&mut editor.focus) {
                        field.request_focus();
                    }
                    ui.horizontal(|ui| {
                        ui.toggle_value(&mut editor.bold, egui::RichText::new("B").strong());
                        ui.toggle_value(&mut editor.italic, egui::RichText::new("I").italics());
                        ui.toggle_value(&mut editor.underline, egui::RichText::new("U").underline());
                        ui.add(egui::DragValue::new(&mut editor.font_pt).range(1.0..=400.0).speed(0.5).suffix(" pt"));
                    });
                    ui.horizontal(|ui| {
                        // F2 goes back from text editing to the selected shape, keeping the text
                        let apply = ui.button("Apply").clicked()
                            || ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F2))
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
                        egui::RichText::new("Ctrl+Enter or F2 applies, Esc discards.")
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
            // another page, or another size (an undo, another drawing): the fields show it; typed values stay until then
            if let Some(size) = page_dimensions(&self.engine, self.page_index) {
                if self.page_setup_read != Some((self.page_index, size)) {
                    self.page_setup_size = size;
                    self.page_setup_read = Some((self.page_index, size));
                }
            }
            let backgrounds = self.engine.background_info_json().ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()).unwrap_or_default();
            // the chosen background page's ID, empty for none
            let assigned = backgrounds["assigned"][self.page_index].as_str().unwrap_or("").to_string();
            let mut background = assigned.clone();
            // the background page that is to be an ordinary page again
            let mut ordinary: Option<String> = None;
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
                    let label = ui.label("Background page");
                    let pages: Vec<(&str, &str)> = backgrounds["backgrounds"].as_array().map_or_else(Vec::new, |list| list.iter().filter_map(|page| Some((page["id"].as_str()?, page["name"].as_str()?))).collect());
                    if pages.is_empty() {
                        ui.label("None in this drawing");
                    } else {
                        let shown = pages.iter().find(|page| page.0 == background).map_or("None", |page| page.1);
                        egui::ComboBox::from_id_salt("page-background")
                            .selected_text(shown)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut background, String::new(), "None");
                                for (id, name) in &pages {
                                    ui.selectable_value(&mut background, id.to_string(), *name);
                                }
                            })
                            .response
                            .labelled_by(label.id)
                            .on_hover_text("Shown beneath this page; choosing applies at once");
                    }
                    ui.end_row();
                });
                for page in backgrounds["backgrounds"].as_array().into_iter().flatten() {
                    if let (Some(id), Some(name)) = (page["id"].as_str(), page["name"].as_str()) {
                        if ui.button(format!("Make {name} an ordinary page")).on_hover_text("It gets a page tab again; pages that showed it no longer do").clicked() {
                            ordinary = Some(id.to_string());
                        }
                    }
                }
                if ui.button("Apply").clicked() {
                    apply = true;
                }
            });
            if let Some(id) = ordinary {
                self.run_page_op(&ctx, 14, 0, 0, &id, "Made an ordinary page again");
            }
            if background != assigned {
                let result = self.engine.page_op(12, self.page_index, 0, &background).map(|_| ());
                let selected = self.selected.clone();
                self.finish_edit(&ctx, result, if background.is_empty() { "The page has no background page now" } else { "Background page set" }, selected);
            }
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
            let id_of = |comment: &serde_json::Value| comment["id"].as_str().map_or_else(|| comment["id"].to_string(), str::to_string);
            // an undo can take away the comment being changed
            if self.comment_edit.as_ref().is_some_and(|(id, _)| list.iter().all(|comment| &id_of(comment) != id)) {
                self.comment_edit = None;
            }
            let mut change: Option<(String, String)> = None;
            let mut delete: Option<String> = None;
            let (mut save, mut cancel) = (false, false);
            egui::Window::new("Comments").open(&mut open).collapsible(false).default_width(420.0).show(&ctx, |ui| {
                if list.is_empty() {
                    ui.label("No comments yet.");
                }
                for comment in &list {
                    let page = comment["page"].as_i64().map_or("-".to_string(), |page| (page + 1).to_string());
                    let shape = comment["shape"].as_str().map_or("the page".to_string(), |shape| format!("shape {shape}"));
                    let author = comment["author"].as_str().unwrap_or("");
                    ui.label(egui::RichText::new(format!("{author} on {shape} (page {page}), {}", comment["date"].as_str().unwrap_or(""))).small().weak());
                    ui.add(egui::Label::new(comment["text"].as_str().unwrap_or("")).wrap());
                    ui.horizontal(|ui| {
                        if ui.small_button("Edit comment").on_hover_text(format!("Change the text of this comment by {author}")).clicked() {
                            change = Some((id_of(comment), comment["text"].as_str().unwrap_or("").to_string()));
                        }
                        if ui.small_button("Delete comment").on_hover_text(format!("Delete this comment by {author} (Undo brings it back)")).clicked() {
                            delete = Some(id_of(comment));
                        }
                    });
                    ui.separator();
                }
                if let Some((_, changed)) = self.comment_edit.as_mut() {
                    let label = ui.label("Change the comment:");
                    ui.add(egui::TextEdit::multiline(changed).desired_rows(3).desired_width(400.0)).labelled_by(label.id);
                    ui.horizontal(|ui| {
                        save = ui.add_enabled(!changed.trim().is_empty(), egui::Button::new("Save comment")).clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                } else {
                    let label = ui.label(match &self.selected {
                        Some(id) => format!("New comment on shape {id}:"),
                        None => "New comment on the page:".to_string(),
                    });
                    ui.add(egui::TextEdit::multiline(&mut self.comment_text).desired_rows(3).desired_width(400.0)).labelled_by(label.id);
                    if ui.add_enabled(!self.comment_text.trim().is_empty(), egui::Button::new("Add comment")).clicked() {
                        add = true;
                    }
                }
            });
            let page = self.page_index;
            let selected = self.selected.clone();
            if add {
                let shape = self.selected.clone().unwrap_or_default();
                // the text comes last, so it may contain the separator
                let text = format!("{shape}|Me|ME|{}|{}", iso_now(), self.comment_text.trim());
                let result = self.engine.page_op(20, page, 0, &text).map(|_| ());
                if result.is_ok() {
                    self.comment_text.clear();
                }
                self.finish_edit(&ctx, result, "Comment added", selected);
            } else if let Some(id) = delete {
                let result = self.engine.page_op(22, page, 0, &id).map(|_| ());
                self.finish_edit(&ctx, result, "Comment deleted", selected);
            } else if save {
                if let Some((id, changed)) = self.comment_edit.clone() {
                    let before = list.iter().find(|comment| id_of(comment) == id).and_then(|comment| comment["text"].as_str());
                    // saving the same text writes nothing
                    if before == Some(changed.trim()) {
                        self.comment_edit = None;
                    } else {
                        let result = self.engine.page_op(21, page, 0, &format!("{id}|{}|{}", iso_now(), changed.trim())).map(|_| ());
                        if result.is_ok() {
                            self.comment_edit = None;
                        }
                        self.finish_edit(&ctx, result, "Comment changed", selected);
                    }
                }
            }
            if cancel {
                self.comment_edit = None;
            }
            if change.is_some() {
                self.comment_edit = change;
            }
            // Esc closes it, unless a text field has the keyboard (then Esc only leaves the field)
            self.comments_open = open && !(!self.focus_at_frame_start && ctx.input(|input| input.key_pressed(egui::Key::Escape)));
        }
        self.draw_find(&ctx);
        self.draw_links(&ctx);
        self.draw_snap_window(&ctx);
        self.draw_styles_window(&ctx);
        self.draw_zoom_window(&ctx);
        self.draw_colour_window(&ctx, &mut action);
        self.draw_sheet_pane(&ctx);
        self.draw_data_pane(&ctx);
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

        // the field first, while its page is still the shown one
        match field_commit {
            Some(Action::ShapeName(ref id, ref wanted)) => self.set_shape_name(&ctx, id, wanted),
            Some(Action::SizeField(ref id, field, value, per_inch)) => self.set_size_field(&ctx, id, field, value, per_inch),
            _ => {}
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
            Some(Action::Hyperlink) => self.open_links(),
            Some(Action::Layers) => self.layers_open = !self.layers_open,
            Some(Action::CopyPicture) => self.copy_page_picture(&ctx),
            // as a dropped file: unsaved changes are asked about first
            Some(Action::OpenRecent(ref path)) => {
                self.path_input = path.display().to_string();
                if self.dirty {
                    self.pending_open = Some(PendingOpen::Load(self.path_input.clone()));
                } else {
                    self.load(&ctx, path);
                }
            }
            Some(Action::ShapeFormulas) => self.sheet_pane = if self.sheet_pane.is_some() { None } else { Some(SheetPane::default()) },
            // the fields are those the inspector drew for the selection; without them there is nothing to go to
            Some(Action::SizePosition) => match (&self.selected, &self.size_pos.fields) {
                (Some(_), Some(Ok(_))) => self.size_pos.focus = Some(0),
                (Some(_), Some(Err(reason))) => self.status = reason.to_string(),
                _ => self.status = "Select a shape to see its size and position.".to_string(),
            },
            Some(Action::DeleteGuide) => self.delete_selected_guide(&ctx),
            Some(Action::SizeField(ref id, field, value, per_inch)) => self.set_size_field(&ctx, id, field, value, per_inch),
            Some(Action::FullScreen) => self.toggle_full_screen(&ctx),
            Some(Action::ShapeData) => self.data_pane = if self.data_pane.is_some() { None } else { Some(DataPane::default()) },
            Some(Action::Theme(op, ref value)) => self.apply_theme(&ctx, op, value),
            Some(Action::PageShape(w, h)) => self.set_page_shape(&ctx, w, h),
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
            Some(Action::StyleShapes(ref ids, ref json)) => self.style_shapes(&ctx, ids, json),
            Some(Action::ReverseEnds) => self.reverse_selected_ends(&ctx),
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
            Some(Action::PageReorder(from, to)) => self.run_page_op(&ctx, 4, from, to, "", "Moved page"),
            Some(Action::Reload) => {
                if self.dirty {
                    self.pending_open = Some(PendingOpen::Reload);
                } else {
                    self.reload(&ctx);
                }
            }
            Some(Action::Page(page)) => self.go_to_page(&ctx, page),
            Some(Action::Fit) => self.fit_pending = true,
            Some(Action::FitWidth) => self.fit_width_pending = true,
            Some(Action::ActualSize) => {
                self.zoom = zoom_for_percent(100.0, self.raster_scale);
                self.pan = egui::Vec2::ZERO;
                self.fit_pending = false;
            }
            Some(Action::Say(ref message)) => self.status = message.clone(),
            Some(Action::ColourDialog(key)) => self.colour_draft = Some(ColourDraft { key, text: String::new(), refused: false, focus: true }),
            Some(Action::WheelZooms(on)) => {
                self.wheel_zooms = on;
                self.status = if on { "The mouse wheel zooms." } else { "The mouse wheel scrolls; Ctrl+wheel zooms." }.to_string();
            }
            Some(Action::ShapeName(ref id, ref wanted)) => self.set_shape_name(&ctx, id, wanted),
            Some(Action::Styles) => self.style_pick = if self.style_pick.is_some() { None } else { Some(styles::StylePick::default()) },
            Some(Action::PageToBackground) => self.run_page_op(&ctx, 13, self.page_index, 0, "", "Made a background page (Page setup makes it ordinary again)"),
            Some(Action::StyleSettings) => self.go_to_style_settings(),
            Some(Action::ZoomDialog) => self.zoom_draft = Some(ZoomDraft { focus: true, ..ZoomDraft::default() }),
            Some(Action::ZoomTo(percent)) => {
                self.zoom = zoom_for_percent(f64::from(percent), self.raster_scale);
                self.fit_pending = false;
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
    CopyPicture,
    /// Open a drawing of the Recent menu.
    OpenRecent(std::path::PathBuf),
    /// Show or hide the Shape Formulas window.
    ShapeFormulas,
    /// Give the keyboard to the first Size and Position field of the inspector.
    SizePosition,
    /// Reverse the ends of the selected connectors.
    ReverseEnds,
    /// Move the page at the first position to the second.
    PageReorder(usize, usize),
    /// Show the page at this percentage of actual size.
    ZoomTo(u32),
    ZoomDialog,
    /// Go to the inspector's Style settings.
    StyleSettings,
    /// Make the shown page a background page: it leaves the tabs.
    PageToBackground,
    /// Show or hide the Styles window.
    Styles,
    /// Name the shape (its ID, the wanted name; empty for the automatic one).
    ShapeName(String, String),
    /// Whether a plain mouse wheel zooms.
    WheelZooms(bool),
    /// Ask for another colour for "fill" or "line".
    ColourDialog(&'static str),
    /// Put a message in the status bar.
    Say(String),
    DeleteGuide,
    /// Make the page as wide as the view.
    FitWidth,
    /// (shape, field, value, page units per inch) of a Size and Position field.
    SizeField(String, usize, f64, f64),
    /// Show or hide the Layers window.
    Layers,
    FullScreen,
    /// Show or hide the Shape Data window.
    ShapeData,
    Hyperlink,
    /// `Engine::page_op` `theme_op` with the theme name or variant number.
    Theme(i32, String),
    /// Set the shown page's width and height (inches).
    PageShape(f64, f64),
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
    /// Style these shapes (op 10 JSON).
    StyleShapes(Vec<String>, String),
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
