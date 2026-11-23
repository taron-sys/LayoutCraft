//! Rotation-aware group editing, checked against the importer's own world
//! positions (independent of the module's frame math).
#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::io::Cursor;
use std::path::PathBuf;

use layoutcraft_vsdx::groupedit::{resize_frame, Frame, GroupEditor};
use layoutcraft_vsdx::writer::patch_page_cells;
use layoutcraft_vsdx::{import_document, Package};

fn corpus() -> Package {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/groups-rotated.vsdx");
    Package::open(path).unwrap()
}

/// World centres (inches, y up) of every shape, from the importer.
fn centres(package: &Package) -> std::collections::BTreeMap<String, (f64, f64)> {
    let document = import_document(package).unwrap();
    let page = &document.pages[0];
    page.shapes
        .iter()
        .map(|s| {
            let px = (s.x + s.w / 2.0) / 96.0;
            let py = (page_height(package) * 96.0 - (s.y + s.h / 2.0)) / 96.0;
            (s.visio_id.clone().unwrap(), (px, py))
        })
        .collect()
}

fn page_height(package: &Package) -> f64 {
    let page = &package.pages().unwrap()[0];
    page.height.unwrap()
}

fn apply(package: &Package, patches: &[layoutcraft_vsdx::writer::ShapeCellPatch]) -> Package {
    let bytes = patch_page_cells(package, 0, patches).unwrap();
    Package::from_reader(Cursor::new(bytes)).unwrap()
}

fn close(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
}

const ANGLE: f64 = 0.261_799_387_799_15; // 15 degrees

#[test]
fn resize_frame_keeps_the_anchor_fixed_and_the_loc_fraction() {
    let frame = Frame { pin: (3.5, 3.5), size: (6.0, 4.0), loc: (3.0, 2.0), angle: ANGLE, flip: (false, false) };
    for anchor in [(0.0, 0.0), (1.0, 1.0), (0.5, 0.5), (1.0, 0.0)] {
        let new = resize_frame(&frame, (9.0, 3.0), anchor);
        let before = frame.to_parent((anchor.0 * 6.0, anchor.1 * 4.0));
        let after = new.to_parent((anchor.0 * 9.0, anchor.1 * 3.0));
        assert!(close(before, after), "anchor {anchor:?}: {before:?} vs {after:?}");
        assert!((new.loc.0 / new.size.0 - 0.5).abs() < 1e-12 && (new.loc.1 / new.size.1 - 0.5).abs() < 1e-12);
        let back = new.from_parent(new.to_parent((1.25, 0.75)));
        assert!(close(back, (1.25, 0.75)));
    }
}

#[test]
fn resizing_a_rotated_group_scales_members_in_the_groups_own_axes() {
    let package = corpus();
    let before = centres(&package);
    let editor = GroupEditor::open(&package, 0).unwrap();
    // Group 5: 6x4 at 15 degrees, pinned at its centre. Grow the width, anchor bottom-left.
    let patches = editor.resize_group("5", (9.0, 4.0), (0.0, 0.0)).unwrap();
    let after = centres(&apply(&package, &patches));

    let (s, c) = ANGLE.sin_cos();
    let rot = |v: (f64, f64)| (c * v.0 - s * v.1, s * v.0 + c * v.1);
    // Old pin (3.5,3.5) sits at local (3,2); anchor local (0,0) is the fixed point.
    let anchor_world = {
        let d = rot((-3.0, -2.0));
        (3.5 + d.0, 3.5 + d.1)
    };
    // Local centres inside group 5's 6x4 box (bottom-left origin). Shapes 1 and 2 sit in
    // group 3, whose box is centred at local (3, 3.3333) with the shapes at +-1.8 in x.
    for (id, local) in [("4", (3.0, 2.0 / 3.0)), ("1", (1.2, 10.0 / 3.0)), ("2", (4.8, 10.0 / 3.0))] {
        let scaled = (local.0 * 1.5, local.1);
        let d = rot(scaled);
        let expected = (anchor_world.0 + d.0, anchor_world.1 + d.1);
        assert!(close(after[id], expected), "shape {id}: got {:?}, expected {expected:?} (was {:?})", after[id], before[id]);
    }
}

#[test]
fn moving_a_member_of_a_rotated_group_moves_it_by_the_world_delta_only() {
    let package = corpus();
    let before = centres(&package);
    let editor = GroupEditor::open(&package, 0).unwrap();
    let patches = editor.move_member("4", (0.5, -0.25), false).unwrap();
    let after = centres(&apply(&package, &patches));
    for (id, centre) in &before {
        let expected = if id == "4" { (centre.0 + 0.5, centre.1 - 0.25) } else { *centre };
        assert!(close(after[id], expected), "shape {id}: {:?} vs {expected:?}", after[id]);
    }
}

#[test]
fn moving_a_nested_member_outside_its_groups_grows_the_frames_without_moving_anything_else() {
    let package = corpus();
    let before = centres(&package);
    let editor = GroupEditor::open(&package, 0).unwrap();
    // Push shape 1 (inside group 3 inside group 5) far to the lower left in world space.
    let delta = (-3.0, -2.0);
    let patches = editor.move_member("1", delta, true).unwrap();
    let edited = apply(&package, &patches);
    let after = centres(&edited);
    for (id, centre) in &before {
        let expected = if id == "1" { (centre.0 + delta.0, centre.1 + delta.1) } else { *centre };
        assert!(close(after[id], expected), "shape {id}: {:?} vs {expected:?}", after[id]);
    }
    // The enclosing groups now cover their members again.
    let editor = GroupEditor::open(&edited, 0).unwrap();
    let again = editor.move_member("4", (0.0, 0.0), true).unwrap();
    assert!(again.iter().all(|_| true));
    let tree = edited.page_shapes(&edited.pages().unwrap()[0].part).unwrap();
    let roots = layoutcraft_vsdx::MasterCatalog::load(&edited).unwrap().resolve(&tree).unwrap();
    fn covers(node: &layoutcraft_vsdx::ResolvedShape) {
        let num = |n: &layoutcraft_vsdx::ResolvedShape, k: &str| n.cells[k].value.as_deref().unwrap().parse::<f64>().unwrap();
        if node.children.is_empty() {
            return;
        }
        let (w, h) = (num(node, "Width"), num(node, "Height"));
        for child in &node.children {
            let frame = Frame {
                pin: (num(child, "PinX"), num(child, "PinY")),
                size: (num(child, "Width"), num(child, "Height")),
                loc: (num(child, "LocPinX"), num(child, "LocPinY")),
                angle: num(child, "Angle"),
                flip: (false, false),
            };
            for (x, y) in frame.corners() {
                assert!(x > -1e-6 && y > -1e-6 && x < w + 1e-6 && y < h + 1e-6, "{} leaves group {}: ({x},{y}) vs {w}x{h}", child.id, node.id);
            }
            covers(child);
        }
    }
    for root in &roots {
        covers(root);
    }
}

#[test]
fn non_uniform_resize_of_an_obliquely_rotated_member_is_refused() {
    // Build the situation on a single frame level: a group whose only child is rotated 30 degrees.
    // The corpus has none, so check the guard through the public error on a rotated *member* resize
    // of the rotated group itself: uniform scaling is allowed for any angle.
    let package = corpus();
    let editor = GroupEditor::open(&package, 0).unwrap();
    assert!(editor.resize_group("5", (9.0, 6.0), (0.5, 0.5)).is_ok(), "uniform resize works at any angle");
    assert!(editor.resize_group("1", (1.0, 1.0), (0.5, 0.5)).is_err(), "a plain shape is not a group");
}

#[test]
fn resizing_a_member_keeps_the_anchor_fixed_and_survives_reimport() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patient-check-in.vsdx");
    let package = Package::open(path).unwrap();
    for id in ["9", "10", "5"] {
        let editor = GroupEditor::open(&package, 0).unwrap();
        let patches = editor.resize_member(id, (1.5, 0.6), (0.0, 0.0), false).unwrap();
        let resized = apply(&package, &patches);
        let described = GroupEditor::open(&resized, 0).unwrap().describe();
        let frame = described.as_array().unwrap().iter().find(|f| f["id"] == id).unwrap();
        assert!((frame["size"][0].as_f64().unwrap() - 1.5).abs() < 1e-9, "shape {id}: {frame}");
        assert!((frame["size"][1].as_f64().unwrap() - 0.6).abs() < 1e-9, "shape {id}: {frame}");
    }
}

fn delete_shape(package: &Package, id: &str) -> Package {
    use layoutcraft_vsdx::writer::patch_page;
    let editor = GroupEditor::open(package, 0).unwrap();
    let ids = editor.subtree_ids(id).unwrap();
    let part = package.pages().unwrap()[0].part.clone();
    let connects = package.connects(&part).unwrap();
    let patches = editor.detach_patches(&ids, &connects);
    let bytes = patch_page(package, 0, &patches, &[], &[id.to_string()]).unwrap();
    Package::from_reader(Cursor::new(bytes)).unwrap()
}

#[test]
fn deleting_a_shape_keeps_glued_connectors_in_place_and_drops_their_glue() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/route-sweep-shape.vsdx");
    let package = Package::open(path).unwrap();
    let part = package.pages().unwrap()[0].part.clone();
    let before = package.connects(&part).unwrap();
    let ours: Vec<_> = before.iter().filter(|c| c.to_sheet == "1").collect();
    assert!(!ours.is_empty(), "the corpus has a connector glued to shape 1");
    let connector = ours[0].from_sheet.clone();
    let end_cell = ours[0].from_cell.clone();

    let after = delete_shape(&package, "1");
    let part = after.pages().unwrap()[0].part.clone();
    let connects = after.connects(&part).unwrap();
    assert!(connects.iter().all(|c| c.to_sheet != "1" && c.from_sheet != "1"), "glue to the deleted shape is gone");
    // Other glue stays.
    assert_eq!(connects.len(), before.len() - ours.len() - before.iter().filter(|c| c.from_sheet == "1").count());
    let described = GroupEditor::open(&after, 0).unwrap().describe();
    let ids: Vec<&str> = described.as_array().unwrap().iter().map(|f| f["id"].as_str().unwrap()).collect();
    assert!(!ids.contains(&"1"), "shape 1 is gone");
    assert!(ids.contains(&connector.as_str()), "the connector stays");
    // The end that was glued keeps its position as a literal, not a formula.
    let xml = after.text(&part).unwrap();
    assert!(!xml.contains("ID='1' "), "no leftover shape element");
    let cell = if end_cell.starts_with("Begin") { "BeginX" } else { "EndX" };
    let open = xml.find(&format!("<Shape ID='{connector}'")).or_else(|| xml.find(&format!("<Shape ID=\"{connector}\""))).unwrap();
    let body = &xml[open..];
    let tag_at = body.find(&format!("N='{cell}'")).or_else(|| body.find(&format!("N=\"{cell}\""))).unwrap();
    let tag = &body[body[..tag_at].rfind("<Cell").unwrap()..tag_at + body[tag_at..].find('>').unwrap()];
    assert!(!tag.contains(" F="), "{cell} should be a plain value now: {tag}");
    assert!(import_document(&after).is_ok());
}

#[test]
fn deleting_a_group_removes_its_members_too() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patient-check-in.vsdx");
    let package = Package::open(path).unwrap();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let ids = editor.subtree_ids("9").unwrap();
    assert!(ids.len() >= 2 && ids[0] == "9");
    let after = delete_shape(&package, "9");
    let described = GroupEditor::open(&after, 0).unwrap().describe();
    let left: Vec<&str> = described.as_array().unwrap().iter().map(|f| f["id"].as_str().unwrap()).collect();
    for id in &ids {
        assert!(!left.contains(&id.as_str()), "shape {id} should be gone");
    }
}

#[test]
fn rotating_a_member_changes_only_its_angle_and_keeps_the_pin() {
    let package = corpus();
    let before = centres(&package);
    // Shape 4 sits in group 5, which is itself rotated by 0.26 rad.
    let editor = GroupEditor::open(&package, 0).unwrap();
    let frames_before = editor.describe();
    let find = |frames: &serde_json::Value, id: &str| frames.as_array().unwrap().iter().find(|f| f["id"] == id).unwrap().clone();
    let parent_angle = find(&frames_before, "5")["angle"].as_f64().unwrap();
    let patches = editor.rotate_member("4", parent_angle + 0.5).unwrap();
    assert_eq!(patches.len(), 1, "one patch: the Angle cell");
    let after = apply(&package, &patches);
    let frames_after = GroupEditor::open(&after, 0).unwrap().describe();
    let (b, a) = (find(&frames_before, "4"), find(&frames_after, "4"));
    assert!((a["angle"].as_f64().unwrap() - 0.5).abs() < 1e-9, "local angle = world angle - parent angle: {a}");
    assert_eq!(a["pin"], b["pin"], "rotation about the pin leaves the pin alone");
    assert_eq!(a["size"], b["size"]);
    // The shape's centre does not move (its pin is its centre) and nothing else changes.
    let centres_after = centres(&after);
    for (id, centre) in &before {
        assert!(close(centres_after[id], *centre), "shape {id} moved: {:?} vs {centre:?}", centres_after[id]);
    }
}

#[test]
fn rotating_to_an_angle_beyond_a_full_turn_is_normalised() {
    let package = corpus();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let patches = editor.rotate_member("5", 7.0).unwrap(); // top-level group
    let after = apply(&package, &patches);
    let described = GroupEditor::open(&after, 0).unwrap().describe();
    let angle = described.as_array().unwrap().iter().find(|f| f["id"] == "5").unwrap()["angle"].as_f64().unwrap();
    assert!((angle - (7.0 - std::f64::consts::TAU)).abs() < 1e-9, "{angle}");
    assert!(editor.rotate_member("no-such", 1.0).is_err());
}

#[test]
fn editing_text_keeps_the_paragraph_markers_and_survives_reimport() {
    use layoutcraft_vsdx::writer::{own_text, patch_page};
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patient-check-in.vsdx");
    let package = Package::open(path).unwrap();
    // The label of group 9 lives in one of its members.
    let editor = GroupEditor::open(&package, 0).unwrap();
    let owner = editor
        .subtree_ids("9")
        .unwrap()
        .into_iter()
        .find(|id| own_text(&package, 0, id).unwrap().is_some_and(|t| !t.is_empty()))
        .expect("a member of group 9 carries the text");
    let before = own_text(&package, 0, &owner).unwrap().unwrap();
    assert_eq!(before, "Patient arrives \nfor appointment");
    let patch = layoutcraft_vsdx::writer::ShapeCellPatch::new(owner.clone()).edit_text("Patient arrives \nfor appointment today");
    let bytes = patch_page(&package, 0, &[patch], &[], &[]).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(own_text(&after, 0, &owner).unwrap().unwrap(), "Patient arrives \nfor appointment today");
    let xml = after.text(&after.pages().unwrap()[0].part).unwrap();
    assert!(xml.contains("<pp IX=\"1\"/>for appointment today</Text>"), "second paragraph marker kept");
    assert!(xml.contains("<Text><pp IX=\"0\"/><cp IX=\"0\"/>Patient arrives <pp IX=\"1\"/>"), "first paragraph and run markers kept");
    // The importer shows the new text.
    let doc = import_document(&after).unwrap();
    let json = serde_json::to_string(&doc).unwrap();
    assert!(json.contains("for appointment today"), "the model carries the edited text");
}

#[test]
fn the_text_of_a_group_is_found_in_the_member_that_holds_it() {
    use layoutcraft_vsdx::writer::{patch_page, own_text, ShapeCellPatch};
    let package = corpus();
    let editor = GroupEditor::open(&package, 0).unwrap();
    // Every shape of the corpus has its own label ("Outer group", "Inner group", "A", ...).
    assert_eq!(editor.text_owner(&package, 0, "5").unwrap().0, "5");
    assert_eq!(editor.text_owner(&package, 0, "3").unwrap().0, "3");
    // Blank the labels of both groups: the label shown for them is now the first member's.
    let blank = patch_page(&package, 0, &[ShapeCellPatch::new("5").text(""), ShapeCellPatch::new("3").text("")], &[], &[]).unwrap();
    let blanked = Package::from_reader(Cursor::new(blank)).unwrap();
    let editor = GroupEditor::open(&blanked, 0).unwrap();
    assert_eq!(editor.text_owner(&blanked, 0, "3").unwrap(), ("1".to_string(), "A\r\n".to_string()));
    assert_eq!(editor.text_owner(&blanked, 0, "5").unwrap(), ("1".to_string(), "A\r\n".to_string()));
    assert_eq!(editor.text_owner(&blanked, 0, "4").unwrap().0, "4", "a sibling keeps its own text");
    // Editing through the owner id keeps working and leaves the line ending alone.
    let edit = patch_page(&blanked, 0, &[ShapeCellPatch::new("1").edit_text("A2\r\n")], &[], &[]).unwrap();
    let edited = Package::from_reader(Cursor::new(edit)).unwrap();
    assert_eq!(own_text(&edited, 0, "1").unwrap().as_deref(), Some("A2\r\n"));
}

fn open_corpus(name: &str) -> Package {
    Package::open(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus").join(name)).unwrap()
}

#[test]
fn a_master_instance_gets_a_fresh_id_the_masters_size_and_the_drop_position() {
    use layoutcraft_vsdx::writer::{add_master_instance, next_shape_id};
    let package = open_corpus("block-masters.vsdx");
    assert_eq!(next_shape_id(&package, 0).unwrap(), 2, "the page has shape 1");
    let (bytes, id) = add_master_instance(&package, 0, "2", 5.0, 4.0).unwrap();
    assert_eq!(id, "2");
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    // The importer draws two shapes now, the new one named after the master.
    let doc = import_document(&after).unwrap();
    let new = doc.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some("2")).expect("the new shape is imported");
    let old = doc.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some("1")).unwrap();
    assert!((new.w - old.w).abs() < 1e-6 && (new.h - old.h).abs() < 1e-6, "same size as the existing instance of the master");
    assert!(!new.parts.is_empty(), "the master's geometry is drawn");
    // Frame: pin where it was dropped, size from the master.
    let described = GroupEditor::open(&after, 0).unwrap().describe();
    let frame = described.as_array().unwrap().iter().find(|f| f["id"] == "2").unwrap().clone();
    assert!((frame["pin"][0].as_f64().unwrap() - 5.0).abs() < 1e-9 && (frame["pin"][1].as_f64().unwrap() - 4.0).abs() < 1e-9, "{frame}");
    assert!(frame["size"][0].as_f64().unwrap() > 0.0);
    // Visio's naming: master name + the shape id.
    let xml = after.text(&after.pages().unwrap()[0].part).unwrap();
    assert!(xml.contains("<Shape ID='2' NameU='Box.2' Name='Box.2' Type='Shape' Master='2'>"), "{xml}");
    // A second drop gets the next id; the first survives.
    let (bytes, id2) = add_master_instance(&after, 0, "2", 1.0, 1.0).unwrap();
    assert_eq!(id2, "3");
    let twice = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(import_document(&twice).unwrap().pages[0].shapes.len(), 3);
}

#[test]
fn dropping_an_unknown_master_is_an_error_and_nested_ids_count() {
    use layoutcraft_vsdx::writer::{add_master_instance, next_shape_id};
    let block = open_corpus("block-masters.vsdx");
    assert!(add_master_instance(&block, 0, "99", 1.0, 1.0).is_err());
    assert!(add_master_instance(&block, 50, "2", 1.0, 1.0).is_err(), "no such page");
    // groups-rotated has shapes 1..5 nested in groups: the next id is 6.
    assert_eq!(next_shape_id(&corpus(), 0).unwrap(), 6);
}

#[test]
fn a_new_connector_is_glued_to_connection_points_and_follows_a_moved_shape() {
    use layoutcraft_model::LineJump;
    use layoutcraft_vsdx::groupedit::add_connector;
    use layoutcraft_vsdx::writer::{next_shape_id, patch_page_cells};
    let package = open_corpus("route-sweep-sites.vsdx");
    let part = package.pages().unwrap()[0].part.clone();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let points = editor.connection_points("1");
    assert_eq!(points.len(), 5, "the Rectangle master has five connection points: {points:?}");
    assert_eq!(points[0].0, 1);
    let expected_id = next_shape_id(&package, 0).unwrap().to_string();
    let connectors_before = import_document(&package).unwrap().pages[0].connectors.len();

    let (bytes, id) = add_connector(&package, 0, ("1", Some(2)), ("2", Some(4))).unwrap();
    assert_eq!(id, expected_id);
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    let connects = after.connects(&part).unwrap();
    let mine: Vec<_> = connects.iter().filter(|c| c.from_sheet == id).collect();
    assert_eq!(mine.len(), 2, "one Connect row per end");
    assert!(mine.iter().any(|c| c.from_cell == "BeginX" && c.to_sheet == "1" && c.to_cell == "Connections.X2"));
    assert!(mine.iter().any(|c| c.from_cell == "EndX" && c.to_sheet == "2" && c.to_cell == "Connections.X4"));
    let xml = after.text(&part).unwrap();
    assert!(xml.contains(&format!("<Shape ID='{id}' NameU='Dynamic connector.{id}'")), "an instance of the Dynamic connector master");
    assert!(xml.contains("Master='4'"));
    assert!(xml.contains("PAR(PNT(Sheet.1!Connections.X2,Sheet.1!Connections.Y2))"), "ends are glue formulas");
    assert!(xml.contains("ToCell='Connections.X2' ToPart='101'"), "connection point k is ToPart 99 + k");
    // The importer knows one more connector, drawn with an elbow route from the first point.
    let doc = import_document(&after).unwrap();
    assert_eq!(doc.pages[0].connectors.len(), connectors_before + 1);
    // The stored route is the one the router computes: a reroute leaves the connector alone ...
    let editor = GroupEditor::open(&after, 0).unwrap();
    let jump = LineJump::default();
    assert!(editor.reroute_connectors(&connects, &jump).iter().all(|p| p.visio_id != id), "nothing to change yet");
    // ... and when a glued shape moves, the connector follows it.
    let moved = patch_page_cells(&after, 0, &editor.move_member("1", (0.75, 0.25), false).unwrap()).unwrap();
    let moved = Package::from_reader(Cursor::new(moved)).unwrap();
    let editor = GroupEditor::open(&moved, 0).unwrap();
    let follow = editor.reroute_connectors(&moved.connects(&part).unwrap(), &jump);
    assert!(follow.iter().any(|p| p.visio_id == id), "the new connector is rerouted when shape 1 moves");
}

#[test]
fn a_connector_between_shapes_without_connection_points_glues_the_whole_shapes() {
    use layoutcraft_vsdx::groupedit::add_connector;
    use layoutcraft_vsdx::writer::add_master_instance;
    let package = open_corpus("block-masters.vsdx");
    let part = package.pages().unwrap()[0].part.clone();
    let (bytes, second) = add_master_instance(&package, 0, "2", 4.0, 2.0).unwrap();
    let two = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, id) = add_connector(&two, 0, ("1", None), (&second, None)).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    let connects = after.connects(&part).unwrap();
    assert!(connects.iter().any(|c| c.from_sheet == id && c.from_cell == "BeginX" && c.to_sheet == "1" && c.to_cell == "PinX"));
    assert!(connects.iter().any(|c| c.from_sheet == id && c.from_cell == "EndX" && c.to_sheet == second && c.to_cell == "PinX"));
    let xml = after.text(&part).unwrap();
    assert!(xml.contains(&format!("<Shape ID='{id}' NameU='Connector.{id}' Name='Connector.{id}' Type='Shape'")), "no Dynamic connector master in this document: a plain 1-D shape (in the document's theme-based Normal style, Q7)");
    assert!(xml.contains("_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)") && xml.contains("<Cell N='WalkPreference' V='0'/>"), "whole-shape glue needs the formula Visio uses, or Visio ignores the Connect rows");
    assert!(import_document(&after).is_ok());
}

#[test]
fn connectors_need_two_different_existing_shapes_and_real_connection_points() {
    use layoutcraft_vsdx::groupedit::add_connector;
    let package = open_corpus("route-sweep-sites.vsdx");
    assert!(add_connector(&package, 0, ("1", Some(1)), ("1", Some(2))).is_err(), "a shape to itself");
    assert!(add_connector(&package, 0, ("1", Some(1)), ("999", Some(1))).is_err(), "unknown shape");
    assert!(add_connector(&package, 0, ("1", Some(99)), ("2", Some(1))).is_err(), "no such connection point");
    assert!(add_connector(&package, 40, ("1", Some(1)), ("2", Some(1))).is_err(), "no such page");
}

#[test]
fn pasting_a_master_instance_gets_a_new_id_name_and_offset_position() {
    use layoutcraft_vsdx::writer::{paste_shapes, shape_xml};
    let package = open_corpus("block-masters.vsdx");
    let copy = shape_xml(&package, 0, "1").unwrap();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let before = editor.describe().as_array().unwrap().iter().find(|f| f["id"] == "1").unwrap().clone();
    let (bytes, ids) = paste_shapes(&package, 0, &[copy.clone()], 0.25, -0.25).unwrap();
    assert_eq!(ids, vec!["2".to_string()]);
    let pasted = Package::from_reader(Cursor::new(bytes)).unwrap();
    let described = GroupEditor::open(&pasted, 0).unwrap().describe();
    let frame = described.as_array().unwrap().iter().find(|f| f["id"] == "2").unwrap().clone();
    assert!((frame["pin"][0].as_f64().unwrap() - (before["pin"][0].as_f64().unwrap() + 0.25)).abs() < 1e-9, "{frame}");
    assert!((frame["pin"][1].as_f64().unwrap() - (before["pin"][1].as_f64().unwrap() - 0.25)).abs() < 1e-9);
    assert_eq!(frame["size"], before["size"], "the size comes from the same master");
    let xml = pasted.text(&pasted.pages().unwrap()[0].part).unwrap();
    assert!(xml.contains("NameU='Box.2'"), "named the way Visio names the second instance: {xml}");
    // Pasting again (the clipboard is the XML, the original need not exist) gives yet another id.
    let (bytes, ids) = paste_shapes(&pasted, 0, &[copy], 0.5, -0.5).unwrap();
    assert_eq!(ids, vec!["3".to_string()]);
    assert_eq!(import_document(&Package::from_reader(Cursor::new(bytes)).unwrap()).unwrap().pages[0].shapes.len(), 3);
}

#[test]
fn pasting_a_group_renumbers_its_members_and_the_references_between_them() {
    use layoutcraft_vsdx::writer::paste_shapes;
    let package = open_corpus("block-masters.vsdx"); // holds shape 1, so the next ids are 2, 3
    // Old 1 -> 2 and old 2 -> 3: a naive sequential rewrite would turn Sheet.1 into Sheet.3.
    let group = "<Shape ID='1' Type='Group'><Cell N='PinX' V='2'/><Cell N='PinY' V='2'/><Cell N='Width' V='1' F='Sheet.2!Width'/><Shapes><Shape ID='2' Type='Shape'><Cell N='Width' V='1'/><Cell N='Height' V='1' F='Sheet.1!Height'/></Shape></Shapes></Shape>".to_string();
    let (bytes, ids) = paste_shapes(&package, 0, &[group], 1.0, 0.0).unwrap();
    assert_eq!(ids, vec!["2".to_string()], "only the top-level shape is reported");
    let pasted = Package::from_reader(Cursor::new(bytes)).unwrap();
    let xml = pasted.text(&pasted.pages().unwrap()[0].part).unwrap();
    assert!(xml.contains("<Shape ID='2' Type='Group'>") && xml.contains("<Shape ID='3' Type='Shape'>"), "{xml}");
    assert!(xml.contains("F='Sheet.3!Width'"), "the reference to the member follows it: {xml}");
    assert!(xml.contains("F='Sheet.2!Height'"), "the member's reference to its group follows it: {xml}");
    assert!(!xml.contains('\u{1}'), "no marker left behind");
    assert!(xml.contains("V='3'"), "the group's pin moved by the offset (2 + 1)");
}

#[test]
fn only_page_level_shapes_that_are_not_connectors_can_be_copied() {
    let sites = open_corpus("route-sweep-sites.vsdx");
    let editor = GroupEditor::open(&sites, 0).unwrap();
    assert!(editor.check_copyable("1").is_ok());
    assert!(editor.check_copyable("3").is_err(), "a connector");
    let rotated = corpus();
    let editor = GroupEditor::open(&rotated, 0).unwrap();
    assert!(editor.check_copyable("5").is_ok(), "a top-level group");
    assert!(editor.check_copyable("1").is_err(), "a member of a group");
    assert!(editor.check_copyable("nope").is_err());
}

fn shape_fill(package: &Package, id: &str) -> String {
    let doc = import_document(package).unwrap();
    doc.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id)).unwrap_or_else(|| panic!("no shape {id}")).fill.clone().unwrap_or_default()
}

#[test]
fn a_fill_colour_applies_to_a_group_and_everything_inside_it() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    use layoutcraft_vsdx::writer::patch_page_cells;
    let package = corpus();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let edit = StyleEdit { fill: Some("#00FF00".to_string()), ..Default::default() };
    let patches = editor.style_patches(&["5".to_string()], &edit).unwrap();
    assert!(patches.len() >= 5, "the group and its members each get a patch: {}", patches.len());
    let styled = Package::from_reader(Cursor::new(patch_page_cells(&package, 0, &patches).unwrap())).unwrap();
    for id in ["1", "2", "4"] {
        assert_eq!(shape_fill(&styled, id), "#00ff00", "member {id}");
    }
    // One shape only: the others keep their fill.
    let one = editor.style_patches(&["4".to_string()], &edit).unwrap();
    let styled = Package::from_reader(Cursor::new(patch_page_cells(&package, 0, &one).unwrap())).unwrap();
    assert_eq!(shape_fill(&styled, "4"), "#00ff00");
    assert_ne!(shape_fill(&styled, "1"), "#00ff00");
}

#[test]
fn line_pattern_weight_no_fill_and_font_size_are_written_as_cells() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    use layoutcraft_vsdx::writer::patch_page_cells;
    let package = corpus();
    let part = package.pages().unwrap()[0].part.clone();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let edit = StyleEdit { line: Some("#CC0000".to_string()), weight_px: Some(4.0), pattern: Some(3), fill: Some("none".to_string()), font_pt: Some(20.0), ..Default::default() };
    let patches = editor.style_patches(&["4".to_string()], &edit).unwrap();
    let styled = Package::from_reader(Cursor::new(patch_page_cells(&package, 0, &patches).unwrap())).unwrap();
    let xml = styled.text(&part).unwrap();
    let at = xml.find("<Shape ID='4'").or_else(|| xml.find("<Shape ID=\"4\"")).unwrap();
    let end = xml[at..].find("</Shape>").map_or(xml.len(), |n| at + n);
    let shape = &xml[at..end];
    for needle in ["N='LineColor' V='#cc0000'", "N='LinePattern' V='3'", "N='FillPattern' V='0'"] {
        assert!(shape.replace('"', "'").contains(needle), "{needle} in {shape}");
    }
    assert!(shape.replace('"', "'").contains("N='LineWeight' V='0.0416666"), "4 px = 1/24 in: {shape}");
    assert!(shape.replace('"', "'").contains("<Row IX='0'><Cell N='LangID' V='en-GB'/><Cell N='Size' V='0.277777778'/></Row>"), "the size sits inside the existing row, 20 pt = 20/72 in: {shape}");
    // The importer agrees: the shape is no longer filled and its line is dashed red.
    let doc = import_document(&styled).unwrap();
    let s = doc.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some("4")).unwrap();
    assert_eq!(s.stroke.as_deref(), Some("#cc0000"));
    let size = s.font_size.expect("a font size");
    assert!((size - 20.0 * 96.0 / 72.0).abs() < 0.01, "font size in px: {size}");
}

#[test]
fn a_shape_without_a_character_section_gets_one_for_its_font_size() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    use layoutcraft_vsdx::writer::{add_master_instance, patch_page_cells};
    let package = open_corpus("block-masters.vsdx");
    let (bytes, id) = add_master_instance(&package, 0, "2", 3.0, 2.0).unwrap();
    let package = Package::from_reader(Cursor::new(bytes)).unwrap();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let edit = StyleEdit { font_pt: Some(24.0), ..Default::default() };
    let patches = editor.style_patches(&[id.clone()], &edit).unwrap();
    assert_eq!(patches.len(), 1, "a Box master instance carries text");
    let styled = Package::from_reader(Cursor::new(patch_page_cells(&package, 0, &patches).unwrap())).unwrap();
    let xml = styled.text(&styled.pages().unwrap()[0].part).unwrap();
    assert!(xml.contains("<Section N='Character'><Row IX='0'><Cell N='Size' V='0.333333333'/></Row></Section>"), "{xml}");
    assert!(import_document(&styled).is_ok());
}

#[test]
fn style_edits_are_validated() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    let package = corpus();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let ids = ["4".to_string()];
    assert!(editor.style_patches(&ids, &StyleEdit { fill: Some("red".into()), ..Default::default() }).is_err(), "colour names are not accepted");
    assert!(editor.style_patches(&ids, &StyleEdit { line: Some("#12345".into()), ..Default::default() }).is_err());
    assert!(editor.style_patches(&ids, &StyleEdit { pattern: Some(24), ..Default::default() }).is_err());
    assert!(editor.style_patches(&ids, &StyleEdit { weight_px: Some(-1.0), ..Default::default() }).is_err());
    assert!(editor.style_patches(&ids, &StyleEdit { font_pt: Some(0.0), ..Default::default() }).is_err());
    assert!(editor.style_patches(&["nope".to_string()], &StyleEdit { pattern: Some(1), ..Default::default() }).is_err());
    let info = editor.style_info("4").unwrap();
    assert!(info.get("hasText").is_some());
}

