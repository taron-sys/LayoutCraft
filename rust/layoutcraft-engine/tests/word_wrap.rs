//! Wrapping a label whose formatting changes inside a word. Drawings made here, by our writer.
use layoutcraft_engine::Engine;

/// The text of each drawn line of a rectangle `wide` inches wide saying `words`, with the letters from `from` on coloured.
fn lines(words: &str, wide: f64, from: Option<usize>) -> Vec<String> {
    let mut engine = Engine::new();
    engine.new_blank(8.0, 6.0, false, "Page-1").unwrap();
    let id = engine.draw(0, 0, 1.0, 1.0, 1.0 + wide, 3.0).unwrap();
    engine.edit(0, 8, &format!("{id}\n{words}"), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    if let Some(from) = from {
        let spec = serde_json::json!({ "start": from, "end": words.chars().count(), "cells": { "Color": "#c00000" } });
        engine.edit(0, 13, &format!("{id}\n{spec}"), 0.0, 0.0, 0.0, 0.0, 0).unwrap();
    }
    let scene = layoutcraft_render::scene_from_document(engine.document().unwrap(), 0).unwrap();
    let svg = layoutcraft_render::to_svg(&scene);
    // a line starts at each positioned span; the text of the spans inside it belongs to it
    let mut out: Vec<(String, String)> = Vec::new();
    let mut rest = svg.as_str();
    let mut at = String::new();
    while let Some(open) = rest.find('<') {
        let text = &rest[..open];
        if !text.is_empty() && !at.is_empty() {
            match out.last_mut() {
                Some((y, line)) if *y == at => line.push_str(text),
                _ => out.push((at.clone(), text.to_string())),
            }
        }
        let close = open + rest[open..].find('>').unwrap();
        let tag = &rest[open..close];
        if tag.starts_with("<text") || tag.starts_with("<tspan") {
            if let Some(y) = tag.split(" y=\"").nth(1) {
                at = y.split('"').next().unwrap().to_string();
            } else if let Some(dy) = tag.split(" dy=\"").nth(1) {
                at = format!("{at}+{}", dy.split('"').next().unwrap());
            }
        } else if tag.starts_with("</text") {
            at.clear();
        }
        rest = &rest[close + 1..];
    }
    out.into_iter().map(|(_, line)| line).collect()
}

#[test]
fn a_change_of_formatting_inside_a_word_does_not_break_it() {
    for (words, wide, from) in [("Virtual machine", 1.0, 9), ("Forced tunnel", 0.8, 8), ("one two three", 3.0, 5)] {
        let plain = lines(words, wide, None);
        assert!(!plain.is_empty(), "{words}");
        assert_eq!(lines(words, wide, Some(from)), plain, "{words}");
    }
}

#[test]
fn a_word_wider_than_the_box_still_breaks_and_keeps_every_letter() {
    let word = "Abcdefghijklmnopqrstuvwxyzabcdefghij";
    let plain = lines(word, 0.8, None);
    assert!(plain.len() > 1, "{plain:?}");
    let split = lines(word, 0.8, Some(5));
    assert_eq!(split.concat(), word);
    assert_eq!(split, plain);
}
