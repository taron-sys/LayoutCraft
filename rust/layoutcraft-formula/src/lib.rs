//! ShapeSheet-style cell formulas: parse, evaluate with units, and recalculate a graph of cells.
//!
//! The crate knows nothing about `.vsdx` packages. A host (the document model) supplies cells and
//! services through the [`Env`] / [`Host`] traits; this crate supplies the language.
//!
//! # What it implements, and from which public sources
//!
//! * Formula syntax and the set of function names: \[MS-VSDX\] "Formula Expressions and
//!   Evaluation" and section 2.5.1 "Formula ABNF and Full Grammar Definition",
//!   <https://learn.microsoft.com/en-us/openspecs/sharepoint_protocols/ms-vsdx/e617d7e0-b0d9-4019-890c-cb4de0e3c6bf>
//!   (also the pages "Formula Expression", "Reference Tokens", "Function Tokens").
//! * Operators, precedence and operand conversion: Microsoft Learn, "About Operators",
//!   <https://learn.microsoft.com/en-us/office/client-developer/visio/about-operators>.
//! * Units of measure, implicit units, internal units: "About Units of Measure",
//!   <https://learn.microsoft.com/en-us/office/client-developer/visio/about-units-of-measure-visio-shapesheet-reference>.
//! * Cell references: "About Cell References",
//!   <https://learn.microsoft.com/en-us/office/client-developer/visio/about-cell-references>.
//! * Error values: "About Error Values",
//!   <https://learn.microsoft.com/en-us/office/client-developer/visio/about-error-values>.
//! * Dates and durations: "About Date, Time, and Duration Values",
//!   <https://learn.microsoft.com/en-us/office/client-developer/visio/about-date-time-and-duration-values>.
//! * Function behaviour: the per-function pages under "Functions (Visio ShapeSheet Reference)",
//!   <https://learn.microsoft.com/en-us/office/client-developer/visio/functions-visio-shapesheet-reference>.
//!
//! Where those pages are silent the code says `Unspecified in the sources:` and `docs/46-formula-engine.md`
//! lists the choice made.
//!
//! # Shape of the API
//!
//! * [`parse`] turns text into an [`Expr`] with byte spans, or a [`ParseError`].
//! * [`evaluate`] runs an [`Expr`] against an [`Env`] and returns a [`Value`] (errors are values).
//! * [`Engine`] keeps many cells, their dependency graph and their values, and recalculates only
//!   what an edit can affect.
//! * [`functions`] lists every known function with its [`Status`].

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod ast;
mod engine;
mod env;
mod error;
mod eval;
mod functions;
mod lexer;
mod parser;
mod unit;
mod value;

pub use ast::{
    BinaryOp, CellRef, CmpOp, Container, Expr, ExprKind, Precision, SheetRef, SheetTarget, UnaryOp,
};
pub use engine::{Engine, Host, Report};
pub use env::{Env, FormulaKind, NoServices, Services, ThemeKey};
pub use error::{ErrorValue, ParseError, ParseErrorKind, Span};
pub use eval::{evaluate, evaluate_str, Evaluation};
pub use functions::{function, functions, FuncInfo, Status};
pub use parser::parse;
pub use unit::{Dim, Unit};
pub use value::{format_number, Quantity, Rgb, Value};

/// Longest formula text accepted, in bytes.
pub const MAX_FORMULA_BYTES: usize = 64 * 1024;

/// Tallest syntax tree accepted (a leaf has height 1). Bounds recursion in the evaluator and when
/// the tree is dropped.
pub const MAX_DEPTH: usize = 256;

/// Deepest nesting of parentheses, calls and prefix operators while parsing. Bounds the parser's
/// own recursion.
pub const MAX_NESTING: usize = 64;

/// Longest string a formula can build, in bytes (characters for the editing functions).
pub const MAX_STRING_BYTES: usize = 1 << 20;