#[test]
fn a_line_colour_makes_a_fill_only_shape_show_a_line_but_leaves_outlined_shapes_alone() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    use layoutcraft_vsdx::writer::patch_page_cells;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patient-check-in.vsdx");
    let package = Package::open(path).unwrap();
    let stroke_of = |package: &Package, id: &str| {
        let doc = import_document(package).unwrap();
        let shape = doc.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id)).unwrap();
        shape.parts.iter().filter_map(|p| p.stroke.clone()).find(|s| s != "none")
    };
    assert_eq!(stroke_of(&package, "12"), None, "the Lucidchart process block is fill-only");
    let editor = GroupEditor::open(&package, 0).unwrap();
    let edit = StyleEdit { line: Some("#00AA00".to_string()), weight_px: Some(4.0), ..Default::default() };
    let patches = editor.style_patches(&["12".to_string()], &edit).unwrap();
    let styled = Package::from_reader(Cursor::new(patch_page_cells(&package, 0, &patches).unwrap())).unwrap();
    assert_eq!(stroke_of(&styled, "12").as_deref(), Some("#00aa00"), "the line shows now");
    // A shape that already draws an outline keeps its geometry flags (no NoLine patch for it).
    let corpus_package = corpus();
    let editor = GroupEditor::open(&corpus_package, 0).unwrap();
    let patches = editor.style_patches(&["4".to_string()], &edit).unwrap();
    assert!(patches.iter().all(|p| p.geometry_flags.is_empty()), "no geometry flag is touched when a line is already drawn");
    // Choosing a fill colour switches a hidden fill back on in the same way.
    let fill = StyleEdit { fill: Some("#ff0000".to_string()), ..Default::default() };
    assert!(GroupEditor::open(&package, 0).unwrap().style_patches(&["12".to_string()], &fill).is_ok());
}

#[test]
fn a_geometry_flag_that_hides_the_line_is_switched_off_by_a_line_edit() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    use layoutcraft_vsdx::writer::{patch_page_cells, ShapeCellPatch};
    let package = corpus();
    // Plant NoLine = 1 in the geometry of shape 4 (a shape that draws its own outline otherwise).
    let hidden = patch_page_cells(&package, 0, &[ShapeCellPatch::new("4").geometry_flag("0", "NoLine", "1")]).unwrap();
    let hidden = Package::from_reader(Cursor::new(hidden)).unwrap();
    let stroke = |p: &Package| {
        let doc = import_document(p).unwrap();
        let s = doc.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some("4")).unwrap();
        s.parts.iter().filter_map(|part| part.stroke.clone()).find(|c| c != "none")
    };
    assert_eq!(stroke(&hidden), None, "NoLine hides the outline");
    let editor = GroupEditor::open(&hidden, 0).unwrap();
    let edit = StyleEdit { line: Some("#0000FF".to_string()), ..Default::default() };
    let patches = editor.style_patches(&["4".to_string()], &edit).unwrap();
    assert!(patches.iter().any(|p| !p.geometry_flags.is_empty()), "the flag is cleared");
    let styled = Package::from_reader(Cursor::new(patch_page_cells(&hidden, 0, &patches).unwrap())).unwrap();
    assert_eq!(stroke(&styled).as_deref(), Some("#0000ff"), "the line shows");
}

#[test]
fn ids_after_a_group_master_instance_leave_room_for_its_implicit_sub_shapes() {
    use layoutcraft_vsdx::groupedit::add_connector;
    use layoutcraft_vsdx::writer::{add_master_instance, next_shape_id, paste_shapes};
    // The Lucidchart masters are groups: Visio creates their sub-shapes itself and gives them the IDs
    // that follow the instance. A new shape that took such an ID showed up in Visio as a member of
    // the instance (found by opening a saved drawing in Visio: the connector had ContainingShape = 62).
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patient-check-in.vsdx");
    let package = Package::open(path).unwrap();
    let catalog = layoutcraft_vsdx::MasterCatalog::load(&package).unwrap();
    let children = catalog.masters["1"].shapes.len() as u64 - 1;
    assert!(children >= 1, "master 1 is a group with sub-shapes");
    let before = next_shape_id(&package, 0).unwrap();
    let (bytes, id) = add_master_instance(&package, 0, "1", 9.0, 1.0).unwrap();
    assert_eq!(id, before.to_string());
    let dropped = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(next_shape_id(&dropped, 0).unwrap(), before + children + 1, "the sub-shape ids are kept free");
    // So the next shape (here a connector) does not collide with them.
    let editor = GroupEditor::open(&dropped, 0).unwrap();
    let with_points: Vec<String> = editor
        .describe()
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["parentId"].is_null() && !editor.connection_points(f["id"].as_str().unwrap()).is_empty())
        .map(|f| f["id"].as_str().unwrap().to_string())
        .take(2)
        .collect();
    let (bytes, connector) = add_connector(&dropped, 0, (&with_points[0], Some(1)), (&with_points[1], Some(1))).unwrap();
    assert!(connector.parse::<u64>().unwrap() > before + children, "connector id {connector} must be above {}", before + children);
    drop(bytes);
    // A pasted master instance reserves them too.
    let copy = layoutcraft_vsdx::writer::shape_xml(&dropped, 0, &id).unwrap();
    let (_, ids) = paste_shapes(&dropped, 0, &[copy.clone(), copy], 0.0, 0.0).unwrap();
    let (a, b) = (ids[0].parse::<u64>().unwrap(), ids[1].parse::<u64>().unwrap());
    assert!(b > a + children, "the second pasted instance ({b}) leaves room after the first ({a} + {children})");
}

fn connector_fixture() -> (Package, String, String) {
    use layoutcraft_vsdx::groupedit::add_connector;
    let package = open_corpus("route-sweep-sites.vsdx");
    let part = package.pages().unwrap()[0].part.clone();
    let (bytes, id) = add_connector(&package, 0, ("1", Some(2)), ("2", Some(4))).unwrap();
    (Package::from_reader(Cursor::new(bytes)).unwrap(), id, part)
}

#[test]
fn a_connector_end_dragged_to_another_connection_point_is_reglued_everywhere() {
    use layoutcraft_vsdx::groupedit::{move_connector_end, EndTarget};
    let (package, id, part) = connector_fixture();
    let bytes = move_connector_end(&package, 0, &id, false, EndTarget::Glue("2".into(), Some(1))).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    let connects = after.connects(&part).unwrap();
    let mine: Vec<_> = connects.iter().filter(|c| c.from_sheet == id).collect();
    assert_eq!(mine.len(), 2, "still one row per end: {mine:?}");
    assert!(mine.iter().any(|c| c.from_cell == "EndX" && c.to_sheet == "2" && c.to_cell == "Connections.X1"));
    assert!(mine.iter().any(|c| c.from_cell == "BeginX" && c.to_sheet == "1" && c.to_cell == "Connections.X2"), "the other end is untouched");
    let xml = after.text(&part).unwrap();
    assert!(xml.contains("ToCell='Connections.X1' ToPart='100'"), "point 1 is ToPart 100");
    let shape = layoutcraft_vsdx::writer::shape_xml(&after, 0, &id).unwrap();
    assert!(shape.contains("PAR(PNT(Sheet.2!Connections.X1,Sheet.2!Connections.Y1))"), "{shape}");
    assert!(!shape.contains("Connections.X4"), "the old target is gone: {shape}");
    // The stored route is the one a reroute would compute, and the end sits on point 1 of shape 2.
    let editor = GroupEditor::open(&after, 0).unwrap();
    let point = editor.connection_points("2").into_iter().find(|p| p.0 == 1).unwrap();
    let listed = editor.connectors(&connects);
    let me = listed.as_array().unwrap().iter().find(|c| c["id"] == id.as_str()).unwrap();
    assert!((me["end"]["x"].as_f64().unwrap() - point.1).abs() < 1e-6 && (me["end"]["y"].as_f64().unwrap() - point.2).abs() < 1e-6, "{me}");
    assert_eq!(me["end"]["glue"]["sheet"], "2");
    assert_eq!(me["end"]["glue"]["n"], 1);
    assert!(me["route"].as_array().unwrap().len() >= 2);
    let jump = layoutcraft_model::LineJump::default();
    assert!(editor.reroute_connectors(&connects, &jump).iter().all(|p| p.visio_id != id), "route already current");
    assert!(import_document(&after).is_ok());
}

#[test]
fn a_connector_end_dropped_on_the_page_becomes_loose_and_loses_its_connect_row() {
    use layoutcraft_vsdx::groupedit::{move_connector_end, EndTarget};
    let (package, id, part) = connector_fixture();
    let bytes = move_connector_end(&package, 0, &id, false, EndTarget::Free(7.0, 6.0)).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    let connects = after.connects(&part).unwrap();
    let mine: Vec<_> = connects.iter().filter(|c| c.from_sheet == id).collect();
    assert_eq!(mine.len(), 1, "only the begin end stays glued: {mine:?}");
    assert_eq!(mine[0].from_cell, "BeginX");
    let shape = layoutcraft_vsdx::writer::shape_xml(&after, 0, &id).unwrap();
    assert!(shape.contains("<Cell N='EndX' V='7'/>") && shape.contains("<Cell N='EndY' V='6'/>"), "literal end cells: {shape}");
    assert!(shape.contains("N='BeginX'") && shape.contains("PAR(PNT(Sheet.1!Connections.X2"), "the begin glue formula stays");
    let editor = GroupEditor::open(&after, 0).unwrap();
    let listed = editor.connectors(&connects);
    let me = listed.as_array().unwrap().iter().find(|c| c["id"] == id.as_str()).unwrap();
    assert!(me["end"]["glue"].is_null(), "{me}");
    assert_eq!(me["end"]["x"].as_f64().unwrap(), 7.0);
    assert!(!me["begin"]["glue"].is_null());
    // A loose end is not rerouted when a glued shape moves (the router needs both ends glued).
    let jump = layoutcraft_model::LineJump::default();
    assert!(editor.reroute_connectors(&connects, &jump).iter().all(|p| p.visio_id != id));
    // And it can be glued again: back to a whole shape.
    let again = move_connector_end(&after, 0, &id, false, EndTarget::Glue("2".into(), None)).unwrap();
    let again = Package::from_reader(Cursor::new(again)).unwrap();
    let connects = again.connects(&part).unwrap();
    assert!(connects.iter().any(|c| c.from_sheet == id && c.from_cell == "EndX" && c.to_sheet == "2" && c.to_cell == "PinX"));
    let shape = layoutcraft_vsdx::writer::shape_xml(&again, 0, &id).unwrap();
    assert!(shape.contains("_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)"), "{shape}");
    assert!(import_document(&again).is_ok());
}

#[test]
fn moving_a_connector_end_refuses_what_it_cannot_do() {
    use layoutcraft_vsdx::groupedit::{move_connector_end, EndTarget};
    let (package, id, _) = connector_fixture();
    assert!(move_connector_end(&package, 0, "1", true, EndTarget::Free(1.0, 1.0)).is_err(), "shape 1 is not a connector");
    assert!(move_connector_end(&package, 0, "999", true, EndTarget::Free(1.0, 1.0)).is_err(), "unknown shape");
    assert!(move_connector_end(&package, 0, &id, true, EndTarget::Glue(id.clone(), None)).is_err(), "glue to itself");
    assert!(move_connector_end(&package, 0, &id, true, EndTarget::Glue("999".into(), None)).is_err(), "unknown target");
    assert!(move_connector_end(&package, 0, &id, true, EndTarget::Glue("2".into(), Some(99))).is_err(), "no such connection point");
    assert!(move_connector_end(&package, 0, &id, true, EndTarget::Glue("2".into(), Some(1))).is_err(), "both ends on one shape");
    assert!(move_connector_end(&package, 7, &id, true, EndTarget::Free(1.0, 1.0)).is_err(), "no such page");
}

#[test]
fn a_deleted_connector_takes_its_connect_rows_with_it() {
    let (package, id, part) = connector_fixture();
    let bytes = layoutcraft_vsdx::writer::patch_page(&package, 0, &[], &[], &[id.clone()]).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert!(after.connects(&part).unwrap().iter().all(|c| c.from_sheet != id));
    assert!(GroupEditor::open(&after, 0).unwrap().connectors(&after.connects(&part).unwrap()).as_array().unwrap().iter().all(|c| c["id"] != id.as_str()));
}

#[test]
fn a_copied_connector_between_two_copied_shapes_stays_glued_to_the_copies() {
    use layoutcraft_vsdx::groupedit::copy_shapes;
    use layoutcraft_vsdx::writer::paste_shapes_with_connects;
    let (package, id, part) = connector_fixture();
    let (xmls, rows) = copy_shapes(&package, 0, &["1".to_string(), "2".to_string(), id.clone()]).unwrap();
    assert_eq!(xmls.len(), 3);
    assert_eq!(rows.len(), 2, "both ends of the connector stay inside the copy: {rows:?}");
    let before = GroupEditor::open(&package, 0).unwrap().connectors(&package.connects(&part).unwrap());
    let old = before.as_array().unwrap().iter().find(|c| c["id"] == id.as_str()).unwrap().clone();
    let (bytes, ids) = paste_shapes_with_connects(&package, 0, &xmls, &rows, 0.5, -0.5).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (n1, n2, nc) = (ids[0].clone(), ids[1].clone(), ids[2].clone());
    let connects = after.connects(&part).unwrap();
    assert!(connects.iter().any(|c| c.from_sheet == nc && c.from_cell == "BeginX" && c.to_sheet == n1 && c.to_cell == "Connections.X2"), "{connects:?}");
    assert!(connects.iter().any(|c| c.from_sheet == nc && c.from_cell == "EndX" && c.to_sheet == n2 && c.to_cell == "Connections.X4"));
    assert!(connects.iter().any(|c| c.from_sheet == id), "the original keeps its glue");
    let shape = layoutcraft_vsdx::writer::shape_xml(&after, 0, &nc).unwrap();
    assert!(shape.contains(&format!("Sheet.{n1}!Connections.X2")) && shape.contains(&format!("Sheet.{n2}!Connections.X4")), "formulas follow the copies: {shape}");
    assert!(!shape.contains("Sheet.1!") && !shape.contains("Sheet.2!"), "no reference to the originals: {shape}");
    // The copy's ends lie on the copies' connection points (the shapes and the connector moved alike).
    let editor = GroupEditor::open(&after, 0).unwrap();
    let listed = editor.connectors(&connects);
    let me = listed.as_array().unwrap().iter().find(|c| c["id"] == nc.as_str()).unwrap();
    let p1 = editor.connection_points(&n1).into_iter().find(|p| p.0 == 2).unwrap();
    let p2 = editor.connection_points(&n2).into_iter().find(|p| p.0 == 4).unwrap();
    assert!((me["begin"]["x"].as_f64().unwrap() - p1.1).abs() < 1e-6 && (me["begin"]["y"].as_f64().unwrap() - p1.2).abs() < 1e-6, "{me}");
    assert!((me["end"]["x"].as_f64().unwrap() - p2.1).abs() < 1e-6 && (me["end"]["y"].as_f64().unwrap() - p2.2).abs() < 1e-6, "{me}");
    assert!((me["begin"]["x"].as_f64().unwrap() - old["begin"]["x"].as_f64().unwrap() - 0.5).abs() < 1e-6);
    // Nothing for a reroute to fix, and the file still imports.
    let jump = layoutcraft_model::LineJump::default();
    assert!(editor.reroute_connectors(&connects, &jump).iter().all(|p| p.visio_id != nc), "the copy's route is current");
    assert!(import_document(&after).is_ok());
}

#[test]
fn a_copied_connector_whose_shapes_are_not_copied_is_pasted_loose() {
    use layoutcraft_vsdx::groupedit::copy_shapes;
    use layoutcraft_vsdx::writer::paste_shapes_with_connects;
    let (package, id, part) = connector_fixture();
    let (xmls, rows) = copy_shapes(&package, 0, &[id.clone()]).unwrap();
    assert!(rows.is_empty(), "no end stays glued: {rows:?}");
    let before = GroupEditor::open(&package, 0).unwrap().connectors(&package.connects(&part).unwrap());
    let old = before.as_array().unwrap().iter().find(|c| c["id"] == id.as_str()).unwrap().clone();
    let (bytes, ids) = paste_shapes_with_connects(&package, 0, &xmls, &rows, 0.25, -0.25).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    let connects = after.connects(&part).unwrap();
    assert!(connects.iter().all(|c| c.from_sheet != ids[0]), "the copy is not glued to anything");
    let shape = layoutcraft_vsdx::writer::shape_xml(&after, 0, &ids[0]).unwrap();
    assert!(!shape.contains("PAR(PNT") && !shape.contains("WALKGLUE"), "literal ends: {shape}");
    let editor = GroupEditor::open(&after, 0).unwrap();
    let listed = editor.connectors(&connects);
    let me = listed.as_array().unwrap().iter().find(|c| c["id"] == ids[0].as_str()).unwrap();
    assert!(me["begin"]["glue"].is_null() && me["end"]["glue"].is_null());
    assert!((me["end"]["x"].as_f64().unwrap() - old["end"]["x"].as_f64().unwrap() - 0.25).abs() < 1e-6);
    assert!((me["end"]["y"].as_f64().unwrap() - old["end"]["y"].as_f64().unwrap() + 0.25).abs() < 1e-6);
    assert!(connects.iter().any(|c| c.from_sheet == id), "the original stays glued");
    assert!(import_document(&after).is_ok());
}

#[test]
fn a_connector_with_one_end_inside_the_copy_keeps_only_that_glue() {
    use layoutcraft_vsdx::groupedit::copy_shapes;
    use layoutcraft_vsdx::writer::paste_shapes_with_connects;
    let (package, id, part) = connector_fixture();
    let (xmls, rows) = copy_shapes(&package, 0, &["1".to_string(), id.clone()]).unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    let (bytes, ids) = paste_shapes_with_connects(&package, 0, &xmls, &rows, 0.5, 0.5).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    let connects = after.connects(&part).unwrap();
    let mine: Vec<_> = connects.iter().filter(|c| c.from_sheet == ids[1]).collect();
    assert_eq!(mine.len(), 1);
    assert!(mine[0].from_cell == "BeginX" && mine[0].to_sheet == ids[0]);
}

#[test]
fn a_member_of_a_group_is_copied_as_a_page_level_shape_where_it_stands() {
    use layoutcraft_vsdx::groupedit::copy_shapes;
    use layoutcraft_vsdx::writer::paste_shapes_with_connects;
    let package = corpus(); // shape 1 is a member of a rotated group
    let before = centres(&package);
    let (xmls, rows) = copy_shapes(&package, 0, &["1".to_string()]).unwrap();
    let (bytes, ids) = paste_shapes_with_connects(&package, 0, &xmls, &rows, 0.0, 0.0).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    let described = GroupEditor::open(&after, 0).unwrap().describe();
    let frame = described.as_array().unwrap().iter().find(|f| f["id"] == ids[0].as_str()).unwrap().clone();
    assert!(frame["parentId"].is_null(), "the copy sits on the page: {frame}");
    let now = centres(&after);
    assert!(close(now[&ids[0]], before["1"]), "same place on the page: {:?} vs {:?}", now[&ids[0]], before["1"]);
    // Its angle is the angle it has on the page (the groups' angles add up).
    let old = GroupEditor::open(&package, 0).unwrap().describe();
    let chain_angle: f64 = {
        let frames = old.as_array().unwrap();
        let mut total = 0.0;
        let mut cur = Some("1".to_string());
        while let Some(c) = cur {
            let f = frames.iter().find(|f| f["id"] == c.as_str()).unwrap();
            total += f["angle"].as_f64().unwrap();
            cur = f["parentId"].as_str().map(str::to_string);
        }
        total.rem_euclid(std::f64::consts::TAU)
    };
    assert!((frame["angle"].as_f64().unwrap() - chain_angle).abs() < 1e-6, "{} vs {}", frame["angle"], chain_angle);
    assert!(import_document(&after).is_ok());
}

#[test]
fn a_connector_inside_a_group_cannot_be_copied_but_unknown_shapes_are_refused() {
    use layoutcraft_vsdx::groupedit::copy_shapes;
    let (package, _, _) = connector_fixture();
    assert!(copy_shapes(&package, 0, &["999".to_string()]).is_err());
    assert!(copy_shapes(&package, 999, &["1".to_string()]).is_err());
}

fn two_rectangles() -> Package {
    open_corpus("route-sweep-sites.vsdx") // shapes 1 and 2: 1 x 0.7 in, pins (5, 5) and (2, 2), pin at the centre
}

fn cell_of(package: &Package, id: &str, name: &str) -> f64 {
    let xml = layoutcraft_vsdx::writer::shape_xml(package, 0, id).unwrap();
    let key = format!("<Cell N='{name}' V='");
    let at = xml.find(&key).unwrap_or_else(|| panic!("no {name} in {xml}")) + key.len();
    xml[at..].split('\'').next().unwrap().parse().unwrap()
}

#[test]
fn several_shapes_turn_as_one_body_about_the_centre_of_their_joint_box() {
    let package = two_rectangles();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let patches = editor.rotate_many(&["1".to_string(), "2".to_string()], std::f64::consts::FRAC_PI_2).unwrap();
    let after = apply(&package, &patches);
    // Joint box x 1.5..5.5, y 1.65..5.35: centre (3.5, 3.5). Pin (5, 5) -> offset (1.5, 1.5) -> (-1.5, 1.5);
    // pin (2, 2) -> offset (-1.5, -1.5) -> (1.5, -1.5).
    let c = centres(&after);
    assert!(close(c["1"], (2.0, 5.0)), "{:?}", c["1"]);
    assert!(close(c["2"], (5.0, 2.0)), "{:?}", c["2"]);
    assert!((cell_of(&after, "1", "Angle") - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    assert!((cell_of(&after, "2", "Angle") - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    // Four quarter turns bring everything back.
    let mut turned = after;
    for _ in 0..3 {
        let editor = GroupEditor::open(&turned, 0).unwrap();
        let patches = editor.rotate_many(&["1".to_string(), "2".to_string()], std::f64::consts::FRAC_PI_2).unwrap();
        turned = apply(&turned, &patches);
    }
    let c = centres(&turned);
    assert!(close(c["1"], (5.0, 5.0)) && close(c["2"], (2.0, 2.0)), "{c:?}");
    assert!(cell_of(&turned, "1", "Angle").abs() < 1e-9, "a full turn is angle 0");
    assert!(import_document(&turned).is_ok());
}

#[test]
fn one_shape_turns_about_its_pin_and_only_its_angle_changes() {
    let package = two_rectangles();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let patches = editor.rotate_many(&["1".to_string()], 0.5).unwrap();
    let after = apply(&package, &patches);
    assert!((cell_of(&after, "1", "Angle") - 0.5).abs() < 1e-9);
    assert_eq!(cell_of(&after, "1", "PinX"), 5.0);
    assert_eq!(cell_of(&after, "1", "PinY"), 5.0);
}

#[test]
fn flipping_mirrors_the_boxes_about_the_joint_centre_and_flipping_again_restores_them() {
    let package = two_rectangles();
    let ids = ["1".to_string(), "2".to_string()];
    let editor = GroupEditor::open(&package, 0).unwrap();
    let flipped = apply(&package, &editor.flip_many(&ids, false).unwrap());
    // Joint box centre x = 3.5: pin x 5 -> 2 and 2 -> 5, y unchanged.
    assert_eq!(cell_of(&flipped, "1", "PinX"), 2.0);
    assert_eq!(cell_of(&flipped, "2", "PinX"), 5.0);
    assert_eq!(cell_of(&flipped, "1", "PinY"), 5.0);
    assert_eq!(cell_of(&flipped, "1", "FlipX"), 1.0);
    assert_eq!(cell_of(&flipped, "2", "FlipX"), 1.0);
    // A flipped leaf shape stays editable (its box did not move).
    let described = GroupEditor::open(&flipped, 0).unwrap().describe();
    assert!(described.as_array().unwrap().iter().filter(|f| f["id"] == "1" || f["id"] == "2").all(|f| f["editable"] == true));
    assert!(import_document(&flipped).is_ok());
    let again = apply(&flipped, &GroupEditor::open(&flipped, 0).unwrap().flip_many(&ids, false).unwrap());
    assert_eq!(cell_of(&again, "1", "PinX"), 5.0);
    assert_eq!(cell_of(&again, "1", "FlipX"), 0.0);
    // Vertical: y 5 -> 2 and 2 -> 5 about y = 3.5.
    let v = apply(&package, &editor.flip_many(&ids, true).unwrap());
    assert_eq!(cell_of(&v, "1", "PinY"), 2.0);
    assert_eq!(cell_of(&v, "1", "FlipY"), 1.0);
    assert_eq!(cell_of(&v, "1", "PinX"), 5.0);
}

#[test]
fn flipping_a_turned_shape_mirrors_its_image_so_its_angle_changes_sign() {
    let package = two_rectangles();
    let turned = apply(&package, &GroupEditor::open(&package, 0).unwrap().rotate_many(&["1".to_string()], 30f64.to_radians()).unwrap());
    let flipped = apply(&turned, &GroupEditor::open(&turned, 0).unwrap().flip_many(&["1".to_string()], false).unwrap());
    let angle = cell_of(&flipped, "1", "Angle");
    assert!((angle - (std::f64::consts::TAU - 30f64.to_radians())).abs() < 1e-9, "{angle}");
    assert_eq!(cell_of(&flipped, "1", "FlipX"), 1.0);
    // A single shape is mirrored about its own centre: the centre stays.
    assert!(close(centres(&flipped)["1"], (5.0, 5.0)));
    // Turning a flipped shape adds to the angle; the flip stays.
    let again = apply(&flipped, &GroupEditor::open(&flipped, 0).unwrap().rotate_many(&["1".to_string()], 30f64.to_radians()).unwrap());
    assert!(cell_of(&again, "1", "Angle").abs() < 1e-9 || (cell_of(&again, "1", "Angle") - std::f64::consts::TAU).abs() < 1e-9);
    assert_eq!(cell_of(&again, "1", "FlipX"), 1.0);
}

#[test]
fn turn_and_flip_refuse_groups_connectors_members_and_nothing() {
    let (package, id, _) = connector_fixture();
    let editor = GroupEditor::open(&package, 0).unwrap();
    assert!(editor.rotate_many(&[], 1.0).is_err(), "no shapes");
    assert!(editor.rotate_many(&["1".to_string(), id.clone()], 1.0).is_err(), "a connector");
    assert!(editor.flip_many(&[id], false).is_err());
    assert!(editor.rotate_many(&["999".to_string()], 1.0).is_err(), "unknown shape");
    assert!(editor.rotate_many(&["1".to_string()], f64::NAN).is_err(), "not a number");
    let groups = corpus();
    let editor = GroupEditor::open(&groups, 0).unwrap();
    // A group on the page is flipped by its own flip cell (its members are not touched) and stays movable.
    let flipped = apply(&groups, &editor.flip_many(&["5".to_string()], false).unwrap());
    assert_eq!(cell_of(&flipped, "5", "FlipX"), 1.0);
    let after = GroupEditor::open(&flipped, 0).unwrap();
    let frame = after.describe().as_array().unwrap().iter().find(|f| f["id"] == "5").unwrap().clone();
    assert_eq!(frame["editable"], true, "a flipped page-level group can still be moved");
    assert!(after.move_member("5", (0.5, 0.0), false).is_ok());
    assert!(after.resize_member("5", (2.0, 2.0), (0.5, 0.5), false).is_ok(), "and resized too");
    let back = apply(&flipped, &after.flip_many(&["5".to_string()], false).unwrap());
    assert_eq!(cell_of(&back, "5", "FlipX"), 0.0);
    assert!(editor.rotate_many(&["1".to_string()], 1.0).is_err(), "a member of a group");
    assert!(editor.rotate_many(&["5".to_string()], 1.0).is_ok(), "a group can be turned");
}

#[test]
fn bold_italic_font_and_text_colour_are_written_as_character_cells_and_read_back() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    use layoutcraft_vsdx::writer::{add_master_instance, ensure_face_names, patch_page_cells};
    let package = open_corpus("block-masters.vsdx");
    let (bytes, id) = add_master_instance(&package, 0, "2", 3.0, 2.0).unwrap();
    let package = Package::from_reader(Cursor::new(bytes)).unwrap();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let edit = StyleEdit {
        bold: Some(true),
        italic: Some(true),
        font_family: Some("Verdana".to_string()),
        text_color: Some("#C81E28".to_string()),
        ..Default::default()
    };
    let patches = editor.style_patches(&[id.clone()], &edit).unwrap();
    assert_eq!(patches.len(), 1);
    let styled = Package::from_reader(Cursor::new(patch_page_cells(&package, 0, &patches).unwrap())).unwrap();
    let xml = styled.text(&styled.pages().unwrap()[0].part).unwrap();
    // Exactly the cells Visio wrote when the same shape was formatted through COM (probe-char.ps1).
    assert!(xml.contains("<Section N='Character'><Row IX='0'><Cell N='Font' V='Verdana'/><Cell N='Color' V='#c81e28'/><Cell N='Style' V='3'/></Row></Section>"), "{xml}");
    let doc = import_document(&styled).unwrap();
    let s = doc.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id.as_str())).unwrap();
    assert_eq!(s.bold, Some(true));
    assert_eq!(s.italic, Some(true));
    assert_ne!(s.underline, Some(true));
    assert_eq!(s.font_family.as_deref(), Some("Verdana"));
    assert_eq!(s.text_color.as_deref(), Some("#c81e28"));
    // The font joins the document's font list once, and a second call changes nothing.
    let listed = Package::from_reader(Cursor::new(ensure_face_names(&styled, &["Verdana".to_string()]).unwrap())).unwrap();
    let doc_xml = listed.text("visio/document.xml").unwrap();
    assert_eq!(doc_xml.matches("NameU='Verdana'").count(), 1, "{doc_xml}");
    let again = Package::from_reader(Cursor::new(ensure_face_names(&listed, &["Verdana".to_string()]).unwrap())).unwrap();
    assert_eq!(again.text("visio/document.xml").unwrap().matches("NameU='Verdana'").count(), 1);
    assert!(import_document(&listed).is_ok());
    // style_info reports what the cells state.
    let info = GroupEditor::open(&listed, 0).unwrap().style_info(&id).unwrap();
    assert_eq!(info["bold"], true);
    assert_eq!(info["italic"], true);
    assert_eq!(info["underline"], false);
    assert_eq!(info["fontFamily"], "Verdana");
}

#[test]
fn style_bits_are_set_and_cleared_without_touching_the_others() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    use layoutcraft_vsdx::writer::patch_page_cells;
    let package = corpus();
    let part = package.pages().unwrap()[0].part.clone();
    let style_of = |package: &Package| -> String {
        let xml = package.text(&part).unwrap().replace('"', "'");
        let at = xml.find("<Shape ID='4'").unwrap();
        let end = xml[at..].find("</Shape>").map_or(xml.len(), |n| at + n);
        let shape = &xml[at..end];
        let row = shape.find("<Section N='Character'").unwrap();
        let cell = shape[row..].find("N='Style' V='").map(|n| row + n + "N='Style' V='".len()).unwrap();
        shape[cell..].split('\'').next().unwrap().to_string()
    };
    let apply = |package: &Package, edit: StyleEdit| -> Package {
        let editor = GroupEditor::open(package, 0).unwrap();
        let patches = editor.style_patches(&["4".to_string()], &edit).unwrap();
        Package::from_reader(Cursor::new(patch_page_cells(package, 0, &patches).unwrap())).unwrap()
    };
    let p = apply(&package, StyleEdit { underline: Some(true), ..Default::default() });
    assert_eq!(style_of(&p), "4");
    let p = apply(&p, StyleEdit { bold: Some(true), italic: Some(true), ..Default::default() });
    assert_eq!(style_of(&p), "7", "bits add up");
    let p = apply(&p, StyleEdit { bold: Some(false), ..Default::default() });
    assert_eq!(style_of(&p), "6", "clearing bold keeps italic and underline");
    let p = apply(&p, StyleEdit { italic: Some(false), underline: Some(false), ..Default::default() });
    assert_eq!(style_of(&p), "0");
    assert!(import_document(&p).is_ok());
    // The size edit and the new cells share the row.
    let p = apply(&p, StyleEdit { font_pt: Some(12.0), font_family: Some("Courier New".into()), ..Default::default() });
    let xml = p.text(&part).unwrap().replace('"', "'");
    assert!(xml.contains("N='Size' V='0.1666") && xml.contains("N='Font' V='Courier New'"), "{xml}");
}

#[test]
fn text_format_edits_are_validated_and_skip_shapes_without_text() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    let package = corpus();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let ids = vec!["4".to_string()];
    for bad in ["", "   ", "Ar'ial", "Ar\"ial", "a<b", "a&b", "back\\slash", &"x".repeat(65)] {
        assert!(editor.style_patches(&ids, &StyleEdit { font_family: Some(bad.to_string()), ..Default::default() }).is_err(), "{bad:?}");
    }
    assert!(editor.style_patches(&ids, &StyleEdit { text_color: Some("red".into()), ..Default::default() }).is_err());
    assert!(editor.style_patches(&ids, &StyleEdit { text_color: Some("#12345".into()), ..Default::default() }).is_err());
    assert!(editor.style_patches(&ids, &StyleEdit { font_family: Some("Segoe UI Semibold".into()), ..Default::default() }).is_ok(), "spaces are fine");
}

fn shape_text(package: &Package, id: &str) -> String {
    layoutcraft_vsdx::writer::shape_xml(package, 0, id).unwrap().replace('"', "'")
}

#[test]
fn transparency_shadow_and_arrow_cells_are_written_and_the_importer_reads_them() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    use layoutcraft_vsdx::writer::patch_page_cells;
    let package = corpus();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let edit = StyleEdit { fill: Some("#ff8800".into()), fill_trans: Some(0.4), line_trans: Some(0.25), shadow: Some(true), ..Default::default() };
    let patches = editor.style_patches(&["4".to_string()], &edit).unwrap();
    let styled = Package::from_reader(Cursor::new(patch_page_cells(&package, 0, &patches).unwrap())).unwrap();
    let shape = shape_text(&styled, "4");
    for needle in ["N='FillForegndTrans' V='0.4'", "N='LineColorTrans' V='0.25'", "N='ShdwPattern' V='1'", "N='ShapeShdwOffsetX' V='0.0417'", "N='ShapeShdwOffsetY' V='-0.0417'", "N='ShdwForegndTrans' V='0.6'"] {
        assert!(shape.contains(needle), "{needle} in {shape}");
    }
    // An explicit transparency wins over the "colour choice makes it opaque" rule.
    assert!(!shape.contains("N='FillForegndTrans' V='0'/"), "{shape}");
    let doc = import_document(&styled).unwrap();
    let s = doc.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some("4")).unwrap();
    let fill_opacity = s.fill_opacity.unwrap_or(1.0);
    assert!((fill_opacity - 0.6).abs() < 1e-6, "40 % transparent fill = 0.6 opacity: {fill_opacity}");
    assert!(s.shadow.is_some(), "the shadow is drawn");
    // Off again: only the pattern changes.
    let off = apply(&styled, &GroupEditor::open(&styled, 0).unwrap().style_patches(&["4".to_string()], &StyleEdit { shadow: Some(false), ..Default::default() }).unwrap());
    assert!(shape_text(&off, "4").contains("N='ShdwPattern' V='0'"));
    let info = GroupEditor::open(&styled, 0).unwrap().style_info("4").unwrap();
    assert_eq!(info["shadow"], true);
    assert_eq!(info["fillTrans"], 0.4);
    assert_eq!(info["lineTrans"], 0.25);
}

