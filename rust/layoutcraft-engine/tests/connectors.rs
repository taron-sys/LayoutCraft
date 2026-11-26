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
