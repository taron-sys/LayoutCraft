//! Drawing connectors: loose ends, reversing them, and adding a connected neighbour. Runs without the private corpus.
use layoutcraft_engine::Engine;

#[test]
fn connectors_can_be_drawn_with_loose_ends_and_reversed() {
    use layoutcraft_engine::EndTarget;
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    let a = engine.draw(0, 0, 1.0, 1.0, 2.0, 2.0).unwrap();
    let read = |engine: &Engine, id: &str| -> serde_json::Value {
        let all: serde_json::Value = serde_json::from_str(&engine.connectors_json(0).unwrap()).unwrap();
        all.as_array().unwrap().iter().find(|c| c["id"] == id).unwrap().clone()
    };
    let near = |value: &serde_json::Value, at: f64| (value.as_f64().unwrap() - at).abs() < 1e-6;
    // both ends loose
    let loose = engine.add_connector_between(0, &EndTarget::Free(3.0, 3.0), &EndTarget::Free(5.0, 4.0)).unwrap();
    let c = read(&engine, &loose);
    assert!(c["begin"]["glue"].is_null() && c["end"]["glue"].is_null());
    assert!(near(&c["begin"]["x"], 3.0) && near(&c["end"]["y"], 4.0));
    // from the shape to nowhere: the begin is glued, the end stays where it was put
    let half = engine.add_connector_between(0, &EndTarget::Glue(a.clone(), None), &EndTarget::Free(6.0, 1.5)).unwrap();
    let c = read(&engine, &half);
    assert_eq!(c["begin"]["glue"]["sheet"], a.as_str());
    assert!(c["end"]["glue"].is_null() && near(&c["end"]["x"], 6.0));
    // reversed: the glue is at the end now, in one undo step
    let depth = engine.undo_depth();
    engine.reverse_ends(0, &half).unwrap();
    let c = read(&engine, &half);
    assert_eq!(c["end"]["glue"]["sheet"], a.as_str());
    assert!(c["begin"]["glue"].is_null() && near(&c["begin"]["x"], 6.0));
    assert_eq!(engine.undo_depth(), depth + 1);
    // too short, and an unknown shape, change nothing
    assert!(engine.add_connector_between(0, &EndTarget::Free(1.0, 1.0), &EndTarget::Free(1.0, 1.0)).is_err());
    assert!(engine.add_connector_between(0, &EndTarget::Glue("999".into(), None), &EndTarget::Free(1.0, 1.0)).is_err());
    assert_eq!(engine.undo_depth(), depth + 1);
}

#[test]
fn a_neighbour_is_placed_beside_a_shape_and_connected_in_one_step() {
    use layoutcraft_engine::Direction;
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    let a = engine.draw(0, 0, 4.0, 4.0, 5.0, 5.0).unwrap();
    engine.copy(0, std::slice::from_ref(&a)).unwrap();
    let clipboard = engine.clipboard.clone();
    for direction in [Direction::Right, Direction::Left, Direction::Up, Direction::Down] {
        let depth = engine.undo_depth();
        let made = engine.connect_new_neighbour(0, &a, direction, None).unwrap();
        assert_eq!(engine.undo_depth(), depth + 1);
        let shapes = &engine.document().unwrap().pages[0].shapes;
        let find = |id: &str| shapes.iter().find(|s| s.visio_id.as_deref() == Some(id)).unwrap();
        let (from, new) = (find(&a), find(&made));
        // clear of the shape, on the asked side (model y grows downward)
        let (dx, dy) = direction.step();
        let gap = if dy == 0.0 { (new.x - from.x) * dx - from.w } else { (from.y - new.y) * dy - from.h };
        assert!(gap > 0.0, "{direction:?} {gap}");
        let connectors: serde_json::Value = serde_json::from_str(&engine.connectors_json(0).unwrap()).unwrap();
        assert!(connectors.as_array().unwrap().iter().any(|c| c["begin"]["glue"]["sheet"] == a.as_str() && c["end"]["glue"]["sheet"] == made.as_str()));
        engine.undo().unwrap();
    }
    assert_eq!(engine.clipboard, clipboard);
    let depth = engine.undo_depth();
    assert!(engine.connect_new_neighbour(0, "999", Direction::Right, None).is_err());
    assert_eq!(engine.undo_depth(), depth);
}

#[test]
fn a_connector_between_inner_points_leaves_towards_the_other_shape() {
    // as the ports of switch modules one above the other: points without a direction, inside the frame (or off a thin one)
    use layoutcraft_engine::EndTarget;
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    let lower = engine.draw(0, 0, 3.0, 1.0, 7.0, 2.0).unwrap();
    let upper = engine.draw(0, 0, 3.5, 4.0, 7.5, 5.0).unwrap();
    let inner = |engine: &mut Engine, shape: &str, fx: f64| -> usize {
        engine.page_op(24, 0, 0, &format!("{shape}|{fx}|0.5")).unwrap();
        let points: serde_json::Value = serde_json::from_str(&engine.connection_points_json(0, shape).unwrap()).unwrap();
        points.as_array().unwrap().iter().map(|p| p["n"].as_u64().unwrap() as usize).max().unwrap()
    };
    let (from, to) = (inner(&mut engine, &lower, 0.3), inner(&mut engine, &upper, 0.6));
    let id = engine.add_connector_between(0, &EndTarget::Glue(lower.clone(), Some(from)), &EndTarget::Glue(upper.clone(), Some(to))).unwrap();
    let all: serde_json::Value = serde_json::from_str(&engine.connectors_json(0).unwrap()).unwrap();
    let c = all.as_array().unwrap().iter().find(|c| c["id"] == id.as_str()).unwrap().clone();
    let at = |end: &serde_json::Value| (end["x"].as_f64().unwrap(), end["y"].as_f64().unwrap());
    let (begin, end) = (at(&c["begin"]), at(&c["end"]));
    let mut path = vec![begin];
    path.extend(c["route"].as_array().unwrap().iter().map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())));
    path.push(end);
    path.dedup_by(|a, b| (a.0 - b.0).hypot(a.1 - b.1) < 1e-9);
    // out of the top of the lower shape, into the bottom of the upper one, and never round the side of either
    let (first, last) = ((path[0], path[1]), (path[path.len() - 2], path[path.len() - 1]));
    assert!(first.1 .1 > first.0 .1 && (first.1 .0 - first.0 .0).abs() < 1e-9, "leaves upward: {path:?}");
    assert!(last.1 .1 > last.0 .1 && (last.1 .0 - last.0 .0).abs() < 1e-9, "arrives from below: {path:?}");
    let (left, right) = (begin.0.min(end.0), begin.0.max(end.0));
    assert!(path.iter().all(|p| p.0 >= left - 1e-9 && p.0 <= right + 1e-9), "stays between the two points: {path:?}");
}
