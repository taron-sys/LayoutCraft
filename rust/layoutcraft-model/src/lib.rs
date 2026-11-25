//! Typed portable document model for LayoutCraft.
//!
//! The canonical golden models and the editor's JSON interchange use the same
//! field names. Unknown fields are preserved in each structure's `extra` map so
//! a reader never silently discards a newer or extension field.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

pub const SCHEMA_VERSION: u32 = 1;

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

fn default_version() -> u32 {
    1
}

pub type Extra = BTreeMap<String, Value>;

#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Error(value.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error(value.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Counts {
    pub pages: usize,
    pub shapes: usize,
    pub connectors: usize,
    pub media: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub page_index: usize,
    pub pages: Vec<Page>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub media: BTreeMap<String, Media>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub shapes: Vec<Shape>,
    #[serde(default)]
    pub connectors: Vec<Connector>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub containers: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visio: Option<Value>,
    /// Page layers from the PageSheet `Layer` section (index = Visio row IX).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
    /// Ruler guides (`Type="Guide"` shapes). Not drawn; kept for the editor.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<Guide>,
    /// PageSheet line-jump settings (`LineJumpCode/Style/FactorX/FactorY`), when the page has them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_jump: Option<LineJump>,
    /// Paper/internal drawing-unit ratio. `1` for standard 1:1 pages.
    #[serde(
        default = "default_page_scale",
        skip_serializing_if = "page_scale_is_one"
    )]
    pub scale: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A stretch of a connector that Visio drew as a hop over a crossing connector, in page pixels
/// (top-down). Without `apex` and `via` it is a straight line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorHop {
    pub from: [f64; 2],
    pub to: [f64; 2],
    /// Corner points of a polyline hop, between `from` and `to`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<[f64; 2]>,
    /// Highest point of an arc hop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apex: Option<[f64; 2]>,
    /// The pen is lifted across the crossing (line-jump style "gap": a `MoveTo` row).
    #[serde(default, skip_serializing_if = "is_false")]
    pub gap: bool,
}

/// Where connectors that cross hop over each other. `code`: 0/6 never, 1/7 horizontal lines hop,
/// 2 vertical lines hop, 3/4 the connector routed last hops, 5 the one routed first hops.
/// `style`: 0/1 arc, 2 gap, 3 square, 4..=8 polygons with 2..=6 sides. The hop is
/// `2.5 mm * factor_x` wide and `1.25 mm * factor_y` high.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineJump {
    pub code: i32,
    pub style: i32,
    pub factor_x: f64,
    pub factor_y: f64,
}

impl Default for LineJump {
    /// Visio's page defaults: horizontal lines hop, arcs, 2/3 of the standard size.
    fn default() -> Self {
        Self { code: 1, style: 0, factor_x: 2.0 / 3.0, factor_y: 2.0 / 3.0 }
    }
}

/// A Visio page layer. A shape is drawn when it has no layers or at least one
/// of its layers is visible. `print = false` only affects printing/export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub index: u32,
    pub name: String,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default = "default_true")]
    pub print: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

fn default_true() -> bool {
    true
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// A ruler guide in Visio page inches (origin bottom-left, Y up).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Guide {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visio_id: Option<String>,
    pub x_in: f64,
    pub y_in: f64,
    /// Radians; `0` is horizontal.
    #[serde(default)]
    pub angle: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

fn default_page_scale() -> f64 {
    1.0
}

fn page_scale_is_one(value: &f64) -> bool {
    (*value - 1.0).abs() < 1e-9
}

/// A Visio `FillPattern` 2-40 entry: a two-colour bitmap pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillPattern {
    pub kind: u32,
    pub fore: String,
    pub back: String,
}