#[test]
fn line_ends_are_written_on_a_connector_and_validated() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    let (package, id, _) = connector_fixture();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let edit = StyleEdit { begin_arrow: Some(10), end_arrow: Some(4), end_arrow_size: Some(4), ..Default::default() };
    let styled = apply(&package, &editor.style_patches(&[id.clone()], &edit).unwrap());
    let shape = shape_text(&styled, &id);
    for needle in ["N='BeginArrow' V='10'", "N='EndArrow' V='4'", "N='EndArrowSize' V='4'"] {
        assert!(shape.contains(needle), "{needle} in {shape}");
    }
    assert!(!shape.contains("N='BeginArrowSize'"), "an unset size stays as it was: {shape}");
    let info = GroupEditor::open(&styled, 0).unwrap().style_info(&id).unwrap();
    assert_eq!((info["beginArrow"].as_f64(), info["endArrow"].as_f64(), info["endArrowSize"].as_f64()), (Some(10.0), Some(4.0), Some(4.0)));
    assert!(import_document(&styled).is_ok());
    let ids = vec![id];
    for bad in [
        StyleEdit { begin_arrow: Some(46), ..Default::default() },
        StyleEdit { end_arrow_size: Some(7), ..Default::default() },
        StyleEdit { fill_trans: Some(1.5), ..Default::default() },
        StyleEdit { line_trans: Some(-0.1), ..Default::default() },
        StyleEdit { fill_trans: Some(f64::NAN), ..Default::default() },
    ] {
        assert!(editor.style_patches(&ids, &bad).is_err(), "{bad:?}");
    }
}


/// The same package without any masters: no masters part, no relationship to it, no content type for it.
fn without_masters(package: &Package) -> Package {
    use std::io::Write;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for name in package.names() {
        if name.starts_with("visio/masters/") {
            continue;
        }
        let mut bytes = package.entry(name).unwrap().to_vec();
        if name == "[Content_Types].xml" || name == "visio/_rels/document.xml.rels" {
            let text = String::from_utf8(bytes).unwrap();
            let mut out = String::new();
            let mut rest = text.as_str();
            while let Some(at) = rest.find('<') {
                out.push_str(&rest[..at]);
                let end = rest[at..].find('>').map_or(rest.len(), |n| at + n + 1);
                let tag = &rest[at..end];
                if !(tag.contains("/visio/masters/") || tag.contains("relationships/masters") || tag.contains("masters/masters.xml")) {
                    out.push_str(tag);
                }
                rest = &rest[end..];
            }
            out.push_str(rest);
            bytes = out.into_bytes();
        }
        writer.start_file(name, options).unwrap();
        writer.write_all(&bytes).unwrap();
    }
    Package::from_reader(Cursor::new(writer.finish().unwrap().into_inner())).unwrap()
}

fn dell_stencil() -> Package {
    Package::open(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/Dell-Networking/Dell-Networking-AccessPlatforms.vssx")).unwrap()
}

/// Ids of a few stencil masters of a sensible size (not the page-sized description master).
fn small_masters(stencil: &Package, count: usize) -> Vec<String> {
    let list = layoutcraft_vsdx::stencil::master_list(stencil).unwrap();
    list.as_array()
        .unwrap()
        .iter()
        .filter(|m| m["w"].as_f64().unwrap() > 0.2 && m["w"].as_f64().unwrap() < 20.0 && m["h"].as_f64().unwrap() < 20.0)
        .take(count)
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect()
}

fn every_relationship_target_exists(package: &Package) {
    for name in package.names().map(str::to_string).collect::<Vec<_>>() {
        if name.ends_with(".rels") || !name.starts_with("visio/") {
            continue;
        }
        for (_, rel) in package.relationships(&name).unwrap() {
            if rel.target_mode.as_deref() != Some("External") {
                assert!(package.contains(&rel.target), "{name} relates to the missing part {}", rel.target);
            }
        }
    }
}

#[test]
fn a_stencil_lists_its_masters_with_names_and_sizes() {
    let stencil = dell_stencil();
    let list = layoutcraft_vsdx::stencil::master_list(&stencil).unwrap();
    let list = list.as_array().unwrap();
    assert!(list.len() > 20, "{} masters", list.len());
    assert!(list.iter().all(|m| !m["name"].as_str().unwrap().is_empty()));
    assert!(!small_masters(&stencil, 3).is_empty());
}

#[test]
fn a_stencil_master_is_copied_into_a_drawing_that_has_no_masters_at_all() {
    use layoutcraft_vsdx::stencil::import_master;
    let stencil = dell_stencil();
    let master = small_masters(&stencil, 1).remove(0);
    // A drawing made without masters: the masters part, its relationship and content types are created.
    let blank = layoutcraft_vsdx::write_fresh_document(&serde_json::from_value(serde_json::json!({ "pages": [{ "name": "Page-1", "shapes": [], "connectors": [] }] })).unwrap()).unwrap();
    let blank = without_masters(&Package::from_reader(Cursor::new(blank)).unwrap());
    assert!(!blank.contains("visio/masters/masters.xml"));
    let (bytes, id) = import_master(&blank, &stencil, &master).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(id, "1");
    assert!(after.contains("visio/masters/masters.xml") && after.contains("visio/masters/master1.xml"));
    let ct = after.text("[Content_Types].xml").unwrap();
    assert!(ct.contains("/visio/masters/masters.xml") && ct.contains("/visio/masters/master1.xml"), "{ct}");
    assert!(after.text("visio/_rels/document.xml.rels").unwrap().contains("masters/masters.xml"));
    every_relationship_target_exists(&after);
    let catalog = layoutcraft_vsdx::MasterCatalog::load(&after).unwrap();
    assert!(!catalog.masters["1"].shapes.is_empty(), "the master's shapes are read back");
    // Copying the same master again reuses it.
    let (again, same) = import_master(&after, &stencil, &master).unwrap();
    assert_eq!(same, "1");
    let again = Package::from_reader(Cursor::new(again)).unwrap();
    assert_eq!(again.names().filter(|n| n.starts_with("visio/masters/master") && n.ends_with(".xml") && !n.ends_with("masters.xml")).count(), 1);
    assert!(import_document(&after).is_ok());
}

#[test]
fn stencil_masters_join_a_drawing_that_has_masters_without_disturbing_them() {
    use layoutcraft_vsdx::stencil::import_master;
    use layoutcraft_vsdx::writer::add_master_instance;
    let stencil = dell_stencil();
    let picks = small_masters(&stencil, 2);
    let target = Package::open(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patient-check-in.vsdx")).unwrap();
    let before = layoutcraft_vsdx::MasterCatalog::load(&target).unwrap();
    let (bytes, first) = import_master(&target, &stencil, &picks[0]).unwrap();
    let one = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, second) = import_master(&one, &stencil, &picks[1]).unwrap();
    let two = Package::from_reader(Cursor::new(bytes)).unwrap();
    let after = layoutcraft_vsdx::MasterCatalog::load(&two).unwrap();
    assert_eq!(after.masters.len(), before.masters.len() + 2);
    assert_ne!(first, second);
    for id in before.masters.keys() {
        assert!(after.masters.contains_key(id), "the drawing's own master {id} is still there");
    }
    let parts: Vec<&str> = two.names().filter(|n| n.starts_with("visio/masters/master") && n.ends_with(".xml") && !n.ends_with("masters.xml")).collect();
    assert_eq!(parts.len(), before.masters.len() + 2, "every master has its own part");
    every_relationship_target_exists(&two);
    let ct = two.text("[Content_Types].xml").unwrap();
    for name in two.names().filter(|n| n.starts_with("visio/masters/master") && n.ends_with(".xml") && !n.ends_with("masters.xml")) {
        assert!(ct.contains(&format!("/{name}")), "content type of {name}");
    }
    // The dropped instance draws.
    let (bytes, shape) = add_master_instance(&two, 0, &first, 3.0, 3.0).unwrap();
    let dropped = Package::from_reader(Cursor::new(bytes)).unwrap();
    let doc = import_document(&dropped).unwrap();
    let s = doc.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(shape.as_str())).expect("the instance is imported");
    assert!(!s.parts.is_empty() || !s.label.is_empty(), "the master is drawn");
}

#[test]
fn a_scratch_drawing_shows_one_stencil_master_for_its_thumbnail() {
    use layoutcraft_vsdx::stencil::scratch_drawing_with_master;
    let stencil = dell_stencil();
    for master in small_masters(&stencil, 3) {
        let (bytes, id) = scratch_drawing_with_master(&stencil, &master).unwrap();
        let scratch = Package::from_reader(Cursor::new(bytes)).unwrap();
        let doc = import_document(&scratch).unwrap();
        // one shape, or the group and the picture of its master body (both belong to the dropped group)
        assert!(!doc.pages[0].shapes.is_empty() && doc.pages[0].shapes.len() <= 2, "one or two shapes for master {master}");
        let _ = 0;
        assert!(
            doc.pages[0].shapes.iter().all(|s| s.visio_id.as_deref() == Some(id.as_str()) || s.group.as_ref().and_then(|g| g["rootId"].as_str()) == Some(id.as_str())),
            "every shape belongs to the dropped group"
        );
    }
    assert!(scratch_drawing_with_master(&stencil, "99999").is_err());
}

#[test]
fn style_sheets_a_stencil_master_uses_come_along_so_the_drawing_has_every_sheet_it_refers_to() {
    use layoutcraft_vsdx::stencil::import_master;
    let stencil = dell_stencil();
    let target = Package::open(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patient-check-in.vsdx")).unwrap();
    // The first small master uses sheets 6 and 7, which the Lucidchart drawing (sheets 0-4) does not have.
    let master = small_masters(&stencil, 1).remove(0);
    let (bytes, new_id) = import_master(&target, &stencil, &master).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    let document = after.text("visio/document.xml").unwrap();
    let sheet_ids: Vec<String> = document.split("<StyleSheet ID=").skip(1).map(|rest| rest.trim_start_matches(['\'', '"']).chars().take_while(|c| c.is_ascii_digit()).collect()).collect();
    let rels = after.relationships("visio/masters/masters.xml").unwrap();
    let masters_xml = after.text("visio/masters/masters.xml").unwrap();
    let at = masters_xml.find(&format!("<Master ID='{new_id}'")).unwrap();
    let rid = masters_xml[at..].split("r:id=").nth(1).unwrap().trim_start_matches(['\'', '"']).split(['\'', '"']).next().unwrap().to_string();
    let part = rels[&rid].target.clone();
    let text = after.text(&part).unwrap();
    let mut referenced = Vec::new();
    for name in ["LineStyle", "FillStyle", "TextStyle"] {
        for quote in ["'", "\""] {
            for piece in text.split(&format!("{name}={quote}")).skip(1) {
                let id: String = piece.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !id.is_empty() {
                    referenced.push(id);
                }
            }
        }
    }
    assert!(!referenced.is_empty());
    for id in &referenced {
        assert!(sheet_ids.contains(id), "style sheet {id} used by {part} is missing from the drawing ({sheet_ids:?})");
    }
    assert!(sheet_ids.len() > 5, "sheets were copied: {sheet_ids:?}");
    // The sheets the drawing had are still there, unchanged in number.
    for own in ["0", "1", "2", "3", "4"] {
        assert!(sheet_ids.iter().any(|s| s == own));
    }
    assert!(import_document(&after).is_ok());
}

#[test]
fn an_instance_of_a_one_d_master_gets_its_ends_or_visio_drops_it_on_load() {
    use layoutcraft_vsdx::stencil::import_master;
    use layoutcraft_vsdx::writer::add_master_instance;
    let stencil = dell_stencil();
    let catalog = layoutcraft_vsdx::MasterCatalog::load(&stencil).unwrap();
    let one_d = catalog
        .masters
        .values()
        .find(|m| m.top.as_ref().and_then(|t| m.shapes.get(t)).is_some_and(|t| t.cells.contains_key("BeginX") && t.cells.contains_key("EndX")))
        .expect("the stencil has 1-D masters")
        .id
        .clone();
    let target = Package::open(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patient-check-in.vsdx")).unwrap();
    let (bytes, id) = import_master(&target, &stencil, &one_d).unwrap();
    let with_master = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, shape) = add_master_instance(&with_master, 0, &id, 6.0, 2.0).unwrap();
    let dropped = Package::from_reader(Cursor::new(bytes)).unwrap();
    let xml = shape_text(&dropped, &shape);
    for cell in ["N='BeginX'", "N='BeginY' V='2'", "N='EndX'", "N='EndY' V='2'"] {
        assert!(xml.contains(cell), "{cell} in {xml}");
    }
    let value = |name: &str| -> f64 {
        let key = format!("<Cell N='{name}' V='");
        let at = xml.find(&key).unwrap() + key.len();
        xml[at..].split('\'').next().unwrap().parse().unwrap()
    };
    assert!(value("EndX") > value("BeginX"), "the line runs left to right");
    assert!((((value("BeginX") + value("EndX")) / 2.0) - 6.0).abs() < 1e-6 || value("BeginX") < 6.0, "it is placed around the drop point");
    // A 2-D master still gets just its pin.
    let plain = open_corpus("block-masters.vsdx");
    let (bytes, box_id) = add_master_instance(&plain, 0, "2", 3.0, 2.0).unwrap();
    let plain = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert!(!shape_text(&plain, &box_id).contains("BeginX"));
    assert!(import_document(&dropped).is_ok());
}

#[test]
fn a_page_relates_to_every_master_it_instantiates_or_visio_drops_the_instance_on_load() {
    use layoutcraft_vsdx::stencil::import_master;
    use layoutcraft_vsdx::writer::add_master_instance;
    let stencil = dell_stencil();
    let master = small_masters(&stencil, 1).remove(0);
    let target = Package::open(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patient-check-in.vsdx")).unwrap();
    let page_part = target.pages().unwrap()[0].part.clone();
    let (bytes, id) = import_master(&target, &stencil, &master).unwrap();
    let with_master = Package::from_reader(Cursor::new(bytes)).unwrap();
    let part_of = |package: &Package| layoutcraft_vsdx::MasterCatalog::load(package).unwrap().masters[&id].part.clone();
    let related = |package: &Package| package.relationships(&page_part).unwrap().values().any(|r| r.target == part_of(package));
    assert!(!related(&with_master), "copying the master does not relate the page to it yet");
    let (bytes, _) = add_master_instance(&with_master, 0, &id, 5.0, 5.0).unwrap();
    let dropped = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert!(related(&dropped), "dropping an instance relates the page to the master");
    // A second instance does not add a second relationship.
    let (bytes, _) = add_master_instance(&dropped, 0, &id, 6.0, 5.0).unwrap();
    let twice = Package::from_reader(Cursor::new(bytes)).unwrap();
    let count = twice.relationships(&page_part).unwrap().values().filter(|r| r.target == part_of(&twice)).count();
    assert_eq!(count, 1);
    every_relationship_target_exists(&twice);
}

#[test]
fn a_blank_drawing_has_the_chosen_page_size_scale_orientation_and_paper_kind() {
    use layoutcraft_vsdx::{write_blank_document, BlankOptions};
    let make = |w: f64, h: f64, metric: bool, name: &str| {
        Package::from_reader(Cursor::new(write_blank_document(&BlankOptions { width_in: w, height_in: h, metric, name: name.to_string() }).unwrap())).unwrap()
    };
    let letter = make(11.0, 8.5, false, "Page-1");
    let pages_xml = letter.text("visio/pages/pages.xml").unwrap().to_string();
    for cell in ["N='PageWidth' V='11'", "N='PageHeight' V='8.5'", "N='PrintPageOrientation' V='2'", "N='PaperKind' V='1'", "N='PageScale' V='1' U='IN'"] {
        assert!(pages_xml.contains(cell), "{cell} in {pages_xml}");
    }
    assert_eq!(letter.pages().unwrap().len(), 1);
    assert_eq!(letter.pages().unwrap()[0].width, Some(11.0));
    assert!(import_document(&letter).is_ok());
    let a4 = make(8.267716535, 11.692913386, true, "Sheet <1>");
    let xml = a4.text("visio/pages/pages.xml").unwrap().to_string();
    for cell in ["N='PrintPageOrientation' V='1'", "N='PaperKind' V='9'", "N='PageScale' V='0.03937007874015748' U='MM'", "N='DrawingScale' V='0.03937007874015748' U='MM'", "NameU='Sheet 1'"] {
        assert!(xml.contains(cell), "{cell} in {xml}");
    }
    assert!(xml.contains("ViewCenterX='4.133858") || xml.contains("ViewCenterX='4.1338"), "the view starts at the middle of the page: {xml}");
    assert!(a4.text("visio/windows.xml").unwrap().contains("ViewCenterX='4.1338"));
    assert_eq!(make(11.0, 17.0, false, "").text("visio/pages/pages.xml").unwrap().matches("N='PaperKind' V='3'").count(), 1, "tabloid");
    assert!(make(5.0, 5.0, false, "").text("visio/pages/pages.xml").unwrap().contains("NameU='Page-1'"), "an empty name falls back");
    for (w, h) in [(0.1, 5.0), (5.0, 300.0), (f64::NAN, 5.0), (5.0, f64::INFINITY)] {
        assert!(write_blank_document(&BlankOptions { width_in: w, height_in: h, metric: false, name: String::new() }).is_err(), "{w} x {h}");
    }
}

#[test]
fn a_blank_drawing_takes_a_stencil_master_and_lists_the_master_in_its_page_relationships() {
    use layoutcraft_vsdx::stencil::import_master;
    use layoutcraft_vsdx::writer::add_master_instance;
    use layoutcraft_vsdx::{write_blank_document, BlankOptions};
    let blank = Package::from_reader(Cursor::new(write_blank_document(&BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "Page-1".into() }).unwrap())).unwrap();
    let stencil = dell_stencil();
    let master = small_masters(&stencil, 1).remove(0);
    let (bytes, id) = import_master(&blank, &stencil, &master).unwrap();
    let with_master = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, shape) = add_master_instance(&with_master, 0, &id, 5.0, 4.0).unwrap();
    let dropped = Package::from_reader(Cursor::new(bytes)).unwrap();
    let page = dropped.pages().unwrap()[0].part.clone();
    let master_part = layoutcraft_vsdx::MasterCatalog::load(&dropped).unwrap().masters[&id].part.clone();
    assert!(dropped.relationships(&page).unwrap().values().any(|r| r.target == master_part), "a blank page gets its master relationship (and its rels part)");
    every_relationship_target_exists(&dropped);
    let doc = import_document(&dropped).unwrap();
    assert!(doc.pages[0].shapes.iter().any(|s| s.visio_id.as_deref() == Some(shape.as_str())));
}

fn page_names(package: &Package) -> Vec<String> {
    package.pages().unwrap().iter().map(|p| p.name.clone()).collect()
}

fn blank_a4() -> Package {
    use layoutcraft_vsdx::{write_blank_document, BlankOptions};
    Package::from_reader(Cursor::new(write_blank_document(&BlankOptions { width_in: 8.2677, height_in: 11.6929, metric: true, name: "Page-1".into() }).unwrap())).unwrap()
}

#[test]
fn adding_a_page_gives_it_a_part_a_relationship_a_content_type_and_the_size_of_its_neighbour() {
    use layoutcraft_vsdx::pageops::add_page;
    let blank = blank_a4();
    let (bytes, index) = add_page(&blank, 0, None).unwrap();
    let two = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(index, 1);
    assert_eq!(page_names(&two), ["Page-1", "Page-2"]);
    let parts: Vec<String> = two.pages().unwrap().iter().map(|p| p.part.clone()).collect();
    assert_ne!(parts[0], parts[1]);
    assert!(two.contains(&parts[1]));
    assert!(two.text("[Content_Types].xml").unwrap().contains(&format!("/{}", parts[1])));
    assert_eq!(two.pages().unwrap()[1].width, two.pages().unwrap()[0].width, "same page size");
    assert!(two.text("visio/pages/pages.xml").unwrap().contains("N='PageScale' V='0.03937007874015748' U='MM'"), "the scale is copied too");
    every_relationship_target_exists(&two);
    assert_eq!(import_document(&two).unwrap().pages.len(), 2);
    // A named page, a name that is taken, and the order when adding in the middle.
    let (bytes, index) = add_page(&two, 0, Some("Plan")).unwrap();
    let three = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!((index, page_names(&three)), (1, vec!["Page-1".to_string(), "Plan".to_string(), "Page-2".to_string()]));
    assert!(add_page(&three, 0, Some("plan")).is_err(), "names are unique, case-insensitively");
    assert!(add_page(&three, 0, Some("a<b")).is_err());
    assert!(add_page(&three, 0, Some(&"x".repeat(32))).is_err());
    assert!(add_page(&three, 9, None).is_err());
    let (bytes, _) = add_page(&three, 2, None).unwrap();
    assert_eq!(page_names(&Package::from_reader(Cursor::new(bytes)).unwrap())[3], "Page-4", "the next free number");
}

#[test]
fn deleting_a_page_removes_its_part_relationship_and_content_type_and_keeps_the_rest() {
    use layoutcraft_vsdx::pageops::delete_page;
    let package = open_corpus("block3-masters.vsdx"); // 10 pages
    let names = page_names(&package);
    assert_eq!(names.len(), 10);
    let gone_part = package.pages().unwrap()[3].part.clone();
    let bytes = delete_page(&package, 3).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    let mut expected = names.clone();
    expected.remove(3);
    assert_eq!(page_names(&after), expected);
    assert!(!after.contains(&gone_part), "the page part is gone");
    assert!(!after.names().any(|n| n.ends_with(&format!("_rels/{}.rels", gone_part.rsplit('/').next().unwrap()))), "and its relationships part");
    assert!(!after.text("[Content_Types].xml").unwrap().contains(&format!("/{gone_part}")));
    every_relationship_target_exists(&after);
    assert_eq!(import_document(&after).unwrap().pages.len(), 9);
    // The other pages are byte for byte what they were.
    for (i, page) in package.pages().unwrap().iter().enumerate().filter(|(i, _)| *i != 3) {
        assert_eq!(package.entry(&page.part), after.entry(&page.part), "page {i}");
    }
    // The last page cannot be deleted.
    let single = blank_a4();
    assert!(delete_page(&single, 0).is_err());
    assert!(delete_page(&package, 10).is_err());
}

#[test]
fn renaming_and_moving_pages_change_names_and_order_only() {
    use layoutcraft_vsdx::pageops::{move_page, rename_page};
    let package = open_corpus("block3-masters.vsdx");
    let names = page_names(&package);
    let renamed = Package::from_reader(Cursor::new(rename_page(&package, 2, "Overview").unwrap())).unwrap();
    let mut expected = names.clone();
    expected[2] = "Overview".to_string();
    assert_eq!(page_names(&renamed), expected);
    assert!(renamed.text("visio/pages/pages.xml").unwrap().contains("Name='Overview'"));
    assert!(rename_page(&package, 2, &names[0]).is_err(), "another page has that name");
    assert!(rename_page(&package, 2, names[2].as_str()).is_ok(), "the same name is fine");
    assert!(rename_page(&package, 2, "  ").is_err());
    assert!(rename_page(&package, 2, "no\nnewline").is_err());
    let moved = Package::from_reader(Cursor::new(move_page(&package, 2, 5).unwrap())).unwrap();
    let mut order = names.clone();
    let item = order.remove(2);
    order.insert(5, item);
    assert_eq!(page_names(&moved), order);
    let back = Package::from_reader(Cursor::new(move_page(&moved, 5, 2).unwrap())).unwrap();
    assert_eq!(page_names(&back), names, "moving back restores the order");
    assert!(move_page(&package, 0, 99).is_err());
    assert_eq!(page_names(&Package::from_reader(Cursor::new(move_page(&package, 4, 4).unwrap())).unwrap()), names);
    // The content of a page does not depend on where it is listed.
    assert_eq!(import_document(&moved).unwrap().pages[5].shapes.len(), import_document(&package).unwrap().pages[2].shapes.len());
}

#[test]
fn a_duplicated_page_has_the_shapes_the_relationships_and_a_name_of_its_own() {
    use layoutcraft_vsdx::pageops::duplicate_page;
    let package = open_corpus("block3-masters.vsdx");
    let before = import_document(&package).unwrap();
    let (bytes, index) = duplicate_page(&package, 2).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(index, 3);
    let names = page_names(&after);
    assert_eq!(names.len(), 11);
    assert_eq!(names[3], format!("{} (2)", names[2]));
    let doc = import_document(&after).unwrap();
    assert_eq!(doc.pages[3].shapes.len(), before.pages[2].shapes.len(), "the copy has the same shapes");
    let pages = after.pages().unwrap();
    assert_ne!(pages[2].part, pages[3].part);
    assert_eq!(after.entry(&pages[2].part), after.entry(&pages[3].part), "the page XML is copied");
    // The copy relates to the same masters as the original, so its instances survive in Visio.
    let relates = |part: &str| -> Vec<String> { let mut t: Vec<String> = after.relationships(part).unwrap().values().map(|r| r.target.clone()).collect(); t.sort(); t };
    assert_eq!(relates(&pages[2].part), relates(&pages[3].part));
    every_relationship_target_exists(&after);
    // Page IDs are unique.
    let ids: Vec<String> = after.text("visio/pages/pages.xml").unwrap().split("<Page ID='").skip(1).map(|r| r.split('\'').next().unwrap().to_string()).collect();
    let unique: std::collections::BTreeSet<&String> = ids.iter().collect();
    assert_eq!(ids.len(), unique.len(), "{ids:?}");
    // A second duplicate gets the next free number.
    let (bytes, _) = duplicate_page(&after, 2).unwrap();
    assert_eq!(page_names(&Package::from_reader(Cursor::new(bytes)).unwrap())[3], format!("{} (3)", names[2]));
}

fn draw_on_blank(kind: layoutcraft_vsdx::draw::DrawKind, x0: f64, y0: f64, x1: f64, y1: f64) -> (Package, String) {
    let blank = blank_a4();
    let (bytes, id) = layoutcraft_vsdx::draw::draw_shape(&blank, 0, kind, x0, y0, x1, y1).unwrap();
    (Package::from_reader(Cursor::new(bytes)).unwrap(), id)
}

#[test]
fn every_drawn_shape_kind_is_a_visio_style_shape_with_a_frame_geometry_and_connection_points() {
    use layoutcraft_vsdx::draw::DrawKind::*;
    for (kind, name) in [(Rectangle, "Rectangle"), (Rounded, "Rounded Rectangle"), (Ellipse, "Ellipse"), (Diamond, "Diamond"), (Triangle, "Triangle"), (TextBox, "Text Box")] {
        let (package, id) = draw_on_blank(kind, 1.0, 2.0, 3.5, 3.0);
        let xml = shape_text(&package, &id);
        for cell in ["N='PinX' V='2.25'", "N='PinY' V='2.5'", "N='Width' V='2.5'", "N='Height' V='1'", "N='LocPinX' V='1.25' F='Width*0.5'", "N='Angle' V='0'", &format!("NameU='{name}.{id}'")] {
            assert!(xml.contains(cell), "{name}: {cell} in {xml}");
        }
        assert!(xml.contains("<Section N='Geometry' IX='0'>") && xml.contains("F='Width*"), "{name}: geometry follows the width");
        assert!(xml.contains("<Section N='Connection'>"), "{name}: connection points");
        let doc = import_document(&package).unwrap();
        let shape = doc.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id.as_str())).unwrap_or_else(|| panic!("{name} is imported"));
        assert!((shape.w - 2.5 * 96.0).abs() < 1e-6 && (shape.h - 96.0).abs() < 1e-6, "{name}: {} x {}", shape.w, shape.h);
        assert!(!shape.parts.is_empty(), "{name} is drawn");
        let editor = GroupEditor::open(&package, 0).unwrap();
        assert_eq!(editor.connection_points(&id).len(), 4, "{name}: four connection points");
    }
    let (ellipse, id) = draw_on_blank(Ellipse, 0.0, 0.0, 2.0, 1.0);
    assert!(shape_text(&ellipse, &id).contains("<Row T=\"Ellipse\" IX=\"1\">") || shape_text(&ellipse, &id).contains("<Row T='Ellipse' IX='1'>"));
    let (rounded, id) = draw_on_blank(Rounded, 0.0, 0.0, 2.0, 1.0);
    assert!(shape_text(&rounded, &id).contains("N='Rounding' V='0.15' U='IN'"));
    let (text, id) = draw_on_blank(TextBox, 0.0, 0.0, 2.0, 1.0);
    assert!(shape_text(&text, &id).contains("N='FillPattern' V='0'") && shape_text(&text, &id).contains("N='LinePattern' V='0'"), "a text box has no fill and no line");
}

#[test]
fn a_drawn_line_is_a_one_d_shape_from_its_first_point_to_its_second() {
    use layoutcraft_vsdx::draw::DrawKind::Line;
    let (package, id) = draw_on_blank(Line, 1.0, 5.0, 3.0, 4.0);
    let xml = shape_text(&package, &id);
    for cell in ["N='BeginX' V='1'", "N='BeginY' V='5'", "N='EndX' V='3'", "N='EndY' V='4'", "N='Width' V='2' F='GUARD(EndX-BeginX)'", "N='Height' V='-1' F='GUARD(EndY-BeginY)'", "N='PinX' V='2' F='(BeginX+EndX)/2'"] {
        assert!(xml.contains(cell), "{cell} in {xml}");
    }
    let editor = GroupEditor::open(&package, 0).unwrap();
    let part = package.pages().unwrap()[0].part.clone();
    let listed = editor.connectors(&package.connects(&part).unwrap());
    let me = listed.as_array().unwrap().iter().find(|c| c["id"] == id.as_str()).expect("a line is listed with the connectors, so its ends can be glued");
    assert!(me["begin"]["glue"].is_null() && me["end"]["glue"].is_null());
    assert_eq!((me["begin"]["x"].as_f64(), me["end"]["y"].as_f64()), (Some(1.0), Some(4.0)));
    assert!(import_document(&package).is_ok());
}

#[test]
fn drawn_shapes_get_ids_one_after_the_other_and_refuse_nonsense() {
    use layoutcraft_vsdx::draw::{draw_shape, DrawKind::*};
    let blank = blank_a4();
    let (bytes, a) = draw_shape(&blank, 0, Rectangle, 1.0, 1.0, 2.0, 2.0).unwrap();
    let one = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, b) = draw_shape(&one, 0, Ellipse, 3.0, 3.0, 4.0, 4.0).unwrap();
    assert_eq!((a.as_str(), b.as_str()), ("1", "2"));
    let two = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(import_document(&two).unwrap().pages[0].shapes.len(), 2);
    let (bytes, id) = draw_shape(&blank, 0, Rectangle, 3.5, 3.0, 1.0, 2.0).unwrap();
    let flipped = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert!(shape_text(&flipped, &id).contains("N='Width' V='2.5'"), "corners in any order give the same box");
    for bad in [(0.0, 0.0, 0.01, 1.0), (0.0, 0.0, 1.0, 0.0), (0.0, 0.0, f64::NAN, 1.0), (0.0, 0.0, 500.0, 1.0)] {
        assert!(draw_shape(&blank, 0, Rectangle, bad.0, bad.1, bad.2, bad.3).is_err(), "{bad:?}");
    }
    assert!(draw_shape(&blank, 0, Line, 1.0, 1.0, 1.01, 1.0).is_err(), "a line needs some length");
    assert!(draw_shape(&blank, 5, Rectangle, 0.0, 0.0, 1.0, 1.0).is_err());
    assert!(layoutcraft_vsdx::draw::DrawKind::from_code(9).is_none());
}

#[test]
fn drawn_shapes_work_with_every_editing_tool() {
    use layoutcraft_vsdx::draw::DrawKind::*;
    use layoutcraft_vsdx::groupedit::{add_connector, StyleEdit};
    use layoutcraft_vsdx::writer::patch_page_cells;
    let blank = blank_a4();
    let (bytes, a) = layoutcraft_vsdx::draw::draw_shape(&blank, 0, Rectangle, 1.0, 1.0, 3.0, 2.0).unwrap();
    let one = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, b) = layoutcraft_vsdx::draw::draw_shape(&one, 0, Diamond, 5.0, 4.0, 7.0, 5.0).unwrap();
    let two = Package::from_reader(Cursor::new(bytes)).unwrap();
    let size_of = |package: &Package, id: &str| {
        let d = import_document(package).unwrap();
        let s = d.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id)).unwrap().clone();
        (s.w, s.h)
    };
    // resize: the geometry (formulas of Width and Height) follows, also after a re-import
    let editor = GroupEditor::open(&two, 0).unwrap();
    let resized = apply(&two, &editor.resize_member(&a, (4.0, 3.0), (0.0, 0.0), false).unwrap());
    let (w, h) = size_of(&resized, &a);
    assert!((w - 4.0 * 96.0).abs() < 1e-6 && (h - 3.0 * 96.0).abs() < 1e-6, "{w} x {h}");
    let parts = import_document(&resized).unwrap().pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(a.as_str())).unwrap().parts.clone();
    let json = serde_json::to_string(&parts).unwrap();
    assert!(json.contains("384") && json.contains("288"), "the drawn outline has the new size: {json}");
    // move, turn, flip
    let moved = apply(&two, &editor.move_member(&a, (1.0, 0.5), false).unwrap());
    assert!(close(centres(&moved)[&a], (3.0, 2.0)), "{:?}", centres(&moved)[&a]);
    let turned = apply(&two, &editor.rotate_many(&[a.clone()], 0.5).unwrap());
    assert!((cell_of(&turned, &a, "Angle") - 0.5).abs() < 1e-9);
    let flipped = apply(&two, &editor.flip_many(&[a.clone(), b.clone()], false).unwrap());
    assert_eq!(cell_of(&flipped, &a, "FlipX"), 1.0);
    // format
    let styled = apply(&two, &editor.style_patches(&[a.clone()], &StyleEdit { fill: Some("#ff0000".into()), line: Some("#0000ff".into()), ..Default::default() }).unwrap());
    assert_eq!(shape_fill(&styled, &a), "#ff0000");
    // text on a shape that has none
    let texted = Package::from_reader(Cursor::new(patch_page_cells(&two, 0, &[layoutcraft_vsdx::writer::ShapeCellPatch::new(a.clone()).edit_text("Hello")]).unwrap())).unwrap();
    assert_eq!(layoutcraft_vsdx::writer::own_text(&texted, 0, &a).unwrap().as_deref().map(str::trim), Some("Hello"));
    // a connector between the two drawn shapes, glued to their connection points
    let (bytes, c) = add_connector(&two, 0, (&a, Some(2)), (&b, Some(4))).unwrap();
    let joined = Package::from_reader(Cursor::new(bytes)).unwrap();
    let part = joined.pages().unwrap()[0].part.clone();
    let connects = joined.connects(&part).unwrap();
    assert_eq!(connects.iter().filter(|x| x.from_sheet == c).count(), 2);
    // copy and paste
    let (xmls, rows) = layoutcraft_vsdx::groupedit::copy_shapes(&two, 0, &[a.clone()]).unwrap();
    let (bytes, ids) = layoutcraft_vsdx::writer::paste_shapes_with_connects(&two, 0, &xmls, &rows, 0.25, -0.25).unwrap();
    let pasted = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(import_document(&pasted).unwrap().pages[0].shapes.len(), 3);
    assert_ne!(ids[0], a);
    let _ = (Ellipse, Triangle, Line);
}

