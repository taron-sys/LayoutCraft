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
