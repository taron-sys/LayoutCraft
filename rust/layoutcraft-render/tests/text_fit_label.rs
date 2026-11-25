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