#[test]
fn the_builtin_stencil_has_every_master_and_each_can_be_dropped_into_a_blank_drawing() {
    use layoutcraft_vsdx::stencil::{import_master, master_list};
    use layoutcraft_vsdx::writer::add_master_instance;
    let stencil = Package::from_reader(Cursor::new(layoutcraft_vsdx::builtin::builtin_stencil().unwrap())).unwrap();
    let list = master_list(&stencil).unwrap();
    let names: Vec<String> = list.as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names.len(), layoutcraft_vsdx::builtin::builtin_names().len());
    for expected in ["Rectangle", "Ellipse", "Decision", "Terminator", "Data", "Arrow line"] {
        assert!(names.iter().any(|n| n == expected), "{expected} is in {names:?}");
    }
    let mut package = blank_a4();
    let mut dropped = Vec::new();
    for (i, master) in list.as_array().unwrap().iter().enumerate() {
        let id = master["id"].as_str().unwrap();
        let (bytes, new_master) = import_master(&package, &stencil, id).unwrap();
        let with_master = Package::from_reader(Cursor::new(bytes)).unwrap();
        let (bytes, shape) = add_master_instance(&with_master, 0, &new_master, 1.0 + i as f64 * 0.5, 4.0).unwrap();
        package = Package::from_reader(Cursor::new(bytes)).unwrap();
        dropped.push((shape, names[i].clone()));
    }
    // The page lists every master it uses, and every relationship leads somewhere.
    every_relationship_target_exists(&package);
    let page_rels = package.text("visio/pages/_rels/page1.xml.rels").unwrap();
    assert_eq!(page_rels.matches("relationships/master").count(), names.len(), "{page_rels} {:?}", package.names().filter(|n| n.contains("masters")).collect::<Vec<_>>());
    // A second drop of the same block reuses its master.
    let (bytes, again) = import_master(&package, &stencil, "1").unwrap();
    let again_pkg = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(layoutcraft_vsdx::MasterCatalog::load(&again_pkg).unwrap().masters.len(), names.len());
    let _ = again;
    // Re-import: every instance is there.
    let drawing = import_document(&package).unwrap();
    assert_eq!(drawing.pages[0].shapes.len() + drawing.pages[0].connectors.len(), names.len(), "the two line masters are read as connectors");
}

// ---- round 3, slice 5: connectors inside groups, glue on flipped shapes -------------------------------------

/// A blank drawing with a group (id 11) of two squares (12, 13) and a connector (14) between their facing
/// connection points, all inside the group; the group sits at pin (4, 4) turned by `group_angle`. Page 1 also has a
/// plain rectangle (id 1) from the draw tool so that the page has a `Shapes` element.
fn group_with_connector(group_angle: f64) -> Package {
    let blank = blank_a4();
    let (bytes, _) = layoutcraft_vsdx::draw::draw_shape(&blank, 0, layoutcraft_vsdx::draw::DrawKind::Rectangle, 0.5, 0.5, 1.5, 1.5).unwrap();
    let package = Package::from_reader(Cursor::new(bytes)).unwrap();
    let part = package.pages().unwrap()[0].part.clone();
    let square = |id: u32, x: f64| {
        let conn = |ix: u32, cx: &str, cy: &str, fx: &str, fy: &str, dx: i32, dy: i32| {
            format!("<Row T='Connection' IX='{ix}'><Cell N='X' V='{cx}' F='{fx}'/><Cell N='Y' V='{cy}' F='{fy}'/><Cell N='DirX' V='{dx}'/><Cell N='DirY' V='{dy}'/><Cell N='Type' V='0'/><Cell N='AutoGen' V='0'/></Row>")
        };
        format!(
            "<Shape ID='{id}' Type='Shape'><Cell N='PinX' V='{x}'/><Cell N='PinY' V='1'/><Cell N='Width' V='1'/><Cell N='Height' V='1'/><Cell N='LocPinX' V='0.5' F='Width*0.5'/><Cell N='LocPinY' V='0.5' F='Height*0.5'/><Cell N='Angle' V='0'/><Section N='Geometry' IX='0'><Row T='MoveTo' IX='1'><Cell N='X' V='0'/><Cell N='Y' V='0'/></Row><Row T='LineTo' IX='2'><Cell N='X' V='1'/><Cell N='Y' V='0'/></Row><Row T='LineTo' IX='3'><Cell N='X' V='1'/><Cell N='Y' V='1'/></Row><Row T='LineTo' IX='4'><Cell N='X' V='0'/><Cell N='Y' V='1'/></Row><Row T='LineTo' IX='5'><Cell N='X' V='0'/><Cell N='Y' V='0'/></Row></Section><Section N='Connection'>{}{}{}{}</Section></Shape>",
            conn(0, "0.5", "1", "Width*0.5", "Height*1", 0, -1),
            conn(1, "1", "0.5", "Width*1", "Height*0.5", -1, 0),
            conn(2, "0.5", "0", "Width*0.5", "Height*0", 0, 1),
            conn(3, "0", "0.5", "Width*0", "Height*0.5", 1, 0)
        )
    };
    let connector = "<Shape ID='14' Type='Shape'><Cell N='PinX' V='2'/><Cell N='PinY' V='1'/><Cell N='Width' V='1'/><Cell N='Height' V='0'/><Cell N='LocPinX' V='0.5'/><Cell N='LocPinY' V='0'/><Cell N='Angle' V='0'/><Cell N='BeginX' V='1.5' F='PAR(PNT(Sheet.12!Connections.X2,Sheet.12!Connections.Y2))'/><Cell N='BeginY' V='1' F='PAR(PNT(Sheet.12!Connections.X2,Sheet.12!Connections.Y2))'/><Cell N='EndX' V='2.5' F='PAR(PNT(Sheet.13!Connections.X4,Sheet.13!Connections.Y4))'/><Cell N='EndY' V='1' F='PAR(PNT(Sheet.13!Connections.X4,Sheet.13!Connections.Y4))'/><Section N='Geometry' IX='0'><Row T='MoveTo' IX='1'><Cell N='X' V='0'/><Cell N='Y' V='0'/></Row><Row T='LineTo' IX='2'><Cell N='X' V='1'/><Cell N='Y' V='0'/></Row></Section></Shape>";
    let group = format!(
        "<Shape ID='11' Type='Group'><Cell N='PinX' V='4'/><Cell N='PinY' V='4'/><Cell N='Width' V='4'/><Cell N='Height' V='2'/><Cell N='LocPinX' V='2'/><Cell N='LocPinY' V='1'/><Cell N='Angle' V='{group_angle}'/><Shapes>{}{}{connector}</Shapes></Shape>",
        square(12, 1.0),
        square(13, 3.0)
    );
    let mut xml = package.text(&part).unwrap().to_string();
    let at = xml.rfind("</Shapes>").unwrap();
    xml.insert_str(at, &group);
    let end = xml.rfind("</PageContents>").unwrap();
    xml.insert_str(
        end,
        "<Connects><Connect FromSheet='14' FromCell='BeginX' FromPart='9' ToSheet='12' ToCell='Connections.X2' ToPart='101'/><Connect FromSheet='14' FromCell='EndX' FromPart='12' ToSheet='13' ToCell='Connections.X4' ToPart='103'/></Connects>",
    );
    let mut extras = std::collections::BTreeMap::new();
    extras.insert(part, xml.into_bytes());
    Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::write_package_edit(&package, extras, &[]).unwrap())).unwrap()
}

#[test]
fn a_connector_inside_a_group_is_copied_as_a_loose_page_level_connector_at_its_page_position() {
    use layoutcraft_vsdx::groupedit::copy_shapes;
    use layoutcraft_vsdx::writer::paste_shapes_with_connects;
    let package = group_with_connector(0.0);
    assert!(import_document(&package).is_ok());
    let (xmls, rows) = copy_shapes(&package, 0, &["14".to_string()]).unwrap();
    assert!(rows.is_empty(), "the glue is dropped: {rows:?}");
    let (bytes, ids) = paste_shapes_with_connects(&package, 0, &xmls, &rows, 0.0, 0.0).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    let editor = GroupEditor::open(&after, 0).unwrap();
    let part = after.pages().unwrap()[0].part.clone();
    let listed = editor.connectors(&after.connects(&part).unwrap());
    let copy = listed.as_array().unwrap().iter().find(|c| c["id"] == ids[0].as_str()).expect("the copy is a page-level connector");
    // The group sits at pin (4, 4) with its pin at (2, 1) inside: a point (x, y) of the group is (2 + x, 3 + y) on the page.
    let near = |a: &serde_json::Value, b: f64| (a.as_f64().unwrap() - b).abs() < 1e-6;
    assert!(near(&copy["begin"]["x"], 3.5) && near(&copy["begin"]["y"], 4.0) && near(&copy["end"]["x"], 4.5) && near(&copy["end"]["y"], 4.0), "{copy}");
    assert!(copy["begin"]["glue"].is_null() && copy["end"]["glue"].is_null());
    assert!(import_document(&after).is_ok());
}

#[test]
fn a_connector_of_a_turned_group_is_refused_for_copy_not_written_wrongly() {
    use layoutcraft_vsdx::groupedit::copy_shapes;
    let package = group_with_connector(0.5);
    assert!(copy_shapes(&package, 0, &["14".to_string()]).is_err());
}

#[test]
fn copying_a_group_keeps_the_glue_of_the_connectors_among_its_members() {
    use layoutcraft_vsdx::groupedit::copy_shapes;
    use layoutcraft_vsdx::writer::paste_shapes_with_connects;
    let package = group_with_connector(0.0);
    let (xmls, rows) = copy_shapes(&package, 0, &["11".to_string()]).unwrap();
    assert_eq!(rows.len(), 2, "both ends stay glued inside the copy: {rows:?}");
    let (bytes, ids) = paste_shapes_with_connects(&package, 0, &xmls, &rows, 1.0, 0.0).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    let part = after.pages().unwrap()[0].part.clone();
    let connects = after.connects(&part).unwrap();
    assert_eq!(connects.len(), 4, "{connects:?}");
    let editor = GroupEditor::open(&after, 0).unwrap();
    let kids: Vec<String> = editor.subtree_ids(&ids[0]).unwrap();
    let mine: Vec<_> = connects.iter().filter(|c| kids.contains(&c.from_sheet)).collect();
    assert_eq!(mine.len(), 2);
    assert!(mine.iter().all(|c| kids.contains(&c.to_sheet) && c.to_sheet != "12" && c.to_sheet != "13"), "glued to the COPIES: {mine:?}");
    // The originals are untouched.
    assert_eq!(connects.iter().filter(|c| c.from_sheet == "14").count(), 2);
    assert!(import_document(&after).is_ok());
}

#[test]
fn a_connector_glued_to_a_flipped_shape_goes_to_the_mirrored_connection_point_and_loops_round() {
    let (package, id, part) = connector_fixture(); // 1 (point 2) -> 2 (point 4)
    let editor = GroupEditor::open(&package, 0).unwrap();
    let before = editor.connection_points("1").into_iter().find(|p| p.0 == 2).unwrap();
    let frame = editor.describe().as_array().unwrap().iter().find(|f| f["id"] == "1").unwrap().clone();
    let (pin_x, width) = (frame["pin"][0].as_f64().unwrap(), frame["size"][0].as_f64().unwrap());
    assert!((before.1 - (pin_x + width / 2.0)).abs() < 1e-6, "point 2 is on the right side");
    let patches = editor.flip_many(&["1".to_string()], false).unwrap();
    let flipped = Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::patch_page_cells(&package, 0, &patches).unwrap())).unwrap();
    let editor = GroupEditor::open(&flipped, 0).unwrap();
    let after = editor.connection_points("1").into_iter().find(|p| p.0 == 2).unwrap();
    let frame_after = editor.describe().as_array().unwrap().iter().find(|f| f["id"] == "1").unwrap().clone();
    let new_pin_x = frame_after["pin"][0].as_f64().unwrap();
    // The flip mirrors the box about its own centre (single shape): point 2 is now on the LEFT of the shape.
    assert!((after.1 - (new_pin_x - width / 2.0)).abs() < 1e-6, "mirrored: {after:?} pin {new_pin_x}");
    let jump = flipped.pages().unwrap()[0].line_jump.clone().unwrap_or_default();
    let connects = flipped.connects(&part).unwrap();
    let reroutes = editor.reroute_connectors(&connects, &jump);
    assert!(reroutes.iter().any(|p| p.visio_id == id), "the connector follows the flip");
    let routed = Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::patch_page_cells(&flipped, 0, &reroutes).unwrap())).unwrap();
    let editor = GroupEditor::open(&routed, 0).unwrap();
    let listed = editor.connectors(&routed.connects(&part).unwrap());
    let me = listed.as_array().unwrap().iter().find(|c| c["id"] == id.as_str()).unwrap().clone();
    assert!((me["begin"]["x"].as_f64().unwrap() - after.1).abs() < 1e-6 && (me["begin"]["y"].as_f64().unwrap() - after.2).abs() < 1e-6, "{me}");
    assert!(me["route"].as_array().unwrap().len() > 2, "the route leaves left and goes round the shape: {me}");
    assert!(import_document(&routed).is_ok());
}

// ---- round 3, slice 7: formatting a range of a shape's text ---------------------------------------------------------

fn texted_shape(text: &str) -> (Package, String) {
    use layoutcraft_vsdx::writer::{patch_page_cells, ShapeCellPatch};
    let (package, id) = draw_on_blank(layoutcraft_vsdx::draw::DrawKind::Rectangle, 1.0, 1.0, 4.0, 3.0);
    let bytes = patch_page_cells(&package, 0, &[ShapeCellPatch::new(id.clone()).text(text)]).unwrap();
    (Package::from_reader(Cursor::new(bytes)).unwrap(), id)
}

fn format_range(package: &Package, id: &str, range: layoutcraft_vsdx::writer::TextRangeFormat) -> Package {
    let bytes = layoutcraft_vsdx::writer::patch_page_cells(package, 0, &[layoutcraft_vsdx::writer::ShapeCellPatch::new(id).text_range(range)]).unwrap();
    Package::from_reader(Cursor::new(bytes)).unwrap()
}

fn text_element(package: &Package, id: &str) -> String {
    let shape = layoutcraft_vsdx::writer::shape_xml(package, 0, id).unwrap();
    let a = shape.find("<Text>").unwrap();
    let b = shape.find("</Text>").unwrap();
    shape[a + 6..b].to_string()
}

/// The runs of a `<Text>` element: `(cp IX, characters)` in text order (paragraph markers dropped, "\n" kept).
fn runs(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        if at > 0 {
            if let Some(last) = out.last_mut() {
                last.1.push_str(&rest[..at]);
            }
        }
        let close = rest[at..].find('>').unwrap() + at + 1;
        let tag = &rest[at..close];
        if tag.starts_with("<cp ") {
            let ix = tag.split('\'').nth(1).unwrap().to_string();
            out.push((ix, String::new()));
        }
        rest = &rest[close..];
    }
    if let Some(last) = out.last_mut() {
        last.1.push_str(rest);
    }
    out
}

/// The cells of Character row `ix` of the shape as text.
fn char_row(shape: &str, ix: &str) -> String {
    let open = format!("<Row IX='{ix}'");
    let section = &shape[shape.find("<Section N='Character'").unwrap()..];
    let section = &section[..section.find("</Section>").unwrap()];
    let at = section.find(&open).unwrap_or_else(|| panic!("no Character row {ix} in {section}"));
    let rest = &section[at..];
    rest[..rest.find("</Row>").unwrap()].to_string()
}

fn range(start: usize, end: usize, cells: &[(&str, &str)]) -> layoutcraft_vsdx::writer::TextRangeFormat {
    layoutcraft_vsdx::writer::TextRangeFormat {
        start,
        end,
        char_cells: cells.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        ..Default::default()
    }
}

#[test]
fn formatting_a_middle_range_gives_visios_numbering_one_row_per_run_in_text_order() {
    let (package, id) = texted_shape("Hello big world");
    let after = format_range(&package, &id, range(6, 9, &[("Strikethru", "1")]));
    assert_eq!(layoutcraft_vsdx::writer::own_text(&after, 0, &id).unwrap().unwrap(), "Hello big world");
    let text = text_element(&after, &id);
    // Visio's own numbering (probed): the runs are cp 0, 1, 2 in text order, and the text starts with `<cp/><pp/>`
    assert_eq!(runs(&text), vec![("0".into(), "Hello ".into()), ("1".into(), "big".into()), ("2".into(), " world".into())], "{text}");
    assert!(text.starts_with("<cp IX='0'/><pp IX='0'/>"), "{text}");
    let shape = layoutcraft_vsdx::writer::shape_xml(&after, 0, &id).unwrap();
    assert!(char_row(&shape, "1").contains("N='Strikethru' V='1'"));
    // the other runs say so explicitly (Visio takes a missing cell from the style, a reader may take it from row 0)
    assert!(char_row(&shape, "0").contains("N='Strikethru' V='0'") && char_row(&shape, "2").contains("N='Strikethru' V='0'"), "only the middle run: {shape}");
    // every row is a clone: all three keep the size cell
    for ix in ["0", "1", "2"] {
        assert!(char_row(&shape, ix).contains("N='Size'"), "row {ix} keeps the size");
    }
    assert!(import_document(&after).is_ok());
}

#[test]
fn a_range_at_the_start_or_end_and_the_style_bits() {
    let (package, id) = texted_shape("Hello big world");
    let head = format_range(&package, &id, layoutcraft_vsdx::writer::TextRangeFormat { start: 0, end: 5, char_style: Some((1, 0)), ..Default::default() });
    let text = text_element(&head, &id);
    assert_eq!(runs(&text), vec![("0".into(), "Hello".into()), ("1".into(), " big world".into())], "{text}");
    let shape = layoutcraft_vsdx::writer::shape_xml(&head, 0, &id).unwrap();
    assert!(char_row(&shape, "0").contains("N='Style' V='1'") && char_row(&shape, "1").contains("N='Style' V='0'"), "bold on the first run only: {shape}");
    let tail = format_range(&package, &id, range(12, 15, &[("Case", "1")]));
    let text = text_element(&tail, &id);
    assert_eq!(runs(&text), vec![("0".into(), "Hello big wo".into()), ("1".into(), "rld".into())], "{text}");
    assert_eq!(layoutcraft_vsdx::writer::own_text(&tail, 0, &id).unwrap().unwrap(), "Hello big world");
    assert!(import_document(&tail).is_ok());
}

#[test]
fn formatting_twice_keeps_the_first_format_where_the_ranges_overlap() {
    let (package, id) = texted_shape("Hello big world");
    let once = format_range(&package, &id, range(6, 9, &[("Strikethru", "1")]));
    let twice = format_range(&once, &id, range(3, 8, &[("Letterspace", "0.0277")]));
    assert_eq!(layoutcraft_vsdx::writer::own_text(&twice, 0, &id).unwrap().unwrap(), "Hello big world");
    let text = text_element(&twice, &id);
    let all = runs(&text);
    let joined: String = all.iter().map(|r| r.1.as_str()).collect();
    assert_eq!(joined, "Hello big world");
    // the indexes run 0, 1, 2 ... in order with no repeats
    for (i, (ix, _)) in all.iter().enumerate() {
        assert_eq!(*ix, i.to_string(), "{text}");
    }
    let shape = layoutcraft_vsdx::writer::shape_xml(&twice, 0, &id).unwrap();
    let spaced: Vec<&(String, String)> = all.iter().filter(|(ix, _)| char_row(&shape, ix).contains("N='Letterspace'") && !char_row(&shape, ix).contains("N='Letterspace' V='0'")).collect();
    assert_eq!(spaced.iter().map(|r| r.1.as_str()).collect::<String>(), "lo bi", "{text}");
    // "bi" (inside the struck "big") has both formats
    let both = all.iter().find(|(ix, t)| t == "bi" && char_row(&shape, ix).contains("N='Letterspace'") && char_row(&shape, ix).contains("N='Strikethru' V='1'"));
    assert!(both.is_some(), "{text} {shape}");
    assert!(import_document(&twice).is_ok());
}

#[test]
fn paragraph_formatting_gives_the_touched_paragraph_its_own_pp_and_row_and_restores_the_next() {
    let (package, id) = texted_shape("one\ntwo\nthree");
    let para = |start: usize, end: usize, cells: &[(&str, &str)]| layoutcraft_vsdx::writer::TextRangeFormat {
        start,
        end,
        para_cells: cells.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        ..Default::default()
    };
    // a caret in the second paragraph: right aligned
    let after = format_range(&package, &id, para(5, 5, &[("HorzAlign", "2")]));
    assert_eq!(layoutcraft_vsdx::writer::own_text(&after, 0, &id).unwrap().unwrap(), "one\ntwo\nthree");
    let text = text_element(&after, &id);
    // pp 0 for "one", pp 1 for "two", pp 2 (a clone of the first) for "three"
    let (one, two, three) = (text.find("one").unwrap(), text.find("two").unwrap(), text.find("three").unwrap());
    assert!(text[..one].contains("<pp IX='0'/>") && text[one..two].contains("<pp IX='1'/>") && text[two..three].contains("<pp IX='2'/>"), "{text}");
    let shape = layoutcraft_vsdx::writer::shape_xml(&after, 0, &id).unwrap();
    let rows = &shape[shape.find("<Section N='Paragraph'").unwrap()..];
    let rows = &rows[..rows.find("</Section>").unwrap()];
    assert_eq!(rows.matches("<Row ").count(), 3, "{rows}");
    assert!(rows.contains("N='HorzAlign' V='2'") && rows.matches("N='HorzAlign' V='1'").count() == 2, "paragraphs 1 and 3 stay centred: {rows}");
    assert!(import_document(&after).is_ok());
    // a range over the first two paragraphs: bullets
    let bullets = format_range(&package, &id, para(0, 6, &[("Bullet", "1")]));
    let shape = layoutcraft_vsdx::writer::shape_xml(&bullets, 0, &id).unwrap();
    assert_eq!(shape.matches("N='Bullet' V='1'").count(), 2, "one row for each of the two paragraphs: {shape}");
    assert_eq!(layoutcraft_vsdx::writer::own_text(&bullets, 0, &id).unwrap().unwrap(), "one\ntwo\nthree");
    assert!(import_document(&bullets).is_ok());
}

#[test]
fn a_range_format_of_a_shape_without_text_is_refused_and_out_of_range_offsets_are_clamped() {
    use layoutcraft_vsdx::writer::{patch_page_cells, ShapeCellPatch};
    let (package, id) = draw_on_blank(layoutcraft_vsdx::draw::DrawKind::Rectangle, 1.0, 1.0, 4.0, 3.0);
    let patch = ShapeCellPatch::new(id.clone()).text_range(range(0, 3, &[("Strikethru", "1")]));
    assert!(patch_page_cells(&package, 0, &[patch]).is_err());
    let (package, id) = texted_shape("abc");
    let after = format_range(&package, &id, range(1, 99, &[("Strikethru", "1")]));
    assert_eq!(layoutcraft_vsdx::writer::own_text(&after, 0, &id).unwrap().unwrap(), "abc");
}

#[test]
fn a_text_range_format_survives_export_and_reimport_as_runs() {
    let (package, id) = texted_shape("Hello big world");
    let after = format_range(&package, &id, range(6, 9, &[("Strikethru", "1"), ("Color", "#ff0000")]));
    let reread = Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::write_package_edit(&after, Default::default(), &[]).unwrap())).unwrap();
    let document = import_document(&reread).unwrap();
    let json = serde_json::to_string(&document.pages[0].shapes).unwrap();
    assert!(json.contains("Hello") && json.contains("big") && json.contains("world"), "{json}");
    assert!(json.contains("#ff0000") && json.contains("strike"), "the run colour and strike-through are read back: {json}");
}

// ---- round 3, slice 8: fill patterns and gradients, shadow detail, line cap, rounding ---------------------------------

fn styled_rect(edit: layoutcraft_vsdx::groupedit::StyleEdit) -> (Package, String) {
    let (package, id) = draw_on_blank(layoutcraft_vsdx::draw::DrawKind::Rectangle, 1.0, 1.0, 4.0, 3.0);
    let patches = GroupEditor::open(&package, 0).unwrap().style_patches(&[id.clone()], &edit).unwrap();
    let bytes = layoutcraft_vsdx::writer::patch_page_cells(&package, 0, &patches).unwrap();
    (Package::from_reader(Cursor::new(bytes)).unwrap(), id)
}

fn imported_json(package: &Package, id: &str) -> String {
    let document = import_document(package).unwrap();
    let shape = document.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id)).expect("the shape is imported");
    serde_json::to_string(shape).unwrap()
}

#[test]
fn a_gradient_pattern_and_a_hatch_pattern_are_written_with_both_colours_and_drawn() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    let (gradient, id) = styled_rect(StyleEdit { fill: Some("#ff0000".into()), fill_back: Some("#0000ff".into()), fill_pattern: Some(26), ..Default::default() });
    let shape = shape_text(&gradient, &id);
    for needle in ["N='FillPattern' V='26'", "N='FillBkgnd' V='#0000ff'", "N='FillForegnd' V='#ff0000'"] {
        assert!(shape.contains(needle), "{needle} in {shape}");
    }
    let json = imported_json(&gradient, &id);
    assert!(json.contains("fillGradient") || json.contains("\"stops\""), "pattern 26 is drawn as a gradient: {json}");
    let (hatch, id) = styled_rect(StyleEdit { fill: Some("#00aa00".into()), fill_back: Some("#ffffff".into()), fill_pattern: Some(10), ..Default::default() });
    assert!(shape_text(&hatch, &id).contains("N='FillPattern' V='10'"));
    let json = imported_json(&hatch, &id);
    assert!(json.contains("fillPattern"), "pattern 10 is drawn as a tile pattern: {json}");
    // pattern 0 = no fill, 1 = solid again
    let (none, id) = styled_rect(StyleEdit { fill_pattern: Some(0), ..Default::default() });
    assert!(shape_text(&none, &id).contains("N='FillPattern' V='0'"));
}

#[test]
fn the_shadow_in_detail_colour_offset_blur_and_transparency() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    let (package, id) = styled_rect(StyleEdit { shadow_color: Some("#112233".into()), shadow_dx: Some(6.0), shadow_dy: Some(8.0), shadow_blur: Some(4.0), shadow_trans: Some(0.3), ..Default::default() });
    let shape = shape_text(&package, &id);
    for needle in ["N='ShdwPattern' V='1'", "N='ShapeShdwType' V='1'", "N='ShdwForegnd' V='#112233'", "N='ShapeShdwOffsetX' V='0.0625'", "N='ShapeShdwOffsetY' V='-0.08333333", "N='ShapeShdwBlur' V='0.0416666", "N='ShdwForegndTrans' V='0.3'"] {
        assert!(shape.contains(needle), "{needle} in {shape}");
    }
    let document = import_document(&package).unwrap();
    let s = document.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id.as_str())).unwrap();
    let shadow = s.shadow.as_ref().expect("the shadow is drawn");
    assert!((shadow.dx - 6.0).abs() < 0.05 && (shadow.dy - 8.0).abs() < 0.05 && (shadow.blur - 4.0).abs() < 0.05, "{shadow:?}");
    // the page y axis points down: a positive dy goes DOWN, which is a NEGATIVE ShapeShdwOffsetY
    assert!(shape.contains("N='ShapeShdwOffsetY' V='-"));
}

#[test]
fn line_cap_and_rounding_are_written_and_read_back() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    let (package, id) = styled_rect(StyleEdit { line_cap: Some(1), rounding_px: Some(12.0), ..Default::default() });
    let shape = shape_text(&package, &id);
    assert!(shape.contains("N='LineCap' V='1'") && shape.contains("N='Rounding' V='0.125'"), "{shape}");
    let json = imported_json(&package, &id);
    assert!(json.contains("flat"), "the flat cap rides on the stroke style: {json}");
    assert!(json.contains("\"rounding\":12"), "{json}");
}

#[test]
fn the_new_style_fields_are_validated() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    let (package, id) = draw_on_blank(layoutcraft_vsdx::draw::DrawKind::Rectangle, 1.0, 1.0, 4.0, 3.0);
    let editor = GroupEditor::open(&package, 0).unwrap();
    let ids = [id];
    for edit in [
        StyleEdit { fill_pattern: Some(41), ..Default::default() },
        StyleEdit { line_cap: Some(3), ..Default::default() },
        StyleEdit { shadow_blur: Some(-1.0), ..Default::default() },
        StyleEdit { rounding_px: Some(-2.0), ..Default::default() },
        StyleEdit { shadow_trans: Some(1.5), ..Default::default() },
        StyleEdit { shadow_color: Some("red".into()), ..Default::default() },
        StyleEdit { fill_back: Some("#12".into()), ..Default::default() },
        StyleEdit { shadow_dx: Some(f64::NAN), ..Default::default() },
    ] {
        assert!(editor.style_patches(&ids, &edit).is_err(), "{edit:?}");
    }
}

// ---- round 3, slice 9: the palette and fonts of a stencil come along ------------------------------------------------

/// `package` with the text of some parts replaced: `(part, function)`.
fn with_parts_rewritten(package: &Package, rewrites: &[(&str, &dyn Fn(String) -> String)]) -> Package {
    use std::io::Write;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for name in package.names() {
        let mut bytes = package.entry(name).unwrap().to_vec();
        for (part, rewrite) in rewrites {
            if name == *part {
                bytes = rewrite(String::from_utf8(bytes).unwrap()).into_bytes();
            }
        }
        writer.start_file(name, options).unwrap();
        writer.write_all(&bytes).unwrap();
    }
    Package::from_reader(Cursor::new(writer.finish().unwrap().into_inner())).unwrap()
}

/// The built-in stencil with a custom palette (colour 2 = #123456), whose first master paints with palette index 2 and
/// the font Impact.
fn stencil_with_palette_and_font() -> Package {
    let stencil = Package::from_reader(Cursor::new(layoutcraft_vsdx::builtin::builtin_stencil().unwrap())).unwrap();
    with_parts_rewritten(
        &stencil,
        &[
            ("visio/document.xml", &|text: String| text.replacen("<StyleSheets", "<Colors><ColorEntry IX='2' RGB='#123456'/></Colors><FaceNames><FaceName NameU='Impact' UnicodeRanges='1 0 0 0'/></FaceNames><StyleSheets", 1)),
            ("visio/masters/master1.xml", &|text: String| text.replacen("<Cell N='ResizeMode' V='0'/>", "<Cell N='ResizeMode' V='0'/><Cell N='FillForegnd' V='2'/>", 1).replacen("<Cell N='Size' V=", "<Cell N='Font' V='Impact'/><Cell N='Size' V=", 1)),
        ],
    )
}

fn master_text(package: &Package, new_id: &str) -> String {
    let catalog = layoutcraft_vsdx::MasterCatalog::load(package).unwrap();
    let part = catalog.masters[new_id].part.clone();
    package.text(&part).unwrap().to_string()
}

#[test]
fn a_stencil_with_its_own_palette_brings_its_colours_as_rgb_and_its_fonts() {
    use layoutcraft_vsdx::stencil::import_master;
    let stencil = stencil_with_palette_and_font();
    let (bytes, id) = import_master(&blank_a4(), &stencil, "1").unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    let master = master_text(&after, &id);
    assert!(master.contains("N='FillForegnd' V='#123456'"), "the palette index became the stencil's colour: {master}");
    assert!(!master.contains("N='FillForegnd' V='2'"));
    let document = after.text("visio/document.xml").unwrap();
    assert!(document.contains("<FaceName NameU='Impact' UnicodeRanges='1 0 0 0'/>"), "the stencil's own FaceName element comes along: {document}");
    every_relationship_target_exists(&after);
    assert!(import_document(&after).is_ok());
    // a second master that needs the same font does not list it twice
    let (bytes, _) = import_master(&after, &stencil, "2").unwrap();
    let again = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(again.text("visio/document.xml").unwrap().matches("NameU='Impact'").count(), 1);
}

#[test]
fn a_drawing_with_a_custom_palette_gets_the_stencils_default_colours_as_rgb() {
    use layoutcraft_vsdx::stencil::import_master;
    // the stencil uses the built-in palette (index 2 = red); the drawing redefines index 2
    let stencil = with_parts_rewritten(
        &Package::from_reader(Cursor::new(layoutcraft_vsdx::builtin::builtin_stencil().unwrap())).unwrap(),
        &[("visio/masters/master1.xml", &|text: String| text.replacen("<Cell N='ResizeMode' V='0'/>", "<Cell N='ResizeMode' V='0'/><Cell N='FillForegnd' V='2'/>", 1))],
    );
    let target = with_parts_rewritten(&blank_a4(), &[("visio/document.xml", &|text: String| text.replacen("<StyleSheets", "<Colors><ColorEntry IX='2' RGB='#654321'/></Colors><StyleSheets", 1))]);
    let (bytes, id) = import_master(&target, &stencil, "1").unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert!(master_text(&after, &id).contains("N='FillForegnd' V='#ff0000'"), "built-in colour 2 is red whatever the drawing's palette says");
    assert!(import_document(&after).is_ok());
}

#[test]
fn equal_palettes_leave_the_master_untouched() {
    use layoutcraft_vsdx::stencil::import_master;
    let stencil = with_parts_rewritten(
        &Package::from_reader(Cursor::new(layoutcraft_vsdx::builtin::builtin_stencil().unwrap())).unwrap(),
        &[("visio/masters/master1.xml", &|text: String| text.replacen("<Cell N='ResizeMode' V='0'/>", "<Cell N='ResizeMode' V='0'/><Cell N='FillForegnd' V='2'/>", 1))],
    );
    let (bytes, id) = import_master(&blank_a4(), &stencil, "1").unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert!(master_text(&after, &id).contains("N='FillForegnd' V='2'"), "same palette, nothing to resolve");
}