/// Outer shadow resolved from `ShapeShdw*` cells. Offsets are pixels in the
/// page's top-down coordinate space.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shadow {
    pub dx: f64,
    pub dy: f64,
    #[serde(default)]
    pub blur: f64,
    pub color: String,
    #[serde(default = "default_shadow_opacity")]
    pub opacity: f64,
    /// 1 = simple, 2 = oblique, 3 = inner/page default.
    #[serde(default = "default_shadow_kind")]
    pub kind: u8,
    #[serde(default = "default_shadow_opacity")]
    pub scale: f64,
    /// Visio `ShapeShdwObliqueAngle` in radians.
    #[serde(default)]
    pub oblique: f64,
}

fn default_shadow_kind() -> u8 {
    1
}

fn default_shadow_opacity() -> f64 {
    1.0
}

/// Top/bottom bevel selected by `BevelTopType`/`BevelBottomType`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bevel {
    pub kind: u32,
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub bottom: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Glow {
    pub radius: f64,
    pub color: String,
    #[serde(default = "default_shadow_opacity")]
    pub opacity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoftEdge {
    pub radius: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reflection {
    /// Fraction of the shape height that is mirrored.
    pub size: f64,
    pub distance: f64,
    pub blur: f64,
    #[serde(default = "default_shadow_opacity")]
    pub opacity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visio_id: Option<String>,
    #[serde(rename = "type")]
    pub shape_type: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(default)]
    pub rot: f64,
    /// Extra text-block rotation from `TxtAngle`, in radians, in the same
    /// screen-space convention as `rot`. Visio's electrical stencils set
    /// `TxtAngle = -Angle` so labels stay upright when the symbol is rotated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_rot: Option<f64>,
    /// Corner radius from `Rounding`, in pixels. Rounded stencil frames (UML
    /// states, containers, flowchart terminators) use this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rounding: Option<f64>,
    #[serde(default)]
    pub flip_x: bool,
    #[serde(default)]
    pub flip_y: bool,
    #[serde(default)]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v_align: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<ShapePart>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub_texts: Vec<SubText>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data: Vec<DataRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_box: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_box_rel: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rich: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<Image>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hyperlinks: Vec<Value>,
    /// Alt text (Visio's `User.visAltDescription`), for assistive technology.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_fit: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callout: Option<DataGraphic>,
    /// Visio-native data graphic instance (callout, heading, bar, icon set).
    /// Metadata only: the shape keeps Visio's cached geometry and is not
    /// regenerated from this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_data_graphic: Option<NativeDataGraphic>,
    /// Page layer indices from `LayerMember` (`0;2`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<u32>,
    /// True when every layer the shape belongs to is hidden: not drawn.
    #[serde(default, skip_serializing_if = "is_false")]
    pub layer_hidden: bool,
    /// The colour of the first layer of the shape that has one (the layer's `Color` cell): Visio then draws the shape's lines and
    /// text in it and its fill white. NOT serialized: the golden model JSON stays as it was; the renderer reads the in-memory model.
    #[serde(default, skip_serializing)]
    pub layer_color: Option<String>,
    /// Per paragraph of `rich`: `[line spacing, space before, space after, first-line indent]` (`SpLine` as a negative multiple or a
    /// positive pixel distance, the spaces and `IndFirst` in pixels); empty when no paragraph differs from single spacing. NOT serialized: the golden model JSON
    /// stays as it was; the renderer reads the in-memory model.
    #[serde(default, skip_serializing)]
    pub paragraph_spacing: Vec<[f64; 4]>,
    /// The text block's width is `TEXTWIDTH(TheText)` (own or inherited from the master: a saved instance carries only `Inh`), so the text is one
    /// line wide by definition and the renderer never wraps it. NOT serialized: `text_fit` is compared with the golden models and keeps its meaning.
    #[serde(default, skip_serializing)]
    pub text_fits_line: bool,
    /// The first `front_parts` entries of `parts` are a group's OWN geometry that Visio draws IN FRONT of the members' parts that follow
    /// (the group's `DisplayMode` cell is absent or 2; step 219). NOT serialized: the golden models keep the order they have, the renderer
    /// draws these parts last.
    #[serde(default, skip_serializing)]
    pub front_parts: usize,
    /// True when every layer the shape belongs to has printing switched off.
    /// Drawn on screen, left out of print/export renders (Visio's `Page.Export`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub layer_nonprinting: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_color: Option<DataColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attached_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<Shadow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bevel: Option<Bevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glow: Option<Glow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soft_edge: Option<SoftEdge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reflection: Option<Reflection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub containers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list_of: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_info: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_kind: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Text carried by a master-body child (UML `PK`/`FK` markers, list labels).
/// Coordinates are local to the owning shape, in pixels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubText {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub text: String,
    pub font_size: f64,
    pub font_family: String,
    pub color: String,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub underline: bool,
    pub align: String,
    pub v_align: String,
    /// The child that owns this text paints nothing else: Visio exports such a label with room
    /// around its ink, like a text-only shape.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pad: bool,
    /// Left indent of the paragraph (`Para.IndLeft`), pixels.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub indent: f64,
    /// Extra rotation of the text block from the child's `TxtAngle`, radians, screen
    /// direction (clockwise positive), about the block centre.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub text_rot: f64,
    /// The block's width is `TEXTWIDTH(TheText)` (own or the master child's): one line wide by definition, never wrapped by the renderer.
    /// NOT serialized: the golden model JSON stays as it was.
    #[serde(default, skip_serializing)]
    pub fit_line: bool,
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapePart {
    pub d: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_gradient: Option<Value>,
    /// Line gradient stops/angle, same shape as `fill_gradient`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_gradient: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_pattern: Option<FillPattern>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom_edge: Option<Value>,
    /// Arrowhead at the start / end of an open path (a line-shaped master child or shape).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub begin_arrow: Option<LineEnd>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_arrow: Option<LineEnd>,
    /// Where the part came from, so a style edit can be written back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PartSource>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A Visio line end: `BeginArrow`/`EndArrow` type with its size index (`BeginArrowSize`/`EndArrowSize`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LineEnd {
    pub kind: u32,
    pub size: u32,
}

/// Provenance of a part composited from a master-body child.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartSource {
    /// Shape ID of the master child (`MasterShape` on the page-level child).
    pub master_shape_id: String,
    /// Visio ID of the page-level child that overrides it, when one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_shape_id: Option<String>,
    /// `fill`, `stroke` or `tint` (which paint the part carries).
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataRow {
    pub name: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub r#type: u32,
    #[serde(default)]
    pub invisible: u32,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreign_type: Option<String>,
    #[serde(default)]
    pub offset_x: f64,
    #[serde(default)]
    pub offset_y: f64,
    #[serde(default = "one")]
    pub scale_x: f64,
    #[serde(default = "one")]
    pub scale_y: f64,
    #[serde(default)]
    pub brightness: f64,
    #[serde(default)]
    pub contrast: f64,
    #[serde(default = "one")]
    pub gamma: f64,
    #[serde(default)]
    pub transparency: f64,
    #[serde(default)]
    pub unsupported: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataGraphic {
    #[serde(default)]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_set: Option<String>,
    #[serde(default)]
    pub field: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub position: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<f64>,
    #[serde(default)]
    pub auto_size: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_value: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_visio_id: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Metadata recognised on a Visio-native data graphic instance (master type 34
/// with `User.msvCalloutType`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeDataGraphic {
    /// `text`, `heading`, `bar` or `icon`.
    pub kind: String,
    /// Master name, e.g. `Text callout`, `Data bar 2`.
    #[serde(default)]
    pub master: String,
    /// Visio shape ID of the shape the graphic reads from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_visio_id: Option<String>,
    /// Shape Data row name the graphic displays.
    #[serde(default)]
    pub field: String,
    #[serde(default)]
    pub label: String,
    /// Current formatted value as cached by Visio.
    #[serde(default)]
    pub value: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataColor {
    pub field: String,
    pub min: f64,
    pub max: f64,
    pub low: String,
    pub mid: String,
    pub high: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_fill: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ext: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adjust: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connector {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visio_id: Option<String>,
    #[serde(default)]
    pub r#from: Endpoint,
    #[serde(default)]
    pub to: Endpoint,
    #[serde(default)]
    pub route: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waypoints: Vec<Value>,
    /// Line-jump hops stored in the connector's geometry (`ArcTo` / `PolylineTo` rows).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hops: Vec<ConnectorHop>,
    #[serde(default)]
    pub label: String,
    /// Visio connector text-pin position in page pixels (top-down).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_point: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrow_start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrow_end: Option<String>,
    /// The Visio line-end type and size index of the begin / end of the connector when it is one of the gallery
    /// kinds the coarse `arrow_start` / `arrow_end` names cannot tell apart (see `docs/27`); absent otherwise. NOT serialized: the golden model JSON stays as it was; the
    /// renderer reads the in-memory model.
    #[serde(default, skip_serializing)]
    pub begin_line_end: Option<LineEnd>,
    #[serde(default, skip_serializing)]
    pub end_line_end: Option<LineEnd>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_style: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub con_fixed_code: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_route_ext: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walk_preference: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plow_code: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glue_type: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dyn_feedback: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub containers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z: Option<f64>,
    /// Where the connector sits in the common drawing order of shapes and connectors (just above the shapes that precede it in
    /// the shape tree). NOT serialized: the golden models keep the connector counter in `z`; the renderer reads this one.
    #[serde(default, skip_serializing)]
    pub order_z: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visio: Option<Value>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<Value>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Document {
    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        Ok(serde_json::from_slice(bytes)?)
    }

    pub fn from_json(text: &str) -> Result<Self> {
        Ok(serde_json::from_str(text)?)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = fs::read(path.as_ref())?;
        Self::from_slice(&bytes)
    }

    pub fn to_json_pretty(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version > SCHEMA_VERSION {
            return Err(Error(format!(
                "document schema {} is newer than supported {}",
                self.schema_version, SCHEMA_VERSION
            )));
        }
        if self.pages.is_empty() {
            return Err(Error("document has no pages".to_string()));
        }
        for (index, page) in self.pages.iter().enumerate() {
            if page.name.is_empty() {
                return Err(Error(format!("page {index} has no name")));
            }
        }
        Ok(())
    }

    pub fn counts(&self) -> Counts {
        Counts {
            pages: self.pages.len(),
            shapes: self.pages.iter().map(|p| p.shapes.len()).sum(),
            connectors: self.pages.iter().map(|p| p.connectors.len()).sum(),
            media: self.media.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_minimal_document() {
        let doc = Document::from_json(
            r#"{"schemaVersion":1,"version":1,"pages":[{"name":"P","shapes":[{"type":"rect","x":0,"y":0,"w":10,"h":20}],"connectors":[]}]}"#,
        )
        .unwrap();
        doc.validate().unwrap();
        assert_eq!(
            doc.counts(),
            Counts {
                pages: 1,
                shapes: 1,
                connectors: 0,
                media: 0
            }
        );
    }

    #[test]
    fn rejects_a_newer_schema() {
        let doc = Document::from_json(
            r#"{"schemaVersion":2,"version":1,"pages":[{"name":"P","shapes":[],"connectors":[]}]}"#,
        )
        .unwrap();
        assert!(doc.validate().is_err());
    }

    #[test]
    fn preserves_unknown_fields() {
        let doc = Document::from_json(
            r#"{"schemaVersion":1,"version":1,"futureTop":42,"pages":[{"name":"P","futurePage":true,"shapes":[],"connectors":[]}]}"#,
        )
        .unwrap();
        assert_eq!(doc.extra.get("futureTop"), Some(&serde_json::json!(42)));
        assert_eq!(
            doc.pages[0].extra.get("futurePage"),
            Some(&serde_json::json!(true))
        );
    }
}
