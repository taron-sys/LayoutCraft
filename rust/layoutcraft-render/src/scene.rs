//! Convert a `layoutcraft-model` page into renderer-independent scene items.
use std::collections::BTreeMap;
use std::f64::consts::PI;

use serde_json::Value;
use layoutcraft_model::{Connector, Document, Endpoint, Image, Shape};

pub type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, Copy, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone)]
pub struct Scene {
    /// Emit `textLength` on every text line (see `SceneOptions::fit_text`).
    pub fit_text: bool,
    /// Page name, the accessible name of the whole drawing.
    pub title: String,
    pub min_x: f64,
    pub min_y: f64,
    pub width: f64,
    pub height: f64,
    pub background: String,
    pub items: Vec<SceneItem>,
}

impl Scene {
    pub fn raster_size(&self) -> (u32, u32) {
        (
            self.width.round().max(1.0) as u32,
            self.height.round().max(1.0) as u32,
        )
    }
}

#[derive(Debug, Clone)]
pub enum SceneItem {
    Shape(Box<SceneShape>),
    Connector(Box<SceneConnector>),
}

#[derive(Debug, Clone)]
pub struct SceneShape {
    pub visio_id: Option<String>,
    /// Alt text for assistive technology.
    pub alt_text: Option<String>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub rot: f64,
    /// Extra text rotation inside the shape's rotation group.
    pub text_rot: f64,
    /// Corner radius in pixels (`Rounding`).
    pub rounding: f64,
    pub fill: String,
    pub fill_opacity: f64,
    pub stroke: String,
    pub stroke_opacity: f64,
    pub stroke_width: f64,
    pub dash: Option<String>,
    pub parts: Vec<ScenePart>,
    pub image: Option<SceneImage>,
    pub text: Option<SceneText>,
    pub texts: Vec<SceneText>,
    pub shadow: Option<SceneShadow>,
    pub bevel: Option<SceneBevel>,
    pub glow: Option<SceneGlow>,
    pub soft_edge: Option<SceneSoftEdge>,
    pub reflection: Option<SceneReflection>,
    pub z: f64,
}

#[derive(Debug, Clone)]
pub struct SceneBevel {
    pub kind: u32,
    pub width: f64,
    pub height: f64,
    pub bottom: bool,
}

#[derive(Debug, Clone)]
pub struct SceneGlow {
    pub radius: f64,
    pub color: String,
    pub opacity: f64,
}

#[derive(Debug, Clone)]
pub struct SceneSoftEdge {
    pub radius: f64,
}

#[derive(Debug, Clone)]
pub struct SceneReflection {
    pub size: f64,
    pub distance: f64,
    pub blur: f64,
    pub opacity: f64,
}

#[derive(Debug, Clone)]
pub struct SceneShadow {
    pub dx: f64,
    pub dy: f64,
    pub blur: f64,
    pub color: String,
    pub opacity: f64,
    pub kind: u8,
    pub scale: f64,
    pub oblique: f64,
}

#[derive(Debug, Clone)]
pub struct ScenePart {
    pub d: String,
    pub fill: String,
    pub fill_opacity: f64,
    pub stroke: String,
    pub stroke_opacity: f64,
    pub stroke_width: f64,
    pub dash: Option<String>,
    pub fill_gradient: Option<SceneGradient>,
    pub stroke_gradient: Option<SceneGradient>,
    pub fill_pattern: Option<SceneFillPattern>,
    /// Line ends (arrow type, size index) of an open path.
    pub begin_arrow: Option<(u32, u32)>,
    pub end_arrow: Option<(u32, u32)>,
}

#[derive(Debug, Clone)]
pub struct SceneFillPattern {
    pub kind: u32,
    pub fore: String,
    pub back: String,
}

#[derive(Debug, Clone)]
pub struct SceneGradient {
    /// Visio gradient angle in radians.
    pub angle: f64,
    /// Visio `FillGradientDir`/`LineGradientDir`: 0 linear, 1-7 radial,
    /// 8-12 rectangular, 13 path.
    pub dir: u32,
    pub stops: Vec<SceneGradientStop>,
    /// A legacy fill-pattern gradient (patterns 25-40): blended along a smoothstep curve.
    pub smooth: bool,
    /// A legacy pattern gradient with an explicit rectangular type: eased from its far end.
    pub far_end: bool,
}

#[derive(Debug, Clone)]
pub struct SceneGradientStop {
    pub color: String,
    pub opacity: f64,
    pub position: f64,
}

fn scene_gradient(value: &Value) -> Option<SceneGradient> {
    let angle = value.get("angle").and_then(Value::as_f64).unwrap_or(0.0);
    let dir = value
        .get("dir")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(13) as u32;
    let stops = value
        .get("stops")?
        .as_array()?
        .iter()
        .filter_map(|stop| {
            Some(SceneGradientStop {
                color: stop.get("color")?.as_str()?.to_string(),
                opacity: stop.get("opacity").and_then(Value::as_f64).unwrap_or(1.0),
                position: stop.get("position").and_then(Value::as_f64).unwrap_or(0.0),
            })
        })
        .collect::<Vec<_>>();
    let smooth = value.get("smooth").and_then(Value::as_bool).unwrap_or(false);
    let far_end = value.get("far_end").and_then(Value::as_bool).unwrap_or(false);
    (stops.len() >= 2).then_some(SceneGradient { angle, dir, stops, smooth, far_end })
}

#[derive(Debug, Clone)]
pub struct SceneImage {
    pub data_uri: Option<String>,
    pub name: String,
    pub unsupported: bool,
    pub brightness: f64,
    pub contrast: f64,
    pub gamma: f64,
    pub transparency: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub adjust: Option<bool>,
    /// Path (in the shape's own box) the picture is clipped to, when it is not the whole box.
    pub clip_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SceneConnector {
    pub from: Point,
    pub to: Point,
    pub waypoints: Vec<Point>,
    pub curve: Option<(Point, Point)>,
    pub stroke: String,
    pub stroke_width: f64,
    pub dash: Option<String>,
    pub arrow_start: Option<String>,
    pub arrow_end: Option<String>,
    /// Gallery line ends (type, size index) drawn per kind instead of the generic arrow.
    pub begin_line_end: Option<(u32, u32)>,
    pub end_line_end: Option<(u32, u32)>,
    pub label: String,
    pub label_point: Point,
    pub z: f64,
    /// The connector's Visio id: the SVG group gets `data-cid` so a host can move it with a live preview.
    pub visio_id: Option<String>,
    /// Hops over crossing connectors, from the stored geometry.
    pub hops: Vec<SceneHop>,
}

/// A hop Visio stored in a connector's geometry where it crosses another connector: the straight
/// stretch `from` -> `to` is replaced by an arc through `apex`, or by a polyline through `via`.
#[derive(Debug, Clone)]
pub struct SceneHop {
    pub from: Point,
    pub to: Point,
    pub via: Vec<Point>,
    pub apex: Option<Point>,
    /// Lift the pen between `from` and `to` instead of drawing.
    pub gap: bool,
}

#[derive(Debug, Clone)]
pub struct SceneText {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub paragraphs: Vec<SceneParagraph>,
    pub font_size: f64,
    pub font_family: String,
    pub color: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub v_align: String,
    /// Extra rotation about the block centre, radians, clockwise positive.
    pub rot: f64,
    /// Exported with room around its ink (the label of a child that paints nothing else).
    pub pad: bool,
    /// The block's centre lies outside its shape's box (a label below or above an icon).
    pub outside: bool,
}

#[derive(Debug, Clone)]
pub struct SceneParagraph {
    pub align: String,
    pub bullet: bool,
    pub bullet_str: String,
    pub indent: f64,
    /// Negative: the line pitch as a multiple of the font size (0 = automatic: 1.2 times the font size); positive: an exact distance in pixels.
    pub line_spacing: f64,
    /// Space before / after the paragraph, pixels.
    pub space_before: f64,
    pub space_after: f64,
    /// `IndFirst` in pixels: the first line starts this far from the left indent (negative: a hanging bullet).
    pub indent_first: f64,
    pub runs: Vec<SceneRun>,
}

#[derive(Debug, Clone)]
pub struct SceneRun {
    pub text: String,
    pub font_size: Option<f64>,
    pub font_family: Option<String>,
    pub color: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    /// Strike-through (`Strikethru`), and the letter spacing in pixels (`Letterspace`); the case change
    /// (`Case`: all capitals, initial capitals) is applied to `text` when the run is built.
    pub strike: Option<bool>,
    pub spacing: Option<f64>,
}

/// The text after a Visio `Case` change: 1 = ALL CAPITALS, 2 = Initial Capitals (every word).
fn apply_case(text: &str, caps: u64) -> String {
    match caps {
        1 => text.to_uppercase(),
        2 => {
            let mut out = String::with_capacity(text.len());
            let mut start = true;
            for c in text.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                    start = false;
                } else {
                    out.push(c);
                    start = !c.is_alphanumeric() && c != char::from(39u8);
                }
            }
            out
        }
        _ => text.to_string(),
    }
}

