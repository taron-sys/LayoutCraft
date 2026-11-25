//! What the host provides to the evaluator.
//!
//! The crate knows nothing about `.vsdx` packages: cells, themes, palettes and shape properties all
//! come from the host through these traits.

use crate::ast::CellRef;
use crate::error::ErrorValue;
use crate::value::{Rgb, Value};

/// Which theme value `THEMEVAL` / `THEME` asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThemeKey {
    /// No argument: the themed value of the cell holding the formula.
    HostCell,
    /// A named value such as `"FillColor"`, `"Dark"`, `"AccentColor2"`.
    Name(String),
    /// An index into the colour scheme (1 to 8, documented for `THEMEVAL`).
    ColorIndex(u8),
}

/// Whether a cell has a formula, and where it comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormulaKind {
    /// The cell exists and holds only a constant.
    Absent,
    /// The formula is inherited from a master or style.
    Inherited,
    /// The formula is set on the shape itself.
    Local,
}

/// Context-free services. Every method has a default that says "not available"; the functions that
/// need one then evaluate to [`ErrorValue::Unsupported`] (or the function's documented fallback).
pub trait Services {
    /// The colour at an index of the document palette.
    fn palette_color(&self, _index: i64) -> Option<Rgb> {
        None
    }

    /// A theme value; `None` when the document has no theme or the key is unknown.
    fn theme_value(&self, _key: &ThemeKey) -> Option<Value> {
        None
    }

    /// Whether the shape being evaluated has a theme applied.
    fn is_themed(&self) -> Option<bool> {
        None
    }

    /// The ID of the shape being evaluated.
    fn shape_id(&self) -> Option<u32> {
        None
    }

    fn shape_name(&self) -> Option<String> {
        None
    }

    fn page_name(&self) -> Option<String> {
        None
    }

    /// The master name: `None` when unknown, `Some(None)` for a shape that has no master.
    fn master_name(&self) -> Option<Option<String>> {
        None
    }

    fn is_1d(&self) -> Option<bool> {
        None
    }

    /// Width and height in inches of `text` laid out in the evaluated shape, wrapped at
    /// `max_width` when given.
    fn text_size(&self, _text: &str, _max_width: Option<f64>) -> Option<(f64, f64)> {
        None
    }

    /// A random number in `0.0..1.0`. The engine itself never calls a clock or a random source.
    fn random(&self) -> Option<f64> {
        None
    }
}

/// Services that provide nothing.
pub struct NoServices;

impl Services for NoServices {}

/// What [`crate::evaluate`] needs: cell lookup plus the services.
pub trait Env {
    /// The value of the referenced cell. An unknown cell is `Err(ErrorValue::Ref)`.
    fn cell(&self, reference: &CellRef) -> Result<Value, ErrorValue>;

    /// Whether the referenced cell has a (local or inherited) formula.
    fn formula_kind(&self, _reference: &CellRef) -> Result<FormulaKind, ErrorValue> {
        Err(ErrorValue::Unsupported)
    }

    fn services(&self) -> &dyn Services {
        &NoServices
    }
}
