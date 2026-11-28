use layoutcraft_engine::Engine;

#[test]
fn a_file_of_another_kind_is_refused_by_name() {
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    let whole = engine.export().unwrap().to_vec();
    let mut refused = |bytes: &[u8]| engine.open(bytes.to_vec()).unwrap_err();
    for (bytes, kind) in [(&b"%PDF-1.7 and so on"[..], "PDF"), (b"\x89PNG\r\n\x1a\n", "PNG"), (b"<?xml version=\"1.0\"?><svg/>", "SVG"), (b"  {\"a\": 1}", "JSON"), (b"", "empty")] {
        let message = refused(bytes);
        assert!(message.contains(kind) && message.contains(".vsdx") && !message.contains("Zip"), "{message}");
    }
    // cut short, a drawing is called damaged
    assert!(refused(&whole[..whole.len() / 2]).contains("damaged"));
    assert!(engine.page_count().is_some());
}

#[test]
fn one_shape_that_cannot_be_read_does_not_reject_the_drawing() {
    use layoutcraft_vsdx::Package;
    let mut engine = Engine::new();
    engine.new_blank(11.0, 8.5, false, "Page-1").unwrap();
    let good = engine.draw(0, 0, 1.0, 1.0, 3.0, 2.0).unwrap();
    let bad = engine.draw(0, 0, 5.0, 1.0, 7.0, 2.0).unwrap();
    assert!(engine.open_notice().is_none());
    // the second shape now names a master the file does not have
    let package = Package::from_bytes(engine.export().unwrap()).unwrap();
    let part = package.pages().unwrap()[0].part.clone();
    let xml = package.text(&part).unwrap();
    let broken = xml.replacen(&format!("<Shape ID='{bad}'"), &format!("<Shape ID='{bad}' Master='999'"), 1);
    assert_ne!(broken, xml);
    let bytes = layoutcraft_vsdx::writer::write_package_edit(&package, [(part.clone(), broken.into_bytes())].into(), &[]).unwrap();

    engine.open(bytes).unwrap();
    let ids = |engine: &Engine| engine.document().unwrap().pages[0].shapes.iter().filter_map(|shape| shape.visio_id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&engine), std::slice::from_ref(&good));
    let notice = engine.open_notice().expect("the user is told");
    assert!(notice.starts_with("1 shape ") && notice.contains("Page-1") && notice.contains("999"), "{notice}");
    // editing works, and what could not be read is still saved
    engine.edit(0, 2, &good, 1.0, 0.0, 0.0, 0.0, 0).unwrap();
    let saved = Package::from_bytes(engine.export().unwrap()).unwrap();
    assert!(saved.text(&part).unwrap().contains("Master='999'"));
    assert!(engine.open_notice().is_some());
}
