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

/// The frame holding the first two shapes (the second one also stated from its own end), the fourth shape outside.
fn framed() -> (Engine, Vec<String>) {
    drawing(|ids| {
        vec![
            (0, format!("SUM(DEPENDSON(1,{},{}))", sheet(&ids[1]), sheet(&ids[2]))),
            (1, format!("SUM(DEPENDSON(4,{}))", sheet(&ids[0]))),
            (2, format!("SUM(DEPENDSON(4,{}))", sheet(&ids[0]))),
        ]
    })
}

fn present(engine: &Engine) -> Vec<String> {
    page(engine).shapes.iter().filter_map(|shape| shape.visio_id.clone()).collect()
}

fn xml(engine: &Engine) -> String {
    let package = engine.package.as_ref().unwrap();
    package.text(&package.pages().unwrap()[0].part).unwrap().to_string()
}

#[test]
fn deleting_a_member_takes_its_reference_out_of_the_container() {
    let (mut engine, ids) = framed();
    engine.edit(0, 4, &ids[1], 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    assert!(!xml(&engine).contains(&sheet(&ids[1])), "no cell names the deleted shape");
    assert_eq!(page(&engine).containers[0]["members"], serde_json::json!([ids[2]]));
    // the last member: the term stays, empty, and the container has no record
    engine.edit(0, 4, &ids[2], 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    assert!(page(&engine).containers.is_empty() && xml(&engine).contains("DEPENDSON(1)"));
    assert!(present(&engine).contains(&ids[0]), "the container stays");
}

#[test]
fn deleting_a_container_deletes_what_it_holds_in_one_step() {
    let (mut engine, ids) = framed();
    let before = (present(&engine), engine.undo_depth());
    engine.edit(0, 4, &ids[0], 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    assert_eq!(present(&engine), [ids[3].clone()]);
    assert_eq!(engine.undo_depth(), before.1 + 1);
    engine.undo().unwrap();
    assert_eq!(present(&engine), before.0);
    assert_eq!(page(&engine).containers.len(), 1);
}

#[test]
fn a_pasted_container_brings_its_members_and_holds_the_copies() {
    let (mut engine, ids) = framed();
    assert_eq!(engine.copy(0, &[ids[0].clone()]).unwrap(), 3);
    let made = engine.paste(0, 0.5, -0.5).unwrap();
    assert_eq!(made.len(), 3);
    let page = page(&engine);
    assert_eq!(page.containers.len(), 2, "{:?}", page.containers);
    let copy = page.containers.iter().find(|record| record["visioId"].as_str() == Some(made[0].as_str())).expect("the copy is a container");
    assert_eq!(copy["members"], serde_json::json!(made[1..]));
    let original = page.containers.iter().find(|record| record["visioId"].as_str() == Some(ids[0].as_str())).unwrap();
    assert_eq!(original["members"], serde_json::json!([ids[1], ids[2]]), "the originals are unchanged");
}

#[test]
fn a_member_copied_alone_is_not_in_the_container() {
    let (mut engine, ids) = framed();
    engine.copy(0, &[ids[1].clone()]).unwrap();
    let made = engine.paste(0, 0.5, -0.5).unwrap();
    assert!(of(page(&engine), &made[0]).containers.is_empty(), "{:?}", page(&engine).containers);
    assert_eq!(page(&engine).containers[0]["members"], serde_json::json!([ids[1], ids[2]]));
}

#[test]
fn nested_containers_go_with_the_outer_one() {
    let (engine, ids) = drawing(|ids| vec![(0, format!("SUM(DEPENDSON(1,{}))", sheet(&ids[1]))), (1, format!("SUM(DEPENDSON(1,{}))", sheet(&ids[2])))]);
    assert_eq!(engine.with_contents(0, &[ids[0].clone()]), ids[..3]);
    assert_eq!(engine.with_contents(0, &[ids[3].clone(), ids[3].clone()]), [ids[3].clone()]);
}
