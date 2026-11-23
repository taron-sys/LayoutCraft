//! Headless scene, SVG and `resvg` raster renderer for LayoutCraft documents.
//!
//! The model importer is independent of this crate. `scene_from_document`
//! flattens one page into renderer-independent primitives; `to_svg` and
//! `render_png` share that scene so interactive/export/visual tests can use one
//! path once the desktop shell is built.
pub mod png;
pub mod scene;
pub mod svg;
mod bevel_table;
mod text;

pub use png::{render_png, render_rgb};
pub use scene::{
    scene_from_document, scene_from_document_with_media, scene_from_document_with_options, shape_painted_bounds, SceneOptions, Point, Scene, SceneConnector, SceneImage,
    SceneHop, SceneItem, SceneParagraph, ScenePart, SceneRun, SceneShape, SceneText,
};
pub use svg::{to_svg, to_svg_scaled};
