//! A shape without any Geometry section paints no box (measured in Visio, docs/04). Drawings made here, by our writer; the
//! geometry is taken out of the page part.
use layoutcraft_engine::Engine;
use layoutcraft_model::Shape;
use layoutcraft_vsdx::writer::{patch_page_cells, ShapeCellPatch};

/// A page with one red rectangle that says "words"; with `outline` false its Geometry section is removed.
fn red(outline: bool) -> Engine {
    let mut engine = Engine::new();
    engine.new_blank(4.0, 3.0, false, "Page-1").unwrap();
    let id = engine.draw(0, 0, 0.5, 0.5, 2.5, 2.0).unwrap();
    let patch = ShapeCellPatch::new(&id).set("FillForegnd", "#ff0000").set("FillPattern", "1").set("LineColor", "#ff0000").set("LinePattern", "1").edit_text("words");
    let bytes = patch_page_cells(engine.package.as_ref().unwrap(), 0, &[patch]).unwrap();
    let package = layoutcraft_vsdx::Package::from_bytes(&bytes).unwrap();
    let part = package.pages().unwrap()[0].part.clone();
    let mut xml = package.text(&part).unwrap().to_string();
    if !outline {
        let start = xml.find("<Section N='Geometry'").expect("the drawn shape has a Geometry section");
        let end = start + xml[start..].find("</Section>").unwrap() + "</Section>".len();
        xml.replace_range(start..end, "");
    }
    let mut parts = std::collections::BTreeMap::new();
    parts.insert(part, xml.into_bytes());
    let mut read = Engine::new();
    read.open(layoutcraft_vsdx::writer::write_package_edit(&package, parts, &[]).unwrap()).unwrap();
    read
}

fn only(engine: &Engine) -> &Shape {
    &engine.document().unwrap().pages[0].shapes[0]
}

/// How many pixels of the page are the shape's red.
fn red_pixels(engine: &Engine) -> usize {
    let scene = layoutcraft_render::scene_from_document(engine.document().unwrap(), 0).unwrap();
    let (_, _, rgba) = layoutcraft_render::render_rgba(&scene, 1.0).unwrap();
    rgba.chunks(4).filter(|p| p[0] > 200 && p[1] < 60 && p[2] < 60).count()
}

#[test]
fn without_a_geometry_section_nothing_is_painted_but_the_text_stays() {
    let with = red(true);
    assert!(red_pixels(&with) > 1000);
    let without = red(false);
    assert_eq!(red_pixels(&without), 0, "{:?} {:?}", only(&without).fill, only(&without).stroke);
    assert_eq!(only(&without).label, "words");
    assert_eq!((only(&without).w, only(&without).h), (only(&with).w, only(&with).h), "the box keeps its place for selection");
}
