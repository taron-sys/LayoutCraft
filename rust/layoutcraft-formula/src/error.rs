//! Error types: parse errors with byte spans, and the error *values* a cell can hold.

use std::fmt;

/// A byte range in the formula text (`start..end`, end exclusive).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }

    /// The smallest span that covers both.
    pub fn to(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// Why a formula could not be parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// The formula is empty (or only white space).
    Empty,
    /// The formula is longer than [`crate::MAX_FORMULA_BYTES`].
    TooLong {
        max: usize,
    },
    /// The expression is deeper than [`crate::MAX_DEPTH`].
    TooDeep {
        max: usize,
    },
    /// A character that cannot start any token.
    UnexpectedChar(char),
    UnterminatedString,
    UnterminatedQuotedName,
    UnterminatedBracket,
    /// A number that is malformed or does not fit a finite `f64`.
    BadNumber,
    /// A unit suffix that is not one of the documented abbreviations.
    UnknownUnit(String),
    /// `#` that is not followed by six hexadecimal digits.
    BadColor,
    /// The parser needed something else here (`what` names it).
    Expected {
        what: &'static str,
    },
    /// Input remains after a complete expression.
    TrailingInput,
    /// A malformed cell reference (`why` says what is wrong).
    BadReference {
        why: &'static str,
    },
}

/// A parse failure with the byte range it applies to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub span: Span,
}

impl ParseError {
    pub(crate) fn new(kind: ParseErrorKind, span: Span) -> Self {
        ParseError { kind, span }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ParseErrorKind::*;
        match &self.kind {
            Empty => write!(f, "empty formula")?,
            TooLong { max } => write!(f, "formula is longer than {max} bytes")?,
            TooDeep { max } => write!(f, "expression is nested deeper than {max} levels")?,
            UnexpectedChar(c) => write!(f, "unexpected character {c:?}")?,
            UnterminatedString => write!(f, "string literal is not closed")?,
            UnterminatedQuotedName => write!(f, "quoted name is not closed")?,
            UnterminatedBracket => write!(f, "'[' is not closed")?,
            BadNumber => write!(f, "malformed number")?,
            UnknownUnit(unit) => write!(f, "unknown unit {unit:?}")?,
            BadColor => write!(f, "a colour is '#' followed by six hexadecimal digits")?,
            Expected { what } => write!(f, "expected {what}")?,
            TrailingInput => write!(f, "unexpected input after the end of the expression")?,
            BadReference { why } => write!(f, "malformed cell reference: {why}")?,
        }
        write!(f, " (bytes {}..{})", self.span.start, self.span.end)
    }
}

impl std::error::Error for ParseError {}

/// The error values a cell can evaluate to.
///
/// The first six are the documented ones (see `docs/46-formula-engine.md`). `Unsupported` and
/// `Circular` are this engine's own: the public sources document neither a name nor a display
/// text for them, so they must never be written to a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorValue {
    /// `#DIV/0!` division by zero.
    DivZero,
    /// `#VALUE!` an argument or operand of the wrong type.
    Value,
    /// `#REF!` a reference to a cell that does not exist.
    Ref,
    /// `#NUM!` an invalid number (square root of a negative, overflow, NaN).
    Num,
    /// `#N/A!` a value that is not available.
    Na,
    /// `#DIM!` a dimensional value outside the exponent range or used with an unsuitable operation.
    Dim,
    /// The function is known by name but this engine does not evaluate it.
    Unsupported,
    /// The cell is part of a circular reference.
    Circular,
}

impl ErrorValue {
    /// Whether the public documentation defines this error value.
    pub fn is_documented(self) -> bool {
        !matches!(self, ErrorValue::Unsupported | ErrorValue::Circular)
    }

    /// Text shown for the error.
    pub fn text(self) -> &'static str {
        match self {
            ErrorValue::DivZero => "#DIV/0!",
            ErrorValue::Value => "#VALUE!",
            ErrorValue::Ref => "#REF!",
            ErrorValue::Num => "#NUM!",
            ErrorValue::Na => "#N/A!",
            ErrorValue::Dim => "#DIM!",
            // Unspecified in the sources: names chosen here.
            ErrorValue::Unsupported => "#UNSUPPORTED!",
            ErrorValue::Circular => "#CIRC!",
        }
    }
}

impl fmt::Display for ErrorValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}
