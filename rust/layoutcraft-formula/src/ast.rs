//! The syntax tree. Every node carries the byte span it was parsed from.

use std::collections::HashSet;
use std::fmt;

use crate::error::Span;
use crate::unit::Unit;
use crate::value::Rgb;

/// The object a reference starts from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Container {
    /// No qualifier: the sheet holding the formula (or the named shape in the same page/master).
    Current,
    /// `ThePage!`: the page or master page the shape is on.
    ThePage,
    /// `TheDoc!`
    TheDoc,
    /// `Styles!Name!Cell`
    Styles,
    /// `Pages[Name]!`
    Page(String),
    /// `Masters[Name]!`
    Master(String),
}

/// Which sheet inside the container a reference names.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SheetTarget {
    /// No shape qualifier: the container's own sheet (the current shape for `Current`).
    Itself,
    /// `Sheet.5!`: a sheet by ID.
    Id(u32),
    /// `Name!`: a sheet by name.
    Name(String),
    /// `Name.5!`: a sheet by name and ID, for names that occur more than once.
    NamedId { name: String, id: u32 },
}

/// The sheet qualifier of a reference.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SheetRef {
    pub container: Container,
    pub target: SheetTarget,
}

impl SheetRef {
    /// The unqualified reference: a cell of the shape holding the formula.
    pub fn local() -> Self {
        SheetRef {
            container: Container::Current,
            target: SheetTarget::Itself,
        }
    }
}

/// A reference to a cell, as written. Resolving it to an actual cell is the host's job.
///
/// `path` holds the dot-separated names after the sheet qualifier: `["Width"]`,
/// `["Geometry1", "X1"]`, `["Prop", "Cost"]`, `["Char", "Size"]` with `index == Some(2)` for
/// `Char.Size[2]`, `["Scratch", "A1"]`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CellRef {
    pub sheet: SheetRef,
    pub path: Vec<String>,
    pub index: Option<u32>,
}

/// Whether `name` can be written without quotes.
fn is_plain_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

fn write_name(f: &mut fmt::Formatter<'_>, name: &str) -> fmt::Result {
    if is_plain_name(name) {
        f.write_str(name)
    } else {
        write!(f, "'{}'", name.replace('\'', "''"))
    }
}

impl fmt::Display for CellRef {
    /// The canonical spelling, which parses back to an equal reference.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.sheet.container {
            Container::Current => {}
            Container::ThePage => f.write_str("ThePage!")?,
            Container::TheDoc => f.write_str("TheDoc!")?,
            Container::Styles => f.write_str("Styles!")?,
            Container::Page(name) => write!(f, "Pages[{name}]!")?,
            Container::Master(name) => write!(f, "Masters[{name}]!")?,
        }
        match &self.sheet.target {
            SheetTarget::Itself => {}
            SheetTarget::Id(id) => write!(f, "Sheet.{id}!")?,
            SheetTarget::Name(name) => {
                write_name(f, name)?;
                f.write_str("!")?;
            }
            SheetTarget::NamedId { name, id } => write!(f, "{name}.{id}!")?,
        }
        for (position, part) in self.path.iter().enumerate() {
            if position > 0 {
                f.write_str(".")?;
            }
            f.write_str(part)?;
        }
        if let Some(index) = self.index {
            write!(f, "[{index}]")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Plus,
    Minus,
    /// Postfix `%`.
    Percent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// The symbolic comparison operators (`=`, `<`, ...) tolerate floating-point noise; the word forms
/// (`EQ`, `LT`, ...) compare exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precision {
    Tolerant,
    Exact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Concat,
    Compare(CmpOp, Precision),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    /// A number with an optional unit suffix, as written (not yet converted to internal units).
    Number {
        value: f64,
        unit: Option<Unit>,
    },
    Str(String),
    Bool(bool),
    Color(Rgb),
    Ref(CellRef),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// A function call. `name` is as written; lookup ignores case.
    Call {
        name: String,
        name_span: Span,
        args: Vec<Expr>,
    },
    /// An omitted argument, as in `INDEX(54, ";1;2", , "x")`.
    Empty,
}

impl Expr {
    /// Calls `visit` for every cell reference in the expression, in source order.
    pub fn for_each_ref<'a>(&'a self, visit: &mut impl FnMut(&'a CellRef)) {
        match &self.kind {
            ExprKind::Ref(reference) => visit(reference),
            ExprKind::Unary { operand, .. } => operand.for_each_ref(visit),
            ExprKind::Binary { lhs, rhs, .. } => {
                lhs.for_each_ref(visit);
                rhs.for_each_ref(visit);
            }
            ExprKind::Call { args, .. } => {
                for arg in args {
                    arg.for_each_ref(visit);
                }
            }
            ExprKind::Number { .. }
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Color(_)
            | ExprKind::Empty => {}
        }
    }

    /// All distinct cell references, in order of first use.
    pub fn references(&self) -> Vec<&CellRef> {
        let mut seen: HashSet<&CellRef> = HashSet::new();
        let mut found: Vec<&CellRef> = Vec::new();
        self.for_each_ref(&mut |reference| {
            if seen.insert(reference) {
                found.push(reference);
            }
        });
        found
    }
}
