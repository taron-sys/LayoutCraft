//! The function table and call dispatch.
//!
//! Every function named in the public ShapeSheet function reference, and every function token of
//! the [MS-VSDX] formula grammar (section 2.5.1), has a row in [`functions`]. A row is either
//! implemented (`Full`, `Partial`, `Compat`) or `Unsupported`; calling an unsupported or unknown
//! function evaluates to [`ErrorValue::Unsupported`], never to a made-up number.

mod color;
mod host;
mod logic;
mod math;
mod table;
mod text;

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::ast::Expr;
use crate::env::{Env, Services};
use crate::error::ErrorValue;
use crate::eval::{to_quantity, Evaluator};
use crate::unit::Dim;
use crate::value::{Quantity, Value};

pub use table::functions;

/// How completely a function is implemented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Evaluates as documented.
    Full,
    /// Evaluates, but a documented side effect or option is not modelled (see `note`).
    Partial,
    /// Not in the public reference; kept because the repository's earlier evaluator accepted it.
    Compat,
    /// Known by name, not evaluated (see `note` for why).
    Unsupported,
}

pub(crate) type Outcome = Result<Value, ErrorValue>;
pub(crate) type EagerFn = fn(&mut Call<'_>) -> Outcome;
pub(crate) type LazyFn = fn(&mut Evaluator<'_>, &[Expr]) -> Value;

#[derive(Clone, Copy)]
pub(crate) enum Imp {
    /// Arguments are evaluated first; an error among them is the result.
    Eager(EagerFn),
    /// Arguments are evaluated first and error values are passed to the function.
    Raw(EagerFn),
    /// The function decides which arguments to evaluate.
    Lazy(LazyFn),
    None,
}

/// One row of the function table.
#[derive(Clone, Copy)]
pub struct FuncInfo {
    /// Upper-case name.
    pub name: &'static str,
    pub status: Status,
    pub min_args: u8,
    /// `u8::MAX` means any number.
    pub max_args: u8,
    /// Empty for `Full`; otherwise the limitation or the reason.
    pub note: &'static str,
    pub(crate) imp: Imp,
}

/// The row for `name` (case-insensitive), if the function is known.
pub fn function(name: &str) -> Option<&'static FuncInfo> {
    static INDEX: OnceLock<HashMap<&'static str, &'static FuncInfo>> = OnceLock::new();
    let index = INDEX.get_or_init(|| functions().iter().map(|info| (info.name, info)).collect());
    index.get(name.to_ascii_uppercase().as_str()).copied()
}

/// Evaluates a call. Arity is checked here so every function sees a valid argument count.
pub(crate) fn call(evaluator: &mut Evaluator<'_>, name: &str, args: &[Expr]) -> Value {
    let Some(info) = function(name) else {
        evaluator.note_unsupported(name);
        return Value::Error(ErrorValue::Unsupported);
    };
    if matches!(info.imp, Imp::None) {
        evaluator.note_unsupported(info.name);
        return Value::Error(ErrorValue::Unsupported);
    }
    let too_many = info.max_args != u8::MAX && args.len() > usize::from(info.max_args);
    if args.len() < usize::from(info.min_args) || too_many {
        return Value::Error(ErrorValue::Value);
    }
    let (imp, propagate) = match info.imp {
        Imp::Lazy(function) => return function(evaluator, args),
        Imp::Eager(function) => (function, true),
        Imp::Raw(function) => (function, false),
        Imp::None => return Value::Error(ErrorValue::Unsupported),
    };
    let mut values = Vec::with_capacity(args.len());
    for arg in args {
        let value = match arg.kind {
            crate::ast::ExprKind::Empty => None,
            _ => Some(evaluator.eval(arg)),
        };
        if propagate {
            if let Some(Value::Error(error)) = value {
                return Value::Error(error);
            }
        }
        values.push(value);
    }
    let mut call = Call {
        env: evaluator.env,
        args: values,
    };
    imp(&mut call).unwrap_or_else(Value::Error)
}

/// The evaluated arguments of one call, with typed accessors. A missing or omitted optional
/// argument reads as absent; a missing required one cannot happen (arity is checked first).
pub(crate) struct Call<'a> {
    pub env: &'a dyn Env,
    args: Vec<Option<Value>>,
}

impl Call<'_> {
    pub fn services(&self) -> &dyn Services {
        self.env.services()
    }

    pub fn len(&self) -> usize {
        self.args.len()
    }

    /// The argument, unless it was left out.
    pub fn get(&self, index: usize) -> Option<&Value> {
        self.args.get(index).and_then(Option::as_ref)
    }

    pub fn value(&self, index: usize) -> Result<&Value, ErrorValue> {
        self.get(index).ok_or(ErrorValue::Value)
    }

    pub fn num(&self, index: usize) -> Result<Quantity, ErrorValue> {
        to_quantity(self.value(index)?)
    }

    pub fn num_opt(&self, index: usize) -> Result<Option<Quantity>, ErrorValue> {
        self.get(index).map(to_quantity).transpose()
    }

    /// A dimensionless number; a length or angle here is `#DIM!`.
    pub fn plain(&self, index: usize) -> Result<f64, ErrorValue> {
        let q = self.num(index)?;
        if q.dim.is_none() {
            Ok(q.value)
        } else {
            Err(ErrorValue::Dim)
        }
    }

    pub fn plain_opt(&self, index: usize) -> Result<Option<f64>, ErrorValue> {
        match self.get(index) {
            None => Ok(None),
            Some(_) => self.plain(index).map(Some),
        }
    }

    /// A whole number (the fraction is dropped); huge values saturate.
    pub fn whole(&self, index: usize) -> Result<i64, ErrorValue> {
        Ok(self.plain(index)?.trunc() as i64)
    }

    pub fn whole_opt(&self, index: usize) -> Result<Option<i64>, ErrorValue> {
        Ok(self.plain_opt(index)?.map(|v| v.trunc() as i64))
    }

    pub fn text(&self, index: usize) -> Result<String, ErrorValue> {
        Ok(self.value(index)?.to_text())
    }

    pub fn text_opt(&self, index: usize) -> Option<String> {
        self.get(index).map(Value::to_text)
    }

    pub fn flag_opt(&self, index: usize) -> Result<Option<bool>, ErrorValue> {
        self.get(index).map(crate::eval::truthy).transpose()
    }

    /// The numbers of arguments `from..`, converted to one shared dimension: a dimensionless
    /// number takes the dimension of the others, two different dimensions are `#DIM!`.
    pub fn unified(&self, from: usize) -> Result<(Vec<f64>, Dim), ErrorValue> {
        let mut dim = Dim::NONE;
        let mut numbers = Vec::new();
        for index in from..self.len() {
            let q = self.num(index)?;
            dim = crate::eval::common_dim(dim, q.dim)?;
            numbers.push(q.value);
        }
        Ok((numbers, dim))
    }
}

/// `Number` of `value` in `dim`, or `#NUM!` when it is not finite.
pub(crate) fn number(value: f64, dim: Dim) -> Outcome {
    if value.is_finite() {
        Ok(Value::Number(Quantity::new(value, dim)))
    } else {
        Err(ErrorValue::Num)
    }
}

pub(crate) fn plain(value: f64) -> Outcome {
    number(value, Dim::NONE)
}

pub(crate) fn boolean(flag: bool) -> Outcome {
    Ok(Value::Bool(flag))
}
