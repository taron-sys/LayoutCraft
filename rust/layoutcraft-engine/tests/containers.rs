//! Containers and lists read from a drawing. Drawings made here, by our writer; the membership cells are written as the
//! format notes in `docs/04-visio-import.md` give them.
use layoutcraft_engine::Engine;
use layoutcraft_model::Page;
use layoutcraft_vsdx::writer::{patch_page_cells, ShapeCellPatch};

/// A drawing of a frame, two shapes in it and one outside, with the given `Relationships` formulas.
fn drawing(formulas: impl Fn(&[String]) -> Vec<(usize, String)>) -> (Engine, Vec<String>) {
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    let ids: Vec<String> = [(1.0, 1.0, 7.0, 6.0), (2.0, 2.0, 3.0, 3.0), (4.0, 2.0, 5.0, 3.0), (8.0, 2.0, 9.0, 3.0)]
        .iter()
        .map(|(x0, y0, x1, y1)| engine.draw(0, 0, *x0, *y0, *x1, *y1).unwrap())
        .collect();
    let patches: Vec<ShapeCellPatch> = formulas(&ids).into_iter().map(|(shape, formula)| ShapeCellPatch::new(&ids[shape]).set_with_formula("Relationships", "0", formula)).collect();
    let bytes = patch_page_cells(engine.package.as_ref().unwrap(), 0, &patches).unwrap();
    let mut read = Engine::new();
    read.open(bytes).unwrap();
    (read, ids)
}

fn page(engine: &Engine) -> &Page {
    &engine.document().unwrap().pages[0]
}

fn of<'a>(page: &'a Page, id: &str) -> &'a layoutcraft_model::Shape {
    page.shapes.iter().find(|shape| shape.visio_id.as_deref() == Some(id)).unwrap()
}

fn sheet(id: &str) -> String {
    format!("Sheet.{id}!SheetRef()")
}

#[test]
fn membership_is_read_from_either_end_and_only_for_shapes_that_exist() {
    // the container names the first member (and a shape that is gone); only the second member names the container
    let (engine, ids) = drawing(|ids| vec![(0, format!("SUM(DEPENDSON(1,{},{}))", sheet(&ids[1]), sheet("999"))), (2, format!("SUM(DEPENDSON(4,{}))", sheet(&ids[0])))]);
    let page = page(&engine);
    assert_eq!(page.containers.len(), 1, "{:?}", page.containers);
    let record = &page.containers[0];
    assert_eq!((record["visioId"].as_str(), record["kind"].as_str()), (Some(ids[0].as_str()), Some("container")));
    assert_eq!(record["members"], serde_json::json!([ids[1], ids[2]]));
    for (shape, member) in [(0, false), (1, true), (2, true), (3, false)] {
        assert_eq!(of(page, &ids[shape]).containers.contains(&ids[0]), member, "shape {shape}");
        assert!(of(page, &ids[shape]).list_of.is_empty());
    }
}

#[test]
fn a_list_keeps_its_items_apart_from_members_and_a_shape_can_belong_to_both() {
    let (engine, ids) = drawing(|ids| {
        vec![(0, format!("SUM(DEPENDSON(1,{}))", sheet(&ids[1]))), (3, format!("SUM(DEPENDSON(2,{}))", sheet(&ids[1])))]
    });
    let page = page(&engine);
    let kinds: Vec<&str> = page.containers.iter().filter_map(|record| record["kind"].as_str()).collect();
    assert!(kinds.contains(&"container") && kinds.contains(&"list"), "{kinds:?}");
    let shape = of(page, &ids[1]);
    assert_eq!((&shape.containers, &shape.list_of), (&vec![ids[0].clone()], &vec![ids[3].clone()]));
}

#[test]
fn a_drawing_without_membership_has_no_containers() {
    let (engine, _) = drawing(|_| Vec::new());
    assert!(page(&engine).containers.is_empty() && page(&engine).shapes.iter().all(|shape| shape.containers.is_empty()));
}
