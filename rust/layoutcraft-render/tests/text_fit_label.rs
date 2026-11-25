use layoutcraft_model::Document;
use layoutcraft_render::{scene_from_document, to_svg};

/// A tiny icon (5 px wide) whose label block is 30 px wide, like the HPE Aruba SFP modules: the group's text block is
/// `TxtWidth = TEXTWIDTH(TheText)`, Visio measured "Fibre SFP" in Kalinga (0.319 in at 8 pt) and drew one line under the icon.
fn document(text_fit: &str) -> Document {
    let json = format!(
        r#"{{"pages":[{{"name":"p","shapes":[{{"type":"rect","x":50,"y":10,"w":5,"h":1,"label":"Fibre SFP",
        "fontFamily":"Kalinga","fontSize":10.67,
        "textBox":{{"x":-12.5,"y":6,"w":30,"h":12}}{text_fit}}}]}}]}}"#
    );
    serde_json::from_str(&json).expect("document")
}

fn text_lines(svg: &str) -> Vec<(f64, f64)> {
    svg.split("<text x=\"")
        .skip(1)
        .map(|part| {
            let x: f64 = part.split('"').next().unwrap().parse().unwrap();
            let y: f64 = part.split("y=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
            (x, y)
        })
        .collect()
}

#[test]
fn a_text_width_block_keeps_its_label_on_one_line_centred_on_the_shape() {
    let fitted = to_svg(&scene_from_document(&document(r#","textFitsLine":true"#), 0).unwrap());
    let lines = text_lines(&fitted);
    assert_eq!(lines.len(), 1, "the label is one line: {fitted}");
    assert!((lines[0].0 - 52.5).abs() < 0.6, "centred on the shape centre (52.5): {}", lines[0].0);
}

#[test]
fn a_block_that_is_not_sized_to_its_text_still_wraps() {
    // the control: without the TEXTWIDTH formula the same narrow block wraps, so the test above proves the flag and not the metrics
    let plain = to_svg(&scene_from_document(&document(""), 0).unwrap());
    assert!(text_lines(&plain).len() >= 2, "a 30 px block breaks \"Fibre SFP\": {plain}");
}

/// The spec text of an Aruba power supply: a child block of 10 px with 1.4 px type whose `TxtWidth` is `MAX(TEXTWIDTH(TheText),Width)`.
/// Visio drew its two hard-broken lines as two lines (block 0.1123 x 0.0307 in); wrapping them by our metrics gave four.
fn sub_text_document(fit: &str) -> Document {
    let json = format!(
        r##"{{"pages":[{{"name":"p","shapes":[{{"type":"rect","x":20,"y":10,"w":30,"h":19,"label":"",
        "subTexts":[{{"x":15,"y":1,"w":10.8,"h":3,"text":"110-127V~ 12A 50/60Hz\n200-240V~ 10A 50/60Hz","fontSize":1.4,
        "fontFamily":"Segoe UI Symbol","color":"#333333","bold":true,"align":"right","vAlign":"middle"{fit}}}]}}]}}]}}"##
    );
    serde_json::from_str(&json).expect("document")
}

#[test]
fn a_text_width_child_block_keeps_its_hard_broken_lines_and_no_more() {
    let fitted = to_svg(&scene_from_document(&sub_text_document(r#","fitLine":true"#), 0).unwrap());
    assert_eq!(text_lines(&fitted).len(), 2, "two lines: {fitted}");
    let plain = to_svg(&scene_from_document(&sub_text_document(""), 0).unwrap());
    assert!(text_lines(&plain).len() > 2, "the control wraps the same text in the 10.8 px block: {plain}");
}

/// A subscript label (`Char.Pos` 2), like the HPE Aruba SFP modules: Visio draws it at the font's own subscript size (Kalinga 0.32 of 8 pt) and a
/// little below the normal baseline, so it reads as a small caption, not as 8 pt text over the icon.
fn script_document(text_script: &str) -> Document {
    let json = format!(
        r#"{{"pages":[{{"name":"p","shapes":[{{"type":"rect","x":50,"y":10,"w":5,"h":1,"label":"Fibre SFP",
        "fontFamily":"Kalinga","fontSize":10.67,"textFitsLine":true,"textScript":{text_script},
        "textBox":{{"x":-12.5,"y":6,"w":30,"h":12}}}}]}}]}}"#
    );
    serde_json::from_str(&json).expect("document")
}

fn first_text(svg: &str) -> (f64, f64) {
    let part = svg.split("<text x=\"").nth(1).expect("a text");
    let x: f64 = part.split('"').next().unwrap().parse().unwrap();
    let y: f64 = part.split("y=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
    let size: f64 = part.split("font-size=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
    let _ = x;
    (y, size)
}

#[test]
fn a_subscript_label_is_drawn_smaller_and_lower_than_normal_text() {
    let normal = to_svg(&scene_from_document(&script_document("0"), 0).unwrap());
    let sub = to_svg(&scene_from_document(&script_document("2"), 0).unwrap());
    let (normal_y, normal_size) = first_text(&normal);
    let (sub_y, sub_size) = first_text(&sub);
    assert!(sub_size < normal_size * 0.8, "subscript text is smaller: {sub_size} vs {normal_size}");
    assert!(sub_y > normal_y, "its baseline is lower than the normal baseline: {sub_y} vs {normal_y}");
}

#[test]
fn a_superscript_label_is_raised_but_stays_below_the_top_of_its_block() {
    let normal = to_svg(&scene_from_document(&script_document("0"), 0).unwrap());
    let sup = to_svg(&scene_from_document(&script_document("1"), 0).unwrap());
    let (normal_y, normal_size) = first_text(&normal);
    let (sup_y, sup_size) = first_text(&sup);
    assert!(sup_size < normal_size * 0.8);
    assert!(sup_y < normal_y, "raised: {sup_y} vs {normal_y}");
    assert!(sup_y > 6.0 + sup_size * 0.5, "its glyphs stay inside the block that starts at y 6");
}

/// A group whose label has a text background (the HPE Aruba SFP modules): Visio fills the whole text block over the group's members, so the block
/// hides the lower half of the icon. The background comes after the member in the drawing and before the text.
#[test]
fn a_text_background_is_drawn_over_the_members_and_under_the_text() {
    let json = r##"{"pages":[{"name":"p","shapes":[
        {"type":"rect","x":50,"y":10,"w":5,"h":1,"label":"Fibre SFP","visioId":"1","z":1,"fontFamily":"Kalinga","fontSize":10.67,
         "textBox":{"x":-12.5,"y":0,"w":30,"h":12},"textBkgnd":"#123456"},
        {"type":"rect","x":50,"y":5,"w":5,"h":8,"label":"","visioId":"2","z":2,"group":{"id":"1"},"fill":"#ff0000"}]}]}"##;
    let document: Document = serde_json::from_str(json).expect("document");
    let svg = to_svg(&scene_from_document(&document, 0).unwrap());
    let member = svg.find("#ff0000").expect("the member is drawn");
    let background = svg.find("#123456").expect("the text background is drawn");
    let text = svg.find("<text").expect("the label is drawn");
    assert!(member < background && background < text, "member {member}, background {background}, text {text}");
}
