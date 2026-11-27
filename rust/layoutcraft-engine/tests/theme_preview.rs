//! Showing a theme before it is chosen, and the pictures of a theme gallery. Drawings made here, by our writer.
use layoutcraft_engine::Engine;
use serde_json::Value;

fn drawing() -> Engine {
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    engine.draw(0, 0, 1.0, 5.0, 3.0, 6.0).unwrap();
    engine.page_op(1, 0, 0, "Second").unwrap();
    engine.draw(1, 0, 1.0, 5.0, 3.0, 6.0).unwrap();
    engine
}

fn names(engine: &Engine) -> Vec<String> {
    let themes: Value = serde_json::from_str(&engine.themes_json().unwrap()).unwrap();
    themes["themes"].as_array().unwrap().iter().map(|theme| theme["name"].as_str().unwrap().to_string()).collect()
}

fn fills(document: &layoutcraft_model::Document) -> Vec<Vec<Option<String>>> {
    document.pages.iter().map(|page| page.shapes.iter().map(|shape| shape.fill.clone()).collect()).collect()
}

#[test]
fn a_preview_shows_what_choosing_would_do_and_changes_nothing() {
    let engine = drawing();
    let before = (engine.export().unwrap().to_vec(), engine.undo_depth(), fills(engine.document().unwrap()));
    // a theme that colours its shapes
    let name = names(&engine)
        .into_iter()
        .find(|name| fills(&engine.preview_theme(30, 0, name).unwrap()) != before.2)
        .expect("a theme that changes the fill of a drawn shape");
    for (op, page) in [(30, 0), (32, 1)] {
        let mut chosen = drawing();
        chosen.page_op(op, page, 0, &name).unwrap();
        assert_eq!(fills(&engine.preview_theme(op, page, &name).unwrap()), fills(chosen.document().unwrap()), "operation {op}");
    }
    // a variant belongs to a theme: the drawing needs one first
    assert!(engine.preview_theme(31, 0, "2").is_err());
    for (op, page, value) in [(31, 0, "2"), (33, 1, "1")] {
        let mut chosen = drawing();
        chosen.page_op(30, 0, 0, &name).unwrap();
        let shown = fills(&chosen.preview_theme(op, page, value).unwrap());
        chosen.page_op(op, page, 0, value).unwrap();
        assert_eq!(shown, fills(chosen.document().unwrap()), "operation {op}");
    }
    // one page only: the other keeps its colours
    let one_page = fills(&engine.preview_theme(32, 1, &name).unwrap());
    assert_eq!(one_page[0], before.2[0]);
    assert_ne!(one_page[1], before.2[1]);
    assert_eq!((engine.export().unwrap().to_vec(), engine.undo_depth(), fills(engine.document().unwrap())), before);
}

#[test]
fn a_preview_refuses_what_choosing_refuses() {
    let engine = drawing();
    assert!(engine.preview_theme(30, 0, "no such theme").is_err());
    assert!(engine.preview_theme(31, 0, "nine").is_err());
    assert!(engine.preview_theme(1, 0, "").is_err(), "not a theme operation");
    assert!(Engine::new().preview_theme(31, 0, "1").is_err(), "nothing open");
}

#[test]
fn every_theme_has_a_sample_and_the_samples_tell_themes_and_variants_apart() {
    let engine = drawing();
    let mut first: Vec<_> = Vec::new();
    for name in names(&engine) {
        let sample = Engine::theme_sample(&name, 0).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!((sample.pages.len(), sample.pages[0].shapes.len(), sample.pages[0].connectors.len()), (1, 3, 1), "{name}");
        first.push(fills(&sample));
    }
    let mut distinct = first.clone();
    distinct.sort();
    distinct.dedup();
    assert!(distinct.len() > first.len() / 2, "{distinct:?}");
    let name = &names(&engine)[0];
    let variants: Vec<_> = (0..4).map(|variant| fills(&Engine::theme_sample(name, variant).unwrap())).collect();
    assert!(variants.iter().any(|variant| *variant != variants[0]), "{name}: {variants:?}");
}