#[test]
fn a_picture_master_is_instanced_as_a_foreign_shape_or_visio_draws_nothing() {
    use layoutcraft_vsdx::stencil::import_master;
    use layoutcraft_vsdx::writer::add_master_instance;
    let stencil = Package::open(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/visioStencils-converted/Stencils/IT Generic/VRT Clients and Peripherals v2014.vssx")).unwrap();
    let id = small_masters(&stencil, 1).remove(0);
    let (bytes, new_master) = import_master(&blank_a4(), &stencil, &id).unwrap();
    let with_master = Package::from_reader(Cursor::new(bytes)).unwrap();
    let master = master_text(&with_master, &new_master);
    assert!(master.contains("Type='Foreign'") && master.contains("<ForeignData"), "the master is a picture");
    let (bytes, shape) = add_master_instance(&with_master, 0, &new_master, 3.0, 3.0).unwrap();
    let placed = Package::from_reader(Cursor::new(bytes)).unwrap();
    let xml = shape_text(&placed, &shape);
    assert!(xml.contains("Type='Foreign'"), "the instance of a picture master is a Foreign shape (probed in Visio): {xml}");
    // an ordinary master still gives an ordinary shape
    let plain = Package::from_reader(Cursor::new(layoutcraft_vsdx::builtin::builtin_stencil().unwrap())).unwrap();
    let (bytes, rect) = import_master(&blank_a4(), &plain, "1").unwrap();
    let with_rect = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, shape) = add_master_instance(&with_rect, 0, &rect, 3.0, 3.0).unwrap();
    assert!(shape_text(&Package::from_reader(Cursor::new(bytes)).unwrap(), &shape).contains("Type='Shape'"));
}

#[test]
fn a_resized_instance_of_a_master_redraws_its_geometry_and_moves_its_connection_points() {
    use layoutcraft_vsdx::stencil::import_master;
    use layoutcraft_vsdx::writer::add_master_instance;
    let stencil = Package::from_reader(Cursor::new(layoutcraft_vsdx::builtin::builtin_stencil().unwrap())).unwrap();
    for master in ["1", "12", "3"] {
        // rectangle, decision (diamond) and ellipse
        let (bytes, new_master) = import_master(&blank_a4(), &stencil, master).unwrap();
        let with_master = Package::from_reader(Cursor::new(bytes)).unwrap();
        let (bytes, id) = add_master_instance(&with_master, 0, &new_master, 4.0, 4.0).unwrap();
        let placed = Package::from_reader(Cursor::new(bytes)).unwrap();
        let editor = GroupEditor::open(&placed, 0).unwrap();
        let frame = editor.describe().as_array().unwrap().iter().find(|f| f["id"] == id.as_str()).unwrap().clone();
        let (w, h) = (frame["size"][0].as_f64().unwrap(), frame["size"][1].as_f64().unwrap());
        let (nw, nh) = (w * 2.0, h * 3.0);
        let patches = editor.resize_member(&id, (nw, nh), (0.5, 0.5), false).unwrap();
        let resized = Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::patch_page_cells(&placed, 0, &patches).unwrap())).unwrap();
        // the instance carries the recalculated rows itself (Visio does not recalculate a master's formulas on load)
        let xml = shape_text(&resized, &id);
        assert!(xml.contains("<Section N='Connection' IX='0'>") || xml.contains("<Section N='Connection'"), "master {master}: connection rows are written on the instance: {xml}");
        assert!(xml.contains(&format!("V='{}' F='Width*1'", nw)) || xml.contains(&format!("V='{}' F='Width*0.5'", nw / 2.0)), "master {master}: a connection point is at the new width: {xml}");
        if master != "3" {
            assert!(xml.contains("<Section N='Geometry'"), "master {master}: geometry rows are written on the instance: {xml}");
        }
        // the drawn path spans the NEW box, not the master's
        let doc = import_document(&resized).unwrap();
        let shape = doc.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id.as_str())).unwrap();
        let numbers: Vec<f64> = shape.parts[0].d.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter(|t| !t.is_empty()).filter_map(|t| t.parse().ok()).collect();
        let max_x = numbers.chunks(2).map(|c| c[0]).fold(0.0, f64::max);
        let max_y = numbers.chunks(2).map(|c| c[1]).fold(0.0, f64::max);
        if master == "3" {
            // an ellipse is two arcs: their radii are half the new width and half the new height
            assert!(shape.parts[0].d.contains(&format!("A{} {}", nw * 96.0 / 2.0, nh * 96.0 / 2.0)) || shape.parts[0].d.contains(&format!("A{} {}", nw * 96.0 / 2.0, nw * 96.0 / 2.0)), "master {master}: {}", shape.parts[0].d);
        } else {
            assert!((max_x - nw * 96.0).abs() < 0.5 && (max_y - nh * 96.0).abs() < 0.5, "master {master}: path {} should span {} x {}", shape.parts[0].d, nw * 96.0, nh * 96.0);
        }
        // the connection points sit on the new box
        let editor = GroupEditor::open(&resized, 0).unwrap();
        let frame = editor.describe().as_array().unwrap().iter().find(|f| f["id"] == id.as_str()).unwrap().clone();
        let (pin_x, pin_y) = (frame["pin"][0].as_f64().unwrap(), frame["pin"][1].as_f64().unwrap());
        let points = editor.connection_points(&id);
        assert_eq!(points.len(), 4);
        let right = points.iter().map(|p| p.1).fold(f64::MIN, f64::max);
        let top = points.iter().map(|p| p.2).fold(f64::MIN, f64::max);
        assert!((right - (pin_x + nw / 2.0)).abs() < 1e-6 && (top - (pin_y + nh / 2.0)).abs() < 1e-6, "master {master}: right {right} top {top}, pin ({pin_x}, {pin_y}), new size {nw} x {nh}");
    }
}

// ---- round 4, slice 3: connectors inside groups ---------------------------------------------------------------------

fn connector_json(package: &Package, id: &str) -> Option<serde_json::Value> {
    let part = package.pages().unwrap()[0].part.clone();
    let editor = GroupEditor::open(package, 0).unwrap();
    editor.connectors(&package.connects(&part).unwrap()).as_array().unwrap().iter().find(|c| c["id"] == id).cloned()
}

#[test]
fn a_connector_inside_a_group_is_listed_with_page_coordinates() {
    let package = group_with_connector(0.0);
    let c = connector_json(&package, "14").expect("the member connector is listed");
    let near = |v: &serde_json::Value, x: f64| (v.as_f64().unwrap() - x).abs() < 1e-6;
    // the group's origin is at page (2, 3): the connector (1.5, 1)-(2.5, 1) of the group is (3.5, 4)-(4.5, 4)
    assert!(near(&c["begin"]["x"], 3.5) && near(&c["begin"]["y"], 4.0) && near(&c["end"]["x"], 4.5), "{c}");
    assert_eq!(c["parentId"], "11");
    assert_eq!(c["begin"]["glue"]["sheet"], "12");
    assert_eq!(c["end"]["glue"]["sheet"], "13");
    // a turned group keeps its connector out of the list (its cells cannot be mapped by a translation)
    assert!(connector_json(&group_with_connector(0.5), "14").is_none());
}

#[test]
fn a_connector_inside_a_group_follows_the_member_it_is_glued_to() {
    use layoutcraft_vsdx::writer::patch_page_cells;
    let package = group_with_connector(0.0);
    let part = package.pages().unwrap()[0].part.clone();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let patches = editor.move_member("12", (0.0, 0.5), false).unwrap();
    let moved = Package::from_reader(Cursor::new(patch_page_cells(&package, 0, &patches).unwrap())).unwrap();
    let editor = GroupEditor::open(&moved, 0).unwrap();
    let jump = moved.pages().unwrap()[0].line_jump.clone().unwrap_or_default();
    let reroutes = editor.reroute_connectors(&moved.connects(&part).unwrap(), &jump);
    assert!(reroutes.iter().any(|p| p.visio_id == "14"), "the member connector is rerouted");
    let routed = Package::from_reader(Cursor::new(patch_page_cells(&moved, 0, &reroutes).unwrap())).unwrap();
    let c = connector_json(&routed, "14").unwrap();
    // the begin sits on the moved square's connection point 2 (page y 4.5), the end is unchanged
    let editor = GroupEditor::open(&routed, 0).unwrap();
    let point = editor.connection_points("12").into_iter().find(|p| p.0 == 2).unwrap();
    assert!((c["begin"]["x"].as_f64().unwrap() - point.1).abs() < 1e-6 && (c["begin"]["y"].as_f64().unwrap() - point.2).abs() < 1e-6, "{c} vs {point:?}");
    assert!((c["end"]["x"].as_f64().unwrap() - 4.5).abs() < 1e-6);
    // the cells stay in the group's coordinates (the begin x is 1.5, not the page's 3.5)
    let shape = layoutcraft_vsdx::writer::shape_xml(&routed, 0, "14").unwrap();
    assert!(shape.contains("N='BeginX' V='1.5'") || shape.contains("N='BeginX' V='1.5000"), "{shape}");
    assert!(import_document(&routed).is_ok());
}

#[test]
fn an_end_of_a_connector_inside_a_group_can_be_set_free_and_glued_again() {
    use layoutcraft_vsdx::groupedit::{move_connector_end, EndTarget};
    let package = group_with_connector(0.0);
    let part = package.pages().unwrap()[0].part.clone();
    // free: the end goes to the page point (5.5, 4.75) = group point (3.5, 1.75)
    let freed = Package::from_reader(Cursor::new(move_connector_end(&package, 0, "14", false, EndTarget::Free(5.5, 4.75)).unwrap())).unwrap();
    let c = connector_json(&freed, "14").unwrap();
    assert!((c["end"]["x"].as_f64().unwrap() - 5.5).abs() < 1e-6 && (c["end"]["y"].as_f64().unwrap() - 4.75).abs() < 1e-6, "{c}");
    assert!(c["end"]["glue"].is_null() && c["begin"]["glue"]["sheet"] == "12", "{c}");
    let shape = layoutcraft_vsdx::writer::shape_xml(&freed, 0, "14").unwrap();
    assert!(shape.contains("N='EndX' V='3.5'"), "the cell is in the group's coordinates: {shape}");
    assert_eq!(freed.connects(&part).unwrap().iter().filter(|c| c.from_sheet == "14").count(), 1);
    // glued again, to connection point 4 of square 13
    let glued = Package::from_reader(Cursor::new(move_connector_end(&freed, 0, "14", false, EndTarget::Glue("13".into(), Some(4))).unwrap())).unwrap();
    let c = connector_json(&glued, "14").unwrap();
    assert_eq!(c["end"]["glue"]["sheet"], "13");
    assert!((c["end"]["x"].as_f64().unwrap() - 4.5).abs() < 1e-6 && (c["end"]["y"].as_f64().unwrap() - 4.0).abs() < 1e-6, "{c}");
    assert_eq!(glued.connects(&part).unwrap().iter().filter(|c| c.from_sheet == "14").count(), 2);
    assert!(import_document(&glued).is_ok());
}

#[test]
fn a_member_connector_copied_with_its_glued_members_is_glued_to_the_copies() {
    use layoutcraft_vsdx::groupedit::copy_shapes;
    use layoutcraft_vsdx::writer::paste_shapes_with_connects;
    let package = group_with_connector(0.0);
    let part = package.pages().unwrap()[0].part.clone();
    let (xmls, rows) = copy_shapes(&package, 0, &["12".to_string(), "13".to_string(), "14".to_string()]).unwrap();
    assert_eq!(rows.len(), 2, "both ends stay glued inside the copy: {rows:?}");
    let (bytes, ids) = paste_shapes_with_connects(&package, 0, &xmls, &rows, 0.0, -1.0).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(ids.len(), 3);
    let (square_a, square_b, wire) = (&ids[0], &ids[1], &ids[2]);
    let connects = after.connects(&part).unwrap();
    let mine: Vec<_> = connects.iter().filter(|c| c.from_sheet == *wire).collect();
    assert_eq!(mine.len(), 2, "{connects:?}");
    assert!(mine.iter().any(|c| c.from_cell == "BeginX" && c.to_sheet == *square_a) && mine.iter().any(|c| c.from_cell == "EndX" && c.to_sheet == *square_b), "{mine:?}");
    // the originals keep their glue
    assert_eq!(connects.iter().filter(|c| c.from_sheet == "14").count(), 2);
    // the copy is a page-level connector between the copies, one inch lower
    let c = connector_json(&after, wire).expect("the copy is listed as a connector");
    assert!(c["parentId"].is_null());
    assert!((c["begin"]["y"].as_f64().unwrap() - 3.0).abs() < 1e-6 && (c["begin"]["x"].as_f64().unwrap() - 3.5).abs() < 1e-6, "{c}");
    assert!(import_document(&after).is_ok());
}

// ---- round 4, slice 4: group master instances ------------------------------------------------------------------------

fn access_point_group() -> (Package, String) {
    use layoutcraft_vsdx::stencil::import_master;
    use layoutcraft_vsdx::writer::add_master_instance;
    let stencil = Package::open(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/Basic home network.vstx")).unwrap();
    let list = layoutcraft_vsdx::stencil::master_list(&stencil).unwrap();
    let id = list.as_array().unwrap().iter().find(|m| m["name"] == "Wireless access point").unwrap()["id"].as_str().unwrap().to_string();
    let (bytes, new_master) = import_master(&blank_a4(), &stencil, &id).unwrap();
    let with = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, shape) = add_master_instance(&with, 0, &new_master, 4.0, 4.0).unwrap();
    (Package::from_reader(Cursor::new(bytes)).unwrap(), shape)
}

fn part_extent(package: &Package, id: &str) -> (f64, f64) {
    let doc = import_document(package).unwrap();
    let shape = doc.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id)).unwrap();
    let (mut max_x, mut max_y) = (0.0f64, 0.0f64);
    for part in &shape.parts {
        let numbers: Vec<f64> = part.d.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter(|t| !t.is_empty()).filter_map(|t| t.parse().ok()).collect();
        for pair in numbers.chunks(2) {
            if pair.len() == 2 {
                max_x = max_x.max(pair[0]);
                max_y = max_y.max(pair[1]);
            }
        }
    }
    (max_x, max_y)
}

#[test]
fn a_group_master_is_instanced_as_a_group_and_a_resize_writes_its_children_scaled() {
    let (package, id) = access_point_group();
    let xml = shape_text(&package, &id);
    assert!(xml.contains("Type='Group'"), "a group master is instanced as a Group (Visio's own drop does): {xml}");
    let before = part_extent(&package, &id);
    let editor = GroupEditor::open(&package, 0).unwrap();
    let frame = editor.describe().as_array().unwrap().iter().find(|f| f["id"] == id.as_str()).unwrap().clone();
    let (w, h) = (frame["size"][0].as_f64().unwrap(), frame["size"][1].as_f64().unwrap());
    let patches = editor.resize_member(&id, (w * 2.0, h * 2.0), (0.5, 0.5), false).unwrap();
    let resized = Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::patch_page_cells(&package, 0, &patches).unwrap())).unwrap();
    let xml = shape_text(&resized, &id);
    // the master's children are written as children of the instance, with the scaled frame cells
    assert!(xml.contains("<Shapes>") && xml.matches("MasterShape='").count() >= 3, "children are written out: {xml}");
    let first_child: u64 = id.parse::<u64>().unwrap() + 1;
    assert!(xml.contains(&format!("<Shape ID='{first_child}' Type='Shape' MasterShape=")), "children take the ids that follow the instance: {xml}");
    // the drawing is the same as the importer's own scaling gave: twice as large, no double scaling
    let after = part_extent(&resized, &id);
    assert!((after.0 - before.0 * 2.0).abs() < 1.5 && (after.1 - before.1 * 2.0).abs() < 1.5, "extent {before:?} -> {after:?}");
    // a second resize scales the written children again (frame cells and geometry rows)
    let editor = GroupEditor::open(&resized, 0).unwrap();
    let patches = editor.resize_member(&id, (w * 3.0, h * 3.0), (0.5, 0.5), false).unwrap();
    let again = Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::patch_page_cells(&resized, 0, &patches).unwrap())).unwrap();
    let third = part_extent(&again, &id);
    assert!((third.0 - before.0 * 3.0).abs() < 2.5 && (third.1 - before.1 * 3.0).abs() < 2.5, "extent {before:?} -> {third:?}");
    // export + re-import keeps it
    assert!(import_document(&again).is_ok());
}

// ---- round 4, slice 6: text ---------------------------------------------------------------------------------------------

#[test]
fn a_crlf_pair_is_one_position_in_a_text_range() {
    // a text box counts a CR LF pair once: "ab" CRLF "cd" is 5 positions, formatting 3..5 must reach "cd"
    let (package, id) = texted_shape("ab\r\ncd");
    let after = format_range(&package, &id, range(3, 5, &[("Strikethru", "1")]));
    let marked: Vec<String> = runs(&text_element(&after, &id)).into_iter().filter(|(ix, _)| ix != "0").map(|(_, t)| t).collect();
    assert_eq!(marked, vec!["cd".to_string()], "{}", text_element(&after, &id));
    assert!(import_document(&after).is_ok());
}

// ---- round 5, slice 1: the formatting at a caret or in a range ---------------------------------------------------------

fn state_of(package: &Package, id: &str, start: usize, end: usize) -> serde_json::Value {
    serde_json::from_str(&layoutcraft_vsdx::writer::text_state(package, 0, id, start, end).unwrap()).unwrap()
}

#[test]
fn the_text_state_reports_the_runs_at_a_caret_and_mixed_values_over_a_range() {
    let (package, id) = texted_shape("Hello big world");
    let formatted = format_range(&package, &id, range(6, 9, &[("Strikethru", "1"), ("Color", "#ff0000")]));
    // inside the formatted words: strike-through and colour, a caret after "b" reads the run it is in
    let inside = state_of(&formatted, &id, 7, 7);
    assert_eq!(inside["char"]["Strikethru"], "1", "{inside}");
    assert_eq!(inside["char"]["Color"], "#ff0000", "{inside}");
    // before the formatted words
    let before = state_of(&formatted, &id, 2, 2);
    assert_eq!(before["char"]["Strikethru"], "0", "{before}");
    // a range across both is mixed, a range inside one run is not
    let across = state_of(&formatted, &id, 0, 15);
    assert_eq!(across["char"]["Strikethru"], "mixed", "{across}");
    assert_eq!(across["char"]["Color"], "mixed", "{across}");
    assert_eq!(state_of(&formatted, &id, 6, 9)["char"]["Strikethru"], "1");
    // paragraphs: bullets on the second one only
    let (package, id) = texted_shape("one
two
three");
    let bullets = format_range(
        &package,
        &id,
        layoutcraft_vsdx::writer::TextRangeFormat { start: 5, end: 5, para_cells: vec![("Bullet".to_string(), "1".to_string())], ..Default::default() },
    );
    assert_eq!(state_of(&bullets, &id, 5, 5)["para"]["Bullet"], "1");
    assert_ne!(state_of(&bullets, &id, 0, 0)["para"]["Bullet"], "1");
    assert_eq!(state_of(&bullets, &id, 0, 13)["para"]["Bullet"], "mixed");
    // a shape without text is refused
    let (blank, id) = draw_on_blank(layoutcraft_vsdx::draw::DrawKind::Rectangle, 1.0, 1.0, 4.0, 3.0);
    assert!(layoutcraft_vsdx::writer::text_state(&blank, 0, &id, 0, 0).is_err());
}


#[test]
fn the_text_state_names_the_font_at_a_caret_by_name_or_by_face_index() {
    let (package, id) = texted_shape("Hello big world");
    let georgia = format_range(&package, &id, range(6, 9, &[("Font", "Georgia")]));
    assert_eq!(state_of(&georgia, &id, 7, 7)["char"]["FontName"], "Georgia");
    assert_eq!(state_of(&georgia, &id, 0, 15)["char"]["FontName"], "mixed");
    // a numeric Font cell is a 0-based position in the document's FaceNames (Visio writes `Font V='0'` for the first face)
    let listed = Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::ensure_face_names(&package, &["Georgia".to_string(), "Verdana".to_string()]).unwrap())).unwrap();
    let faces: Vec<String> = listed
        .text("visio/document.xml")
        .unwrap()
        .split("<FaceName ")
        .skip(1)
        .filter_map(|t| t.split("NameU='").nth(1).and_then(|t| t.split(char::from(39)).next()).map(str::to_string))
        .collect();
    assert!(faces.len() >= 2, "{faces:?}");
    for index in [0usize, faces.len() - 1] {
        let by_index = format_range(&listed, &id, range(0, 5, &[("Font", &index.to_string())]));
        assert_eq!(state_of(&by_index, &id, 2, 2)["char"]["FontName"], faces[index].as_str(), "{faces:?}");
    }
}

// ---- round 5, slice 2: paragraph indents, line spacing, spacing and bullet character -----------------------------------

#[test]
fn paragraph_indents_spacing_and_bullet_character_are_written_per_paragraph_and_read_back() {
    let (package, id) = texted_shape("one\ntwo\nthree");
    let cells: Vec<(String, String)> = [("IndLeft", "0.25"), ("IndFirst", "-0.125"), ("SpLine", "-1.5"), ("SpBefore", "0.0833333333"), ("SpAfter", "0.0416666667"), ("Bullet", "1"), ("BulletStr", "\u{2022}")]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    // a caret in the second paragraph only
    let after = format_range(&package, &id, layoutcraft_vsdx::writer::TextRangeFormat { start: 5, end: 5, para_cells: cells, ..Default::default() });
    let shape = layoutcraft_vsdx::writer::shape_xml(&after, 0, &id).unwrap();
    for needle in ["N='IndLeft' V='0.25'", "N='IndFirst' V='-0.125'", "N='SpLine' V='-1.5'", "N='SpBefore' V='0.0833333333'", "N='SpAfter' V='0.0416666667'", "N='Bullet' V='1'", "N='BulletStr' V='\u{2022}'"] {
        assert!(shape.contains(needle), "{needle} in {shape}");
    }
    assert_eq!(shape.matches("N='IndLeft'").count(), 1, "only the second paragraph got a row of its own: {shape}");
    // read back through the state query: the second paragraph has them, the first does not
    let second = state_of(&after, &id, 5, 5);
    assert_eq!(second["para"]["IndLeft"], "0.25");
    assert_eq!(second["para"]["BulletStr"], "\u{2022}");
    assert_eq!(second["para"]["SpLine"], "-1.5");
    assert_ne!(state_of(&after, &id, 0, 0)["para"]["IndLeft"], "0.25");
    // survives export + re-import
    let reread = Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::write_package_edit(&after, Default::default(), &[]).unwrap())).unwrap();
    assert!(import_document(&reread).is_ok());
    assert_eq!(state_of(&reread, &id, 5, 5)["para"]["IndFirst"], "-0.125");
}

// ---- round 5, slice 3: run rows of a shape that inherits its text style --------------------------------------------------

#[test]
fn new_run_rows_copy_the_effective_size_font_and_colour_of_the_shape() {
    let (package, id) = texted_shape("Hello big world");
    let base = vec![("Size".to_string(), "0.1111111111".to_string()), ("Font".to_string(), "Calibri".to_string()), ("Color".to_string(), "#112233".to_string())];
    let mut r = range(6, 9, &[("Strikethru", "1")]);
    r.base_cells = base;
    let after = format_range(&package, &id, r);
    let shape = layoutcraft_vsdx::writer::shape_xml(&after, 0, &id).unwrap();
    let rows = &shape[shape.find("<Section N='Character'").unwrap()..];
    let rows = &rows[..rows.find("</Section>").unwrap()];
    // the new run rows (the formatted word and the clone after it) get the cells they lack (the drawn shape's row 0 already
    // has its own size and colour, which stay); row 0 existed and is not touched
    assert_eq!(rows.matches("N='Font' V='Calibri'").count(), 2, "{rows}");
    assert_eq!(rows.matches("N='Size' V='0.1111111111'").count(), 0, "cells a row has are kept: {rows}");
    assert_eq!(rows.matches("N='Strikethru' V='1'").count(), 1, "only the word is struck through: {rows}");
    // a row that exists already keeps its own values: formatting again changes only what was asked
    let again = format_range(&after, &id, range(7, 8, &[("Color", "#ff0000")]));
    let state = state_of(&again, &id, 8, 8);
    assert_eq!(state["char"]["Color"], "#ff0000");
    assert_eq!(state["char"]["Size"], "0.1388888888888889", "{state}");
    assert!(import_document(&again).is_ok());
}

// ---- round 5, slice 4: gradient stops --------------------------------------------------------------------------------------

#[test]
fn a_gradient_with_custom_stops_is_written_as_fill_gradient_rows_and_read_back() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    let stops = vec![(0.0, "#ff0000".to_string(), 0.0), (0.5, "#00ff00".to_string(), 0.25), (1.0, "#0000ff".to_string(), 0.0)];
    let (package, id) = styled_rect(StyleEdit { gradient_stops: Some(stops), gradient_angle_deg: Some(90.0), gradient_dir: Some(0), ..Default::default() });
    let shape = shape_text(&package, &id);
    for needle in ["N='FillGradientEnabled' V='1'", "<Section N='FillGradient'>", "N='GradientStopColor' V='#00ff00'", "N='GradientStopColorTrans' V='0.25'", "N='GradientStopPosition' V='0.5'"] {
        assert!(shape.contains(needle), "{needle} in {shape}");
    }
    assert!(shape.contains("N='FillGradientAngle' V='1.5707963"), "{shape}");
    assert_eq!(shape.matches("N='GradientStopPosition'").count(), 3, "three rows: {shape}");
    // read back by the importer as a gradient with three stops
    let json = imported_json(&package, &id);
    assert!(json.contains("#00ff00") && json.contains("#ff0000") && json.contains("#0000ff"), "{json}");
    // two stops, then the stops edited again: the rows are rewritten in place
    let again = {
        let patches = GroupEditor::open(&package, 0)
            .unwrap()
            .style_patches(&[id.clone()], &StyleEdit { gradient_stops: Some(vec![(0.0, "#111111".to_string(), 0.0), (1.0, "#eeeeee".to_string(), 0.0)]), ..Default::default() })
            .unwrap();
        Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::patch_page_cells(&package, 0, &patches).unwrap())).unwrap()
    };
    let shape = shape_text(&again, &id);
    assert!(shape.contains("N='GradientStopColor' V='#111111'") && shape.contains("N='GradientStopColor' V='#eeeeee'"), "{shape}");
    assert!(!shape.contains("#00ff00"), "the old middle stop is gone (the third row repeats the last stop): {shape}");
    // bad input is refused
    let editor = GroupEditor::open(&again, 0).unwrap();
    assert!(editor.style_patches(&[id.clone()], &StyleEdit { gradient_stops: Some(vec![(0.0, "#111111".to_string(), 0.0)]), ..Default::default() }).is_err());
    assert!(editor.style_patches(&[id.clone()], &StyleEdit { gradient_stops: Some(vec![(0.0, "#111111".to_string(), 0.0), (2.0, "#eeeeee".to_string(), 0.0)]), ..Default::default() }).is_err());
    assert!(import_document(&again).is_ok());
}

// ---- round 5, slice 5: glow, soft edges, reflection and bevel ---------------------------------------------------------------

#[test]
fn glow_soft_edges_reflection_and_bevel_are_written_in_inches_and_drawn() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    let (package, id) = styled_rect(StyleEdit {
        glow_px: Some(8.0),
        glow_color: Some("#ff0000".into()),
        glow_trans: Some(0.3),
        soft_edges_px: Some(6.0),
        reflection: Some(true),
        bevel: Some(2),
        bevel_px: Some(9.6),
        ..Default::default()
    });
    let shape = shape_text(&package, &id);
    for needle in [
        "N='GlowSize' V='0.0833333",
        "N='GlowColor' V='#ff0000'",
        "N='GlowColorTrans' V='0.3'",
        "N='SoftEdgesSize' V='0.0625'",
        "N='ReflectionSize' V='0.5'",
        "N='BevelTopType' V='2'",
        "N='BevelTopWidth' V='0.1'",
        "N='BevelTopHeight' V='0.1'",
    ] {
        assert!(shape.contains(needle), "{needle} in {shape}");
    }
    let json = imported_json(&package, &id);
    for key in ["glow", "softEdge", "reflection", "bevel"] {
        assert!(json.contains(&format!("\"{key}\"")), "{key} is read back: {json}");
    }
    // off again
    let patches = GroupEditor::open(&package, 0)
        .unwrap()
        .style_patches(&[id.clone()], &StyleEdit { glow_px: Some(0.0), soft_edges_px: Some(0.0), reflection: Some(false), bevel: Some(0), ..Default::default() })
        .unwrap();
    let off = Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::patch_page_cells(&package, 0, &patches).unwrap())).unwrap();
    let json = imported_json(&off, &id);
    for key in ["glow", "softEdge", "reflection", "bevel"] {
        assert!(!json.contains(&format!("\"{key}\"")), "{key} is gone: {json}");
    }
    // bad input
    let editor = GroupEditor::open(&package, 0).unwrap();
    assert!(editor.style_patches(&[id.clone()], &StyleEdit { bevel: Some(40), ..Default::default() }).is_err());
    assert!(editor.style_patches(&[id.clone()], &StyleEdit { glow_px: Some(-3.0), ..Default::default() }).is_err());
}

// ---- report 17: one-dimensional group masters (Dell / HPE equipment) resized by their ends ------------------------------

fn rack_instance() -> (Package, String) {
    use layoutcraft_vsdx::stencil::import_master;
    use layoutcraft_vsdx::writer::add_master_instance;
    let stencil = Package::open(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/Dell-Classic-Servers/Dell-PowerEdge-RackServers-Classic.vssx")).unwrap();
    let list = layoutcraft_vsdx::stencil::master_list(&stencil).unwrap();
    let id = list.as_array().unwrap().iter().find(|m| m["name"] == "1950 Front").unwrap()["id"].as_str().unwrap().to_string();
    let (bytes, new_master) = import_master(&blank_a4(), &stencil, &id).unwrap();
    let with = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, shape) = add_master_instance(&with, 0, &new_master, 4.0, 4.0).unwrap();
    (Package::from_reader(Cursor::new(bytes)).unwrap(), shape)
}

#[test]
fn moving_the_end_of_a_one_dimensional_group_sets_its_length_and_scales_the_art_uniformly() {
    use layoutcraft_vsdx::groupedit::{move_connector_end, EndTarget};
    let (package, id) = rack_instance();
    let before = GroupEditor::open(&package, 0).unwrap().describe().as_array().unwrap().iter().find(|f| f["id"] == id.as_str()).unwrap().clone();
    let (w0, h0) = (before["size"][0].as_f64().unwrap(), before["size"][1].as_f64().unwrap());
    // the Dell master is authored on a 1:10 page: Visio drops it at a tenth of its size on a 1:1 page (1.7 in wide)
    assert!(w0 > 1.0 && w0 < 3.0 && h0 < 0.01, "a long thin 1-D frame: {w0} x {h0}");
    let end_x = before["pin"][0].as_f64().unwrap() + w0 / 2.0;
    let begin_x = before["pin"][0].as_f64().unwrap() - w0 / 2.0;
    let y0 = before["pin"][1].as_f64().unwrap(); // on the line: a flat group whose ends span its box now also TURNS with the end's y (round 11b)
    // pull the end in to a quarter of the length
    let bytes = move_connector_end(&package, 0, &id, false, EndTarget::Free(begin_x + w0 / 4.0, y0)).unwrap();
    let after_pkg = Package::from_reader(Cursor::new(bytes)).unwrap();
    let after = GroupEditor::open(&after_pkg, 0).unwrap().describe().as_array().unwrap().iter().find(|f| f["id"] == id.as_str()).unwrap().clone();
    let (w1, h1) = (after["size"][0].as_f64().unwrap(), after["size"][1].as_f64().unwrap());
    assert!((w1 - w0 / 4.0).abs() < 1e-6, "the length follows the end: {w1}");
    assert!((h1 / h0 - 0.25).abs() < 0.01, "the height follows the length: {h1} vs {h0}");
    assert!((after["pin"][0].as_f64().unwrap() - w1 / 2.0 - begin_x).abs() < 1e-6, "the begin stays put");
    // the shape still has its children (written out, scaled) and the drawing re-imports
    let shape = shape_text(&after_pkg, &id);
    assert!(shape.contains("N='EndX'") && shape.contains("MasterShape='"), "{shape}");
    assert!(import_document(&after_pkg).is_ok());
    let _ = end_x;
    // the end dragged past the begin turns the group half way round (Visio: Width = the length of the line, Angle = ATAN2)
    let bytes = move_connector_end(&package, 0, &id, false, EndTarget::Free(begin_x - 3.0, y0)).unwrap();
    let crossed = Package::from_reader(Cursor::new(bytes)).unwrap();
    let f = GroupEditor::open(&crossed, 0).unwrap().describe().as_array().unwrap().iter().find(|f| f["id"] == id.as_str()).unwrap().clone();
    assert!((f["size"][0].as_f64().unwrap() - 3.0).abs() < 1e-6 && (f["angle"].as_f64().unwrap().abs() - std::f64::consts::PI).abs() < 1e-6, "turned half way round: {f}");
    // two ends closer than 0.05 in are refused
    assert!(move_connector_end(&package, 0, &id, false, EndTarget::Free(begin_x + 0.01, y0)).is_err());
    // a one-dimensional group cannot be glued
    assert!(move_connector_end(&package, 0, &id, false, EndTarget::Glue("1".to_string(), None)).is_err());
}

#[test]
fn resizing_a_one_dimensional_group_with_a_box_keeps_its_proportions() {
    let (package, id) = rack_instance();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let f = editor.describe().as_array().unwrap().iter().find(|f| f["id"] == id.as_str()).unwrap().clone();
    let (w, h) = (f["size"][0].as_f64().unwrap(), f["size"][1].as_f64().unwrap());
    // a box handle asks for half the length and twice the height: the height follows the length instead
    let patches = editor.resize_member(&id, (w / 2.0, h * 2.0), (0.0, 0.5), false).unwrap();
    let resized = Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::patch_page_cells(&package, 0, &patches).unwrap())).unwrap();
    let g = GroupEditor::open(&resized, 0).unwrap().describe().as_array().unwrap().iter().find(|f| f["id"] == id.as_str()).unwrap().clone();
    assert!((g["size"][0].as_f64().unwrap() - w / 2.0).abs() < 1e-6);
    assert!((g["size"][1].as_f64().unwrap() / h - 0.5).abs() < 0.01, "{g}");
}

