//! Logic, error-handling and selection functions, and the guard-style pass-throughs.
//!
//! The lazy functions evaluate only the argument they select, so an untaken branch may contain a
//! division by zero, a reference to a missing cell or an unsupported function.

use super::{boolean, Call, Outcome};
use crate::ast::{BinaryOp, CmpOp, Precision};
use crate::ast::{Expr, ExprKind};
use crate::env::FormulaKind;
use crate::error::ErrorValue;
use crate::eval::{binary, truthy, Evaluator};
use crate::value::Value;

fn error(error: ErrorValue) -> Value {
    Value::Error(error)
}

/// `IF(condition, if_true, if_false)`.
pub(crate) fn if_(evaluator: &mut Evaluator<'_>, args: &[Expr]) -> Value {
    let [condition, if_true, if_false] = args else {
        return error(ErrorValue::Value);
    };
    match truthy(&evaluator.eval(condition)) {
        Ok(true) => evaluator.eval(if_true),
        Ok(false) => evaluator.eval(if_false),
        Err(e) => error(e),
    }
}

/// `IFERROR(primary, alternate)`: the alternate is evaluated only if the primary is an error.
pub(crate) fn if_error(evaluator: &mut Evaluator<'_>, args: &[Expr]) -> Value {
    let [primary, alternate] = args else {
        return error(ErrorValue::Value);
    };
    match evaluator.eval(primary) {
        Value::Error(_) => evaluator.eval(alternate),
        value => value,
    }
}

/// `CHOOSE(index, value1, value2, ...)` with a 1-based index; only the chosen value is evaluated.
pub(crate) fn choose(evaluator: &mut Evaluator<'_>, args: &[Expr]) -> Value {
    let Some((index, choices)) = args.split_first() else {
        return error(ErrorValue::Value);
    };
    let index = match crate::eval::to_quantity(&evaluator.eval(index)) {
        Ok(q) => q.value.trunc(),
        Err(e) => return error(e),
    };
    // A position that is not 1..=count has no value to choose.
    if index < 1.0 || index > choices.len() as f64 {
        return error(ErrorValue::Value);
    }
    match choices.get(index as usize - 1) {
        Some(chosen) => evaluator.eval(chosen),
        None => error(ErrorValue::Value),
    }
}

/// `SWITCH(subject, case1, value1, case2, value2, ..., [default])`: the first equal case wins.
pub(crate) fn switch(evaluator: &mut Evaluator<'_>, args: &[Expr]) -> Value {
    let Some((subject, rest)) = args.split_first() else {
        return error(ErrorValue::Value);
    };
    let subject = evaluator.eval(subject);
    if subject.is_error() {
        return subject;
    }
    let (pairs_slice, tail) = rest.as_chunks::<2>();
    let mut pairs = pairs_slice.iter();
    for pair in pairs.by_ref() {
        let [case, value] = pair;
        let case = evaluator.eval(case);
        if case.is_error() {
            return case;
        }
        match binary(
            BinaryOp::Compare(CmpOp::Eq, Precision::Tolerant),
            &subject,
            &case,
        ) {
            Value::Bool(true) => return evaluator.eval(value),
            Value::Bool(false) => {}
            other => return other,
        }
    }
    match tail {
        [default] => evaluator.eval(default),
        _ => error(ErrorValue::Na),
    }
}

pub(crate) fn and(call: &mut Call<'_>) -> Outcome {
    let mut all = true;
    for index in 0..call.len() {
        all &= truthy(call.value(index)?)?;
    }
    boolean(all)
}

pub(crate) fn or(call: &mut Call<'_>) -> Outcome {
    let mut any = false;
    for index in 0..call.len() {
        any |= truthy(call.value(index)?)?;
    }
    boolean(any)
}

pub(crate) fn not(call: &mut Call<'_>) -> Outcome {
    boolean(!truthy(call.value(0)?)?)
}

fn error_test(call: &Call<'_>, test: fn(ErrorValue) -> bool) -> Outcome {
    boolean(call.value(0)?.error().is_some_and(test))
}

/// Any error except `#N/A!`.
pub(crate) fn is_err(call: &mut Call<'_>) -> Outcome {
    error_test(call, |e| e != ErrorValue::Na)
}