pub fn scene_from_document(document: &Document, page_index: usize) -> Result<Scene> {
    scene_from_document_with_media(document, page_index, &BTreeMap::new())
}

/// Build a scene and attach picture bytes by media ID. The model stores media
/// metadata only, so callers that have the source package pass its bytes here.
/// How the scene is going to be used. `print` leaves out shapes that sit only on
/// layers with printing switched off, as Visio's print and `Page.Export` do; the
/// editor keeps drawing them.
#[derive(Debug, Clone, Copy, Default)]
pub struct SceneOptions {
    pub print: bool,
    /// Give every text line the width measured with the baked font metrics (`textLength`), so a
    /// viewer without the drawing's fonts still wraps and spaces text as Visio does.
    pub fit_text: bool,
}

pub fn scene_from_document_with_media(
    document: &Document,
    page_index: usize,
    media: &BTreeMap<String, Vec<u8>>,
) -> Result<Scene> {
    scene_from_document_with_options(document, page_index, media, SceneOptions::default())
}

pub fn scene_from_document_with_options(
    document: &Document,
    page_index: usize,
    media: &BTreeMap<String, Vec<u8>>,
    options: SceneOptions,
) -> Result<Scene> {
    let page = document
        .pages
        .get(page_index)
        .ok_or_else(|| format!("page {page_index} is out of range"))?;
    let shape_by_visio: BTreeMap<&str, &Shape> = page
        .shapes
        .iter()
        .filter_map(|shape| shape.visio_id.as_deref().map(|id| (id, shape)))
        .collect();
    let adjust_by_media: BTreeMap<&str, bool> = document
        .media
        .iter()
        .filter_map(|(id, media)| media.adjust.map(|adjust| (id.as_str(), adjust)))
        .collect();

    let mut ordered: Vec<(f64, usize, SceneItem)> = Vec::new();
    let mut z_index = 0usize;
    for shape in &page.shapes {
        let z = shape.z.unwrap_or(z_index as f64);
        // Shapes only on hidden layers are not drawn (Visio layer visibility).
        if shape.layer_hidden || (options.print && shape.layer_nonprinting) {
            z_index += 1;
            continue;
        }
        if let Some(mut item) = scene_shape(shape, &shape_by_visio, media, &adjust_by_media) {
            if let Some(colour) = shape.layer_color.as_deref() {
                apply_layer_color(&mut item, colour);
            }
            ordered.push((z, z_index, SceneItem::Shape(Box::new(item))));
        }
        z_index += 1;
    }
    for connector in &page.connectors {
        let z = connector.order_z.or(connector.z).unwrap_or(z_index as f64);
        if let Some(item) = scene_connector(connector, &shape_by_visio) {
            ordered.push((z, z_index, SceneItem::Connector(Box::new(item))));
        }
        z_index += 1;
    }
    ordered.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    let mut items: Vec<SceneItem> = ordered.into_iter().map(|(_, _, item)| item).collect();
    let page_scale = page.scale;
    if (page_scale - 1.0).abs() > 1e-9 && page_scale > 0.0 {
        for item in &mut items {
            scale_scene_item(item, page_scale);
        }
    }

    let mut bounds = Bounds::new();
    for item in &items {
        match item {
            SceneItem::Shape(shape) => bounds.add_shape(shape),
            SceneItem::Connector(connector) => bounds.add_connector(connector),
        }
    }
    let (min_x, min_y, width, height) = bounds.finish();
    Ok(Scene {
        fit_text: options.fit_text,
        title: page.name.clone(),
        min_x,
        min_y,
        width,
        height,
        background: "#ffffff".to_string(),
        items,
    })
}

fn scale_scene_item(item: &mut SceneItem, scale: f64) {
    match item {
        SceneItem::Shape(shape) => {
            shape.x *= scale;
            shape.y *= scale;
            shape.w *= scale;
            shape.h *= scale;
            for part in &mut shape.parts {
                part.d = scale_path(&part.d, scale);
                // Line weights are paper-space in Visio: a 4 pt line stays 4 pt on a
                // 1:12 page, so only the geometry scales, not the stroke width.
                // (The page-scale pass multiplies everything else by `scale`.)
            }
            if let Some(text) = &mut shape.text {
                scale_text(text, scale);
            }
            for text in &mut shape.texts {
                scale_text(text, scale);
            }
            if let Some(bevel) = &mut shape.bevel {
                bevel.width *= scale;
                bevel.height *= scale;
            }
            if let Some(shadow) = &mut shape.shadow {
                shadow.dx *= scale;
                shadow.dy *= scale;
                shadow.blur *= scale;
            }
            if let Some(glow) = &mut shape.glow {
                glow.radius *= scale;
            }
            if let Some(edge) = &mut shape.soft_edge {
                edge.radius *= scale;
            }
            if let Some(reflection) = &mut shape.reflection {
                reflection.distance *= scale;
                reflection.blur *= scale;
            }
        }
        SceneItem::Connector(connector) => {
            connector.from.x *= scale;
            connector.from.y *= scale;
            connector.to.x *= scale;
            connector.to.y *= scale;
            for point in &mut connector.waypoints {
                point.x *= scale;
                point.y *= scale;
            }
            if let Some((a, b)) = &mut connector.curve {
                a.x *= scale;
                a.y *= scale;
                b.x *= scale;
                b.y *= scale;
            }
            for hop in &mut connector.hops {
                for point in [&mut hop.from, &mut hop.to].into_iter().chain(hop.via.iter_mut()).chain(hop.apex.iter_mut()) {
                    point.x *= scale;
                    point.y *= scale;
                }
            }
            connector.label_point.x *= scale;
            connector.label_point.y *= scale;
            // Paper-space line weight (see the part case above).
        }
    }
}

fn scale_text(text: &mut SceneText, scale: f64) {
    let center_x = text.x + text.w / 2.0;
    let center_y = text.y + text.h / 2.0;
    text.x *= scale;
    text.y *= scale;
    text.w *= scale;
    text.h *= scale;
    // Text is measured in paper units and does not shrink with an
    // architectural drawing scale. Keep a minimum box wide enough for the longest
    // word (not the whole line: a label in a box that is wide enough must still
    // wrap inside it), so a word is never cut or drawn outside a tiny box.
    let content = text
        .paragraphs
        .iter()
        .flat_map(|paragraph| paragraph.runs.iter())
        .map(|run| run.text.as_str())
        .collect::<String>();
    let lines: Vec<&str> = content.split(['\n', '\r']).collect();
    let longest = lines
        .iter()
        .flat_map(|line| line.split_whitespace())
        .map(|word| word.chars().count())
        .max()
        .unwrap_or(0);
    let min_w = longest as f64 * text.font_size * 0.58;
    let min_h = lines.len().max(1) as f64 * text.font_size * 1.25;
    if text.w < min_w {
        text.x = center_x * scale - min_w / 2.0;
        text.w = min_w;
    }
    if text.h < min_h {
        text.y = center_y * scale - min_h / 2.0;
        text.h = min_h;
    }
}

/// Scale an SVG path's geometry while leaving arc flags/rotations intact.
/// Bounding box of the points of an SVG path made of `M L C Q Z` and `A` (an arc counts as its radii box around the chord midpoint).
/// The first and last vertices of a path, each with the neighbouring point that gives the line's
/// direction there (a curve contributes its control point). Absolute `M L C Q A` paths only.
pub(crate) fn path_ends(path: &str) -> Option<[((f64, f64), (f64, f64)); 2]> {
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut numbers: Vec<f64> = Vec::new();
    let mut command = ' ';
    let mut token = String::new();
    fn take(command: char, numbers: &mut Vec<f64>, points: &mut Vec<(f64, f64)>) {
        let width = match command {
            'M' | 'L' | 'T' => 2,
            'C' => 6,
            'S' | 'Q' => 4,
            'A' => 7,
            _ => 0,
        };
        if width > 0 {
            for chunk in numbers.chunks_exact(width) {
                if width == 6 || width == 4 {
                    for pair in chunk[..width - 2].chunks_exact(2) {
                        points.push((pair[0], pair[1]));
                    }
                }
                points.push((chunk[width - 2], chunk[width - 1]));
            }
        }
        numbers.clear();
    }
    let mut flush = |token: &mut String, numbers: &mut Vec<f64>| {
        if let Ok(value) = token.parse::<f64>() {
            numbers.push(value);
        }
        token.clear();
    };
    for c in path.chars() {
        if c.is_ascii_alphabetic() {
            flush(&mut token, &mut numbers);
            take(command, &mut numbers, &mut points);
            command = c;
        } else if c == ' ' || c == ',' {
            flush(&mut token, &mut numbers);
        } else if c == '-' && !token.is_empty() && !token.ends_with('e') {
            flush(&mut token, &mut numbers);
            token.push(c);
        } else {
            token.push(c);
        }
    }
    flush(&mut token, &mut numbers);
    take(command, &mut numbers, &mut points);
    // Neighbours: skip repeated points so the direction is defined.
    let first = *points.first()?;
    let next = points.iter().find(|p| (p.0 - first.0).hypot(p.1 - first.1) > 1e-6)?;
    let last = *points.last()?;
    let prev = points.iter().rev().find(|p| (p.0 - last.0).hypot(p.1 - last.1) > 1e-6)?;
    Some([(first, *next), (last, *prev)])
}