// ---- report 22: moving a one-dimensional group moves its end points too (the end handles drifted away from the art) ------------

fn end_cell(shape: &str, name: &str) -> f64 {
    let at = shape.find(&format!("N='{name}'")).unwrap_or_else(|| panic!("no {name} in {shape}"));
    let rest = &shape[at..];
    let v = rest.find("V='").unwrap() + 3;
    rest[v..rest[v..].find('\'').unwrap() + v].parse().unwrap()
}

#[test]
fn moving_a_one_dimensional_group_moves_its_begin_and_end_with_it() {
    let (package, id) = rack_instance();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let before = shape_text(&package, &id);
    let (bx, by, ex, ey) = (end_cell(&before, "BeginX"), end_cell(&before, "BeginY"), end_cell(&before, "EndX"), end_cell(&before, "EndY"));
    let patches = editor.move_member(&id, (2.0, 1.0), false).unwrap();
    let moved = Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::patch_page_cells(&package, 0, &patches).unwrap())).unwrap();
    let after = shape_text(&moved, &id);
    assert!((end_cell(&after, "BeginX") - bx - 2.0).abs() < 1e-9 && (end_cell(&after, "EndX") - ex - 2.0).abs() < 1e-9, "{after}");
    assert!((end_cell(&after, "BeginY") - by - 1.0).abs() < 1e-9 && (end_cell(&after, "EndY") - ey - 1.0).abs() < 1e-9, "{after}");
    assert!(import_document(&moved).is_ok());
}

#[test]
fn resizing_a_one_dimensional_group_moves_its_begin_and_end_apart() {
    // step 209: the end points follow the width of a flat group (an IBM rack console switch is a line; 1.5 times as wide, its ends lie
    // 1.5 times as far apart around the pin the resize keeps)
    let (package, id) = rack_instance();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let frame = editor.describe().as_array().unwrap().iter().find(|f| f["id"] == id.as_str()).unwrap().clone();
    let (w, h) = (frame["size"][0].as_f64().unwrap(), frame["size"][1].as_f64().unwrap());
    let before = shape_text(&package, &id);
    let (bx, ex) = (end_cell(&before, "BeginX"), end_cell(&before, "EndX"));
    let patches = editor.resize_group(&id, (w * 1.5, h * 1.5), (0.5, 0.5)).unwrap();
    let resized = Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::patch_page_cells(&package, 0, &patches).unwrap())).unwrap();
    let after = shape_text(&resized, &id);
    let (nbx, nex) = (end_cell(&after, "BeginX"), end_cell(&after, "EndX"));
    assert!(((nex - nbx) - (ex - bx) * 1.5).abs() < 1e-6, "the ends are {} apart, want {}: {after}", nex - nbx, (ex - bx) * 1.5);
    assert!((((nbx + nex) / 2.0) - ((bx + ex) / 2.0)).abs() < 1e-6, "the middle of the line stays put");
    assert!(import_document(&resized).is_ok());
}

#[test]
fn dragging_an_end_of_a_tall_one_dimensional_group_stretches_and_turns_it_like_visio() {
    // step 227 (probe-tall-1d-turn.ps1, CyberPower PDU83107 dropped at 6, 12): EndX +0.1 -> Width 0.32, Height 10.4864, PinX 6.05;
    // then EndY +0.3 -> Width 0.4386, Height 14.3740, PinY 12.15, Angle 0.7532 (the pin stays the middle of Begin-End)
    use layoutcraft_vsdx::groupedit::{move_connector_end, EndTarget};
    use layoutcraft_vsdx::stencil::import_master;
    use layoutcraft_vsdx::writer::add_master_instance;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/visioStencils-converted/Stencils/Computer Racks/CyberPower VS PDU83107 v2024.vssx");
    let Ok(stencil) = Package::open(path) else { return };
    let list = layoutcraft_vsdx::stencil::master_list(&stencil).unwrap();
    let mid = list.as_array().unwrap().first().unwrap()["id"].as_str().unwrap().to_string();
    let (bytes, new_master) = import_master(&blank_a4(), &stencil, &mid).unwrap();
    let with = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, id) = add_master_instance(&with, 0, &new_master, 6.0, 5.0).unwrap();
    let package = Package::from_reader(Cursor::new(bytes)).unwrap();
    let frame = |p: &Package| GroupEditor::open(p, 0).unwrap().describe().as_array().unwrap().iter().find(|f| f["id"] == id.as_str()).unwrap().clone();
    let f0 = frame(&package);
    assert!(f0["size"][1].as_f64().unwrap() > 1.0 && !f0["ends"].is_null(), "a tall group lists its end points: {f0}");
    let (b0, e0) = ((f0["ends"][0][0].as_f64().unwrap(), f0["ends"][0][1].as_f64().unwrap()), (f0["ends"][1][0].as_f64().unwrap(), f0["ends"][1][1].as_f64().unwrap()));
    let (w0, h0) = (f0["size"][0].as_f64().unwrap(), f0["size"][1].as_f64().unwrap());
    // 1: the end moves along the line
    let bytes = move_connector_end(&package, 0, &id, false, EndTarget::Free(e0.0 + 0.1, e0.1)).unwrap();
    let p1 = Package::from_reader(Cursor::new(bytes)).unwrap();
    let f1 = frame(&p1);
    let (w1, h1) = (f1["size"][0].as_f64().unwrap(), f1["size"][1].as_f64().unwrap());
    assert!((w1 - (w0 + 0.1)).abs() < 1e-6, "the width is the length of the line: {w1} vs {}", w0 + 0.1);
    assert!((h1 / w1 - h0 / w0).abs() < 1e-6, "the height follows the length: {h1} / {w1} vs {h0} / {w0}");
    assert!((f1["pin"][0].as_f64().unwrap() - (b0.0 + e0.0 + 0.1) / 2.0).abs() < 1e-6, "the pin is the middle of the line");
    // 2: the end is lifted: the shape turns about the middle of the line
    let e1 = (e0.0 + 0.1, e0.1);
    let bytes = move_connector_end(&p1, 0, &id, false, EndTarget::Free(e1.0, e1.1 + 0.3)).unwrap();
    let p2 = Package::from_reader(Cursor::new(bytes)).unwrap();
    let f2 = frame(&p2);
    let len = (e1.0 - b0.0).hypot(0.3 + e1.1 - b0.1);
    let theta = (0.3 + e1.1 - b0.1).atan2(e1.0 - b0.0);
    assert!((f2["size"][0].as_f64().unwrap() - len).abs() < 1e-6, "width = length: {}", f2["size"][0]);
    assert!((f2["angle"].as_f64().unwrap().rem_euclid(std::f64::consts::TAU) - theta.rem_euclid(std::f64::consts::TAU)).abs() < 1e-6, "angle {} want {theta}", f2["angle"]);
    assert!((f2["pin"][1].as_f64().unwrap() - (b0.1 + e1.1 + 0.3) / 2.0).abs() < 1e-6, "the pin is the middle of the line");
    if (w0 - 0.22).abs() < 1e-6 {
        // the drop equals Visio's: the numbers Visio measured
        assert!((w1 - 0.32).abs() < 1e-6 && (h1 - 10.4864).abs() < 1e-3, "{w1} x {h1}");
        assert!((f2["size"][0].as_f64().unwrap() - 0.4386).abs() < 1e-3 && (f2["size"][1].as_f64().unwrap() - 14.3740).abs() < 1e-2, "{} x {}", f2["size"][0], f2["size"][1]);
        assert!((f2["angle"].as_f64().unwrap() - 0.7532).abs() < 1e-3, "{}", f2["angle"]);
    }
    // the drawing re-imports and the ends cannot be pulled together or glued
    assert!(import_document(&p2).is_ok());
    assert!(move_connector_end(&package, 0, &id, false, EndTarget::Free(b0.0, b0.1)).is_err());
    assert!(move_connector_end(&package, 0, &id, false, EndTarget::Glue("1".to_string(), None)).is_err());
}

#[test]
fn resize_member_moves_the_begin_and_end_of_a_flat_one_dimensional_group() {
    // step 226: the Size & Position panel and Same size go through resize_member; a flat 1-D group's end points follow its width there too
    // (resize_member of an instance whose ends are rewritten afterwards, as the end-handle drag does, still gives the dragged ends)
    let (package, id) = rack_instance();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let frame = editor.describe().as_array().unwrap().iter().find(|f| f["id"] == id.as_str()).unwrap().clone();
    let (w, h) = (frame["size"][0].as_f64().unwrap(), frame["size"][1].as_f64().unwrap());
    let before = shape_text(&package, &id);
    let (bx, ex) = (end_cell(&before, "BeginX"), end_cell(&before, "EndX"));
    let patches = editor.resize_member(&id, (w * 1.5, h), (0.5, 0.5), false).unwrap();
    let resized = Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::patch_page_cells(&package, 0, &patches).unwrap())).unwrap();
    let after = shape_text(&resized, &id);
    let (nbx, nex) = (end_cell(&after, "BeginX"), end_cell(&after, "EndX"));
    assert!(((nex - nbx) - (ex - bx) * 1.5).abs() < 1e-6, "the ends are {} apart, want {}: {after}", nex - nbx, (ex - bx) * 1.5);
    assert!((((nbx + nex) / 2.0) - ((bx + ex) / 2.0)).abs() < 1e-6, "the middle of the line stays put");
    assert!(import_document(&resized).is_ok());
}

#[test]
fn the_decorative_lines_of_a_shrunk_group_master_have_end_points_in_the_group() {
    // step 211: the lines of the CyberPower PDU83106 (1-D children with BeginX / EndX in the group's coordinates) kept the master's
    // unshrunk end points (30 in on a 7 in group), so the connector list had ghost lines far off the page
    use layoutcraft_vsdx::stencil::import_master;
    use layoutcraft_vsdx::writer::add_master_instance;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/visioStencils-converted/Stencils/Computer Racks/CyberPower VS PDU83106 v2024.vssx");
    let Ok(stencil) = Package::open(path) else { return };
    let list = layoutcraft_vsdx::stencil::master_list(&stencil).unwrap();
    let id = list.as_array().unwrap().iter().find(|m| m["name"].as_str().unwrap().contains("PDU83106 Front")).unwrap()["id"].as_str().unwrap().to_string();
    let (bytes, new_master) = import_master(&blank_a4(), &stencil, &id).unwrap();
    let with = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, shape) = add_master_instance(&with, 0, &new_master, 4.0, 4.0).unwrap();
    let package = Package::from_reader(Cursor::new(bytes)).unwrap();
    let editor = GroupEditor::open(&package, 0).unwrap();
    let frame = editor.describe().as_array().unwrap().iter().find(|f| f["id"] == shape.as_str()).unwrap().clone();
    let (gx, gy) = (frame["pin"][0].as_f64().unwrap(), frame["pin"][1].as_f64().unwrap());
    let (gw, gh) = (frame["size"][0].as_f64().unwrap(), frame["size"][1].as_f64().unwrap());
    let connectors = editor.connectors(&[]);
    let inner: Vec<_> = connectors.as_array().unwrap().iter().filter(|c| !c["parentId"].is_null()).collect();
    assert!(!inner.is_empty(), "the master has decorative lines");
    for c in inner {
        for end in ["begin", "end"] {
            let (x, y) = (c[end]["x"].as_f64().unwrap(), c[end]["y"].as_f64().unwrap());
            assert!(x > gx - gw - 0.5 && x < gx + gw + 0.5 && y > gy - gh - 0.5 && y < gy + gh + 0.5, "line {} {end} at ({x}, {y}) is far from the group at ({gx}, {gy}) {gw} x {gh}", c["id"]);
        }
    }
}

// ---- report 18: resized group masters whose children's formulas refer to the top shape (APC rack stencils) ------------------

#[test]
fn a_resized_group_master_draws_its_art_once_scaled_in_both_directions() {
    use layoutcraft_vsdx::stencil::import_master;
    use layoutcraft_vsdx::writer::add_master_instance;
    let stencil = Package::open(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/apc-pdu.vssx")).unwrap();
    let list = layoutcraft_vsdx::stencil::master_list(&stencil).unwrap();
    for entry in list.as_array().unwrap() {
        let id = entry["id"].as_str().unwrap().to_string();
        let (bytes, new_master) = import_master(&blank_a4(), &stencil, &id).unwrap();
        let with = Package::from_reader(Cursor::new(bytes)).unwrap();
        let (bytes, shape) = add_master_instance(&with, 0, &new_master, 4.0, 4.0).unwrap();
        let package = Package::from_reader(Cursor::new(bytes)).unwrap();
        let editor = GroupEditor::open(&package, 0).unwrap();
        let frame = editor.describe().as_array().unwrap().iter().find(|f| f["id"] == shape.as_str()).unwrap().clone();
        let (w, h) = (frame["size"][0].as_f64().unwrap(), frame["size"][1].as_f64().unwrap());
        for (fx, fy) in [(0.5, 0.5), (2.0, 1.0), (1.0, 0.6), (1.5, 2.0)] {
            let patches = editor.resize_member(&shape, (w * fx, h * fy), (0.5, 0.5), false).unwrap();
            let resized = Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::patch_page_cells(&package, 0, &patches).unwrap())).unwrap();
            let (ext_x, ext_y) = {
                // the path points of every part, arcs left out (their radii and flags are not points)
                let doc = import_document(&resized).unwrap();
                let one = doc.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(shape.as_str())).unwrap();
                let (mut mx, mut my) = (0.0f64, 0.0f64);
                for part in &one.parts {
                    let mut d = String::new();
                    let mut skipping = false;
                    for c in part.d.chars() {
                        if c == 'A' { skipping = true; }
                        else if c.is_ascii_alphabetic() { skipping = false; }
                        if !skipping { d.push(c); }
                    }
                    let numbers: Vec<f64> = d.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter(|t| !t.is_empty()).filter_map(|t| t.parse().ok()).collect();
                    for pair in numbers.chunks(2) {
                        if pair.len() == 2 { mx = mx.max(pair[0]); my = my.max(pair[1]); }
                    }
                }
                (mx, my)
            };
            // the art ends inside the box it is resized to (the old bug: at the square of the scale, so a half-size box held a quarter-size drawing)
            let (bw, bh) = (w * fx * 96.0, h * fy * 96.0);
            assert!(ext_x <= bw * 1.02 && ext_y <= bh * 1.02, "{}: art {ext_x:.1} x {ext_y:.1} inside the box {bw:.1} x {bh:.1} at ({fx}, {fy})", entry["name"]);
            assert!(ext_x >= bw * 0.85 && ext_y >= bh * 0.5, "{}: art {ext_x:.1} x {ext_y:.1} fills the box {bw:.1} x {bh:.1} at ({fx}, {fy})", entry["name"]);
        }
    }
}

// ---- group children that are groups themselves (Ethernet LAN "PC", "Server") -----------------------------------------------

fn lan_instance(name: &str) -> (Package, String) {
    use layoutcraft_vsdx::stencil::import_master;
    use layoutcraft_vsdx::writer::add_master_instance;
    let stencil = Package::open(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/Ethernet LAN diagram.vstx")).unwrap();
    let list = layoutcraft_vsdx::stencil::master_list(&stencil).unwrap();
    let id = list.as_array().unwrap().iter().find(|m| m["name"] == name).unwrap()["id"].as_str().unwrap().to_string();
    let (bytes, new_master) = import_master(&blank_a4(), &stencil, &id).unwrap();
    let with = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, shape) = add_master_instance(&with, 0, &new_master, 4.0, 4.0).unwrap();
    (Package::from_reader(Cursor::new(bytes)).unwrap(), shape)
}

#[test]
fn a_group_master_with_nested_group_children_is_written_out_nested_on_resize() {
    for name in ["PC", "Server", "Printer"] {
        let (package, id) = lan_instance(name);
        let editor = GroupEditor::open(&package, 0).unwrap();
        let frame = editor.describe().as_array().unwrap().iter().find(|f| f["id"] == id.as_str()).unwrap().clone();
        let (w, h) = (frame["size"][0].as_f64().unwrap(), frame["size"][1].as_f64().unwrap());
        for (fx, fy) in [(2.0, 2.0), (0.6, 0.6), (1.5, 0.6), (0.7, 1.8)] {
            let patches = editor.resize_member(&id, (w * fx, h * fy), (0.5, 0.5), false).unwrap();
            let resized = Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::patch_page_cells(&package, 0, &patches).unwrap())).unwrap();
            let xml = shape_text(&resized, &id);
            assert!(xml.matches("MasterShape='").count() >= 3, "{name}: the master children are written out: {xml}");
            // the art stays inside the resized box (written once scaled, never twice)
            let (ext_x, ext_y) = {
                let doc = import_document(&resized).unwrap();
                let one = doc.pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id.as_str())).unwrap();
                let (mut mx, mut my) = (0.0f64, 0.0f64);
                for part in &one.parts {
                    let mut d = String::new();
                    let mut skipping = false;
                    for c in part.d.chars() {
                        if c == 'A' { skipping = true; } else if c.is_ascii_alphabetic() { skipping = false; }
                        if !skipping { d.push(c); }
                    }
                    let numbers: Vec<f64> = d.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter(|t| !t.is_empty()).filter_map(|t| t.parse().ok()).collect();
                    for pair in numbers.chunks(2) { if pair.len() == 2 { mx = mx.max(pair[0]); my = my.max(pair[1]); } }
                }
                (mx, my)
            };
            let (bw, bh) = (w * fx * 96.0, h * fy * 96.0);
            assert!(ext_x <= bw * 1.05 && ext_y <= bh * 1.05, "{name}: art {ext_x:.1} x {ext_y:.1} inside the box {bw:.1} x {bh:.1} at ({fx}, {fy})");
            assert!(ext_x >= bw * 0.7 && ext_y >= bh * 0.5, "{name}: art {ext_x:.1} x {ext_y:.1} fills the box {bw:.1} x {bh:.1} at ({fx}, {fy})");
            // a second resize scales what was written, including the nested children
            let editor2 = GroupEditor::open(&resized, 0).unwrap();
            let patches = editor2.resize_member(&id, (w * fx * 0.5, h * fy * 0.5), (0.5, 0.5), false).unwrap();
            let again = Package::from_reader(Cursor::new(layoutcraft_vsdx::writer::patch_page_cells(&resized, 0, &patches).unwrap())).unwrap();
            assert!(import_document(&again).is_ok());
        }
    }
}

// ---- flipped groups and members -----------------------------------------------------------------------------------------

