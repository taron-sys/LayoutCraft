#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use layoutcraft_engine::Engine;

fn example() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/MS Example.vsdx")).expect("example drawing")
}

#[test]
fn nothing_open_reports_none() {
    let mut engine = Engine::new();
    assert!(engine.page_count().is_none() && engine.json().is_none() && engine.export().is_none() && engine.svg(0, false).is_none());
    assert_eq!(engine.undo(), Ok(false));
    assert_eq!(engine.redo(), Ok(false));
    assert_eq!(engine.redo_depth(), 0);
}

#[test]
fn open_gives_model_svg_and_the_same_bytes_back() {
    let bytes = example();
    let mut engine = Engine::new();
    engine.open(bytes.clone()).unwrap();
    assert!(engine.page_count().unwrap() >= 1);
    assert!(engine.json().unwrap().starts_with('{'));
    assert!(engine.svg(0, false).unwrap().unwrap().contains("<svg"));
    assert_eq!(engine.export().unwrap(), &bytes[..]);
}

#[test]
fn bad_bytes_are_an_error_and_keep_the_open_drawing() {
    let mut engine = Engine::new();
    engine.open(example()).unwrap();
    assert!(engine.open(b"not a zip".to_vec()).is_err());
    assert!(engine.page_count().is_some());
}

#[test]
fn new_blank_then_undo_redo_walk_the_history() {
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    assert_eq!(engine.page_count(), Some(1));
    // an edit by hand: the engine's history is what undo / redo move through
    let first = engine.bytes.clone();
    engine.history.push(first.clone());
    let second = example();
    engine.adopt(second.clone()).unwrap();
    assert_eq!(engine.undo_depth(), 1);
    assert_eq!(engine.undo(), Ok(true));
    assert_eq!(engine.export().unwrap(), &first[..]);
    assert_eq!(engine.redo_depth(), 1);
    assert_eq!(engine.redo(), Ok(true));
    assert_eq!(engine.export().unwrap(), &second[..]);
    assert_eq!(engine.undo_depth(), 1);
    assert_eq!(engine.collapse_history(0), 1);
}

#[test]
fn drawing_tools_are_one_undo_step_each() {
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    let before = engine.export().unwrap().to_vec();
    let rect = engine.draw(0, 0, 1.0, 1.0, 3.0, 2.0).unwrap();
    assert!(!rect.is_empty());
    assert_eq!(engine.undo_depth(), 1);
    let line = engine.draw(0, 6, 1.0, 1.0, 4.0, 4.0).unwrap();
    assert_ne!(rect, line);
    assert!(engine.draw(0, 99, 0.0, 0.0, 1.0, 1.0).is_err());
    let points = layoutcraft_engine::parse_points("1,1;2,2;3,1;4,2").unwrap();
    engine.draw_curve(0, &points).unwrap();
    engine.draw_polyline(0, &points, true).unwrap();
    engine.draw_arc(0, (1.0, 1.0), (3.0, 1.0), 0.5).unwrap();
    assert_eq!(engine.undo_depth(), 5);
    assert!(layoutcraft_engine::parse_points("1,x").is_err());
    assert!(engine.json().unwrap().contains(&rect));
    for _ in 0..5 {
        assert_eq!(engine.undo(), Ok(true));
    }
    assert_eq!(engine.export().unwrap(), &before[..]);
}

