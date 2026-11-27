//! Pictures on a page and on its background page, whose shape IDs run alike. Drawings made here, by our writer.
use layoutcraft_engine::Engine;

/// A PNG of one pixel.
const PIXEL: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 0, 5, 0, 1,
    255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

/// Page 0 with a background page beneath it. The first shape of each page (the same ID on both) is a picture or a rectangle.
fn drawing(picture_in_front: bool) -> Engine {
    let mut engine = Engine::new();
    engine.new_blank(8.0, 6.0, false, "Page-1").unwrap();
    engine.page_op(1, 0, 0, "Back").unwrap();
    for (page, picture) in [(0, picture_in_front), (1, !picture_in_front)] {
        if picture {
            engine.insert_picture(page, PIXEL, 1.0, 1.0, 2.0, 2.0).unwrap();
        } else {
            engine.draw(page, 0, 0.0, 0.0, 8.0, 6.0).unwrap();
        }
    }
    let back = engine.package.as_ref().unwrap().pages().unwrap()[1].id.clone().expect("a page ID");
    engine.page_op(13, 1, 0, "").unwrap();
    engine.page_op(12, 0, 0, &back).unwrap();
    engine
}

#[test]
fn a_picture_belongs_to_the_shape_of_its_own_page() {
    for picture_in_front in [true, false] {
        let engine = drawing(picture_in_front);
        let shown = &engine.document().unwrap().pages[0].shapes;
        assert_eq!(shown.len(), 2, "the page and what lies beneath it");
        assert_eq!(shown[0].visio_id, shown[1].visio_id, "the case: one ID on both pages");
        let with: Vec<bool> = shown.iter().map(|shape| shape.image.is_some()).collect();
        let pictures: Vec<usize> = (0..2).filter(|&i| with[i]).collect();
        assert_eq!(pictures.len(), 1, "front {picture_in_front}: {with:?}");
        let picture = &shown[pictures[0]];
        assert!(picture.w < 200.0, "front {picture_in_front}: the picture keeps its own box, {} wide", picture.w);
    }
}
