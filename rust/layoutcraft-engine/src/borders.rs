//! Design > Borders & Titles: a Rust port of `web/borders.js` (docs/34, step N4.4). A border is an ordinary background page made of plain shapes;
//! `border_shapes` is the pure design (same numbers as the web, same order of floating point operations), `Engine::apply_border` / `remove_border` do what the
//! web's `applyBorder` / `removeBorder` do with the same page operations. `tests/borders_oracle.rs` compares both with the web code.
use serde_json::{json, Value};

use crate::Engine;

/// The border styles and their tool tips, in the order of the web's list.
pub const BORDER_STYLES: [(&str, &str); 4] = [
    ("Frame", "A thin frame around the page"),
    ("Double frame", "Two frames, the inner one thinner"),
    ("Frame with title block", "A frame and a title block at the lower right corner"),
    ("Title bar", "A bar with the title along the top edge"),
];

const INK: &str = "#1f2933";

/// One shape of a border: `kind` 0 = rectangle, corners in inches with y up from the lower left corner of the page.
#[derive(Debug, Clone, PartialEq)]
pub struct BorderShape {
    pub kind: i32,
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub text: Option<String>,
    pub style: Value,
}

fn frame(w: f64, h: f64, inset: f64, weight: f64) -> BorderShape {
    BorderShape { kind: 0, x0: inset, y0: inset, x1: w - inset, y1: h - inset, text: None, style: json!({"fill": "none", "line": INK, "weight": weight}) }
}

/// The shapes of border `style_name` for a page of `w` x `h` inches (`title` goes into the title block / bar).
pub fn border_shapes(style_name: &str, w: f64, h: f64, title: &str) -> Result<Vec<BorderShape>, String> {
    let m = 0.25f64.min(w.min(h) / 10.0);
    Ok(match style_name {
        "Frame" => vec![frame(w, h, m, 1.5)],
        "Double frame" => vec![frame(w, h, m, 2.25), frame(w, h, m * 1.6, 0.75)],
        "Frame with title block" => {
            let bw = 3.0f64.min((w - 2.0 * m) * 0.6);
            let bh = 0.8f64.min((h - 2.0 * m) * 0.25);
            vec![
                frame(w, h, m, 1.5),
                BorderShape { kind: 0, x0: w - m - bw, y0: m, x1: w - m, y1: m + bh, text: Some(title.to_string()), style: json!({"fill": "#ffffff", "line": INK, "weight": 1.0, "fontPt": 14.0, "bold": true}) },
            ]
        }
        "Title bar" => {
            let bh = 0.6f64.min(h * 0.12);
            vec![
                frame(w, h, m, 1.0),
                BorderShape { kind: 0, x0: m, y0: h - m - bh, x1: w - m, y1: h - m, text: Some(title.to_string()), style: json!({"fill": "#e6e9ee", "line": INK, "weight": 1.0, "fontPt": 16.0, "bold": true}) },
            ]
        }
        other => return Err(format!("no border style named {other}")),
    })
}

impl Engine {
    /// Add border `style_name` beneath page `page` as a new background page; returns the id of the background page. One undo step.
    pub fn apply_border(&mut self, page: usize, style_name: &str, title: &str) -> Result<String, String> {
        let size: Value = serde_json::from_str(&self.page_size_json(page)?).map_err(|e| e.to_string())?;
        let (w, h) = (size["w"].as_f64().ok_or("no page size")?, size["h"].as_f64().ok_or("no page size")?);
        let shapes = border_shapes(style_name, w, h, title)?;
        let depth = self.undo_depth();
        let result = (|| -> Result<String, String> {
            let index = self.page_count().ok_or("no document")?;
            self.page_op(1, index - 1, 0, &format!("Border - {style_name}"))?;
            self.page_op(12, index, 0, "")?; // a new page follows the background of the page before it: the border page must have none of its own
            self.page_op(6, index, 0, &format!("{w},{h}"))?;
            for s in &shapes {
                let id = self.draw(index, s.kind, s.x0, s.y0, s.x1, s.y1)?;
                self.edit(index, 10, &format!("{id}\n{}", s.style), 0.0, 0.0, 0.0, 0.0, 0)?;
                if let Some(text) = s.text.as_deref().filter(|t| !t.is_empty()) {
                    self.edit(index, 8, &format!("{id}\n{text}"), 0.0, 0.0, 0.0, 0.0, 0)?;
                }
            }
            let ids = |engine: &Engine| -> Result<Vec<String>, String> {
                let info: Value = serde_json::from_str(&engine.background_info_json()?).map_err(|e| e.to_string())?;
                Ok(info["backgrounds"].as_array().map_or_else(Vec::new, |list| list.iter().map(|b| b["id"].to_string().trim_matches('"').to_string()).collect()))
            };
            let before = ids(self)?;
            self.page_op(13, index, 0, "")?;
            let added = ids(self)?.into_iter().find(|id| !before.contains(id)).ok_or("the border page was not made a background")?;
            self.page_op(12, page, 0, &added)?;
            Ok(added)
        })();
        self.collapse_history(depth);
        result
    }

    /// Take the border (the background page) off page `page`; the background page stays in the drawing. One undo step.
    pub fn remove_border(&mut self, page: usize) -> Result<(), String> {
        let depth = self.undo_depth();
        let result = self.page_op(12, page, 0, "").map(|_| ());
        self.collapse_history(depth);
        result
    }

    /// Does `page` have a border or other background page?
    pub fn page_has_background(&self, page: usize) -> bool {
        self.background_info_json().ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).is_some_and(|info| !info["assigned"].get(page).map_or(true, Value::is_null))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_keeps_a_margin_of_a_tenth_of_the_small_side_up_to_a_quarter_inch() {
        let letter = border_shapes("Frame", 11.0, 8.5, "").unwrap();
        assert_eq!((letter[0].x0, letter[0].y0, letter[0].x1, letter[0].y1), (0.25, 0.25, 10.75, 8.25));
        let tiny = border_shapes("Frame", 2.0, 1.0, "").unwrap();
        assert_eq!((tiny[0].x0, tiny[0].x1), (0.1, 1.9));
    }

    #[test]
    fn the_styles_have_the_shapes_the_web_names() {
        let counts: Vec<usize> = BORDER_STYLES.iter().map(|(name, _)| border_shapes(name, 11.0, 8.5, "T").unwrap().len()).collect();
        assert_eq!(counts, [1, 2, 2, 2]);
        let block = border_shapes("Frame with title block", 11.0, 8.5, "Plan").unwrap();
        assert_eq!(block[1].text.as_deref(), Some("Plan"));
        assert_eq!((block[1].x1, block[1].y0), (10.75, 0.25));
        assert!(border_shapes("Nope", 11.0, 8.5, "").is_err());
    }
}