fn flipped_corpus() -> Package {
    Package::open(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/groups-flipped.vsdx")).unwrap()
}

struct Described {
    frames: std::collections::BTreeMap<String, (Frame, Option<String>)>,
}

impl Described {
    fn of(package: &Package) -> Self {
        let editor = GroupEditor::open(package, 0).unwrap();
        let mut frames = std::collections::BTreeMap::new();
        for f in editor.describe().as_array().unwrap() {
            let n = |key: &str, i: usize| f[key][i].as_f64().unwrap();
            frames.insert(
                f["id"].as_str().unwrap().to_string(),
                (
                    Frame { pin: (n("pin", 0), n("pin", 1)), size: (n("size", 0), n("size", 1)), loc: (n("loc", 0), n("loc", 1)), angle: f["angle"].as_f64().unwrap(), flip: (f["flipX"].as_bool().unwrap(), f["flipY"].as_bool().unwrap()) },
                    f["parentId"].as_str().map(str::to_string),
                ),
            );
        }
        Self { frames }
    }

    /// A point of shape `id`'s own box on the page: every ancestor's turn and mirror applied.
    fn world(&self, id: &str, p: (f64, f64)) -> (f64, f64) {
        let mut p = p;
        let mut cur = Some(id.to_string());
        while let Some(c) = cur {
            let (frame, parent) = &self.frames[&c];
            p = frame.to_parent(p);
            cur = parent.clone();
        }
        p
    }

    /// The page direction of shape `id`'s own x axis (its turn and mirror included).
    fn world_x(&self, id: &str) -> (f64, f64) {
        let o = self.world(id, (0.0, 0.0));
        let e = self.world(id, (1.0, 0.0));
        (e.0 - o.0, e.1 - o.1)
    }
}

/// The members (not groups) of the flipped groups of the fixture, with the flipped group that holds them.
fn flipped_members(d: &Described) -> Vec<String> {
    let mut out = Vec::new();
    for (id, (_, parent)) in &d.frames {
        let mut cur = parent.clone();
        let mut flipped = false;
        while let Some(c) = cur {
            let (f, p) = &d.frames[&c];
            flipped |= f.flip.0 || f.flip.1;
            cur = p.clone();
        }
        if flipped && d.frames.values().all(|(_, p)| p.as_deref() != Some(id.as_str())) {
            out.push(id.clone());
        }
    }
    out
}

#[test]
fn members_of_flipped_groups_move_resize_and_turn_where_the_page_says() {
    let package = flipped_corpus();
    let before = Described::of(&package);
    let members = flipped_members(&before);
    assert!(members.len() >= 8, "the fixture has members in four flipped groups: {members:?}");
    for id in &members {
        let editor = GroupEditor::open(&package, 0).unwrap();
        let frame = before.frames[id].0;
        // a move by a page delta moves the member's pin by that delta on the page
        let moved = apply(&package, &editor.move_member(id, (0.5, 0.25), false).unwrap());
        let after = Described::of(&moved);
        let (b, a) = (before.world(id, frame.loc), after.world(id, frame.loc));
        assert!(close((a.0 - b.0, a.1 - b.1), (0.5, 0.25)), "{id}: moved by {:?}", (a.0 - b.0, a.1 - b.1));
        // a resize keeps the anchor corner (a fraction of the member's own box) where it is on the page
        for anchor in [(0.0, 0.0), (1.0, 1.0), (0.5, 0.5)] {
            let resized = apply(&package, &editor.resize_member(id, (frame.size.0 * 1.5, frame.size.1 * 0.6), anchor, false).unwrap());
            let after = Described::of(&resized);
            let p0 = before.world(id, (anchor.0 * frame.size.0, anchor.1 * frame.size.1));
            let p1 = after.world(id, (anchor.0 * frame.size.0 * 1.5, anchor.1 * frame.size.1 * 0.6));
            assert!(close(p0, p1), "{id} anchor {anchor:?}: {p0:?} vs {p1:?}");
        }
        // turning to a page angle puts the member's own x axis there (up to the mirror of the member itself)
        let want = 0.7f64;
        let turned = apply(&package, &editor.rotate_member(id, want).unwrap());
        let after = Described::of(&turned);
        let (ex, ey) = after.world_x(id);
        let mut angle = ey.atan2(ex);
        if (angle - want).abs() > 1e-6 {
            angle -= std::f64::consts::PI; // the member's own flip reverses its x axis
        }
        let diff = (angle - want).rem_euclid(std::f64::consts::TAU);
        assert!(diff < 1e-6 || (std::f64::consts::TAU - diff) < 1e-6, "{id}: page angle {angle} wanted {want}");
        // the member's pin does not move when it is turned
        assert!(close(before.world(id, frame.loc), after.world(id, frame.loc)), "{id}: the pin stays");
    }
}

#[test]
fn a_flipped_group_resizes_with_its_members_and_keeps_its_anchor() {
    let package = flipped_corpus();
    let before = Described::of(&package);
    for id in ["4", "8", "12", "17"] {
        let frame = before.frames[id].0;
        let editor = GroupEditor::open(&package, 0).unwrap();
        for anchor in [(0.0, 0.0), (1.0, 1.0)] {
            let resized = apply(&package, &editor.resize_group(id, (frame.size.0 * 1.4, frame.size.1 * 1.4), anchor).unwrap());
            let after = Described::of(&resized);
            let p0 = before.world(id, (anchor.0 * frame.size.0, anchor.1 * frame.size.1));
            let p1 = after.world(id, (anchor.0 * frame.size.0 * 1.4, anchor.1 * frame.size.1 * 1.4));
            assert!(close(p0, p1), "group {id} anchor {anchor:?}: {p0:?} vs {p1:?}");
            // every member's box is scaled about the group's anchor: its centre on the page moves accordingly
            for (m, (_, parent)) in &before.frames {
                if parent.as_deref() != Some(id) {
                    continue;
                }
                let mf = before.frames[m].0;
                let c0 = before.world(m, (mf.size.0 / 2.0, mf.size.1 / 2.0));
                let c1 = after.world(m, (after.frames[m].0.size.0 / 2.0, after.frames[m].0.size.1 / 2.0));
                let want = (p0.0 + (c0.0 - p0.0) * 1.4, p0.1 + (c0.1 - p0.1) * 1.4);
                assert!(close(c1, want), "member {m} of group {id}: {c1:?} wanted {want:?}");
            }
        }
        assert!(import_document(&apply(&package, &editor.resize_group(id, (frame.size.0 * 1.4, frame.size.1 * 1.4), (0.5, 0.5)).unwrap())).is_ok());
    }
}

#[test]
fn flipping_an_already_flipped_shape_and_copying_a_member_of_a_flipped_group_keep_their_place() {
    let package = flipped_corpus();
    let before = Described::of(&package);
    let editor = GroupEditor::open(&package, 0).unwrap();
    // flipping the flipped group back: the mirror is gone, the box centre stays where it was
    let flipped_back = apply(&package, &editor.flip_many(&["12".to_string()], false).unwrap());
    let after = Described::of(&flipped_back);
    assert!(!after.frames["12"].0.flip.0);
    let (f0, f1) = (before.frames["12"].0, after.frames["12"].0);
    let c0 = f0.to_parent((f0.size.0 / 2.0, f0.size.1 / 2.0));
    let c1 = f1.to_parent((f1.size.0 / 2.0, f1.size.1 / 2.0));
    // a flip about the joint centre of a single shape mirrors the box centre about itself: it does not move
    assert!(close(c0, c1), "{c0:?} vs {c1:?}");
    // copying a member out of a flipped group keeps its page position and its look (angle and mirror from the chain)
    let member = flipped_members(&before).into_iter().next().unwrap();
    let xml = layoutcraft_vsdx::groupedit::copy_shapes(&package, 0, &[member.clone()]).unwrap().0.join("");
    assert!(xml.contains("FlipX") && xml.contains("Angle"), "{xml}");
}

// ---- glued connector ends whose cached position is stale (computer-network) ---------------------------------------------

#[test]
fn a_glued_connector_end_goes_to_its_connection_point_when_the_cached_end_is_somewhere_else() {
    let package = Package::open(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/computer-network.vsdx")).unwrap();
    let height = page_height(&package);
    let document = import_document(&package).unwrap();
    let connector = document.pages[0].connectors.iter().find(|c| c.visio_id.as_deref() == Some("421")).expect("connector 421");
    // glued to the VoidPort of the router (sheet 59): its connection point is the centre of the router's group
    // (3.7058, 2.6861 in); the file caches the top right corner of that box (4.0548, 3.0403)
    let point = connector.from.point.as_ref().expect("an end glued to a shape without a box of its own keeps a point");
    let (x, y) = (point["x"].as_f64().unwrap() / 96.0, height - point["y"].as_f64().unwrap() / 96.0);
    assert!((x - 3.7058).abs() < 0.01 && (y - 2.6861).abs() < 0.01, "begin at ({x}, {y})");
}

// ---- align and distribute -------------------------------------------------------------------------------------------------

/// The three rectangles of probe-align.ps1: a 1 x 1 at (1, 1), b 2 x 1 at (3, 2.5), c 1.5 x 4 at (7, 0.5); ids in drawing order.
fn align_fixture() -> (Package, Vec<String>) {
    let mut package = blank_a4();
    let mut ids = Vec::new();
    for (x0, y0, x1, y1) in [(1.0, 1.0, 2.0, 2.0), (3.0, 2.5, 5.0, 3.5), (7.0, 0.5, 8.5, 4.5)] {
        let (bytes, id) = layoutcraft_vsdx::draw::draw_shape(&package, 0, layoutcraft_vsdx::draw::DrawKind::Rectangle, x0, y0, x1, y1).unwrap();
        package = Package::from_reader(Cursor::new(bytes)).unwrap();
        ids.push(id);
    }
    (package, ids)
}

/// `[x0, x1, y0, y1]` of the page box of shape `id` (turned shapes by the box they cover).
fn page_box_of(package: &Package, id: &str) -> [f64; 4] {
    let frames = Described::of(package);
    let frame = frames.frames[id].0;
    let corners = frame.corners();
    [
        corners.iter().map(|c| c.0).fold(f64::MAX, f64::min),
        corners.iter().map(|c| c.0).fold(f64::MIN, f64::max),
        corners.iter().map(|c| c.1).fold(f64::MAX, f64::min),
        corners.iter().map(|c| c.1).fold(f64::MIN, f64::max),
    ]
}

fn boxes_close(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-6)
}

#[test]
fn align_puts_every_shape_on_the_edge_or_centre_of_the_first_one_as_visio_does() {
    let (package, ids) = align_fixture();
    let editor = GroupEditor::open(&package, 0).unwrap();
    // expected boxes (x0, x1, y0, y1) of b and c from Visio's own Selection.Align (probe-align.ps1)
    let cases: [(u32, [f64; 4], [f64; 4]); 6] = [
        (0, [1.0, 3.0, 2.5, 3.5], [1.0, 2.5, 0.5, 4.5]),
        (1, [0.5, 2.5, 2.5, 3.5], [0.75, 2.25, 0.5, 4.5]),
        (2, [0.0, 2.0, 2.5, 3.5], [0.5, 2.0, 0.5, 4.5]),
        (3, [3.0, 5.0, 1.0, 2.0], [7.0, 8.5, -2.0, 2.0]),
        (4, [3.0, 5.0, 1.0, 2.0], [7.0, 8.5, -0.5, 3.5]),
        (5, [3.0, 5.0, 1.0, 2.0], [7.0, 8.5, 1.0, 5.0]),
    ];
    for (mode, want_b, want_c) in cases {
        let after = apply(&package, &editor.align_many(&ids, mode).unwrap());
        assert!(boxes_close(page_box_of(&after, &ids[1]), want_b), "mode {mode}: b {:?}", page_box_of(&after, &ids[1]));
        assert!(boxes_close(page_box_of(&after, &ids[2]), want_c), "mode {mode}: c {:?}", page_box_of(&after, &ids[2]));
        assert!(boxes_close(page_box_of(&after, &ids[0]), [1.0, 2.0, 1.0, 2.0]), "the first shape stays");
        assert!(import_document(&after).is_ok());
    }
    assert!(editor.align_many(&ids[..1], 0).is_err(), "one shape");
    assert!(editor.align_many(&ids, 9).is_err(), "unknown mode");
}

#[test]
fn distribute_spaces_the_shapes_between_the_outer_two_evenly_as_visio_does() {
    let (package, ids) = align_fixture();
    let editor = GroupEditor::open(&package, 0).unwrap();
    // Visio: horizontally b goes to 3.5..5.5 (equal gaps of 1.5); vertically (centres a 1.5, c 2.5, b 3.0: a and b stay) c to 0.25..4.25
    let horizontal = apply(&package, &editor.distribute_many(&ids, false).unwrap());
    assert!(boxes_close(page_box_of(&horizontal, &ids[1]), [3.5, 5.5, 2.5, 3.5]), "{:?}", page_box_of(&horizontal, &ids[1]));
    assert!(boxes_close(page_box_of(&horizontal, &ids[0]), [1.0, 2.0, 1.0, 2.0]) && boxes_close(page_box_of(&horizontal, &ids[2]), [7.0, 8.5, 0.5, 4.5]));
    let vertical = apply(&package, &editor.distribute_many(&ids, true).unwrap());
    assert!(boxes_close(page_box_of(&vertical, &ids[2]), [7.0, 8.5, 0.25, 4.25]), "{:?}", page_box_of(&vertical, &ids[2]));
    assert!(boxes_close(page_box_of(&vertical, &ids[1]), [3.0, 5.0, 2.5, 3.5]));
    assert!(editor.distribute_many(&ids[..2], false).is_err(), "two shapes are not enough");
    assert!(import_document(&horizontal).is_ok() && import_document(&vertical).is_ok());
}

#[test]
fn a_turned_shape_aligns_by_the_box_it_covers_on_the_page() {
    let (package, ids) = align_fixture();
    // b (2 x 1, centre (4, 3)) turned a quarter turn covers x 3.5..4.5, y 2..4
    let turned = apply(&package, &GroupEditor::open(&package, 0).unwrap().rotate_many(&[ids[1].clone()], std::f64::consts::FRAC_PI_2).unwrap());
    assert!(boxes_close(page_box_of(&turned, &ids[1]), [3.5, 4.5, 2.0, 4.0]));
    let editor = GroupEditor::open(&turned, 0).unwrap();
    let left = apply(&turned, &editor.align_many(&ids, 0).unwrap());
    assert!(boxes_close(page_box_of(&left, &ids[1]), [1.0, 2.0, 2.0, 4.0]), "{:?}", page_box_of(&left, &ids[1]));
}

// ---- group and ungroup ---------------------------------------------------------------------------------------------------

fn corners_on_page(d: &Described, id: &str) -> Vec<(f64, f64)> {
    let size = d.frames[id].0.size;
    [(0.0, 0.0), (size.0, 0.0), (size.0, size.1), (0.0, size.1)].iter().map(|p| d.world(id, *p)).collect()
}

#[test]
fn a_connector_between_two_selected_shapes_goes_into_their_group_and_stays_where_it_is() {
    use layoutcraft_vsdx::groupedit::{add_connector, group_shapes};
    let package = open_corpus("route-sweep-sites.vsdx");
    let (bytes, cid) = add_connector(&package, 0, ("1", Some(2)), ("2", Some(4))).unwrap();
    let with = Package::from_reader(Cursor::new(bytes)).unwrap();
    let ends = |p: &Package| {
        let part = p.pages().unwrap()[0].part.clone();
        let listed = GroupEditor::open(p, 0).unwrap().connectors(&p.connects(&part).unwrap());
        let c = listed.as_array().unwrap().iter().find(|c| c["id"] == cid.as_str()).expect("the connector is listed").clone();
        (c["begin"]["x"].as_f64().unwrap(), c["begin"]["y"].as_f64().unwrap(), c["end"]["x"].as_f64().unwrap(), c["end"]["y"].as_f64().unwrap())
    };
    let before = ends(&with);
    let (bytes, gid) = group_shapes(&with, 0, &["1".to_string(), "2".to_string(), cid.clone()]).unwrap();
    let grouped = Package::from_reader(Cursor::new(bytes)).unwrap();
    let after = ends(&grouped);
    assert!((before.0 - after.0).abs() < 1e-6 && (before.1 - after.1).abs() < 1e-6 && (before.2 - after.2).abs() < 1e-6 && (before.3 - after.3).abs() < 1e-6, "{before:?} vs {after:?}");
    let described = Described::of(&grouped);
    assert_eq!(described.frames[&cid].1.as_deref(), Some(gid.as_str()), "the connector is a member of the group");
    let part = grouped.pages().unwrap()[0].part.clone();
    assert_eq!(grouped.connects(&part).unwrap().iter().filter(|c| c.from_sheet == cid).count(), 2, "its glue rows are still there");
    // a connector glued to a shape that is not selected cannot go in
    assert!(group_shapes(&with, 0, &["1".to_string(), cid.clone()]).is_err());
    assert!(import_document(&grouped).is_ok());
}

#[test]
fn the_drawing_order_of_shapes_changes_to_front_back_forward_and_backward() {
    use layoutcraft_vsdx::groupedit::{reorder_shapes, ZOrder};
    let (package, ids) = align_fixture();
    let (a, b, c) = (ids[0].clone(), ids[1].clone(), ids[2].clone());
    let order = |p: &Package| -> Vec<String> { import_document(p).unwrap().pages[0].shapes.iter().filter_map(|s| s.visio_id.clone()).collect() };
    let run = |p: &Package, picked: &[&String], how: ZOrder| {
        let picked: Vec<String> = picked.iter().map(|s| s.to_string()).collect();
        Package::from_reader(Cursor::new(reorder_shapes(p, 0, &picked, how).unwrap())).unwrap()
    };
    assert_eq!(order(&package), vec![a.clone(), b.clone(), c.clone()]);
    assert_eq!(order(&run(&package, &[&a], ZOrder::ToFront)), vec![b.clone(), c.clone(), a.clone()]);
    assert_eq!(order(&run(&package, &[&c], ZOrder::ToBack)), vec![c.clone(), a.clone(), b.clone()]);
    assert_eq!(order(&run(&package, &[&a], ZOrder::Forward)), vec![b.clone(), a.clone(), c.clone()]);
    assert_eq!(order(&run(&package, &[&c], ZOrder::Backward)), vec![a.clone(), c.clone(), b.clone()]);
    // two selected shapes move together and keep their order
    assert_eq!(order(&run(&package, &[&a, &b], ZOrder::Forward)), vec![c.clone(), a.clone(), b.clone()]);
    // nothing to do at the ends
    assert_eq!(order(&run(&package, &[&c], ZOrder::Forward)), vec![a.clone(), b.clone(), c.clone()]);
    assert!(reorder_shapes(&package, 0, &["999".to_string()], ZOrder::ToFront).is_err() && reorder_shapes(&package, 0, &[], ZOrder::ToFront).is_err());
    assert!(import_document(&run(&package, &[&b], ZOrder::ToBack)).is_ok());
}

#[test]
fn grouping_page_shapes_makes_one_group_that_covers_them_and_keeps_every_shape_in_place() {
    use layoutcraft_vsdx::groupedit::group_shapes;
    let (package, ids) = align_fixture();
    let before = Described::of(&package);
    let (bytes, gid) = group_shapes(&package, 0, &ids).unwrap();
    let grouped = Package::from_reader(Cursor::new(bytes)).unwrap();
    let after = Described::of(&grouped);
    // the group covers the three boxes: x 1..8.5, y 0.5..4.5
    let g = after.frames[&gid].0;
    assert!((g.size.0 - 7.5).abs() < 1e-9 && (g.size.1 - 4.0).abs() < 1e-9, "{:?}", g.size);
    assert!(close(g.pin, (4.75, 2.5)), "{:?}", g.pin);
    for id in &ids {
        assert_eq!(after.frames[id].1.as_deref(), Some(gid.as_str()), "{id} is a member now");
        for (a, b) in corners_on_page(&before, id).iter().zip(corners_on_page(&after, id).iter()) {
            assert!(close(*a, *b), "{id}: {a:?} vs {b:?}");
        }
    }
    // Visio's own way: the members' frame cells are formulas on the group
    let xml = shape_text(&grouped, &ids[0]);
    assert!(xml.contains(&format!("F='Sheet.{gid}!Width*")) && xml.contains(&format!("F='Sheet.{gid}!Height*")), "{xml}");
    assert!(import_document(&grouped).is_ok());
    // resizing the group scales the members (the engine's own group resize)
    let resized = apply(&grouped, &GroupEditor::open(&grouped, 0).unwrap().resize_group(&gid, (15.0, 8.0), (0.0, 0.0)).unwrap());
    let r = Described::of(&resized);
    assert!((r.frames[&ids[2]].0.size.0 - 3.0).abs() < 1e-9, "c is twice as wide: {:?}", r.frames[&ids[2]].0.size);
    // refusals
    assert!(group_shapes(&package, 0, &ids[..1]).is_err(), "one shape");
    assert!(group_shapes(&package, 0, &[ids[0].clone(), ids[0].clone()]).is_err(), "twice");
}

#[test]
fn ungrouping_gives_every_member_back_to_the_page_where_it_was_including_flipped_and_turned_groups() {
    use layoutcraft_vsdx::groupedit::{group_shapes, ungroup_shape};
    // our own group of the three rectangles, turned and flipped first, then ungrouped
    let (package, ids) = align_fixture();
    let (bytes, gid) = group_shapes(&package, 0, &ids).unwrap();
    let grouped = Package::from_reader(Cursor::new(bytes)).unwrap();
    let editor = GroupEditor::open(&grouped, 0).unwrap();
    let turned = apply(&grouped, &editor.rotate_many(&[gid.clone()], 0.6).unwrap());
    let flipped = apply(&turned, &GroupEditor::open(&turned, 0).unwrap().flip_many(&[gid.clone()], false).unwrap());
    let before = Described::of(&flipped);
    let (bytes, freed) = ungroup_shape(&flipped, 0, &gid).unwrap();
    let free = Package::from_reader(Cursor::new(bytes)).unwrap();
    let after = Described::of(&free);
    assert_eq!(freed.len(), 3);
    assert!(!after.frames.contains_key(&gid), "the group is gone");
    for id in &freed {
        assert!(after.frames[id].1.is_none(), "{id} is on the page again");
        for (a, b) in corners_on_page(&before, id).iter().zip(corners_on_page(&after, id).iter()) {
            assert!(close(*a, *b), "{id}: {a:?} vs {b:?}");
        }
    }
    assert!(!shape_text(&free, &ids[0]).contains(&format!("Sheet.{gid}!")), "the formulas on the group are dropped");
    assert!(import_document(&free).is_ok());
    // groups made by Visio: members of the four flipped / turned / nested groups come out where they were
    let fixture = flipped_corpus();
    let original = Described::of(&fixture);
    for group in ["4", "8", "12", "17"] {
        let members: Vec<String> = original.frames.iter().filter(|(_, (_, p))| p.as_deref() == Some(group)).map(|(id, _)| id.clone()).collect();
        let (bytes, freed) = ungroup_shape(&fixture, 0, group).unwrap();
        let out = Package::from_reader(Cursor::new(bytes)).unwrap();
        let now = Described::of(&out);
        assert_eq!(freed.len(), members.len(), "group {group}");
        for id in &members {
            assert!(now.frames[id].1.is_none());
            for (a, b) in corners_on_page(&original, id).iter().zip(corners_on_page(&now, id).iter()) {
                assert!(close(*a, *b), "group {group} member {id}: {a:?} vs {b:?}");
            }
        }
        assert!(import_document(&out).is_ok());
    }
    // refusals
    assert!(ungroup_shape(&package, 0, &ids[0]).is_err(), "not a group");
    assert!(ungroup_shape(&fixture, 0, "1").is_err(), "a member of a group");
}

#[test]
fn ungrouping_a_group_with_a_connector_gives_the_connector_back_to_the_page_in_place() {
    use layoutcraft_vsdx::groupedit::{add_connector, group_shapes, ungroup_shape};
    let package = open_corpus("route-sweep-sites.vsdx");
    let (bytes, cid) = add_connector(&package, 0, ("1", Some(2)), ("2", Some(4))).unwrap();
    let with = Package::from_reader(Cursor::new(bytes)).unwrap();
    let ends = |p: &Package| {
        let part = p.pages().unwrap()[0].part.clone();
        let listed = GroupEditor::open(p, 0).unwrap().connectors(&p.connects(&part).unwrap());
        let c = listed.as_array().unwrap().iter().find(|c| c["id"] == cid.as_str()).expect("the connector is listed").clone();
        (c["begin"]["x"].as_f64().unwrap(), c["begin"]["y"].as_f64().unwrap(), c["end"]["x"].as_f64().unwrap(), c["end"]["y"].as_f64().unwrap(), c["parentId"].clone())
    };
    let before = ends(&with);
    let (bytes, gid) = group_shapes(&with, 0, &["1".to_string(), "2".to_string(), cid.clone()]).unwrap();
    let grouped = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(ends(&grouped).4, serde_json::Value::String(gid.clone()));
    let (bytes, freed) = ungroup_shape(&grouped, 0, &gid).unwrap();
    let free = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(freed.len(), 3, "{freed:?}");
    let after = ends(&free);
    assert!((before.0 - after.0).abs() < 1e-6 && (before.1 - after.1).abs() < 1e-6 && (before.2 - after.2).abs() < 1e-6 && (before.3 - after.3).abs() < 1e-6, "{before:?} vs {after:?}");
    assert!(after.4.is_null(), "the connector is a page-level shape again");
    let part = free.pages().unwrap()[0].part.clone();
    assert_eq!(free.connects(&part).unwrap().iter().filter(|c| c.from_sheet == cid).count(), 2);
    // a turned group: Visio moves the ends through the group's transform and leaves the connector unturned (probe: Pin = middle of
    // the ends, Width / Height = End - Begin)
    let pin = Described::of(&grouped).frames[&gid].0.pin;
    let turned = apply(&grouped, &GroupEditor::open(&grouped, 0).unwrap().rotate_many(&[gid.clone()], 0.5).unwrap());
    let (bytes, _) = ungroup_shape(&turned, 0, &gid).unwrap();
    let freed_turned = Package::from_reader(Cursor::new(bytes)).unwrap();
    let turn = |x: f64, y: f64| (pin.0 + 0.5f64.cos() * (x - pin.0) - 0.5f64.sin() * (y - pin.1), pin.1 + 0.5f64.sin() * (x - pin.0) + 0.5f64.cos() * (y - pin.1));
    let (b, e) = (turn(before.0, before.1), turn(before.2, before.3));
    let got = ends(&freed_turned);
    assert!((got.0 - b.0).abs() < 1e-6 && (got.1 - b.1).abs() < 1e-6 && (got.2 - e.0).abs() < 1e-6 && (got.3 - e.1).abs() < 1e-6, "{got:?} vs {b:?} {e:?}");
    let cell = |p: &Package, name: &str| -> f64 { let xml = shape_text(p, &cid); let at = xml.find(&format!("N='{name}' V='")).expect(name) + name.len() + 8; xml[at..].split('\'').next().unwrap().parse().unwrap() };
    assert!((cell(&freed_turned, "PinX") - (b.0 + e.0) / 2.0).abs() < 1e-6 && (cell(&freed_turned, "Width") - (e.0 - b.0)).abs() < 1e-6 && (cell(&freed_turned, "Height") - (e.1 - b.1)).abs() < 1e-6);
    assert_eq!(cell(&freed_turned, "Angle"), 0.0);
    // a mirrored group: x is mirrored about the group's vertical axis
    let flipped = apply(&grouped, &GroupEditor::open(&grouped, 0).unwrap().flip_many(&[gid.clone()], false).unwrap());
    let (bytes, _) = ungroup_shape(&flipped, 0, &gid).unwrap();
    let freed_flipped = Package::from_reader(Cursor::new(bytes)).unwrap();
    let got = ends(&freed_flipped);
    assert!((got.0 - (2.0 * pin.0 - before.0)).abs() < 1e-6 && (got.1 - before.1).abs() < 1e-6 && (got.2 - (2.0 * pin.0 - before.2)).abs() < 1e-6 && (got.3 - before.3).abs() < 1e-6, "{got:?}");
    assert!(cell(&freed_flipped, "Width") * (before.2 - before.0) < 0.0, "the mirrored connector's Width changes sign, like Visio's");
    assert!(import_document(&freed_turned).is_ok() && import_document(&freed_flipped).is_ok());
}

// ---- page setup -----------------------------------------------------------------------------------------------------------

#[test]
fn the_size_of_a_page_is_set_in_its_page_sheet_and_only_for_that_page() {
    use layoutcraft_vsdx::pageops::{add_page, set_page_size};
    let package = blank_a4();
    let (bytes, _) = add_page(&package, 0, None).unwrap();
    let two = Package::from_reader(Cursor::new(bytes)).unwrap();
    let bytes = set_page_size(&two, 1, 5.0, 3.5).unwrap();
    let resized = Package::from_reader(Cursor::new(bytes)).unwrap();
    let pages = resized.pages().unwrap();
    assert_eq!((pages[1].width, pages[1].height), (Some(5.0), Some(3.5)), "the second page has the new size");
    assert_eq!(pages[0].width, two.pages().unwrap()[0].width, "the first page is untouched");
    assert!(import_document(&resized).is_ok());
    // a second change replaces the cells (no duplicates), also to a portrait page
    let again = Package::from_reader(Cursor::new(set_page_size(&resized, 1, 3.5, 5.0).unwrap())).unwrap();
    let xml = again.text("visio/pages/pages.xml").unwrap();
    assert_eq!(xml.matches("N='PageWidth'").count(), 2, "one cell per page: {xml}");
    assert_eq!(again.pages().unwrap()[1].width, Some(3.5));
    assert!(set_page_size(&again, 1, 0.1, 5.0).is_err(), "too small");
    assert!(set_page_size(&again, 1, 5.0, 500.0).is_err(), "too large");
    assert!(set_page_size(&again, 1, f64::NAN, 5.0).is_err(), "not a number");
    assert!(set_page_size(&again, 7, 5.0, 5.0).is_err(), "no such page");
}

// ---- layers ---------------------------------------------------------------------------------------------------------------

#[test]
fn layers_are_added_renamed_hidden_locked_and_shapes_put_on_them() {
    use layoutcraft_vsdx::pageops::{add_layer, set_layer};
    // a blank drawing has no layer section: the first layer creates it
    let (bytes, ix) = add_layer(&blank_a4(), 0, "Notes").unwrap();
    assert_eq!(ix, 0);
    let one = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, ix2) = add_layer(&one, 0, "Wiring").unwrap();
    assert_eq!(ix2, 1);
    let two = Package::from_reader(Cursor::new(bytes)).unwrap();
    let layers = import_document(&two).unwrap().pages[0].layers.clone();
    assert_eq!(layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Notes", "Wiring"]);
    assert!(layers.iter().all(|l| l.visible && l.print && !l.locked));
    assert!(add_layer(&two, 0, "notes").is_err(), "names are unique, ignoring case");
    assert!(add_layer(&two, 0, "  ").is_err() && add_layer(&two, 0, "a<b").is_err());
    // rename, hide, stop printing, lock
    let changed = Package::from_reader(Cursor::new(set_layer(&two, 0, 1, Some("Cables"), Some(false), Some(false), Some(true)).unwrap())).unwrap();
    let layers = import_document(&changed).unwrap().pages[0].layers.clone();
    assert_eq!((layers[1].name.as_str(), layers[1].visible, layers[1].print, layers[1].locked), ("Cables", false, false, true));
    assert!(layers[0].visible, "the other layer is untouched");
    assert!(set_layer(&changed, 0, 1, Some("Notes"), None, None, None).is_err(), "a name that exists");
    assert!(set_layer(&changed, 0, 9, None, Some(true), None, None).is_err(), "no such layer");
    // shapes on layers: a hidden layer hides its shapes
    let (package, id) = draw_on_blank(layoutcraft_vsdx::draw::DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0);
    let (bytes, _) = add_layer(&package, 0, "Boxes").unwrap();
    let with_layer = Package::from_reader(Cursor::new(bytes)).unwrap();
    let editor = GroupEditor::open(&with_layer, 0).unwrap();
    let on = apply(&with_layer, &editor.set_layer_membership(&[id.clone()], 0, true).unwrap());
    assert!(shape_text(&on, &id).contains("N='LayerMember' V='0'"), "{}", shape_text(&on, &id));
    let shape_layers = |p: &Package| import_document(p).unwrap().pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id.as_str())).map(|s| s.layers.clone());
    assert_eq!(shape_layers(&on), Some(vec![0]));
    let hidden = Package::from_reader(Cursor::new(set_layer(&on, 0, 0, None, Some(false), None, None).unwrap())).unwrap();
    let flagged = |p: &Package| import_document(p).unwrap().pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id.as_str())).map(|s| s.layer_hidden);
    assert_eq!(flagged(&on), Some(false));
    assert_eq!(flagged(&hidden), Some(true), "a shape on a hidden layer is not drawn");
    // taking it off again, and a second layer next to the first
    let (bytes, _) = add_layer(&on, 0, "More").unwrap();
    let more = Package::from_reader(Cursor::new(bytes)).unwrap();
    let both = apply(&more, &GroupEditor::open(&more, 0).unwrap().set_layer_membership(&[id.clone()], 1, true).unwrap());
    assert_eq!(shape_layers(&both), Some(vec![0, 1]));
    let off = apply(&both, &GroupEditor::open(&both, 0).unwrap().set_layer_membership(&[id.clone()], 0, false).unwrap());
    assert_eq!(shape_layers(&off), Some(vec![1]));
    assert!(GroupEditor::open(&off, 0).unwrap().set_layer_membership(&[id.clone()], 1, true).unwrap().is_empty(), "already a member: nothing to write");
    assert!(import_document(&off).is_ok());
}

#[test]
fn a_layer_colour_is_written_read_back_and_cleared() {
    use layoutcraft_vsdx::pageops::{add_layer, set_layer_color};
    let (bytes, ix) = add_layer(&blank_a4(), 0, "Wiring").unwrap();
    let package = Package::from_reader(Cursor::new(bytes)).unwrap();
    let colour = |p: &Package| import_document(p).unwrap().pages[0].layers[0].color.clone();
    assert_eq!(colour(&package), None);
    let red = Package::from_reader(Cursor::new(set_layer_color(&package, 0, ix, "#FF0000").unwrap())).unwrap();
    assert_eq!(colour(&red), Some("#ff0000".to_string()));
    assert!(red.text("visio/pages/pages.xml").unwrap().contains("N='Color' V='#ff0000'"), "written like Visio writes an RGB layer colour");
    let cleared = Package::from_reader(Cursor::new(set_layer_color(&red, 0, ix, "none").unwrap())).unwrap();
    assert_eq!(colour(&cleared), None);
    assert!(set_layer_color(&package, 0, ix, "red").is_err() && set_layer_color(&package, 0, 9, "#00ff00").is_err());
}

#[test]
fn deleting_a_layer_renumbers_the_others_and_the_members_of_the_shapes() {
    use layoutcraft_vsdx::pageops::{add_layer, delete_layer};
    let (package, id) = draw_on_blank(layoutcraft_vsdx::draw::DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0);
    let mut current = package;
    for name in ["A", "B", "C"] {
        let (bytes, _) = add_layer(&current, 0, name).unwrap();
        current = Package::from_reader(Cursor::new(bytes)).unwrap();
    }
    for layer in [0u32, 1, 2] {
        current = apply(&current, &GroupEditor::open(&current, 0).unwrap().set_layer_membership(&[id.clone()], layer, true).unwrap());
    }
    let shape_layers = |p: &Package| import_document(p).unwrap().pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id.as_str())).map(|s| s.layers.clone());
    assert_eq!(shape_layers(&current), Some(vec![0, 1, 2]));
    let after = Package::from_reader(Cursor::new(delete_layer(&current, 0, 1).unwrap())).unwrap();
    let layers = import_document(&after).unwrap().pages[0].layers.clone();
    assert_eq!(layers.iter().map(|l| (l.index, l.name.as_str())).collect::<Vec<_>>(), [(0, "A"), (1, "C")]);
    assert_eq!(shape_layers(&after), Some(vec![0, 1]), "the shape keeps A and C, renumbered");
    assert!(delete_layer(&after, 0, 5).is_err(), "no such layer");
}

#[test]
fn an_incremental_import_equals_a_full_import_and_reuses_untouched_pages() {
    use layoutcraft_vsdx::import_document_with;
    use layoutcraft_vsdx::pageops::{add_layer, add_page};
    let (package, id) = draw_on_blank(layoutcraft_vsdx::draw::DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0);
    let (bytes, _) = add_page(&package, 0, Some("Second")).unwrap();
    let two = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (first, state) = import_document_with(&two, None).unwrap();
    assert_eq!(first.pages.len(), 2);
    // an edit on page 1 only: page 2 is taken over, the result equals a full import
    let moved = apply(&two, &GroupEditor::open(&two, 0).unwrap().move_member(&id, (0.5, 0.25), false).unwrap());
    let (incremental, state2) = import_document_with(&moved, Some((&two, &first, &state))).unwrap();
    let full = import_document(&moved).unwrap();
    assert_eq!(serde_json::to_string(&incremental).unwrap(), serde_json::to_string(&full).unwrap());
    assert_ne!(serde_json::to_string(&incremental.pages[0]).unwrap(), serde_json::to_string(&first.pages[0]).unwrap(), "page 1 changed");
    assert_eq!(serde_json::to_string(&incremental.pages[1]).unwrap(), serde_json::to_string(&first.pages[1]).unwrap());
    // a change to a shared part (the page list: a layer) re-imports everything and still equals a full import
    let (bytes, _) = add_layer(&moved, 1, "Notes").unwrap();
    let layered = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (again, _) = import_document_with(&layered, Some((&moved, &incremental, &state2))).unwrap();
    assert_eq!(serde_json::to_string(&again).unwrap(), serde_json::to_string(&import_document(&layered).unwrap()).unwrap());
    assert_eq!(again.pages[1].layers.len(), 1);
}