pub(crate) fn path_extent(path: &str) -> Option<(f64, f64, f64, f64)> {
    let mut numbers: Vec<f64> = Vec::new();
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut command = ' ';
    let mut token = String::new();
    let flush = |token: &mut String, numbers: &mut Vec<f64>| {
        if !token.is_empty() {
            if let Ok(value) = token.parse::<f64>() {
                numbers.push(value);
            }
            token.clear();
        }
    };
    let take = |command: char, numbers: &mut Vec<f64>, points: &mut Vec<(f64, f64)>| {
        let width = match command {
            'M' | 'L' | 'T' => 2,
            'C' => 6,
            'S' | 'Q' => 4,
            'A' => 7,
            _ => return,
        };
        for chunk in numbers.chunks_exact(width) {
            if width == 7 {
                // An arc bulges out of its chord: take the radii box around the chord's midpoint
                // (exact for the half circles Visio's ellipses become).
                if let Some(&(sx, sy)) = points.last() {
                    let (mx, my) = ((sx + chunk[5]) / 2.0, (sy + chunk[6]) / 2.0);
                    points.push((mx - chunk[0], my - chunk[1]));
                    points.push((mx + chunk[0], my + chunk[1]));
                }
            }
            points.push((chunk[width - 2], chunk[width - 1]));
            if width == 6 || width == 4 {
                for pair in chunk[..width - 2].chunks_exact(2) {
                    points.push((pair[0], pair[1]));
                }
            }
        }
        numbers.clear();
    };
    for c in path.chars() {
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            flush(&mut token, &mut numbers);
            take(command, &mut numbers, &mut points);
            command = c;
        } else if c == ',' || c == ' ' {
            flush(&mut token, &mut numbers);
        } else if c == '-' && !token.is_empty() && !token.ends_with('e') && !token.ends_with('E') {
            flush(&mut token, &mut numbers);
            token.push(c);
        } else {
            token.push(c);
        }
    }
    flush(&mut token, &mut numbers);
    take(command, &mut numbers, &mut points);
    let first = points.first().copied()?;
    Some(points.iter().fold((first.0, first.1, first.0, first.1), |(x0, y0, x1, y1), p| {
        (x0.min(p.0), y0.min(p.1), x1.max(p.0), y1.max(p.1))
    }))
}

fn scale_path(path: &str, scale: f64) -> String {
    let bytes = path.as_bytes();
    let mut index = 0;
    let mut command = ' ';
    let mut out = String::new();
    while index < bytes.len() {
        while index < bytes.len() && (bytes[index].is_ascii_whitespace() || bytes[index] == b',') {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }
        if bytes[index].is_ascii_alphabetic() {
            command = bytes[index] as char;
            index += 1;
            if matches!(command, 'Z' | 'z') {
                out.push(command);
                continue;
            }
        }
        let mut numbers = Vec::new();
        while index < bytes.len() {
            while index < bytes.len()
                && (bytes[index].is_ascii_whitespace() || bytes[index] == b',')
            {
                index += 1;
            }
            if index >= bytes.len() || bytes[index].is_ascii_alphabetic() {
                break;
            }
            let start = index;
            if bytes[index] == b'-' || bytes[index] == b'+' {
                index += 1;
            }
            let mut seen_dot = false;
            while index < bytes.len() {
                let byte = bytes[index];
                if byte.is_ascii_digit() {
                    index += 1;
                } else if byte == b'.' && !seen_dot {
                    seen_dot = true;
                    index += 1;
                } else if byte == b'e' || byte == b'E' {
                    index += 1;
                    if index < bytes.len() && (bytes[index] == b'-' || bytes[index] == b'+') {
                        index += 1;
                    }
                } else {
                    break;
                }
            }
            if let Ok(value) = path[start..index].parse::<f64>() {
                numbers.push(value);
            }
        }
        if numbers.is_empty() {
            continue;
        }
        out.push(command);
        let (group, scale_indices): (usize, &[usize]) = match command.to_ascii_uppercase() {
            'H' | 'V' => (1, &[0]),
            'C' => (6, &[0, 1, 2, 3, 4, 5]),
            'S' | 'Q' => (4, &[0, 1, 2, 3]),
            'A' => (7, &[0, 1, 5, 6]),
            _ => (2, &[0, 1]),
        };
        for (offset, value) in numbers.into_iter().enumerate() {
            if offset % group == 0 && offset > 0 {
                // implicit repeated command segments
                out.push(command);
            }
            let position = offset % group;
            let scaled = if scale_indices.contains(&position) {
                value * scale
            } else {
                value
            };
            if position != 0 || offset == 0 {
                out.push(' ');
            }
            out.push_str(&path_num(scaled));
        }
    }
    out.trim().to_string()
}