#[test]
fn edit_moves_a_shape_as_one_undo_step_and_the_preview_runs() {
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    let id = engine.draw(0, 0, 1.0, 1.0, 3.0, 2.0).unwrap();
    let before = engine.undo_depth();
    engine.edit(0, 2, &id, 1.0, 0.5, 0.0, 0.0, 0).unwrap();
    assert_eq!(engine.undo_depth(), before + 1);
    assert!(engine.edit(0, 99, &id, 0.0, 0.0, 0.0, 0.0, 0).is_err());
    assert_eq!(engine.preview_move(0, &[id.clone()], (1.0, 0.0)).unwrap(), "[]");
    engine.edit(0, 4, &id, 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    assert!(!engine.json().unwrap().contains(&format!("\"visioId\":\"{id}\"")));
}

#[test]
fn page_ops_and_group_ops_are_undoable() {
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    assert_eq!(engine.page_op(1, 0, 0, "Second").unwrap(), "1");
    assert_eq!(engine.page_count(), Some(2));
    assert_eq!(engine.page_op(3, 1, 0, "Renamed").unwrap(), "1");
    assert!(engine.page_op(99, 0, 0, "").is_err());
    assert!(engine.page_op(6, 0, 0, "x").is_err());
    let one = engine.draw(0, 0, 1.0, 1.0, 2.0, 2.0).unwrap();
    let two = engine.draw(0, 2, 3.0, 1.0, 4.0, 2.0).unwrap();
    let group = engine.group_op(0, 0, &format!("{one}\n{two}")).unwrap();
    assert!(!group.is_empty());
    let freed = engine.group_op(0, 1, &group).unwrap();
    assert!(freed.contains(&one) && freed.contains(&two), "{freed}");
    assert!(engine.group_op(0, 99, "").is_err());
    let depth = engine.undo_depth();
    for _ in 0..depth {
        assert_eq!(engine.undo(), Ok(true));
    }
    assert_eq!(engine.page_count(), Some(1));
}

#[test]
fn clipboard_and_connectors() {
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    let a = engine.draw(0, 0, 1.0, 1.0, 2.0, 2.0).unwrap();
    let b = engine.draw(0, 0, 5.0, 1.0, 6.0, 2.0).unwrap();
    assert!(engine.copy(0, &[]).is_err());
    assert!(engine.paste(0, 0.0, 0.0).is_err());
    assert_eq!(engine.copy(0, &[a.clone()]).unwrap(), 1);
    assert_eq!(engine.clipboard_count(), 1);
    let pasted = engine.paste(0, 0.0, 2.0).unwrap();
    assert_eq!(pasted.len(), 1);
    let c = engine.add_connector(0, &a, None, &b, None).unwrap();
    assert!(engine.connectors_json(0).unwrap().contains(&c));
    engine.move_connector_end(0, &c, false, None, (8.0, 8.0)).unwrap();
    assert!(engine.move_connector_end(0, &c, false, None, (f64::NAN, 1.0)).is_err());
    engine.move_connector_end(0, &c, false, Some((&pasted[0], None)), (0.0, 0.0)).unwrap();
}

#[test]
fn stencils_and_masters() {
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    assert!(engine.stencil_open(b"nope".to_vec()).is_err());
    let built = engine.stencil_builtin().unwrap();
    let built: serde_json::Value = serde_json::from_str(&built).unwrap();
    let index = built["index"].as_u64().unwrap() as usize;
    // asking again answers with the same stencil
    assert_eq!(serde_json::from_str::<serde_json::Value>(&engine.stencil_builtin().unwrap()).unwrap()["index"].as_u64().unwrap() as usize, index);
    let master = built["masters"][0]["id"].as_str().unwrap().to_string();
    assert!(engine.stencil_thumb(index, &master).unwrap().contains("<svg"));
    let before = engine.undo_depth();
    let shape = engine.stencil_drop(0, index, &master, 4.0, 4.0).unwrap();
    assert_eq!(engine.undo_depth(), before + 1);
    assert!(engine.masters_json().unwrap().starts_with('['));
    let drawing_master = serde_json::from_str::<serde_json::Value>(&engine.masters_json().unwrap()).unwrap()[0]["id"].as_str().unwrap().to_string();
    assert!(engine.master_thumb(&drawing_master).unwrap().contains("<svg"));
    engine.add_master(0, &drawing_master, 2.0, 2.0).unwrap();
    assert!(engine.replace_shape(0, -1, &format!("{shape}|{drawing_master}")).is_ok());
    assert!(engine.stencil_close(index));
    assert!(engine.stencil_thumb(index, &master).is_err());
    assert!(!engine.stencil_close(99));
}

#[test]
fn read_only_info_is_json() {
    let mut engine = Engine::new();
    assert!(engine.page_names_json().is_err());
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    let id = engine.draw(0, 0, 1.0, 1.0, 3.0, 2.0).unwrap();
    let json = |text: String| serde_json::from_str::<serde_json::Value>(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
    assert_eq!(json(engine.page_names_json().unwrap()), serde_json::json!(["Page-1"]));
    assert!(json(engine.doc_props_json().unwrap()).is_object());
    assert!(json(engine.shape_sheet_json(0, &id).unwrap()).is_object());
    assert!(engine.shape_sheet_json(0, "9999").is_err());
    assert!(json(engine.themes_json().unwrap())["themes"].is_array());
    assert!(json(engine.comments_json().unwrap()).is_array());
    assert!(json(engine.hyperlinks_json(0, &id).unwrap()).is_array());
    assert!(json(engine.guides_json(0).unwrap()).is_array());
    assert!(engine.guides_json(9).is_err());
    assert!(json(engine.background_info_json().unwrap())["backgrounds"].is_array());
    assert!(json(engine.shape_names_json(0).unwrap()).is_object());
    assert!(json(engine.group_info_json(0).unwrap()).as_array().unwrap().iter().any(|s| s["id"] == id));
    assert!(json(engine.style_info_json(0, &id).unwrap()).is_object());
    assert!(json(engine.styles_list_json().unwrap()).is_array());
    assert!(json(engine.connection_points_json(0, &id).unwrap()).is_array());
    assert!(json(engine.data_sort_json(0, &id).unwrap()).is_array());
    assert!(engine.text_info_json(0, &id).is_ok());
    let size = json(engine.page_size_json(0).unwrap());
    assert!((size["w"].as_f64().unwrap() - 11.0).abs() < 1e-9 && (size["h"].as_f64().unwrap() - 8.5).abs() < 1e-9);
    assert!(engine.page_size_json(5).is_err());
    engine.edit(0, 8, &format!("{id}\nHello"), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    assert!(engine.text_state_json(0, &id, 0, 5).is_ok());
}

#[test]
fn the_apps_edit_pattern_draw_text_collapse_then_move_then_delete() {
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    let base = engine.undo_depth();
    let id = engine.draw(0, 0, 1.0, 1.0, 2.0, 2.0).unwrap();
    engine.edit(0, 8, &format!("{id}\nRectangle"), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    assert_eq!(engine.collapse_history(base), base + 1);
    assert_eq!(engine.undo_depth(), base + 1);
    let model = |engine: &Engine| serde_json::from_str::<serde_json::Value>(&engine.json().unwrap()).unwrap();
    let find = |m: &serde_json::Value| m["pages"][0]["shapes"].as_array().unwrap().iter().find(|s| s["visioId"] == id).cloned();
    let before = find(&model(&engine)).expect("the new shape");
    assert_eq!(before["label"], "Rectangle");
    engine.edit(0, 2, &id, 1.5, -0.5, 0.0, 0.0, 0).unwrap();
    let after = find(&model(&engine)).unwrap();
    assert!((after["x"].as_f64().unwrap() - before["x"].as_f64().unwrap() - 1.5 * 96.0).abs() < 1e-6, "{before} {after}");
    assert!((after["y"].as_f64().unwrap() - before["y"].as_f64().unwrap() - 0.5 * 96.0).abs() < 1e-6, "{before} {after}");
    engine.edit(0, 4, &id, 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    assert!(find(&model(&engine)).is_none());
    assert_eq!(engine.undo(), Ok(true));
    assert!(find(&model(&engine)).is_some());
}

#[test]
fn every_draw_tool_makes_a_shape_in_the_dragged_box() {
    // the native app's pattern: scene px (y down) -> inches (y up) with the page height, left-bottom and right-top corners
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    let page_h = 8.5;
    let (px0, py0, px1, py1) = (96.0, 96.0, 288.0, 192.0); // a 192 x 96 px drag (2 x 1 inch)
    for kind in 0..=5 {
        let before = engine.undo_depth();
        let id = engine.draw(0, kind, px0 / 96.0, page_h - py1 / 96.0, px1 / 96.0, page_h - py0 / 96.0).unwrap();
        assert_eq!(engine.undo_depth(), before + 1, "kind {kind} is one undo step");
        let model: serde_json::Value = serde_json::from_str(&engine.json().unwrap()).unwrap();
        let shape = model["pages"][0]["shapes"].as_array().unwrap().iter().find(|s| s["visioId"] == id).unwrap_or_else(|| panic!("kind {kind} shape {id}")).clone();
        let near = |name: &str, want: f64| assert!((shape[name].as_f64().unwrap() - want).abs() < 0.5, "kind {kind} {name}: {shape}");
        near("x", px0);
        near("y", py0);
        near("w", px1 - px0);
        near("h", py1 - py0);
    }
    // a line runs from the first point to the second
    let id = engine.draw(0, 6, 1.0, 1.0, 4.0, 3.0).unwrap();
    assert!(engine.json().unwrap().contains(&id));
}

#[test]
fn the_connector_tool_glues_two_shapes_in_one_undo_step() {
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    let a = engine.draw(0, 0, 1.0, 1.0, 2.0, 2.0).unwrap();
    let b = engine.draw(0, 2, 5.0, 1.0, 6.0, 2.0).unwrap();
    let before = engine.undo_depth();
    let c = engine.add_connector(0, &a, None, &b, None).unwrap();
    assert_eq!(engine.undo_depth(), before + 1);
    let list: serde_json::Value = serde_json::from_str(&engine.connectors_json(0).unwrap()).unwrap();
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0]["id"], c);
    // moving a glued shape reroutes the connector
    let route = |l: &serde_json::Value| l[0]["route"].to_string();
    let first = route(&serde_json::from_str::<serde_json::Value>(&engine.connectors_json(0).unwrap()).unwrap());
    engine.edit(0, 2, &b, 0.0, 2.0, 0.0, 0.0, 0).unwrap();
    let second = route(&serde_json::from_str::<serde_json::Value>(&engine.connectors_json(0).unwrap()).unwrap());
    assert_ne!(first, second);
    assert_eq!(engine.undo(), Ok(true));
    assert_eq!(engine.undo(), Ok(true));
    assert_eq!(engine.connectors_json(0).unwrap(), "[]");
}

#[test]
fn multiline_text_and_character_format_are_one_undo_step() {
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    let id = engine.draw(0, 0, 1.0, 1.0, 3.0, 2.0).unwrap();
    let base = engine.undo_depth();
    // the native app's pattern: text (op 8), format (op 10), collapse
    engine.edit(0, 8, &format!("{id}\nfirst line\nsecond line"), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    engine.edit(0, 10, &format!("{id}\n{}", r#"{"bold":true,"italic":true,"underline":true,"fontPt":18}"#), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    assert_eq!(engine.collapse_history(base), base + 1);
    let model: serde_json::Value = serde_json::from_str(&engine.json().unwrap()).unwrap();
    let shape = model["pages"][0]["shapes"].as_array().unwrap().iter().find(|s| s["visioId"] == id).unwrap().clone();
    assert_eq!(shape["label"], "first line\nsecond line", "{shape}");
    assert_eq!(shape["bold"], true, "{shape}");
    assert_eq!(shape["italic"], true, "{shape}");
    assert_eq!(shape["underline"], true, "{shape}");
    assert!((shape["fontSize"].as_f64().unwrap() - 18.0 * 96.0 / 72.0).abs() < 0.01, "{shape}");
    assert_eq!(engine.undo(), Ok(true));
    let model: serde_json::Value = serde_json::from_str(&engine.json().unwrap()).unwrap();
    let shape = model["pages"][0]["shapes"].as_array().unwrap().iter().find(|s| s["visioId"] == id).unwrap().clone();
    assert_ne!(shape["label"], "first line\nsecond line");
}

#[test]
fn editing_text_and_bold_on_the_example_drawing() {
    let mut engine = Engine::new();
    engine.open(example()).unwrap();
    let id = "6";
    let plain = engine.edit(0, 8, &format!("{id}\nInput Device\nLine two"), 0.0, 0.0, 0.0, 0.0, 0);
    assert!(plain.is_ok(), "text: {plain:?}");
    let bold = engine.edit(0, 10, &format!("{id}\n{}", r#"{"bold":true}"#), 0.0, 0.0, 0.0, 0.0, 0);
    assert!(bold.is_ok(), "bold: {bold:?}");
}

#[test]
fn text_and_format_edits_leave_every_connector_route_alone() {
    let mut engine = Engine::new();
    engine.open(example()).unwrap();
    let before = engine.connectors_json(0).unwrap();
    engine.edit(0, 8, "6\nInput Device\nLine two", 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    engine.edit(0, 10, &format!("6\n{}", r#"{"bold":true}"#), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    assert_eq!(engine.connectors_json(0).unwrap(), before);
}

#[test]
fn style_edits_on_the_example_drawing_change_fill_line_weight_pattern_only() {
    let mut engine = Engine::new();
    engine.open(example()).unwrap();
    let shape_of = |engine: &Engine, id: &str| {
        let model: serde_json::Value = serde_json::from_str(&engine.json().unwrap()).unwrap();
        model["pages"][0]["shapes"].as_array().unwrap().iter().find(|s| s["visioId"] == id).unwrap_or_else(|| panic!("shape {id}")).clone()
    };
    let connectors = engine.connectors_json(0).unwrap();
    let before = shape_of(&engine, "6");
    let base = engine.undo_depth();
    engine.edit(0, 10, &format!("6\n{}", r##"{"fill":"#ff0000","line":"#00ff00","weight":4,"pattern":2}"##), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    assert_eq!(engine.undo_depth(), base + 1);
    let after = shape_of(&engine, "6");
    let part_fills = |shape: &serde_json::Value| shape["parts"].as_array().unwrap().iter().filter_map(|p| p["fill"].as_str()).map(str::to_string).collect::<Vec<_>>();
    assert!(part_fills(&after).iter().any(|f| f == "#ff0000"), "{after}");
    assert_eq!(after["stroke"], "#00ff00", "{after}");
    assert!((after["strokeWidth"].as_f64().unwrap() - 4.0).abs() < 0.01, "{after}");
    assert_ne!(after["strokeStyle"], before["strokeStyle"], "{before} {after}");
    assert_eq!(after["x"], before["x"]);
    assert_eq!(after["w"], before["w"]);
    assert_eq!(engine.connectors_json(0).unwrap(), connectors);
    // no fill, then undo
    engine.edit(0, 10, &format!("6\n{}", r#"{"fill":"none"}"#), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    assert!(!part_fills(&shape_of(&engine, "6")).iter().any(|f| f == "#ff0000"));
    assert_eq!(engine.undo(), Ok(true));
    assert!(part_fills(&shape_of(&engine, "6")).iter().any(|f| f == "#ff0000"));
}

#[test]
fn page_operations_on_the_example_drawing_and_their_undo() {
    let mut engine = Engine::new();
    engine.open(example()).unwrap();
    let names = |engine: &Engine| serde_json::from_str::<Vec<String>>(&engine.page_names_json().unwrap()).unwrap();
    assert_eq!(names(&engine).len(), 1);
    let first = names(&engine)[0].clone();
    // add after page 0: the new page is shown
    assert_eq!(engine.page_op(1, 0, 0, "").unwrap(), "1");
    assert_eq!(names(&engine).len(), 2);
    // rename it, duplicate page 0, move the duplicate to the end, delete the added page
    assert_eq!(engine.page_op(3, 1, 0, "Second").unwrap(), "1");
    assert_eq!(names(&engine)[1], "Second");
    let dup = engine.page_op(5, 0, 0, "").unwrap();
    assert_eq!(dup, "1");
    assert_eq!(names(&engine).len(), 3);
    assert_eq!(engine.page_op(4, 1, 2, "").unwrap(), "2");
    assert_eq!(engine.page_op(2, 0, 0, "").unwrap(), "0");
    assert_eq!(names(&engine).len(), 2);
    // the first page's drawing is intact in the model of a duplicate
    let model: serde_json::Value = serde_json::from_str(&engine.json().unwrap()).unwrap();
    assert!(model["pages"].as_array().unwrap().iter().any(|p| p["shapes"].as_array().map_or(0, |s| s.len()) >= 15), "a page still has the example's shapes");
    assert!(engine.page_op(1, 9, 0, "").is_err(), "no such page");
    // everything is undoable: back to the single original page
    while engine.undo_depth() > 0 {
        assert_eq!(engine.undo(), Ok(true));
    }
    assert_eq!(names(&engine), vec![first]);
}

#[test]
fn layers_add_toggle_assign_and_delete() {
    let mut engine = Engine::new();
    engine.open(example()).unwrap();
    let layers = |engine: &Engine| {
        let model: serde_json::Value = serde_json::from_str(&engine.json().unwrap()).unwrap();
        model["pages"][0]["layers"].as_array().cloned().unwrap_or_default()
    };
    let shape_layers = |engine: &Engine, id: &str| {
        let model: serde_json::Value = serde_json::from_str(&engine.json().unwrap()).unwrap();
        model["pages"][0]["shapes"].as_array().unwrap().iter().find(|s| s["visioId"] == id).unwrap()["layers"].clone()
    };
    let before = layers(&engine).len();
    let base = engine.undo_depth();
    assert_eq!(engine.page_op(7, 0, 0, "Notes").unwrap(), "0");
    let now = layers(&engine);
    assert_eq!(now.len(), before + 1, "{now:?}");
    let added = now.last().unwrap().clone();
    assert_eq!(added["name"], "Notes");
    let index = added["index"].as_u64().unwrap() as usize;
    // hide it, make it non-printing and locked (name empty = keep)
    engine.page_op(8, 0, index, "|0|0|1").unwrap();
    let changed = layers(&engine).into_iter().find(|l| l["name"] == "Notes").unwrap();
    assert_eq!(changed["visible"], false, "{changed}");
    assert_eq!(changed["print"], false, "{changed}");
    assert_eq!(changed["locked"], true, "{changed}");
    // put a shape on it and take it off again
    engine.edit(0, 16, "6", index as f64, 0.0, 0.0, 0.0, 1).unwrap();
    let on = |engine: &Engine| shape_layers(engine, "6").as_array().map_or(false, |a| a.iter().any(|v| v.as_u64() == Some(index as u64)));
    assert!(on(&engine), "{}", shape_layers(&engine, "6"));
    engine.edit(0, 16, "6", index as f64, 0.0, 0.0, 0.0, 0).unwrap();
    assert!(!on(&engine), "{}", shape_layers(&engine, "6"));
    // delete it, then undo everything
    engine.page_op(9, 0, index, "").unwrap();
    assert!(layers(&engine).iter().all(|l| l["name"] != "Notes"));
    while engine.undo_depth() > base {
        assert_eq!(engine.undo(), Ok(true));
    }
    assert_eq!(layers(&engine).len(), before);
}

#[test]
fn hiding_a_layer_hides_the_shapes_on_it_and_showing_it_brings_them_back() {
    let mut engine = Engine::new();
    engine.open(example()).unwrap();
    let model = |engine: &Engine| serde_json::from_str::<serde_json::Value>(&engine.json().unwrap()).unwrap();
    let hidden = |engine: &Engine| model(engine)["pages"][0]["shapes"].as_array().unwrap().iter().filter(|s| s["layerHidden"] == true).count();
    let layer = model(&engine)["pages"][0]["layers"][0]["index"].as_u64().unwrap() as usize;
    // the example's layer has no shapes: hiding it hides nothing
    engine.page_op(8, 0, layer, "|0|1|0").unwrap();
    assert_eq!(hidden(&engine), 0);
    engine.page_op(8, 0, layer, "|1|1|0").unwrap();
    // put the Input Device on it, then hide the layer
    engine.edit(0, 16, "6", layer as f64, 0.0, 0.0, 0.0, 1).unwrap();
    assert_eq!(hidden(&engine), 0);
    engine.page_op(8, 0, layer, "|0|1|0").unwrap();
    assert!(hidden(&engine) >= 1, "the shape on the hidden layer is hidden");
    engine.page_op(8, 0, layer, "|1|1|0").unwrap();
    assert_eq!(hidden(&engine), 0);
}

#[test]
fn copy_paste_and_duplicate_on_the_example_drawing() {
    let mut engine = Engine::new();
    engine.open(example()).unwrap();
    let shapes = |engine: &Engine| {
        let model: serde_json::Value = serde_json::from_str(&engine.json().unwrap()).unwrap();
        model["pages"][0]["shapes"].as_array().unwrap().clone()
    };
    let before = shapes(&engine);
    assert!(engine.paste(0, 0.25, -0.25).is_err(), "nothing copied yet");
    assert_eq!(engine.copy(0, &["6".to_string()]).unwrap(), 1);
    assert_eq!(engine.clipboard_count(), 1);
    let depth = engine.undo_depth();
    let first = engine.paste(0, 0.25, -0.25).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(engine.undo_depth(), depth + 1, "one undo step per paste");
    let second = engine.paste(0, 0.5, -0.5).unwrap();
    assert_ne!(first, second, "every paste gets a new id");
    let after = shapes(&engine);
    assert_eq!(after.len(), before.len() + 2);
    let original = before.iter().find(|s| s["visioId"] == "6").unwrap();
    let copy = after.iter().find(|s| s["visioId"] == first[0].as_str()).expect("the pasted shape");
    assert_eq!(copy["label"], original["label"]);
    assert!((copy["x"].as_f64().unwrap() - original["x"].as_f64().unwrap() - 0.25 * 96.0).abs() < 0.5, "{copy} {original}");
    assert!((copy["y"].as_f64().unwrap() - original["y"].as_f64().unwrap() - 0.25 * 96.0).abs() < 0.5, "{copy} {original}");
    // the original and the connectors did not change
    assert_eq!(after.iter().find(|s| s["visioId"] == "6").unwrap()["x"], original["x"]);
    // a connector cannot be copied; undo removes the pastes one by one
    assert!(engine.copy(0, &[]).is_err());
    assert_eq!(engine.undo(), Ok(true));
    assert_eq!(engine.undo(), Ok(true));
    assert_eq!(shapes(&engine).len(), before.len());
}

fn model_shapes(engine: &Engine) -> Vec<serde_json::Value> {
    let model: serde_json::Value = serde_json::from_str(&engine.json().unwrap()).unwrap();
    model["pages"][0]["shapes"].as_array().unwrap().clone()
}

/// Ids of the example's three boxes in a row: Input Device, Memory Unit, Output Device.
fn box_ids(engine: &Engine) -> Vec<String> {
    ["Input Device", "Memory Unit", "Output Device"]
        .iter()
        .map(|label| {
            model_shapes(engine)
                .iter()
                .find(|s| s["label"] == *label && s["group"].is_null())
                .unwrap_or_else(|| panic!("a shape labelled {label}"))["visioId"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

fn shape_of(engine: &Engine, id: &str) -> serde_json::Value {
    model_shapes(engine).into_iter().find(|s| s["visioId"] == id).unwrap_or_else(|| panic!("shape {id}"))
}

#[test]
fn align_and_distribute_the_example_boxes() {
    let mut engine = Engine::new();
    engine.open(example()).unwrap();
    let ids = box_ids(&engine);
    assert!(ids.len() >= 3, "{ids:?}");
    let (a, b, c) = (&ids[0], &ids[1], &ids[2]);
    let list = format!("{a}\n{b}\n{c}");
    let depth = engine.undo_depth();
    let connectors = engine.connectors_json(0).unwrap();
    // align left to the first: every x equals the first one's
    engine.edit(0, 14, &list, 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    assert_eq!(engine.undo_depth(), depth + 1, "one undo step");
    let x = shape_of(&engine, a)["x"].as_f64().unwrap();
    for id in [b, c] {
        assert!((shape_of(&engine, id)["x"].as_f64().unwrap() - x).abs() < 0.5, "{id} left-aligned");
    }
    assert_ne!(engine.connectors_json(0).unwrap(), connectors, "glued connectors follow the moved shapes");
    // top align (mode 3) to the first
    engine.edit(0, 14, &list, 3.0, 0.0, 0.0, 0.0, 0).unwrap();
    let y = shape_of(&engine, a)["y"].as_f64().unwrap();
    for id in [b, c] {
        assert!((shape_of(&engine, id)["y"].as_f64().unwrap() - y).abs() < 0.5, "{id} top-aligned");
    }
    // distribute horizontally: equal gaps between the three (sorted by x) after spreading two of them first
    engine.edit(0, 5, &format!("{b}\n{c}"), 200.0 / 96.0, 0.0, 0.0, 0.0, 0).unwrap();
    engine.edit(0, 5, c, 150.0 / 96.0, 0.0, 0.0, 0.0, 0).unwrap();
    engine.edit(0, 15, &list, 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    let mut boxes: Vec<(f64, f64)> = [a, b, c].iter().map(|id| (shape_of(&engine, id)["x"].as_f64().unwrap(), shape_of(&engine, id)["w"].as_f64().unwrap())).collect();
    boxes.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap());
    let gap1 = boxes[1].0 - (boxes[0].0 + boxes[0].1);
    let gap2 = boxes[2].0 - (boxes[1].0 + boxes[1].1);
    assert!((gap1 - gap2).abs() < 0.5, "equal gaps: {gap1} {gap2}");
}

#[test]
fn group_ungroup_order_rotate_and_flip() {
    let mut engine = Engine::new();
    engine.open(example()).unwrap();
    let ids = box_ids(&engine);
    let (a, b) = (&ids[0], &ids[1]);
    // z-order: to front gives the shape the highest z, to back the lowest
    let z_of = |engine: &Engine, id: &str| shape_of(engine, id)["z"].as_f64().unwrap();
    engine.group_op(0, 2, a).unwrap();
    let top = model_shapes(&engine).iter().filter_map(|s| s["z"].as_f64()).fold(f64::MIN, f64::max);
    assert!((z_of(&engine, a) - top).abs() < 1e-9, "to front");
    engine.group_op(0, 3, a).unwrap();
    let bottom = model_shapes(&engine).iter().filter_map(|s| s["z"].as_f64()).fold(f64::MAX, f64::min);
    assert!((z_of(&engine, a) - bottom).abs() < 1e-9, "to back");
    // rotate by a quarter turn and flip
    engine.edit(0, 11, b, std::f64::consts::FRAC_PI_2, 0.0, 0.0, 0.0, 0).unwrap();
    let rot = shape_of(&engine, b)["rot"].as_f64().unwrap().abs();
    // the model reports the angle in degrees
    assert!((rot - 90.0).abs() < 1e-3 || (rot - 270.0).abs() < 1e-3, "rot {rot}");
    engine.edit(0, 12, a, 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    assert_eq!(shape_of(&engine, a)["flipX"], true);
    engine.edit(0, 12, a, 0.0, 0.0, 0.0, 0.0, 1).unwrap();
    assert_eq!(shape_of(&engine, a)["flipY"], true);
    // group the two, then ungroup: the freed ids are the two shapes
    let before = model_shapes(&engine).len();
    let group = engine.group_op(0, 0, &format!("{a}\n{b}")).unwrap();
    assert!(!group.is_empty());
    assert!(shape_of(&engine, a)["group"].is_string() || !shape_of(&engine, a)["group"].is_null(), "members belong to the group");
    let freed = engine.group_op(0, 1, &group).unwrap();
    assert!(freed.contains(a.as_str()) && freed.contains(b.as_str()), "{freed}");
    assert_eq!(model_shapes(&engine).len(), before, "the shape count is the same after group + ungroup");
}

#[test]
fn resize_op_keeps_the_anchor_corner_but_not_the_text_block() {
    let mut engine = Engine::new();
    engine.open(example()).unwrap();
    let id = "6";
    let before = shape_of(&engine, id);
    let (x, y, w, h) = (before["x"].as_f64().unwrap(), before["y"].as_f64().unwrap(), before["w"].as_f64().unwrap(), before["h"].as_f64().unwrap());
    let depth = engine.undo_depth();
    let connectors = engine.connectors_json(0).unwrap();
    // grow to 2.5 x 1.25 inch keeping the bottom-left corner (anchor 0,0 in the shape's own y-up fractions)
    engine.edit(0, 3, id, 2.5, 1.25, 0.0, 0.0, 0).unwrap();
    assert_eq!(engine.undo_depth(), depth + 1, "one undo step");
    let a = shape_of(&engine, id);
    let (ax, ay, aw, ah) = (a["x"].as_f64().unwrap(), a["y"].as_f64().unwrap(), a["w"].as_f64().unwrap(), a["h"].as_f64().unwrap());
    assert!((aw - 240.0).abs() < 0.5 && (ah - 120.0).abs() < 0.5, "size {aw} x {ah}");
    assert!((ax - x).abs() < 0.5, "left edge fixed: {ax} vs {x}");
    assert!(((ay + ah) - (y + h)).abs() < 0.5, "bottom edge fixed (y is down): {} vs {}", ay + ah, y + h);
    // Known difference: this engine op leaves the text block at its old size; the native app's own
    // resize scales it with the shape, so the app keeps that code path for resizing.
    assert_eq!(a["textBox"]["w"], before["textBox"]["w"], "the engine op does not scale the text block");
    assert_ne!(engine.connectors_json(0).unwrap(), connectors, "glued connectors follow the new size");
    // top-right anchor (1,1): the right and top edges stay
    engine.edit(0, 3, id, 1.0, 0.5, 1.0, 1.0, 0).unwrap();
    let b = shape_of(&engine, id);
    assert!(((b["x"].as_f64().unwrap() + b["w"].as_f64().unwrap()) - (ax + aw)).abs() < 0.5, "right edge fixed");
    assert!((b["y"].as_f64().unwrap() - ay).abs() < 0.5, "top edge fixed");
}

#[test]
fn deleting_a_connector_leaves_the_shapes_and_undo_brings_it_back() {
    let mut engine = Engine::new();
    engine.open(example()).unwrap();
    let list = |engine: &Engine| serde_json::from_str::<serde_json::Value>(&engine.connectors_json(0).unwrap()).unwrap().as_array().unwrap().clone();
    let connectors = list(&engine);
    let id = connectors[0]["id"].as_str().unwrap().to_string();
    // every route has at least two points in page inches (what the app hit-tests)
    assert!(connectors.iter().all(|c| c["route"].as_array().is_some_and(|r| r.len() >= 2 && r.iter().all(|p| p[0].is_f64() && p[1].is_f64()))));
    let shapes_before = model_shapes(&engine).iter().filter(|s| s["w"].as_f64().unwrap_or(0.0) > 100.0).count();
    let depth = engine.undo_depth();
    engine.edit(0, 4, &id, 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    assert_eq!(engine.undo_depth(), depth + 1);
    assert_eq!(list(&engine).len(), connectors.len() - 1);
    assert!(list(&engine).iter().all(|c| c["id"] != id.as_str()));
    assert_eq!(model_shapes(&engine).iter().filter(|s| s["w"].as_f64().unwrap_or(0.0) > 100.0).count(), shapes_before, "the boxes stay");
    // every other connector keeps its route exactly
    let others = |engine: &Engine| list(engine).into_iter().map(|c| (c["id"].to_string(), c["route"].to_string())).collect::<Vec<_>>();
    let mut expected: Vec<(String, String)> = connectors.iter().map(|c| (c["id"].to_string(), c["route"].to_string())).filter(|(cid, _)| *cid != format!("\"{id}\"")).collect();
    let mut got = others(&engine);
    expected.sort();
    got.sort();
    assert_eq!(got, expected, "deleting a connector reroutes nothing else");
    assert_eq!(engine.undo(), Ok(true));
    assert_eq!(list(&engine).len(), connectors.len());
}

#[test]
fn style_op_on_several_shapes_and_cut_as_copy_then_delete() {
    let mut engine = Engine::new();
    engine.open(example()).unwrap();
    let ids = box_ids(&engine);
    let (a, b) = (&ids[0], &ids[1]);
    // the ribbon's B / I / U / size / colour buttons: ONE op 10 on every selected shape
    let depth = engine.undo_depth();
    engine.edit(0, 10, &format!("{a}\n{b}\n{}", r##"{"bold":true,"italic":true,"underline":true,"fontPt":20,"fill":"#ff0000"}"##), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    assert_eq!(engine.undo_depth(), depth + 1, "one undo step for both shapes");
    for id in [a, b] {
        let shape = shape_of(&engine, id);
        assert_eq!(shape["bold"], true, "{id}");
        assert_eq!(shape["italic"], true, "{id}");
        assert_eq!(shape["underline"], true, "{id}");
        assert!((shape["fontSize"].as_f64().unwrap() - 20.0 * 96.0 / 72.0).abs() < 0.01, "{id}: {}", shape["fontSize"]);
        assert!(shape["parts"].as_array().unwrap().iter().any(|p| p["fill"] == "#ff0000"), "{id} fill");
    }
    // cut = copy + delete: one undo step, and the clipboard pastes the shape back
    let before = model_shapes(&engine).len();
    let depth = engine.undo_depth();
    assert_eq!(engine.copy(0, std::slice::from_ref(a)).unwrap(), 1);
    engine.edit(0, 4, a, 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    assert_eq!(engine.undo_depth(), depth + 1, "cut is one undo step");
    assert_eq!(model_shapes(&engine).len(), before - 1);
    let pasted = engine.paste(0, 0.0, 0.0).unwrap();
    assert_eq!(pasted.len(), 1);
    assert_eq!(model_shapes(&engine).len(), before, "pasting the cut shape brings it back");
    // as the web ribbon: rotate right = -PI/2 (the model reports 90 degrees clockwise), rotate left = +PI/2 (270)
    engine.edit(0, 11, b, -std::f64::consts::FRAC_PI_2, 0.0, 0.0, 0.0, 0).unwrap();
    assert!((shape_of(&engine, b)["rot"].as_f64().unwrap().rem_euclid(360.0) - 90.0).abs() < 1e-3);
    engine.edit(0, 11, b, std::f64::consts::PI, 0.0, 0.0, 0.0, 0).unwrap();
    assert!((shape_of(&engine, b)["rot"].as_f64().unwrap().rem_euclid(360.0) - 270.0).abs() < 1e-3, "left turn of the shape that was turned right: 90 -> 270 after +180");
}

#[test]
fn page_setup_size_and_comments_on_the_example_drawing() {
    let mut engine = Engine::new();
    engine.open(example()).unwrap();
    let size = |engine: &Engine| serde_json::from_str::<serde_json::Value>(&engine.page_size_json(0).unwrap()).unwrap();
    let depth = engine.undo_depth();
    assert_eq!(engine.page_op(6, 0, 0, "12,9").unwrap(), "0");
    assert_eq!(engine.undo_depth(), depth + 1, "one undo step");
    let after = size(&engine);
    assert!((after["w"].as_f64().unwrap() - 12.0).abs() < 1e-9 && (after["h"].as_f64().unwrap() - 9.0).abs() < 1e-9, "{after}");
    assert!(engine.page_op(6, 0, 0, "wide,9").is_err());
    assert_eq!(engine.undo(), Ok(true));
    assert!((size(&engine)["w"].as_f64().unwrap() - 11.0).abs() < 1e-9, "undo restores the width");
    // a comment on a shape: listed with its author, shape and text
    let comments = |engine: &Engine| serde_json::from_str::<serde_json::Value>(&engine.comments_json().unwrap()).unwrap().as_array().unwrap().clone();
    let before = comments(&engine).len();
    engine.page_op(20, 0, 0, "6|Me|ME|2026-11-20T10:00:00|Check the label").unwrap();
    let list = comments(&engine);
    assert_eq!(list.len(), before + 1);
    let comment = list.iter().find(|c| c["text"] == "Check the label").expect("the new comment");
    assert_eq!(comment["author"], "Me");
    assert_eq!(comment["shape"], "6");
    assert_eq!(comment["page"], 0);
    // a comment on the page itself has no shape
    engine.page_op(20, 0, 0, "|Me|ME|2026-11-20T10:01:00|About the page").unwrap();
    assert!(comments(&engine).iter().any(|c| c["text"] == "About the page" && c["shape"].is_null()));
    assert!(engine.page_op(20, 0, 0, "too|few|fields").is_err());
}