pub(crate) fn is_err_na(call: &mut Call<'_>) -> Outcome {
    error_test(call, |e| e == ErrorValue::Na)
}

pub(crate) fn is_error(call: &mut Call<'_>) -> Outcome {
    error_test(call, |_| true)
}

pub(crate) fn is_err_value(call: &mut Call<'_>) -> Outcome {
    error_test(call, |e| e == ErrorValue::Value)
}

pub(crate) fn na(_call: &mut Call<'_>) -> Outcome {
    Err(ErrorValue::Na)
}

pub(crate) fn ref_error(_call: &mut Call<'_>) -> Outcome {
    Err(ErrorValue::Ref)
}

/// `GUARD(expression)` evaluates to the expression. Protecting the cell from edits is the host's
/// business; the engine only computes values.
pub(crate) fn guard(call: &mut Call<'_>) -> Outcome {
    Ok(call.value(0)?.clone())
}

/// `BOUND(value, type, ignore, low, high, [ignore, low, high]...)` evaluates to `value`; the
/// clamping happens when the host writes the cell. The arguments after the first two come in
/// threes, so any other count is `#VALUE!`. An omitted `value` (the page's own example leaves it
/// out) counts as 0.
pub(crate) fn bound(call: &mut Call<'_>) -> Outcome {
    if call.len() < 5 || !(call.len() - 2).is_multiple_of(3) {
        return Err(ErrorValue::Value);
    }
    Ok(call.get(0).cloned().unwrap_or_else(|| Value::number(0.0)))
}

/// `SETATREF(reference, [set_expression], [ignore_eval])`: the value of `reference`, or 0 when
/// `ignore_eval` is true. Redirecting edits is the host's business.
pub(crate) fn set_at_ref(evaluator: &mut Evaluator<'_>, args: &[Expr]) -> Value {
    let Some((reference, rest)) = args.split_first() else {
        return error(ErrorValue::Value);
    };
    if let Some(ignore) = rest.get(1) {
        match truthy(&evaluator.eval(ignore)) {
            Ok(true) => return Value::number(0.0),
            Ok(false) => {}
            Err(e) => return error(e),
        }
    }
    evaluator.eval(reference)
}

pub(crate) fn set_at_ref_eval(evaluator: &mut Evaluator<'_>, args: &[Expr]) -> Value {
    match args {
        [expr] => evaluator.eval(expr),
        _ => error(ErrorValue::Value),
    }
}

/// `SETATREFEXPR([expr])`: the expression, or 0 when it is left out.
pub(crate) fn set_at_ref_expr(evaluator: &mut Evaluator<'_>, args: &[Expr]) -> Value {
    match args {
        [] => Value::number(0.0),
        [expr] if matches!(expr.kind, ExprKind::Empty) => Value::number(0.0),
        [expr] => evaluator.eval(expr),
        _ => error(ErrorValue::Value),
    }
}

/// `DEPENDSON(cell, ...)` only declares dependencies (the engine reads them from the syntax tree)
/// and is always FALSE.
pub(crate) fn depends_on(_evaluator: &mut Evaluator<'_>, _args: &[Expr]) -> Value {
    Value::Bool(false)
}

fn formula_exists(
    evaluator: &mut Evaluator<'_>,
    args: &[Expr],
    wanted: fn(FormulaKind) -> bool,
) -> Value {
    let [argument] = args else {
        return error(ErrorValue::Value);
    };
    // The argument names a cell; it is not evaluated.
    let ExprKind::Ref(reference) = &argument.kind else {
        return error(ErrorValue::Value);
    };
    match evaluator.env.formula_kind(reference) {
        Ok(kind) => Value::Bool(wanted(kind)),
        Err(e) => error(e),
    }
}

pub(crate) fn formula_exists_any(evaluator: &mut Evaluator<'_>, args: &[Expr]) -> Value {
    formula_exists(evaluator, args, |kind| kind != FormulaKind::Absent)
}

pub(crate) fn local_formula_exists(evaluator: &mut Evaluator<'_>, args: &[Expr]) -> Value {
    formula_exists(evaluator, args, |kind| kind == FormulaKind::Local)
}