fn path_num(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded == 0.0 {
        return "0".to_string();
    }
    if rounded.fract() == 0.0 {
        return format!("{}", rounded as i64);
    }
    let mut text = format!("{rounded:.3}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

/// A shape on a layer with a colour (measured in Visio, `probe-layer-color.ps1`, a coloured box over a yellow one): the lines and the
/// text are drawn in the layer's colour and the fill is opaque white (gradients and patterns included).
fn apply_layer_color(item: &mut SceneShape, colour: &str) {
    if !item.fill.eq_ignore_ascii_case("none") {
        item.fill = "#ffffff".to_string();
        item.fill_opacity = 1.0;
    }
    if !item.stroke.eq_ignore_ascii_case("none") {
        item.stroke = colour.to_string();
    }
    for part in &mut item.parts {
        if !part.fill.eq_ignore_ascii_case("none") {
            part.fill = "#ffffff".to_string();
            part.fill_opacity = 1.0;
            part.fill_gradient = None;
            part.fill_pattern = None;
        }
        if !part.stroke.eq_ignore_ascii_case("none") {
            part.stroke = colour.to_string();
            part.stroke_gradient = None;
        }
    }
    for text in item.text.iter_mut().chain(item.texts.iter_mut()) {
        text.color = colour.to_string();
        for paragraph in &mut text.paragraphs {
            for run in &mut paragraph.runs {
                run.color = Some(colour.to_string());
            }
        }
    }
}

fn scene_shape(
    shape: &Shape,
    shapes: &BTreeMap<&str, &Shape>,
    media: &BTreeMap<String, Vec<u8>>,
    adjust_by_media: &BTreeMap<&str, bool>,
) -> Option<SceneShape> {
    let text = shape_text(shape, shapes);
    let fill = data_color_fill(shape)
        .unwrap_or_else(|| shape.fill.clone().unwrap_or_else(|| "#dbe9f6".to_string()));
    let image = shape
        .image
        .as_ref()
        .map(|image| scene_image(image, media, adjust_by_media));
    // a group's own geometry drawn in front of its members (DisplayMode, step 219): the first `front_parts` parts go last
    let split = shape.front_parts.min(shape.parts.len());
    let parts = shape.parts[split..]
        .iter()
        .chain(shape.parts[..split].iter())
        .map(|part| ScenePart {
            d: part.d.clone(),
            fill: part.fill.clone().unwrap_or_else(|| "none".to_string()),
            fill_opacity: part.fill_opacity.unwrap_or(1.0),
            stroke: part.stroke.clone().unwrap_or_else(|| "none".to_string()),
            stroke_opacity: part.stroke_opacity.unwrap_or(1.0),
            stroke_width: part.stroke_width.unwrap_or(0.0),
            dash: part.dash.clone(),
            fill_gradient: part.fill_gradient.as_ref().and_then(scene_gradient),
            stroke_gradient: part.stroke_gradient.as_ref().and_then(scene_gradient),
            fill_pattern: part.fill_pattern.as_ref().map(|pattern| SceneFillPattern {
                kind: pattern.kind,
                fore: pattern.fore.clone(),
                back: pattern.back.clone(),
            }),
            begin_arrow: part.begin_arrow.map(|end| (end.kind, end.size)),
            end_arrow: part.end_arrow.map(|end| (end.kind, end.size)),
        })
        .collect();
    let texts: Vec<SceneText> = shape
        .sub_texts
        .iter()
        .map(|sub| SceneText {
            x: shape.x + sub.x,
            y: shape.y + sub.y,
            w: sub.w,
            h: sub.h,
            paragraphs: vec![SceneParagraph {
                align: sub.align.clone(),
                bullet: false,
                bullet_str: String::new(),
                indent: sub.indent,
                line_spacing: 0.0,
                space_before: 0.0,
                space_after: 0.0,
                indent_first: 0.0,
                runs: vec![SceneRun {
                    text: sub.text.clone(),
                    font_size: Some(sub.font_size),
                    font_family: Some(sub.font_family.clone()),
                    color: Some(sub.color.clone()),
                    bold: Some(sub.bold),
                    italic: Some(sub.italic),
                    underline: Some(sub.underline),
                    strike: None,
                    spacing: None,
                }],
            }],
            font_size: sub.font_size,
            font_family: sub.font_family.clone(),
            color: sub.color.clone(),
            bold: sub.bold,
            italic: sub.italic,
            underline: sub.underline,
            v_align: sub.v_align.clone(),
            rot: sub.text_rot,
            pad: sub.pad,
            outside: sub.h <= sub.font_size * 1.8,
        })
        .collect();
    // A group's label and a child that draws the very same text (a DefaultLabel group: its placeholder text became the
    // instance's label, its child carries the text too) would show it twice; Visio draws it once, with the child.
    let plain = |text: &SceneText| -> String { text.paragraphs.iter().map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>()).collect::<Vec<_>>().join("
") };
    let text = match text {
        Some(own) if !plain(&own).trim().is_empty() && texts.iter().any(|sub| plain(sub).trim() == plain(&own).trim()) => None,
        other => other,
    };
    Some(SceneShape {
        visio_id: shape.visio_id.clone(),
        alt_text: shape.alt_text.clone(),
        x: shape.x,
        y: shape.y,
        w: shape.w,
        h: shape.h,
        rot: shape.rot,
        text_rot: shape.text_rot.unwrap_or(0.0),
        rounding: shape.rounding.unwrap_or(0.0),
        fill,
        fill_opacity: shape.fill_opacity.unwrap_or(1.0),
        stroke: shape
            .stroke
            .clone()
            .unwrap_or_else(|| "#000000".to_string()),
        stroke_opacity: shape.stroke_opacity.unwrap_or(1.0),
        stroke_width: shape.stroke_width.unwrap_or(0.0),
        dash: shape.stroke_style.clone(),
        parts,
        image,
        text,
        texts,
        shadow: shape.shadow.as_ref().map(|shadow| SceneShadow {
            dx: shadow.dx,
            dy: shadow.dy,
            blur: shadow.blur,
            color: shadow.color.clone(),
            opacity: shadow.opacity,
            kind: shadow.kind,
            scale: shadow.scale,
            oblique: shadow.oblique,
        }),
        bevel: shape.bevel.as_ref().map(|bevel| SceneBevel {
            kind: bevel.kind,
            width: bevel.width,
            height: bevel.height,
            bottom: bevel.bottom,
        }),
        glow: shape.glow.as_ref().map(|glow| SceneGlow {
            radius: glow.radius,
            color: glow.color.clone(),
            opacity: glow.opacity,
        }),
        soft_edge: shape.soft_edge.as_ref().map(|edge| SceneSoftEdge {
            radius: edge.radius,
        }),
        reflection: shape.reflection.as_ref().map(|reflection| SceneReflection {
            size: reflection.size,
            distance: reflection.distance,
            blur: reflection.blur,
            opacity: reflection.opacity,
        }),
        z: shape.z.unwrap_or(0.0),
    })
}

fn scene_image(
    image: &Image,
    media: &BTreeMap<String, Vec<u8>>,
    adjust_by_media: &BTreeMap<&str, bool>,
) -> SceneImage {
    let data_uri = image
        .media_id
        .as_deref()
        .and_then(|id| media.get(id))
        .map(|bytes| {
            // The embedded bytes win over the recorded type: BMP pictures are served
            // as PNG (`media_bytes`), which the SVG renderers can actually draw.
            let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
                "image/png"
            } else {
                image
                    .media_type
                    .as_deref()
                    .unwrap_or("application/octet-stream")
            };
            format!("data:{mime};base64,{}", base64_encode(bytes))
        });
    SceneImage {
        data_uri,
        name: image.name.clone().unwrap_or_default(),
        unsupported: image.unsupported,
        brightness: image.brightness,
        contrast: image.contrast,
        gamma: image.gamma,
        transparency: image.transparency,
        offset_x: image.offset_x,
        offset_y: image.offset_y,
        scale_x: image.scale_x,
        scale_y: image.scale_y,
        adjust: image
            .media_id
            .as_deref()
            .and_then(|id| adjust_by_media.get(id).copied()),
        clip_path: image
            .extra
            .get("clipPath")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    }
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let value = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((value >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((value >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((value >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(value & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn scene_connector(
    connector: &Connector,
    shapes: &BTreeMap<&str, &Shape>,
) -> Option<SceneConnector> {
    let imported_waypoints: Vec<Point> = connector
        .waypoints
        .iter()
        .filter_map(|point| {
            Some(Point {
                x: point.get("x")?.as_f64()?,
                y: point.get("y")?.as_f64()?,
            })
        })
        .collect();
    let from_center = endpoint_center(&connector.from, shapes);
    let to_center = endpoint_center(&connector.to, shapes);
    let toward_from = imported_waypoints.first().copied().unwrap_or(to_center);
    let toward_to = imported_waypoints.last().copied().unwrap_or(from_center);
    let from = endpoint_point(&connector.from, shapes, Some(toward_from));
    let to = endpoint_point(&connector.to, shapes, Some(toward_to));

    let fallback_a = normalize(Point {
        x: to.x - from.x,
        y: to.y - from.y,
    });
    let fallback_b = normalize(Point {
        x: from.x - to.x,
        y: from.y - to.y,
    });
    let direction_from = end_direction(&connector.from, shapes, fallback_a);
    let direction_to = end_direction(&connector.to, shapes, fallback_b);

    let (waypoints, curve) = if !imported_waypoints.is_empty() {
        (imported_waypoints, None)
    } else {
        auto_route(
            &connector.route,
            connector.route_style,
            from,
            to,
            direction_from,
            direction_to,
        )
    };
    let label_point = connector
        .label_point
        .as_ref()
        .and_then(|value| {
            Some(Point {
                x: value.get("x")?.as_f64()?,
                y: value.get("y")?.as_f64()?,
            })
        })
        .unwrap_or_else(|| midpoint(from, to, &waypoints));
    if connector.stroke.is_none()
        && connector.stroke_width == Some(0.0)
        && waypoints.is_empty()
        && from.x == to.x
        && from.y == to.y
    {
        return None;
    }
    Some(SceneConnector {
        from,
        to,
        waypoints,
        curve,
        stroke: connector
            .stroke
            .clone()
            .unwrap_or_else(|| "#000000".to_string()),
        stroke_width: connector.stroke_width.unwrap_or(1.0),
        dash: connector.stroke_style.clone(),
        arrow_start: connector.arrow_start.clone(),
        arrow_end: connector.arrow_end.clone(),
        begin_line_end: connector.begin_line_end.map(|end| (end.kind, end.size)),
        end_line_end: connector.end_line_end.map(|end| (end.kind, end.size)),
        label: connector.label.clone(),
        label_point,
        z: connector.z.unwrap_or(0.0),
        visio_id: connector.visio_id.clone().or_else(|| connector.id.clone()),
        hops: connector
            .hops
            .iter()
            .map(|hop| SceneHop {
                from: Point { x: hop.from[0], y: hop.from[1] },
                to: Point { x: hop.to[0], y: hop.to[1] },
                via: hop.via.iter().map(|p| Point { x: p[0], y: p[1] }).collect(),
                apex: hop.apex.map(|p| Point { x: p[0], y: p[1] }),
                gap: hop.gap,
            })
            .collect(),
    })
}

fn endpoint_center(endpoint: &Endpoint, shapes: &BTreeMap<&str, &Shape>) -> Point {
    let Some(shape_id) = endpoint.shape_id.as_deref() else {
        return endpoint
            .point
            .as_ref()
            .and_then(|point| {
                Some(Point {
                    x: point.get("x")?.as_f64()?,
                    y: point.get("y")?.as_f64()?,
                })
            })
            .unwrap_or_default();
    };
    let visio_id = shape_id.strip_prefix("shape:visio:").unwrap_or(shape_id);
    shapes
        .get(visio_id)
        .map(|shape| Point {
            x: shape.x + shape.w / 2.0,
            y: shape.y + shape.h / 2.0,
        })
        .unwrap_or_default()
}

fn endpoint_shape<'a>(
    endpoint: &Endpoint,
    shapes: &'a BTreeMap<&str, &Shape>,
) -> Option<&'a Shape> {
    let shape_id = endpoint.shape_id.as_deref()?;
    let visio_id = shape_id.strip_prefix("shape:visio:").unwrap_or(shape_id);
    shapes.get(visio_id).copied()
}

fn endpoint_point(
    endpoint: &Endpoint,
    shapes: &BTreeMap<&str, &Shape>,
    toward: Option<Point>,
) -> Point {
    if let Some(shape) = endpoint_shape(endpoint, shapes) {
        if let Some(local) = &endpoint.local {
            let lx = local.get("x").and_then(Value::as_f64).unwrap_or(0.5);
            let ly = local.get("y").and_then(Value::as_f64).unwrap_or(0.5);
            return local_to_world(
                shape,
                Point {
                    x: lx * shape.w,
                    y: ly * shape.h,
                },
            );
        }
        return anchor_on_shape_at(shape, endpoint.side.as_deref().unwrap_or("e"), toward);
    }
    endpoint
        .point
        .as_ref()
        .and_then(|point| {
            Some(Point {
                x: point.get("x")?.as_f64()?,
                y: point.get("y")?.as_f64()?,
            })
        })
        .unwrap_or_default()
}

fn local_to_world(shape: &Shape, local: Point) -> Point {
    let cx = shape.x + shape.w / 2.0;
    let cy = shape.y + shape.h / 2.0;
    let fx = if shape.flip_x { -1.0 } else { 1.0 };
    let fy = if shape.flip_y { -1.0 } else { 1.0 };
    let angle = shape.rot * PI / 180.0;
    let dx = fx * (local.x - shape.w / 2.0);
    let dy = fy * (local.y - shape.h / 2.0);
    Point {
        x: cx + dx * angle.cos() - dy * angle.sin(),
        y: cy + dx * angle.sin() + dy * angle.cos(),
    }
}

fn anchor_on_shape_at(shape: &Shape, side: &str, toward: Option<Point>) -> Point {
    let Some(toward) = toward else {
        return local_to_world(shape, side_local_point(shape, side));
    };
    let cx = shape.x + shape.w / 2.0;
    let cy = shape.y + shape.h / 2.0;
    let dx = toward.x - cx;
    let dy = toward.y - cy;
    let angle = -shape.rot * PI / 180.0;
    let rx = dx * angle.cos() - dy * angle.sin();
    let ry = dx * angle.sin() + dy * angle.cos();
    let lx = rx / if shape.flip_x { -1.0 } else { 1.0 } + shape.w / 2.0;
    let ly = ry / if shape.flip_y { -1.0 } else { 1.0 } + shape.h / 2.0;
    let clamp = |value: f64, low: f64, high: f64| value.max(low).min(high);
    let local = match side {
        "n" => Point {
            x: clamp(lx, 0.0, shape.w),
            y: 0.0,
        },
        "s" => Point {
            x: clamp(lx, 0.0, shape.w),
            y: shape.h,
        },
        "w" => Point {
            x: 0.0,
            y: clamp(ly, 0.0, shape.h),
        },
        _ => Point {
            x: shape.w,
            y: clamp(ly, 0.0, shape.h),
        },
    };
    local_to_world(shape, local)
}

fn side_local_point(shape: &Shape, side: &str) -> Point {
    match side {
        "n" => Point {
            x: shape.w / 2.0,
            y: 0.0,
        },
        "s" => Point {
            x: shape.w / 2.0,
            y: shape.h,
        },
        "w" => Point {
            x: 0.0,
            y: shape.h / 2.0,
        },
        _ => Point {
            x: shape.w,
            y: shape.h / 2.0,
        },
    }
}

fn side_vector_for(shape: &Shape, side: &str) -> Point {
    let base = match side {
        "n" => Point { x: 0.0, y: -1.0 },
        "e" => Point { x: 1.0, y: 0.0 },
        "s" => Point { x: 0.0, y: 1.0 },
        _ => Point { x: -1.0, y: 0.0 },
    };
    let angle = shape.rot * PI / 180.0;
    Point {
        x: (base.x * angle.cos() - base.y * angle.sin()) * if shape.flip_x { -1.0 } else { 1.0 },
        y: (base.x * angle.sin() + base.y * angle.cos()) * if shape.flip_y { -1.0 } else { 1.0 },
    }
}

fn end_direction(endpoint: &Endpoint, shapes: &BTreeMap<&str, &Shape>, fallback: Point) -> Point {
    endpoint_shape(endpoint, shapes)
        .map(|shape| side_vector_for(shape, endpoint.side.as_deref().unwrap_or("e")))
        .unwrap_or(fallback)
}

fn normalize(vector: Point) -> Point {
    let length = vector.x.hypot(vector.y).max(1e-12);
    Point {
        x: vector.x / length,
        y: vector.y / length,
    }
}

fn auto_route(
    route: &str,
    route_style: Option<u32>,
    from: Point,
    to: Point,
    direction_from: Point,
    direction_to: Point,
) -> (Vec<Point>, Option<(Point, Point)>) {
    if route == "curved" {
        let distance = (to.x - from.x).hypot(to.y - from.y);
        let k = (distance * 0.35).clamp(16.0, 90.0);
        return (
            Vec::new(),
            Some((
                Point {
                    x: from.x + direction_from.x * k,
                    y: from.y + direction_from.y * k,
                },
                Point {
                    x: to.x + direction_to.x * k,
                    y: to.y + direction_to.y * k,
                },
            )),
        );
    }
    if route != "elbow" {
        return (Vec::new(), None);
    }
    let route_style = route_style.unwrap_or(0);
    if route_style == 3 || route_style == 5 {
        let t = if route_style == 5 { 0.25 } else { 0.75 };
        let y = from.y + (to.y - from.y) * t;
        return (
            dedupe(vec![Point { x: from.x, y }, Point { x: to.x, y }]),
            None,
        );
    }
    if route_style == 4 || route_style == 6 {
        let t = if route_style == 6 { 0.1 } else { 0.9 };
        let x = from.x + (to.x - from.x) * t;
        return (
            dedupe(vec![Point { x, y: from.y }, Point { x, y: to.y }]),
            None,
        );
    }
    let ortho = |vector: Point| {
        if vector.x.abs() >= vector.y.abs() {
            'h'
        } else {
            'v'
        }
    };
    let a = ortho(direction_from);
    let b = ortho(direction_to);
    if a == 'h' && b == 'h' {
        let y = (from.y + to.y) / 2.0;
        return (
            dedupe(vec![Point { x: from.x, y }, Point { x: to.x, y }]),
            None,
        );
    }
    if a == 'v' && b == 'v' {
        let x = (from.x + to.x) / 2.0;
        return (
            dedupe(vec![Point { x, y: from.y }, Point { x, y: to.y }]),
            None,
        );
    }
    if a == 'h' {
        let x = (from.x + to.x) / 2.0;
        return (
            dedupe(vec![Point { x, y: from.y }, Point { x, y: to.y }]),
            None,
        );
    }
    let y = (from.y + to.y) / 2.0;
    (
        dedupe(vec![Point { x: from.x, y }, Point { x: to.x, y }]),
        None,
    )
}

fn dedupe(points: Vec<Point>) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    for point in points {
        if out
            .last()
            .is_none_or(|last| (point.x - last.x).hypot(point.y - last.y) > 0.01)
        {
            out.push(point);
        }
    }
    if out.len() == 1 {
        out.push(out[0]);
    }
    out
}

fn midpoint(from: Point, to: Point, waypoints: &[Point]) -> Point {
    if waypoints.is_empty() {
        return Point {
            x: (from.x + to.x) / 2.0,
            y: (from.y + to.y) / 2.0,
        };
    }
    // Walk half the routed length rather than picking the middle waypoint;
    // Visio places the connector label at the visual midpoint of the path.
    let mut points = Vec::with_capacity(waypoints.len() + 2);
    points.push(from);
    points.extend_from_slice(waypoints);
    points.push(to);
    let total: f64 = points
        .windows(2)
        .map(|pair| distance(pair[0], pair[1]))
        .sum();
    let mut remaining = total / 2.0;
    for pair in points.windows(2) {
        let length = distance(pair[0], pair[1]);
        if remaining <= length && length > 1e-9 {
            let t = remaining / length;
            return Point {
                x: pair[0].x + (pair[1].x - pair[0].x) * t,
                y: pair[0].y + (pair[1].y - pair[0].y) * t,
            };
        }
        remaining -= length;
    }
    points[points.len() / 2]
}

fn distance(a: Point, b: Point) -> f64 {
    ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
}

fn shape_text(shape: &Shape, shapes: &BTreeMap<&str, &Shape>) -> Option<SceneText> {
    if let Some(callout) = &shape.callout {
        if callout.kind == "icon" {
            return None;
        }
        let text = callout_text(shape, shapes);
        return (!text.is_empty()).then(|| text_scene(shape, &text));
    }
    if let Some(rich) = &shape.rich {
        let paragraphs = rich_paragraphs(rich, &shape.align, &shape.paragraph_spacing);
        if !paragraphs.is_empty() {
            return Some(text_scene_with_paragraphs(shape, paragraphs));
        }
    }
    if shape.label.is_empty() {
        return None;
    }
    Some(text_scene(shape, &shape.label))
}

fn callout_text(shape: &Shape, shapes: &BTreeMap<&str, &Shape>) -> String {
    let Some(callout) = &shape.callout else {
        return shape.label.clone();
    };
    let target = callout
        .target_visio_id
        .as_deref()
        .and_then(|id| shapes.get(id))
        .or_else(|| {
            shape
                .attached_to
                .as_deref()
                .and_then(|value| value.strip_prefix("shape:visio:"))
                .and_then(|id| shapes.get(id))
        });
    let Some(target) = target else {
        return "(missing target)".to_string();
    };
    let value = data_row_value(target, &callout.field);
    if callout.kind == "bar" && callout.show_value == Some(false) {
        return callout.label.clone();
    }
    let prefix = if callout.label.is_empty() {
        String::new()
    } else {
        format!("{} ", callout.label)
    };
    let text = format!("{prefix}{value}").trim().to_string();
    if text.is_empty() {
        if value.is_empty() {
            "…".to_string()
        } else {
            value
        }
    } else {
        text
    }
}

fn data_row_value(shape: &Shape, field: &str) -> String {
    shape
        .data
        .iter()
        .find(|row| row.name == field)
        .map(|row| row.value.clone())
        .unwrap_or_default()
}

fn data_color_fill(shape: &Shape) -> Option<String> {
    let color = shape.data_color.as_ref()?;
    let value = data_row_value(shape, &color.field)
        .trim()
        .parse::<f64>()
        .ok();
    let Some(value) = value else {
        return color.base_fill.clone();
    };
    let span = if (color.max - color.min).abs() < 1e-12 {
        0.0
    } else {
        (value - color.min) / (color.max - color.min)
    };
    Some(if span < 1.0 / 3.0 {
        color.low.clone()
    } else if span < 2.0 / 3.0 {
        color.mid.clone()
    } else {
        color.high.clone()
    })
}

/// Whether a text block's centre lies outside its shape's box (the label of an icon).
fn block_outside(shape: &Shape, cx: f64, cy: f64) -> bool {
    cx < shape.x || cx > shape.x + shape.w || cy < shape.y || cy > shape.y + shape.h
}

fn text_scene(shape: &Shape, text: &str) -> SceneText {
    let paragraphs = text
        .split('\n')
        .map(|line| SceneParagraph {
            align: shape.align.clone().unwrap_or_else(|| "center".to_string()),
            bullet: false,
            bullet_str: "•".to_string(),
            indent: 0.0,
            line_spacing: 0.0,
            space_before: 0.0,
            space_after: 0.0,
            indent_first: 0.0,
            runs: vec![SceneRun {
                text: line.to_string(),
                font_size: None,
                font_family: None,
                color: None,
                bold: None,
                italic: None,
                underline: None,
                strike: None,
                spacing: None,
            }],
        })
        .collect();
    text_scene_with_paragraphs(shape, paragraphs)
}

fn text_scene_with_paragraphs(shape: &Shape, paragraphs: Vec<SceneParagraph>) -> SceneText {
    let text_box = shape.text_box.as_ref();
    let value = |key: &str, fallback: f64| {
        text_box
            .and_then(|text_box| text_box.get(key))
            .and_then(Value::as_f64)
            .unwrap_or(fallback)
    };
    let (x, y, w, h) = if let Some(relative) = &shape.text_box_rel {
        let value = |key: &str, fallback: f64| {
            relative
                .get(key)
                .and_then(Value::as_f64)
                .unwrap_or(fallback)
        };
        (
            shape.x + value("x", 0.0) * shape.w,
            shape.y + value("y", 0.0) * shape.h,
            value("w", 1.0) * shape.w,
            value("h", 1.0) * shape.h,
        )
    } else {
        (
            shape.x + value("x", 0.0),
            shape.y + value("y", 0.0),
            value("w", shape.w),
            value("h", shape.h),
        )
    };
    SceneText {
        x,
        y,
        w,
        h,
        paragraphs,
        font_size: shape.font_size.unwrap_or(12.0),
        font_family: shape
            .font_family
            .clone()
            .unwrap_or_else(|| "\"Segoe UI\", Calibri, Helvetica, Arial, sans-serif".to_string()),
        color: shape
            .text_color
            .clone()
            .unwrap_or_else(|| "#1b2733".to_string()),
        bold: shape.bold.unwrap_or(false),
        italic: shape.italic.unwrap_or(false),
        underline: shape.underline.unwrap_or(false),
        v_align: shape
            .v_align
            .clone()
            .unwrap_or_else(|| "middle".to_string()),
        rot: 0.0,
        pad: false,
        outside: block_outside(shape, x + w / 2.0, y + h / 2.0),
    }
}

fn rich_paragraphs(rich: &Value, fallback_align: &Option<String>, spacing: &[[f64; 4]]) -> Vec<SceneParagraph> {
    let Some(paragraphs) = rich.get("paragraphs").and_then(Value::as_array) else {
        return Vec::new();
    };
    paragraphs
        .iter()
        .enumerate()
        .map(|(paragraph_index, paragraph)| {
            let spacing = spacing.get(paragraph_index);
            let runs = paragraph
                .get("runs")
                .and_then(Value::as_array)
                .map(|runs| {
                    runs.iter()
                        .map(|run| {
                            let style = run.get("style");
                            let caps = style.and_then(|style| style.get("caps")).and_then(Value::as_u64).unwrap_or(0);
                            SceneRun {
                                strike: style.and_then(|style| style.get("strike")).and_then(Value::as_bool),
                                spacing: style.and_then(|style| style.get("spacing")).and_then(Value::as_f64),
                                text: apply_case(
                                    run.get("text").and_then(Value::as_str).unwrap_or(""),
                                    caps,
                                ),
                                font_size: style
                                    .and_then(|style| style.get("fontSize"))
                                    .and_then(Value::as_f64),
                                font_family: style
                                    .and_then(|style| style.get("fontFamily"))
                                    .and_then(Value::as_str)
                                    .map(str::to_string),
                                color: style
                                    .and_then(|style| style.get("textColor"))
                                    .and_then(Value::as_str)
                                    .map(str::to_string),
                                bold: style
                                    .and_then(|style| style.get("bold"))
                                    .and_then(Value::as_bool),
                                italic: style
                                    .and_then(|style| style.get("italic"))
                                    .and_then(Value::as_bool),
                                underline: style
                                    .and_then(|style| style.get("underline"))
                                    .and_then(Value::as_bool),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            SceneParagraph {
                align: paragraph
                    .get("align")
                    .and_then(Value::as_str)
                    .or(fallback_align.as_deref())
                    .unwrap_or("center")
                    .to_string(),
                bullet: paragraph
                    .get("bullet")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                bullet_str: paragraph
                    .get("bulletStr")
                    .and_then(Value::as_str)
                    .unwrap_or("•")
                    .to_string(),
                indent: paragraph
                    .get("indent")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
                line_spacing: spacing.map_or(0.0, |s| s[0]),
                space_before: spacing.map_or(0.0, |s| s[1]),
                space_after: spacing.map_or(0.0, |s| s[2]),
                indent_first: spacing.map_or(0.0, |s| s[3]),
                runs,
            }
        })
        .collect()
}

/// A shape whose painted parts cover less than this share of its box is exported by Visio as the ink
/// of those parts: the empty box does not widen the picture.
const SLIVER_INK_RATIO: f64 = 0.75;
/// Along one axis, a shape whose ink covers less than this share of its box is exported by its ink.
const SLIVER_AXIS_RATIO: f64 = 0.5;

/// Padding (in ems) Visio leaves around the text of a shape that paints nothing.
const TEXT_INK_PAD_X: f64 = 0.5;
const TEXT_INK_PAD_TOP: f64 = 0.25;
const TEXT_INK_PAD_BOTTOM: f64 = 0.55;

/// Whether a geometry part leaves any mark: a fill, a gradient, a pattern or a visible line.
fn part_paints(part: &ScenePart) -> bool {
    (part.fill != "none" && part.fill_opacity > 0.0)
        || (part.stroke != "none" && part.stroke_width > 0.0 && part.stroke_opacity > 0.0)
        || part.fill_gradient.is_some()
        || part.fill_pattern.is_some()
}

/// The painted extent of one shape as (min_x, min_y, width, height) in scene px: its frame plus parts, pictures and shadows that reach outside it
/// (a group whose frame is a hairline, such as a 1-D rack-unit master, still shows its picture). The page build uses the same rule for the whole page.
pub fn shape_painted_bounds(shape: &SceneShape) -> (f64, f64, f64, f64) {
    let mut bounds = Bounds::new();
    bounds.add_shape(shape);
    bounds.finish()
}

struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Bounds {
    fn new() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        }
    }

    fn add(&mut self, x: f64, y: f64) {
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }

    fn add_shape(&mut self, shape: &SceneShape) {
        // Glyphs can poke above/below their text block (ascenders, descenders,
        // centred text in a tight box). Include a font-sized margin so the
        // viewport does not clip titles that sit at the top of the page.
        // A shape that paints nothing and holds only text is exported by Visio as the text itself,
        // not as its (empty) box.
        let text_only = shape.rot.abs() < 1e-9
            && shape.image.is_none()
            && shape.shadow.is_none()
            && (shape.text.is_some() || !shape.texts.is_empty())
            && !shape.parts.iter().any(part_paints);
        if text_only {
            for text in shape.text.iter().chain(shape.texts.iter()) {
                if text.rot.abs() > 1e-6 || shape.text_rot.abs() > 1e-6 {
                    self.add_text(text);
                    continue;
                }
                let (x0, y0, x1, y1) = crate::svg::text_ink_extent(text);
                // Visio pads the exported text by about half an em at the sides, a quarter em above
                // and a little over half an em below (measured on 8 pt and 12 pt blocks).
                let em = text.font_size;
                self.add(x0 - em * TEXT_INK_PAD_X, y0 - em * TEXT_INK_PAD_TOP);
                self.add(x1 + em * TEXT_INK_PAD_X, y1 + em * TEXT_INK_PAD_BOTTOM);
            }
            return;
        }
        self.add_shape_texts(shape);
        let pad = shape.stroke_width.max(0.0) / 2.0;
        // A shadow may extend beyond the shape box; include it so raster and
        // SVG viewports do not clip the offset/blur.
        // an inner shadow (type 3) is drawn inside the shape and adds nothing to the bounds
        let outer_shadow = shape.shadow.as_ref().filter(|shadow| shadow.kind != 3);
        let shadow_pad = outer_shadow
            .map(|shadow| shadow.blur.max(0.0) + shadow.dx.abs().max(shadow.dy.abs()))
            .unwrap_or(0.0);
        let effect_pad = shape
            .glow
            .as_ref()
            .map(|glow| glow.radius)
            .unwrap_or(0.0)
            .max(
                shape
                    .soft_edge
                    .as_ref()
                    .map(|edge| edge.radius)
                    .unwrap_or(0.0),
            );
        let reflection_extra = shape
            .reflection
            .as_ref()
            .map(|reflection| reflection.distance + reflection.blur + reflection.size * shape.h)
            .unwrap_or(0.0);
        let shadow_dx = outer_shadow.map(|shadow| shadow.dx).unwrap_or(0.0);
        let shadow_dy = outer_shadow.map(|shadow| shadow.dy).unwrap_or(0.0);
        if shape.rot.abs() < 1e-9 {
            // A group whose box is mostly empty (an invisible child, `NoLine` and `NoFill`, spans the
            // box while the visible parts cover a sliver) is exported by Visio as the ink of its
            // parts, so the box must not widen the picture.
            let mut ink: Option<(f64, f64, f64, f64)> = None;
            for part in &shape.parts {
                let paints = (part.fill != "none" && part.fill_opacity > 0.0)
                    || (part.stroke != "none" && part.stroke_width > 0.0 && part.stroke_opacity > 0.0)
                    || part.fill_gradient.is_some()
                    || part.fill_pattern.is_some();
                if let (true, Some((x0, y0, x1, y1))) = (paints, path_extent(&part.d)) {
                    ink = Some(match ink {
                        Some((a0, b0, a1, b1)) => (a0.min(x0), b0.min(y0), a1.max(x1), b1.max(y1)),
                        None => (x0, y0, x1, y1),
                    });
                }
            }
            let sliver = ink.is_some_and(|(x0, y0, x1, y1)| {
                shape.image.is_none()
                    && shape.w > 0.0
                    && shape.h > 0.0
                    && (x1 - x0).max(1.0) * (y1 - y0).max(1.0) < SLIVER_INK_RATIO * shape.w * shape.h
            });
            // A group whose geometry paints nothing at all (NoFill and NoLine) and that carries no
            // picture, text or effect is not part of the export.
            let paints_nothing = shape.image.is_none()
                && !shape.parts.is_empty()
                && !shape.parts.iter().any(part_paints)
                && shape.text.is_none()
                && shape.texts.is_empty()
                && shape.shadow.is_none()
                && shape.glow.is_none()
                && shape.soft_edge.is_none()
                && shape.reflection.is_none();
            if !sliver && !paints_nothing {
                // A box much longer than the ink along one axis (a 10 in wide line shape whose
                // only ink is two short ticks in the middle) does not stretch the picture that way.
                let (mut box_x0, mut box_x1) = (shape.x - pad, shape.x + shape.w + pad);
                let (mut box_y0, mut box_y1) = (shape.y - pad, shape.y + shape.h + pad);
                if let (Some((x0, y0, x1, y1)), true) = (ink, shape.image.is_none() && shape.w > 0.0 && shape.h > 0.0) {
                    if x1 - x0 < SLIVER_AXIS_RATIO * shape.w {
                        (box_x0, box_x1) = (shape.x + x0, shape.x + x1);
                    }
                    if y1 - y0 < SLIVER_AXIS_RATIO * shape.h {
                        (box_y0, box_y1) = (shape.y + y0, shape.y + y1);
                    }
                }
                self.add(box_x0, box_y0);
                self.add(box_x1, box_y1);
            }
            // Geometry may reach outside the shape box (the lead wires of a process-engineering
            // pump); Visio's export covers it.
            for part in &shape.parts {
                // Geometry with neither fill nor line (NoLine and NoFill) paints nothing.
                let paints_fill = part.fill != "none" && part.fill_opacity > 0.0;
                let paints_line = part.stroke != "none" && part.stroke_width > 0.0 && part.stroke_opacity > 0.0;
                if !paints_fill && !paints_line && part.fill_gradient.is_none() && part.fill_pattern.is_none() {
                    continue;
                }
                if let Some((x0, y0, x1, y1)) = path_extent(&part.d) {
                    // The part's own line reaches half its width beyond the geometry, which can be
                    // more than the shape's (a group has no line of its own).
                    let part_pad = if paints_line { pad.max(part.stroke_width / 2.0) } else { pad };
                    self.add(shape.x + x0 - part_pad, shape.y + y0 - part_pad);
                    self.add(shape.x + x1 + part_pad, shape.y + y1 + part_pad);
                }
            }
            if let Some(shadow) = shape.shadow.as_ref().filter(|shadow| shadow.kind == 2) {
                // an oblique shadow is the box tilted and scaled about its bottom edge (see `render_shadow`)
                let (sin, cos) = shadow.oblique.sin_cos();
                let k = shadow.scale.max(0.01);
                for (cx, cy) in [(0.0, 0.0), (shape.w, 0.0), (0.0, shape.h), (shape.w, shape.h)] {
                    let up = shape.h - cy;
                    self.add(
                        shape.x + cx + shadow.dx + k * sin * up - shadow.blur,
                        shape.y + shape.h + shadow.dy - k * cos * up - shadow.blur,
                    );
                    self.add(
                        shape.x + cx + shadow.dx + k * sin * up + shadow.blur,
                        shape.y + shape.h + shadow.dy - k * cos * up + shadow.blur,
                    );
                }
            }
            if outer_shadow.is_some() {
                self.add(
                    shape.x + shadow_dx - shadow_pad,
                    shape.y + shadow_dy - shadow_pad,
                );
                self.add(
                    shape.x + shape.w + shadow_dx + shadow_pad,
                    shape.y + shape.h + shadow_dy + shadow_pad,
                );
            }
            if effect_pad > 0.0 {
                self.add(shape.x - effect_pad, shape.y - effect_pad);
                self.add(
                    shape.x + shape.w + effect_pad,
                    shape.y + shape.h + effect_pad,
                );
            }
            if reflection_extra > 0.0 {
                self.add(shape.x, shape.y + shape.h + reflection_extra);
            }
            return;
        }
        let cx = shape.x + shape.w / 2.0;
        let cy = shape.y + shape.h / 2.0;
        let angle = shape.rot * PI / 180.0;
        for (x, y) in [
            (shape.x, shape.y),
            (shape.x + shape.w, shape.y),
            (shape.x + shape.w, shape.y + shape.h),
            (shape.x, shape.y + shape.h),
        ] {
            let dx = x - cx;
            let dy = y - cy;
            self.add(
                cx + dx * angle.cos() - dy * angle.sin() - pad,
                cy + dx * angle.sin() + dy * angle.cos() - pad,
            );
            self.add(
                cx + dx * angle.cos() - dy * angle.sin() + pad,
                cy + dx * angle.sin() + dy * angle.cos() + pad,
            );
        }
        // Painted geometry that reaches outside the box (CALOUT's dotted leader) also counts.
        for part in &shape.parts {
            let paints_fill = part.fill != "none" && part.fill_opacity > 0.0;
            let paints_line = part.stroke != "none" && part.stroke_width > 0.0 && part.stroke_opacity > 0.0;
            if !paints_fill && !paints_line && part.fill_gradient.is_none() && part.fill_pattern.is_none() {
                continue;
            }
            let Some((x0, y0, x1, y1)) = path_extent(&part.d) else { continue };
            let part_pad = if paints_line { pad.max(part.stroke_width / 2.0) } else { pad };
            for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
                let dx = shape.x + x - cx;
                let dy = shape.y + y - cy;
                let (rx, ry) = (cx + dx * angle.cos() - dy * angle.sin(), cy + dx * angle.sin() + dy * angle.cos());
                self.add(rx - part_pad, ry - part_pad);
                self.add(rx + part_pad, ry + part_pad);
            }
        }
        let rotated_pad = shadow_pad.max(effect_pad);
        if outer_shadow.is_some() || effect_pad > 0.0 {
            for x in [shape.x, shape.x + shape.w] {
                for y in [shape.y, shape.y + shape.h] {
                    let dx = x - cx;
                    let dy = y - cy;
                    for sign_x in [-1.0, 1.0] {
                        for sign_y in [-1.0, 1.0] {
                            self.add(
                                cx + dx * angle.cos() - dy * angle.sin() + sign_x * rotated_pad,
                                cy + dx * angle.sin() + dy * angle.cos() + sign_y * rotated_pad,
                            );
                        }
                    }
                }
            }
        }
    }

    fn add_text(&mut self, text: &SceneText) {
        // The declared box can be one line high while the content wraps to
        // several lines (Visio titles often do). Lay the text out exactly and
        // take the union of the box and the laid-out lines; only genuine
        // overflow grows the scene, so fitted text keeps Visio's content
        // bounds.
        let total = crate::svg::text_layout_height(text);
        let top = match text.v_align.as_str() {
            "top" => text.y,
            "bottom" => text.y + text.h - total,
            _ => text.y + (text.h - total) / 2.0,
        };
        let min_y = top.min(text.y);
        let max_y = (top + total).max(text.y + text.h);
        self.add(text.x, min_y);
        self.add(text.x + text.w, max_y);
    }

    fn add_shape_texts(&mut self, shape: &SceneShape) {
        // Rotated text (vertical swimlane labels) is rendered inside the
        // shape's rotation group; its unrotated box would wrongly stretch the
        // scene sideways. The shape box already covers it.
        if shape.rot.abs() > 1e-6 || shape.text_rot.abs() > 1e-6 {
            // A label that hangs outside a rotated shape's box (a scissors icon turned 90 degrees:
            // its text pin lies below the icon in local space, so on screen it is beside it) is
            // placed by rotating the block's centre with the shape. Count its ink there.
            if shape.rot.abs() > 1e-6 {
                let (scx, scy) = (shape.x + shape.w / 2.0, shape.y + shape.h / 2.0);
                let angle = shape.rot * PI / 180.0;
                for text in shape.text.iter().chain(shape.texts.iter()) {
                    let (x0, y0, x1, y1) = crate::svg::text_ink_extent(text);
                    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                    if cx >= shape.x && cx <= shape.x + shape.w && cy >= shape.y && cy <= shape.y + shape.h {
                        continue;
                    }
                    let (dx, dy) = (cx - scx, cy - scy);
                    let (rx, ry) = (scx + dx * angle.cos() - dy * angle.sin(), scy + dx * angle.sin() + dy * angle.cos());
                    let net = angle + text.rot;
                    let (w, h) = (x1 - x0, y1 - y0);
                    let hx = net.cos().abs() * w / 2.0 + net.sin().abs() * h / 2.0;
                    let hy = net.sin().abs() * w / 2.0 + net.cos().abs() * h / 2.0;
                    self.add(rx - hx, ry - hy);
                    self.add(rx + hx, ry + hy);
                }
            }
            return;
        }
        // Visio's export covers the text's ink, not its whole frame: a frame much wider than the
        // drawing (the label of a bus line) must not widen the picture. Fitted text lies inside the
        // shape box, so its ink changes nothing there.
        for text in shape.text.iter().chain(shape.texts.iter()) {
            if text.rot.abs() > 1e-6 {
                self.add_text(text);
                continue;
            }
            let (x0, y0, x1, y1) = crate::svg::text_ink_extent(text);
            self.add(x0, y0);
            self.add(x1, y1);
            if text.pad {
                let em = text.font_size;
                self.add(x0 - em * TEXT_INK_PAD_X, y0 - em * TEXT_INK_PAD_TOP);
                self.add(x1 + em * TEXT_INK_PAD_X, y1 + em * TEXT_INK_PAD_BOTTOM);
            }
            // A label that hangs below the shape (a bus's "W1") is exported with the same room
            // under its ink as a text-only shape gets.
            if y1 > shape.y + shape.h + 1.0 {
                self.add(x1, y1 + text.font_size * TEXT_INK_PAD_BOTTOM);
            }
            // Likewise a label that sticks out to the side or above (the "C" beside an ALARM
            // keypad icon) keeps its padding on that side.
            let em = text.font_size;
            if x1 > shape.x + shape.w + 1.0 {
                self.add(x1 + em * TEXT_INK_PAD_X, y0);
            }
            if x0 < shape.x - 1.0 {
                self.add(x0 - em * TEXT_INK_PAD_X, y0);
            }
            if y0 < shape.y - 1.0 {
                self.add(x0, y0 - em * TEXT_INK_PAD_TOP);
            }
        }
    }

    fn add_connector(&mut self, connector: &SceneConnector) {
        let pad = connector.stroke_width.max(0.0) / 2.0;
        self.add(connector.from.x - pad, connector.from.y - pad);
        self.add(connector.from.x + pad, connector.from.y + pad);
        self.add(connector.to.x - pad, connector.to.y - pad);
        self.add(connector.to.x + pad, connector.to.y + pad);
        for point in &connector.waypoints {
            self.add(point.x - pad, point.y - pad);
            self.add(point.x + pad, point.y + pad);
        }
        if !connector.label.is_empty() {
            let lines: Vec<&str> = connector
                .label
                .split(['\n', '\r'])
                .filter(|line| !line.is_empty())
                .collect();
            let longest = lines
                .iter()
                .map(|line| line.chars().count())
                .max()
                .unwrap_or(0);
            let width = longest as f64 * 6.2;
            let height = lines.len().max(1) as f64 * 12.5;
            self.add(
                connector.label_point.x - width / 2.0,
                connector.label_point.y - height,
            );
            self.add(
                connector.label_point.x + width / 2.0,
                connector.label_point.y + height / 2.0,
            );
        }
    }

    fn finish(self) -> (f64, f64, f64, f64) {
        if !self.min_x.is_finite() {
            return (0.0, 0.0, 1.0, 1.0);
        }
        (
            self.min_x,
            self.min_y,
            (self.max_x - self.min_x).max(1.0),
            (self.max_y - self.min_y).max(1.0),
        )
    }
}