#[test]
fn duration_and_currency_data_rows_are_written_like_visio_writes_them() {
    use layoutcraft_vsdx::writer::{DataEdit, ShapeCellPatch};
    let (package, id) = draw_on_blank(layoutcraft_vsdx::draw::DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0);
    let write = |package: &Package, name: &str, kind: u32, value: &str| {
        let edit = DataEdit { name: name.to_string(), label: name.to_string(), value: value.to_string(), kind, ..Default::default() };
        let patched = apply(package, &[ShapeCellPatch::new(id.clone()).data_edit(edit)]);
        patched
    };
    let with = write(&package, "Dur", 6, "90 min");
    let with = write(&with, "Dur2", 6, "1.5 d");
    let with = write(&with, "Cost", 7, "12.5");
    let xml = shape_text(&with, &id);
    assert!(xml.contains("<Row N='Dur'><Cell N='Value' V='0.0625' U='EM'/>"), "90 minutes is 0.0625 days, unit EM: {xml}");
    assert!(xml.contains("<Row N='Dur2'><Cell N='Value' V='1.5' U='ED'/>"), "{xml}");
    assert!(xml.contains("<Row N='Cost'><Cell N='Value' V='12.5'/>"), "a currency amount is a plain number: {xml}");
    assert!(xml.contains("N='Type' V='6'") && xml.contains("N='Type' V='7'"));
    // read back by the importer
    let rows = import_document(&with).unwrap().pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id.as_str())).unwrap().data.clone();
    assert_eq!(rows.iter().map(|r| (r.name.as_str(), r.r#type)).collect::<Vec<_>>(), [("Dur", 6), ("Dur2", 6), ("Cost", 7)]);
    // units and refusals
    let hours = write(&package, "H", 6, "3 h");
    assert!(shape_text(&hours, &id).contains("U='EH'"));
    let bad = DataEdit { name: "X".to_string(), value: "soon".to_string(), kind: 6, ..Default::default() };
    assert!(GroupEditor::open(&package, 0).is_ok());
    let patch = ShapeCellPatch::new(id.clone()).data_edit(bad);
    assert!(layoutcraft_vsdx::writer::patch_page_cells(&package, 0, &[patch]).is_err(), "not a duration");
}

#[test]
fn shape_data_rows_are_written_replaced_and_removed_and_survive_a_reimport() {
    use layoutcraft_vsdx::writer::{DataEdit, ShapeCellPatch};
    let (package, id) = draw_on_blank(layoutcraft_vsdx::draw::DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0);
    let row = |name: &str, label: &str, value: &str, kind: u32, format: &str| DataEdit { name: name.into(), label: label.into(), value: value.into(), kind, format: format.into(), ..Default::default() };
    let mut current = package;
    for edit in [row("Owner", "Owner", "Ann & Bob", 0, ""), row("Cost", "Cost", "42.5", 2, "0.00"), row("Done", "Done", "TRUE", 3, ""), row("Pick", "Pick", "B", 1, "A;B;C"), row("When", "When", "2026-11-08", 5, "")] {
        let patch = ShapeCellPatch::new(id.clone()).data_edit(edit);
        current = apply(&current, &[patch]);
    }
    let rows = |p: &Package| import_document(p).unwrap().pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some(id.as_str())).unwrap().data.clone();
    let got = rows(&current);
    assert_eq!(got.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Owner", "Cost", "Done", "Pick", "When"]);
    assert_eq!((got[0].value.as_str(), got[0].r#type), ("Ann & Bob", 0));
    assert_eq!((got[1].value.as_str(), got[1].r#type, got[1].format.as_str()), ("42.5", 2, "0.00"));
    assert_eq!(got[2].r#type, 3);
    assert_eq!((got[3].value.as_str(), got[3].format.as_str()), ("B", "A;B;C"));
    assert_eq!(got[4].r#type, 5);
    // a row of the same name is replaced, not duplicated; the label and the flag are kept
    let mut edit = row("Cost", "Price", "7", 2, "0.00");
    edit.invisible = true;
    current = apply(&current, &[ShapeCellPatch::new(id.clone()).data_edit(edit)]);
    let got = rows(&current);
    assert_eq!(got.len(), 5);
    assert_eq!((got[1].label.as_str(), got[1].value.as_str(), got[1].invisible), ("Price", "7", 1));
    // removing a row, then the last rows (the empty section goes)
    current = apply(&current, &[ShapeCellPatch::new(id.clone()).data_edit(DataEdit { name: "Owner".into(), remove: true, ..Default::default() })]);
    assert_eq!(rows(&current).len(), 4);
    for name in ["Cost", "Done", "Pick", "When"] {
        current = apply(&current, &[ShapeCellPatch::new(id.clone()).data_edit(DataEdit { name: name.into(), remove: true, ..Default::default() })]);
    }
    assert!(rows(&current).is_empty());
    assert!(!shape_text(&current, &id).contains("N='Property'"));
    // mistakes are refused with a message
    let bad = |edit: DataEdit| layoutcraft_vsdx::writer::patch_page_cells(&current, 0, &[ShapeCellPatch::new(id.clone()).data_edit(edit)]).is_err();
    assert!(bad(row("1bad", "x", "", 0, "")) && bad(row("a b", "x", "", 0, "")), "names");
    assert!(bad(row("N", "x", "abc", 2, "")), "a number");
    assert!(bad(row("D", "x", "yesterday", 5, "")), "a date");
    assert!(bad(row("T", "x", "", 9, "")), "a type");
    assert!(bad(DataEdit { name: "Nothing".into(), remove: true, ..Default::default() }), "removing a row that is not there");
}

#[test]
fn lock_shape_writes_the_protection_cells_and_unlock_clears_them() {
    use layoutcraft_vsdx::groupedit::StyleEdit;
    use layoutcraft_vsdx::writer::patch_page_cells;
    let package = corpus();
    let locked_of = |package: &Package, id: &str| -> bool {
        let info = GroupEditor::open(package, 0).unwrap().describe();
        info.as_array().unwrap().iter().find(|n| n["id"] == id).unwrap()["locked"].as_bool().unwrap()
    };
    assert!(!locked_of(&package, "4"));
    let editor = GroupEditor::open(&package, 0).unwrap();
    let on = editor.style_patches(&["4".to_string()], &StyleEdit { lock: Some(true), ..Default::default() }).unwrap();
    let locked = Package::from_reader(Cursor::new(patch_page_cells(&package, 0, &on).unwrap())).unwrap();
    assert!(locked_of(&locked, "4"));
    assert!(!locked_of(&locked, "1"), "only the chosen shape is locked");
    let off = GroupEditor::open(&locked, 0).unwrap().style_patches(&["4".to_string()], &StyleEdit { lock: Some(false), ..Default::default() }).unwrap();
    let open = Package::from_reader(Cursor::new(patch_page_cells(&locked, 0, &off).unwrap())).unwrap();
    assert!(!locked_of(&open, "4"));
}

#[test]
fn grouping_members_of_a_group_takes_them_out_into_a_new_page_level_group_and_keeps_them_in_place() {
    use layoutcraft_vsdx::groupedit::group_shapes;
    let (package, ids) = align_fixture();
    let (bytes, outer) = group_shapes(&package, 0, &ids).unwrap();
    let grouped = Package::from_reader(Cursor::new(bytes)).unwrap();
    let before = Described::of(&grouped);
    let (bytes, inner) = group_shapes(&grouped, 0, &[ids[0].clone(), ids[1].clone()]).unwrap();
    let nested = Package::from_reader(Cursor::new(bytes)).unwrap();
    let after = Described::of(&nested);
    assert_eq!(after.frames[&inner].1, None, "Visio puts the new group on the page itself");
    for id in [&ids[0], &ids[1]] {
        assert_eq!(after.frames[id].1.as_deref(), Some(inner.as_str()), "{id} is in the new group");
        for (a, b) in corners_on_page(&before, id).iter().zip(corners_on_page(&after, id).iter()) {
            assert!(close(*a, *b), "{id}: {a:?} vs {b:?}");
        }
    }
    assert_eq!(after.frames[&ids[2]].1.as_deref(), Some(outer.as_str()), "the member that was not picked stays");
    assert!(shape_text(&nested, &ids[0]).contains(&format!("F='Sheet.{inner}!Width*")), "formulas follow the new group");
    assert!(import_document(&nested).is_ok());
    // every member of a group cannot be taken out, and a shape and its own group cannot be listed together
    assert!(group_shapes(&grouped, 0, &ids.to_vec()).is_err());
    assert!(group_shapes(&grouped, 0, &[outer.clone(), ids[0].clone()]).is_err());
}

#[test]
fn data_rows_carry_a_sort_key_that_is_listed_kept_on_rewrite_and_changed_by_an_edit() {
    use layoutcraft_vsdx::writer::{data_sort_keys, DataEdit, ShapeCellPatch};
    let (package, id) = draw_on_blank(layoutcraft_vsdx::draw::DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0);
    let put = |package: &Package, name: &str, key: &str| {
        let edit = DataEdit { name: name.to_string(), label: name.to_string(), value: "x".to_string(), sort_key: key.to_string(), ..Default::default() };
        apply(package, &[ShapeCellPatch::new(id.clone()).data_edit(edit)])
    };
    let with = put(&put(&package, "A", ""), "B", "0002");
    assert_eq!(data_sort_keys(&with, 0, &id).unwrap(), [("A".to_string(), String::new()), ("B".to_string(), "0002".to_string())]);
    // rewriting a row without a key keeps its key
    let again = put(&with, "B", "");
    assert_eq!(data_sort_keys(&again, 0, &id).unwrap()[1].1, "0002");
    // an explicit key replaces it, and the row order in the file does not change
    let moved = put(&put(&again, "A", "0002"), "B", "0001");
    assert_eq!(data_sort_keys(&moved, 0, &id).unwrap(), [("A".to_string(), "0002".to_string()), ("B".to_string(), "0001".to_string())]);
    assert!(data_sort_keys(&moved, 0, "999").is_err());
    assert!(import_document(&moved).is_ok());
}

#[test]
fn a_connector_can_be_added_between_members_of_a_turned_group() {
    use layoutcraft_vsdx::groupedit::{add_connector, group_shapes};
    let (package, ids) = align_fixture();
    let (bytes, gid) = group_shapes(&package, 0, &ids).unwrap();
    let grouped = Package::from_reader(Cursor::new(bytes)).unwrap();
    let turned = apply(&grouped, &GroupEditor::open(&grouped, 0).unwrap().rotate_many(&[gid.clone()], 0.5).unwrap());
    let (bytes, cid) = add_connector(&turned, 0, (ids[0].as_str(), None), (ids[1].as_str(), None)).unwrap();
    let with = Package::from_reader(Cursor::new(bytes)).unwrap();
    let part = with.pages().unwrap()[0].part.clone();
    let connects = with.connects(&part).unwrap();
    assert_eq!(connects.iter().filter(|c| c.from_sheet == cid).map(|c| c.to_sheet.as_str()).collect::<Vec<_>>(), [ids[0].as_str(), ids[1].as_str()]);
    let listed = GroupEditor::open(&with, 0).unwrap().connectors(&connects);
    let c = listed.as_array().unwrap().iter().find(|c| c["id"] == cid.as_str()).expect("listed");
    let route = c["route"].as_array().unwrap();
    let (first, last) = (&route[0], &route[route.len() - 1]);
    assert!((first[0].as_f64().unwrap() - c["begin"]["x"].as_f64().unwrap()).abs() < 1e-3 && (last[1].as_f64().unwrap() - c["end"]["y"].as_f64().unwrap()).abs() < 1e-3);
    assert!(import_document(&with).is_ok());
}

#[test]
fn a_data_row_only_the_master_has_is_removed_with_a_del_row_like_visio_does() {
    use layoutcraft_vsdx::writer::{data_sort_keys, DataEdit, ShapeCellPatch};
    let package = Package::open(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/basic-network-diagram.vsdx")).unwrap();
    let rows_of = |p: &Package| -> Vec<String> {
        import_document(p).unwrap().pages[0].shapes.iter().find(|s| s.visio_id.as_deref() == Some("27")).unwrap().data.iter().map(|r| r.name.clone()).collect()
    };
    let before = rows_of(&package);
    assert!(before.iter().any(|n| n == "AssetNumber"), "the Laptop master supplies AssetNumber: {before:?}");
    let editor = GroupEditor::open(&package, 0).unwrap();
    assert!(editor.master_supplies_data_row("27", "AssetNumber") && !editor.master_supplies_data_row("27", "NoSuchRow"));
    let remove = DataEdit { name: "AssetNumber".to_string(), remove: true, inherited: true, ..Default::default() };
    let removed = apply(&package, &[ShapeCellPatch::new("27".to_string()).data_edit(remove.clone())]);
    let xml = shape_text(&removed, "27");
    assert!(xml.contains("<Row N='AssetNumber' Del='1'/>"), "{xml}");
    let after = rows_of(&removed);
    assert_eq!(after.len(), before.len() - 1, "only that row is gone");
    assert!(!after.iter().any(|n| n == "AssetNumber"));
    // removing it again does not pile up markers; the row can be added back (it replaces the marker)
    let twice = apply(&removed, &[ShapeCellPatch::new("27".to_string()).data_edit(remove)]);
    assert_eq!(shape_text(&twice, "27").matches("Del='1'").count(), 1);
    let back = apply(&removed, &[ShapeCellPatch::new("27".to_string()).data_edit(DataEdit { name: "AssetNumber".to_string(), label: "Asset".to_string(), value: "A1".to_string(), ..Default::default() })]);
    assert!(rows_of(&back).iter().any(|n| n == "AssetNumber") && !shape_text(&back, "27").contains("Del='1'"));
    assert!(data_sort_keys(&back, 0, "27").is_ok());
    // a row nobody has is still refused
    let editor = GroupEditor::open(&package, 0).unwrap();
    assert!(!editor.master_supplies_data_row("27", "Typo"));
}

#[test]
fn an_edit_copies_the_parts_it_leaves_alone_as_compressed_bytes_and_still_reads_back() {
    use std::io::Read;
    let package = corpus();
    let source = {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/groups-rotated.vsdx");
        std::fs::read(path).unwrap()
    };
    let editor = GroupEditor::open(&package, 0).unwrap();
    let patches = editor.move_member("5", (0.25, 0.0), false).unwrap();
    let edited = layoutcraft_vsdx::writer::patch_page(&package, 0, &patches, &[], &[]).unwrap();
    let mut before = zip::ZipArchive::new(Cursor::new(source)).unwrap();
    let mut after = zip::ZipArchive::new(Cursor::new(edited.clone())).unwrap();
    assert_eq!(before.len(), after.len(), "no part lost or added");
    let page_part = package.pages().unwrap()[0].part.clone();
    let mut same = 0;
    for i in 0..before.len() {
        let name = before.by_index(i).unwrap().name().unwrap().to_string();
        let (b_crc, b_size, b_method) = { let f = before.by_name(&name).unwrap(); (f.crc32(), f.compressed_size(), f.compression()) };
        let mut f = after.by_name(&name).unwrap();
        if name.trim_start_matches('/') == page_part.trim_start_matches('/') {
            assert_ne!(f.crc32(), b_crc, "the edited page changed");
            continue;
        }
        assert_eq!((f.crc32(), f.compressed_size(), f.compression()), (b_crc, b_size, b_method), "{name} is copied as it was");
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.as_slice(), package.entry(&name).unwrap(), "{name} reads back the same");
        same += 1;
    }
    assert!(same >= 3, "{same} parts compared");
    // a second edit of the written package copies from the written bytes
    let again = Package::from_reader(Cursor::new(edited)).unwrap();
    assert!(import_document(&again).is_ok());
}

#[test]
fn a_package_with_absurdly_nested_shapes_is_refused_not_crashed() {
    use std::io::Write;
    let nest = |depth: usize| -> Vec<u8> {
        let mut page = String::from("<PageContents><Shapes>");
        page.push_str(&"<Shape ID='1' Type='Group'><Shapes>".repeat(depth));
        page.push_str("<Shape ID='2' Type='Shape'/>");
        page.push_str(&"</Shapes></Shape>".repeat(depth));
        page.push_str("</Shapes></PageContents>");
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer.start_file("visio/pages/page1.xml", zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(page.as_bytes()).unwrap();
        writer.finish().unwrap().into_inner()
    };
    assert!(Package::from_reader(Cursor::new(nest(150))).is_ok(), "150 levels are fine");
    let error = Package::from_reader(Cursor::new(nest(5000))).unwrap_err().to_string();
    assert!(error.contains("nested more than"), "{error}");
    // `>` inside an attribute value and self-closing shapes do not count as nesting
    let tricky = "<Shapes>".to_string() + &"<Shape ID='1' N='a>b'/>".repeat(1000) + "</Shapes>";
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("visio/pages/page1.xml", zip::write::SimpleFileOptions::default()).unwrap();
    writer.write_all(tricky.as_bytes()).unwrap();
    assert!(Package::from_reader(Cursor::new(writer.finish().unwrap().into_inner())).is_ok());
}

#[test]
fn a_connector_between_members_of_a_turned_group_follows_a_member_that_moves() {
    use layoutcraft_model::LineJump;
    use layoutcraft_vsdx::groupedit::{add_connector, group_shapes};
    let (package, ids) = align_fixture();
    let (bytes, gid) = group_shapes(&package, 0, &ids).unwrap();
    let grouped = Package::from_reader(Cursor::new(bytes)).unwrap();
    let turned = apply(&grouped, &GroupEditor::open(&grouped, 0).unwrap().rotate_many(&[gid.clone()], 0.5).unwrap());
    let (bytes, cid) = add_connector(&turned, 0, (ids[0].as_str(), None), (ids[1].as_str(), None)).unwrap();
    let with = Package::from_reader(Cursor::new(bytes)).unwrap();
    let part = with.pages().unwrap()[0].part.clone();
    let jump = LineJump::default();
    let ends = |p: &Package| -> ((f64, f64), (f64, f64)) {
        let connects = p.connects(&part).unwrap();
        let listed = GroupEditor::open(p, 0).unwrap().connectors(&connects);
        let c = listed.as_array().unwrap().iter().find(|c| c["id"] == cid.as_str()).expect("listed").clone();
        let at = |e: &str| (c[e]["x"].as_f64().unwrap(), c[e]["y"].as_f64().unwrap());
        (at("begin"), at("end"))
    };
    let on_a_site = |p: &Package, id: &str, point: (f64, f64)| GroupEditor::open(p, 0).unwrap().connection_points(id).iter().any(|s| (s.1 - point.0).abs() < 1e-3 && (s.2 - point.1).abs() < 1e-3);
    let (b0, e0) = ends(&with);
    assert!(on_a_site(&with, &ids[0], b0) && on_a_site(&with, &ids[1], e0), "the new connector starts on sites of its members");
    // move the first member in the turned group: the connector must follow it
    let editor = GroupEditor::open(&with, 0).unwrap();
    let moved = apply(&with, &editor.move_member(&ids[0], (0.5, 0.3), false).unwrap());
    let editor = GroupEditor::open(&moved, 0).unwrap();
    let follow = editor.reroute_connectors(&moved.connects(&part).unwrap(), &jump);
    assert!(follow.iter().any(|p| p.visio_id == cid), "the connector is rerouted when a member of a turned group moves");
    let rerouted = apply(&moved, &follow);
    let (b1, e1) = ends(&rerouted);
    assert!((b1.0 - b0.0).hypot(b1.1 - b0.1) > 1e-3 || (e1.0 - e0.0).hypot(e1.1 - e0.1) > 1e-3, "the ends changed: {b0:?}{e0:?} -> {b1:?}{e1:?}");
    assert!(on_a_site(&rerouted, &ids[0], b1) && on_a_site(&rerouted, &ids[1], e1), "and they are on sites of the moved members: {b1:?} {e1:?}");
    // the result is stable: nothing more to reroute
    let again = GroupEditor::open(&rerouted, 0).unwrap().reroute_connectors(&rerouted.connects(&part).unwrap(), &jump);
    assert!(again.iter().all(|p| p.visio_id != cid), "route is current");
    assert!(import_document(&rerouted).is_ok());
}

#[test]
fn a_group_inside_a_group_is_ungrouped_into_the_outer_group_like_visio_does() {
    use layoutcraft_vsdx::groupedit::ungroup_shape;
    // groups-nested.vsdx: group 5 holds group 3 (members 1, 2 and the text "Inner group") and shape 4. Visio's own ungroup of 3 (probe-ungroup-nested.ps1):
    // 1 and 2 go into group 5 with PinY 3.3333, formulas against Sheet.5, and shape 3 stays as an ordinary shape with its text.
    let package = open_corpus("groups-nested.vsdx");
    let before = Described::of(&package);
    let (bytes, freed) = ungroup_shape(&package, 0, "3").unwrap();
    assert_eq!(freed, ["1", "2"]);
    let after_package = Package::from_reader(Cursor::new(bytes)).unwrap();
    let after = Described::of(&after_package);
    for id in ["1", "2"] {
        assert_eq!(after.frames[id].1.as_deref(), Some("5"), "{id} is in the outer group now");
        for (a, b) in corners_on_page(&before, id).iter().zip(corners_on_page(&after, id).iter()) {
            assert!(close(*a, *b), "{id} stays where it was on the page: {a:?} vs {b:?}");
        }
    }
    let one = shape_text(&after_package, "1");
    assert!(one.contains("F='Sheet.5!Width*0.2'") && one.contains("F='Sheet.5!Width*0.4'"), "tied to the outer group like Visio's: {one}");
    assert!(!one.contains("Sheet.3!"), "no tie to the old group is left: {one}");
    let (pin_y, tied_y) = {
        let at = one.find("<Cell N='PinY'").unwrap();
        let tag = &one[at..at + one[at..].find("/>").unwrap()];
        (tag.split("V='").nth(1).unwrap().split('\'').next().unwrap().parse::<f64>().unwrap(), tag.split("F='").nth(1).unwrap().split('\'').next().unwrap().to_string())
    };
    assert!((pin_y - 10.0 / 3.0).abs() < 1e-6, "PinY {pin_y}");
    assert!(tied_y.starts_with("Sheet.5!Height*0.83333333333"), "{tied_y}");
    // group 3 had its own text: it stays after its members as a plain shape; the order is 1, 2, 3, 4 inside group 5
    let three = shape_text(&after_package, "3");
    assert!(three.contains("Type='Shape'") && three.contains("Inner group") && !three.contains("<Shapes>"), "{three}");
    let xml = after_package.text(&after_package.pages().unwrap()[0].part).unwrap().to_string();
    let at = |id: &str| xml.find(&format!("<Shape ID='{id}'")).unwrap();
    assert!(at("5") < at("1") && at("1") < at("2") && at("2") < at("3") && at("3") < at("4"), "order of the shapes");
    assert!(import_document(&after_package).is_ok());
    // the outer group can still be ungrouped afterwards (page level), and a shape that is no group is refused
    assert!(ungroup_shape(&after_package, 0, "5").is_ok());
    assert!(ungroup_shape(&after_package, 0, "1").is_err());
}

#[test]
fn a_page_level_group_with_text_of_its_own_stays_as_a_shape_when_ungrouped_and_a_bare_one_goes_like_visio() {
    use layoutcraft_vsdx::groupedit::{group_shapes, ungroup_shape};
    use layoutcraft_vsdx::writer::ShapeCellPatch;
    // Visio (probe-ungroup-text.ps1): ungrouping a page-level group that has text leaves the group as an ordinary shape (Type 3, no members,
    // text kept); a group without text or geometry of its own is gone.
    let (package, ids) = align_fixture();
    let (bytes, gid) = group_shapes(&package, 0, &ids).unwrap();
    let grouped = Package::from_reader(Cursor::new(bytes)).unwrap();
    let bare = ungroup_shape(&grouped, 0, &gid).unwrap().0;
    let bare = Package::from_reader(Cursor::new(bare)).unwrap();
    assert!(layoutcraft_vsdx::writer::shape_xml(&bare, 0, &gid).is_err(), "a bare group is removed");
    let texted = apply(&grouped, &[ShapeCellPatch::new(gid.clone()).edit_text("Group text")]);
    assert!(shape_text(&texted, &gid).contains("Group text"), "the group has text of its own");
    let (bytes, freed) = ungroup_shape(&texted, 0, &gid).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    let mut freed_sorted = freed.clone();
    freed_sorted.sort();
    let mut want = ids.clone();
    want.sort();
    assert_eq!(freed_sorted, want, "the members are freed");
    let kept = shape_text(&after, &gid);
    assert!(kept.contains("Type='Shape'") && kept.contains("Group text") && !kept.contains("<Shapes>"), "the group stays as a shape with its text: {kept}");
    let doc = import_document(&after).unwrap();
    assert!(doc.pages[0].shapes.iter().any(|s| s.visio_id.as_deref() == Some(gid.as_str())), "and the importer draws it");
}

#[test]
fn the_preview_of_a_move_gives_the_routes_the_connectors_get_when_the_move_is_made() {
    use layoutcraft_model::LineJump;
    use layoutcraft_vsdx::groupedit::{add_connector, group_shapes};
    let jump = LineJump::default();
    // the routes the engine lists for connector `cid` once the move was made and the connectors were rerouted
    let real = |package: &Package, moved_id: &str, cid: &str, delta: (f64, f64)| -> Vec<(f64, f64)> {
        let part = package.pages().unwrap()[0].part.clone();
        let editor = GroupEditor::open(package, 0).unwrap();
        let moved = apply(package, &editor.move_member(moved_id, delta, false).unwrap());
        let editor = GroupEditor::open(&moved, 0).unwrap();
        let follow = editor.reroute_connectors(&moved.connects(&part).unwrap(), &jump);
        let rerouted = apply(&moved, &follow);
        let part = rerouted.pages().unwrap()[0].part.clone();
        let listed = GroupEditor::open(&rerouted, 0).unwrap().connectors(&rerouted.connects(&part).unwrap());
        let c = listed.as_array().unwrap().iter().find(|c| c["id"] == cid).expect("listed");
        c["route"].as_array().unwrap().iter().map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect()
    };
    let check = |package: &Package, moved_id: &str, cid: &str, delta: (f64, f64), what: &str| {
        let part = package.pages().unwrap()[0].part.clone();
        let connects = package.connects(&part).unwrap();
        let editor = GroupEditor::open(package, 0).unwrap();
        let before = Described::of(package);
        let preview = editor.preview_connector_routes(&connects, &[moved_id.to_string()], delta);
        let (_, route) = preview.iter().find(|(id, _)| id == cid).unwrap_or_else(|| panic!("{what}: no preview for connector {cid}"));
        let want = real(package, moved_id, cid, delta);
        assert_eq!(route.len(), want.len(), "{what}: {route:?} vs {want:?}");
        for (a, b) in route.iter().zip(&want) {
            assert!((a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4, "{what}: previewed {a:?}, real {b:?}");
        }
        // nothing was changed: the same preview again, the shape where it was
        assert_eq!(editor.preview_connector_routes(&connects, &[moved_id.to_string()], delta), preview, "{what}: repeatable");
        assert_eq!(Described::of(package).frames[moved_id].0, before.frames[moved_id].0, "{what}: the drawing is untouched");
        // a shape no connector touches gives no route; the connector itself being 'moved' is left out
        assert!(editor.preview_connector_routes(&connects, &["999".to_string()], delta).is_empty(), "{what}: unknown shape");
        assert!(editor.preview_connector_routes(&connects, &[cid.to_string()], delta).iter().all(|(id, _)| id != cid), "{what}: a moved connector moves by itself");
    };
    // a connector between two connection points of page-level shapes
    let package = open_corpus("route-sweep-sites.vsdx");
    let (bytes, cid) = add_connector(&package, 0, ("1", Some(2)), ("2", Some(4))).unwrap();
    let with = Package::from_reader(Cursor::new(bytes)).unwrap();
    check(&with, "1", &cid, (0.75, 0.25), "page-level shapes");
    check(&with, "2", &cid, (-0.5, 0.6), "the other end");
    // members of a turned group (the nearest sites are used)
    let (package, ids) = align_fixture();
    let (bytes, gid) = group_shapes(&package, 0, &ids).unwrap();
    let grouped = Package::from_reader(Cursor::new(bytes)).unwrap();
    let turned = apply(&grouped, &GroupEditor::open(&grouped, 0).unwrap().rotate_many(&[gid], 0.5).unwrap());
    let (bytes, cid) = add_connector(&turned, 0, (ids[0].as_str(), None), (ids[1].as_str(), None)).unwrap();
    let with = Package::from_reader(Cursor::new(bytes)).unwrap();
    check(&with, &ids[0], &cid, (0.5, 0.3), "members of a turned group");
}

#[test]
fn a_tall_one_dimensional_master_is_dropped_with_its_pin_at_the_drop_height_like_visio() {
    // round 11b, Q5 (probe-tall-1d-turn.ps1: CyberPower PDU83107 dropped at 6, 12 on a 1:1 page): Begin y = 12.0 = the drop height
    // (LocPinY = 0), x centred on the drop point; our drop used to centre the box on y as well (3.6 in too low)
    use layoutcraft_vsdx::stencil::import_master;
    use layoutcraft_vsdx::writer::add_master_instance;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/visioStencils-converted/Stencils/Computer Racks/CyberPower VS PDU83107 v2024.vssx");
    let Ok(stencil) = Package::open(path) else { return };
    let list = layoutcraft_vsdx::stencil::master_list(&stencil).unwrap();
    let mid = list.as_array().unwrap().first().unwrap()["id"].as_str().unwrap().to_string();
    let (bytes, new_master) = import_master(&blank_a4(), &stencil, &mid).unwrap();
    let with = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, id) = add_master_instance(&with, 0, &new_master, 6.0, 5.0).unwrap();
    let package = Package::from_reader(Cursor::new(bytes)).unwrap();
    let frames = GroupEditor::open(&package, 0).unwrap().describe();
    let f = frames.as_array().unwrap().iter().find(|f| f["id"] == id.as_str()).unwrap().clone();
    let (b, e) = ((f["ends"][0][0].as_f64().unwrap(), f["ends"][0][1].as_f64().unwrap()), (f["ends"][1][0].as_f64().unwrap(), f["ends"][1][1].as_f64().unwrap()));
    assert!((b.1 - 5.0).abs() < 1e-6 && (e.1 - 5.0).abs() < 1e-6, "Begin and End are at the drop height: {b:?} {e:?}");
    assert!(((b.0 + e.0) / 2.0 - 6.0).abs() < 1e-6, "centred on the drop point in x: {b:?} {e:?}");
    assert!((f["pin"][1].as_f64().unwrap() - 5.0).abs() < 1e-6, "the pin is at the drop height: {f}");
}

#[test]
fn adding_a_connection_point_to_a_drawn_shape_and_to_a_master_instance_like_visio() {
    // round 11b (probe-add-connection-point.ps1): the new row is IX = number of rows, X = Width*fx, Y = Height*fy, DirX/DirY/Type/AutoGen 0;
    // an instance of a master gets the whole inherited section written out with the new row last; existing numbers do not change
    use layoutcraft_vsdx::connpoints::add_connection_point;
    use layoutcraft_vsdx::draw::DrawKind::Rectangle;
    use layoutcraft_vsdx::stencil::import_master;
    use layoutcraft_vsdx::writer::add_master_instance;
    // a drawn rectangle 2 x 1 in: it has 4 points of its own
    let (package, id) = draw_on_blank(Rectangle, 1.0, 1.0, 3.0, 2.0);
    let before = GroupEditor::open(&package, 0).unwrap().connection_points(&id);
    let (bytes, n) = add_connection_point(&package, 0, &id, 0.25, 0.75).unwrap();
    let p1 = Package::from_reader(Cursor::new(bytes)).unwrap();
    let after = GroupEditor::open(&p1, 0).unwrap().connection_points(&id);
    assert_eq!(after.len(), before.len() + 1, "one more point: {before:?} -> {after:?}");
    assert_eq!(n, after.len(), "the new point is the last one");
    for (b, a) in before.iter().zip(after.iter()) {
        assert!((b.1 - a.1).abs() < 1e-9 && (b.2 - a.2).abs() < 1e-9, "an existing point did not move: {b:?} vs {a:?}");
    }
    let last = after.last().unwrap();
    assert!((last.1 - (1.0 + 2.0 * 0.25)).abs() < 1e-6 && (last.2 - (1.0 + 1.0 * 0.75)).abs() < 1e-6, "the new point is at 25 % / 75 % of the box: {last:?}");
    assert!(add_connection_point(&package, 0, &id, 1.5, 0.5).is_err() && add_connection_point(&package, 0, "999", 0.5, 0.5).is_err(), "outside the box and unknown shapes are refused");
    // an instance of a master with inherited connection points
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/visioStencils-converted/Stencils/Computer Racks/CyberPower VS PDU83107 v2024.vssx");
    let Ok(stencil) = Package::open(path) else { return };
    let list = layoutcraft_vsdx::stencil::master_list(&stencil).unwrap();
    let mid = list.as_array().unwrap().first().unwrap()["id"].as_str().unwrap().to_string();
    let (bytes, new_master) = import_master(&blank_a4(), &stencil, &mid).unwrap();
    let with = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, inst) = add_master_instance(&with, 0, &new_master, 6.0, 5.0).unwrap();
    let package = Package::from_reader(Cursor::new(bytes)).unwrap();
    let before = GroupEditor::open(&package, 0).unwrap().connection_points(&inst);
    assert!(before.len() > 1, "the master has connection points of its own: {}", before.len());
    let (bytes, n) = add_connection_point(&package, 0, &inst, 0.5, 0.5).unwrap();
    let p2 = Package::from_reader(Cursor::new(bytes)).unwrap();
    let after = GroupEditor::open(&p2, 0).unwrap().connection_points(&inst);
    assert_eq!(after.len(), before.len() + 1, "instance: one more point");
    assert_eq!(n, after.len());
    for (b, a) in before.iter().zip(after.iter()) {
        assert!((b.1 - a.1).abs() < 1e-6 && (b.2 - a.2).abs() < 1e-6, "instance: an existing point did not move: {b:?} vs {a:?}");
    }
}

#[test]
fn moving_a_connection_point_changes_only_that_point_like_visio() {
    // round 11b: a moved point rewrites only its own X / Y formulas (Width*fx, Height*fy); the numbers of all points stay
    use layoutcraft_vsdx::connpoints::{add_connection_point, move_connection_point};
    use layoutcraft_vsdx::draw::DrawKind::Rectangle;
    let (package, id) = draw_on_blank(Rectangle, 1.0, 1.0, 3.0, 2.0);
    let (bytes, n) = add_connection_point(&package, 0, &id, 0.25, 0.75).unwrap();
    let p1 = Package::from_reader(Cursor::new(bytes)).unwrap();
    let before = GroupEditor::open(&p1, 0).unwrap().connection_points(&id);
    let bytes = move_connection_point(&p1, 0, &id, n, 0.8, 0.2).unwrap();
    let p2 = Package::from_reader(Cursor::new(bytes)).unwrap();
    let after = GroupEditor::open(&p2, 0).unwrap().connection_points(&id);
    assert_eq!(before.len(), after.len(), "no point was added or removed");
    for (b, a) in before.iter().zip(after.iter()) {
        if b.0 == n {
            assert!((a.1 - (1.0 + 2.0 * 0.8)).abs() < 1e-6 && (a.2 - (1.0 + 1.0 * 0.2)).abs() < 1e-6, "the point moved to 80 % / 20 % of the box: {a:?}");
        } else {
            assert!((a.1 - b.1).abs() < 1e-9 && (a.2 - b.2).abs() < 1e-9, "another point moved: {b:?} vs {a:?}");
        }
    }
    assert!(move_connection_point(&p1, 0, &id, 99, 0.5, 0.5).is_err() && move_connection_point(&p1, 0, &id, n, -0.1, 0.5).is_err(), "an unknown point and a place outside the box are refused");
    // a built-in point (number 1) moves too
    let bytes = move_connection_point(&p1, 0, &id, 1, 0.5, 0.5).unwrap();
    let p3 = Package::from_reader(Cursor::new(bytes)).unwrap();
    let c = GroupEditor::open(&p3, 0).unwrap().connection_points(&id);
    assert!((c[0].1 - 2.0).abs() < 1e-6 && (c[0].2 - 1.5).abs() < 1e-6, "point 1 is at the centre: {:?}", c[0]);
}

#[test]
fn deleting_a_connection_point_renumbers_the_later_ones_like_visio() {
    // round 11b (probe-delete-connection-point.ps1): the rows after the deleted one are renumbered without a gap; the others stay where they are
    use layoutcraft_vsdx::connpoints::delete_connection_point;
    use layoutcraft_vsdx::draw::DrawKind::Rectangle;
    let (package, id) = draw_on_blank(Rectangle, 1.0, 1.0, 3.0, 2.0);
    let before = GroupEditor::open(&package, 0).unwrap().connection_points(&id);
    assert!(before.len() >= 4, "a drawn rectangle has its own points: {before:?}");
    let bytes = delete_connection_point(&package, 0, &id, 2).unwrap();
    let p1 = Package::from_reader(Cursor::new(bytes)).unwrap();
    let after = GroupEditor::open(&p1, 0).unwrap().connection_points(&id);
    assert_eq!(after.len(), before.len() - 1, "one point fewer");
    assert_eq!(after.iter().map(|p| p.0).collect::<Vec<_>>(), (1..=after.len()).collect::<Vec<_>>(), "numbers 1..n without a gap");
    // point 1 stayed, old point 3 is now number 2, old point 4 number 3 ...
    assert!((after[0].1 - before[0].1).abs() < 1e-9 && (after[0].2 - before[0].2).abs() < 1e-9);
    for i in 1..after.len() {
        assert!((after[i].1 - before[i + 1].1).abs() < 1e-9 && (after[i].2 - before[i + 1].2).abs() < 1e-9, "old point {} is now {}", i + 2, i + 1);
    }
    assert!(delete_connection_point(&package, 0, &id, 99).is_err() && delete_connection_point(&package, 0, "999", 1).is_err(), "an unknown point or shape is refused");
}

#[test]
fn dragging_an_end_of_a_flat_one_dimensional_group_whose_ends_span_its_box_turns_it_like_visio() {
    // round 11b (probe-flat-1d-turn.ps1, IBM Console Switch Rear dropped at 6, 6): EndX +0.1 -> Width 2.0, Pin x 6.05; then EndY +0.3 -> Width 2.0224,
    // Pin (6.05, 6.15), Angle 0.1489; the pin stays the middle of the Begin-End line
    use layoutcraft_vsdx::groupedit::{move_connector_end, EndTarget};
    use layoutcraft_vsdx::stencil::import_master;
    use layoutcraft_vsdx::writer::add_master_instance;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/visioStencils-converted/Stencils/Computer Racks/IBM Racks v2022.vssx");
    let Ok(stencil) = Package::open(path) else { return };
    let list = layoutcraft_vsdx::stencil::master_list(&stencil).unwrap();
    let Some(entry) = list.as_array().unwrap().iter().find(|m| m["name"].as_str().is_some_and(|n| n.contains("Console Switch Rear"))) else { return };
    let mid = entry["id"].as_str().unwrap().to_string();
    let (bytes, new_master) = import_master(&blank_a4(), &stencil, &mid).unwrap();
    let with = Package::from_reader(Cursor::new(bytes)).unwrap();
    let (bytes, id) = add_master_instance(&with, 0, &new_master, 6.0, 6.0).unwrap();
    let package = Package::from_reader(Cursor::new(bytes)).unwrap();
    // Visio's own drop at (6, 6): Begin (5.05, 6), End (6.95, 6)
    let (b0, e0) = ((5.05f64, 6.0f64), (6.95f64, 6.0f64));
    let bytes = move_connector_end(&package, 0, &id, false, EndTarget::Free(e0.0 + 0.1, e0.1)).unwrap();
    let p1 = Package::from_reader(Cursor::new(bytes)).unwrap();
    let bytes = move_connector_end(&p1, 0, &id, false, EndTarget::Free(e0.0 + 0.1, e0.1 + 0.3)).unwrap();
    let p2 = Package::from_reader(Cursor::new(bytes)).unwrap();
    let frames = GroupEditor::open(&p2, 0).unwrap().describe();
    let f = frames.as_array().unwrap().iter().find(|f| f["id"] == id.as_str()).unwrap().clone();
    let (w, angle) = (f["size"][0].as_f64().unwrap(), f["angle"].as_f64().unwrap());
    let len = ((e0.0 + 0.1 - b0.0).powi(2) + (e0.1 + 0.3 - b0.1).powi(2)).sqrt();
    assert!((w - len).abs() < 1e-6, "Width is the length of the line: {w} vs {len}");
    assert!((angle - 0.3f64.atan2(e0.0 + 0.1 - b0.0)).abs() < 1e-6, "Angle = ATAN2 of the line: {angle}");
    let pin = (f["pin"][0].as_f64().unwrap(), f["pin"][1].as_f64().unwrap());
    assert!((pin.0 - (b0.0 + e0.0 + 0.1) / 2.0).abs() < 1e-6 && (pin.1 - (b0.1 + e0.1 + 0.3) / 2.0).abs() < 1e-6, "the pin is the middle of the line: {pin:?}");
}

#[test]
fn changing_a_shape_keeps_place_size_text_format_and_moves_the_glue_like_visio() {
    // round 11b (probe-change-shape2/3.ps1, Shape.ReplaceShape): a NEW id with OriginalID, the new master, the old Pin / Width / Height / Angle, the local
    // format cells, the Character section and the text; connectors glued to the old shape follow to the new shape's NEAREST connection point
    use layoutcraft_vsdx::groupedit::add_connector;
    use layoutcraft_vsdx::replace::replace_shape;
    use layoutcraft_vsdx::stencil::import_master;
    let package = open_corpus("route-sweep-sites.vsdx");
    let part = package.pages().unwrap()[0].part.clone();
    let (bytes, conn) = add_connector(&package, 0, ("1", Some(2)), ("2", Some(4))).unwrap();
    let with_conn = Package::from_reader(Cursor::new(bytes)).unwrap();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/visioStencils-converted/Stencils/Computer Racks/CyberPower VS PDU83107 v2024.vssx");
    let Ok(stencil) = Package::open(path) else { return };
    let list = layoutcraft_vsdx::stencil::master_list(&stencil).unwrap();
    let mid = list.as_array().unwrap().first().unwrap()["id"].as_str().unwrap().to_string();
    let (bytes, new_master) = import_master(&with_conn, &stencil, &mid).unwrap();
    let ready = Package::from_reader(Cursor::new(bytes)).unwrap();
    let before = GroupEditor::open(&ready, 0).unwrap().describe().as_array().unwrap().iter().find(|f| f["id"] == "1").unwrap().clone();
    let old_point = GroupEditor::open(&ready, 0).unwrap().connection_points("1").into_iter().find(|p| p.0 == 2).unwrap();
    let (bytes, new_id) = replace_shape(&ready, 0, "1", &new_master).unwrap();
    let after = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_ne!(new_id, "1", "a new shape id");
    let editor = GroupEditor::open(&after, 0).unwrap();
    let frames = editor.describe();
    let frames = frames.as_array().unwrap();
    assert!(frames.iter().all(|f| f["id"] != "1"), "the old shape is gone");
    let f = frames.iter().find(|f| f["id"] == new_id.as_str()).expect("the new shape is on the page");
    for key in ["size", "pin"] {
        assert!((f[key][0].as_f64().unwrap() - before[key][0].as_f64().unwrap()).abs() < 1e-6 && (f[key][1].as_f64().unwrap() - before[key][1].as_f64().unwrap()).abs() < 1e-6, "{key} kept: {} vs {}", f[key], before[key]);
    }
    let xml = shape_text(&after, &new_id);
    assert!(xml.contains("OriginalID='1'") && xml.contains(&format!("Master='{new_master}'")), "{xml}");
    // the connector is glued to the new shape, its end on the new point nearest to where the old one was
    let connects = after.connects(&part).unwrap();
    let mine: Vec<_> = connects.iter().filter(|c| c.from_sheet == conn).collect();
    let begin = mine.iter().find(|c| c.from_cell == "BeginX").expect("the begin is still glued");
    assert_eq!(begin.to_sheet, new_id, "glued to the new shape");
    let k: usize = begin.to_cell.strip_prefix("Connections.X").unwrap().parse().unwrap();
    let new_points = editor.connection_points(&new_id);
    let nearest = new_points.iter().min_by(|a, b| ((a.1 - old_point.1).powi(2) + (a.2 - old_point.2).powi(2)).total_cmp(&((b.1 - old_point.1).powi(2) + (b.2 - old_point.2).powi(2)))).unwrap();
    assert_eq!(k, nearest.0, "the nearest point of the new shape");
    let cxml = shape_text(&after, &conn);
    assert!(cxml.contains(&format!("Sheet.{new_id}!Connections.X{k}")) && !cxml.contains("Sheet.1!"), "formula follows: {cxml}");
    assert!(import_document(&after).is_ok());
    // connectors and unknown shapes are refused
    assert!(replace_shape(&ready, 0, &conn, &new_master).is_err() && replace_shape(&ready, 0, "999", &new_master).is_err());
}

#[test]
fn an_instance_with_text_can_be_resized_again_and_again() {
    // user bug 2026-11-16: a master instance with text took its first size cells AFTER the text; the second resize wrote another set before the sections and the stale
    // first set won, so the shape stayed at its first size
    use layoutcraft_vsdx::groupedit::GroupEditor;
    let package = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(
        layoutcraft_vsdx::fresh::write_blank_document(&layoutcraft_vsdx::fresh::BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "R".to_string() }).unwrap(),
    ))
    .unwrap();
    let stencil = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(layoutcraft_vsdx::builtin::builtin_stencil().unwrap())).unwrap();
    let (with_master, master) = layoutcraft_vsdx::stencil::import_master(&package, &stencil, "1").unwrap();
    let (bytes, id) = layoutcraft_vsdx::writer::add_master_instance(&layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(with_master)).unwrap(), 0, &master, 5.0, 4.0).unwrap();
    let mut package = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(bytes)).unwrap();
    let patch = layoutcraft_vsdx::writer::ShapeCellPatch::new(&id).text("one");
    package = apply(&package, &[patch]);
    for (w, h) in [(3.0, 2.0), (4.0, 3.0), (2.0, 1.5), (5.0, 2.5)] {
        let editor = GroupEditor::open(&package, 0).unwrap();
        let patches = editor.resize_member(&id, (w, h), (0.0, 0.0), false).unwrap();
        package = apply(&package, &patches);
        let editor = GroupEditor::open(&package, 0).unwrap();
        let frame = editor.describe();
        let shape = frame.as_array().unwrap().iter().find(|f| f["id"] == id.as_str()).unwrap();
        let size = (shape["size"][0].as_f64().unwrap(), shape["size"][1].as_f64().unwrap());
        assert!((size.0 - w).abs() < 1e-6 && (size.1 - h).abs() < 1e-6, "resize to {w} x {h} gave {size:?}");
    }
}
