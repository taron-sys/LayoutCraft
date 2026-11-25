//! Functions that read the shape being evaluated through [`crate::Services`].

use super::{boolean, plain, Call, Outcome};
use crate::error::ErrorValue;
use crate::value::Value;

fn unavailable<T>(answer: Option<T>) -> Result<T, ErrorValue> {
    answer.ok_or(ErrorValue::Unsupported)
}

pub(crate) fn id(call: &mut Call<'_>) -> Outcome {
    plain(f64::from(unavailable(call.services().shape_id())?))
}

pub(crate) fn name(call: &mut Call<'_>) -> Outcome {
    Ok(Value::Str(unavailable(call.services().shape_name())?))
}

pub(crate) fn page_name(call: &mut Call<'_>) -> Outcome {
    Ok(Value::Str(unavailable(call.services().page_name())?))
}

/// The master's name, or `<no master>` for a shape that has none.
pub(crate) fn master_name(call: &mut Call<'_>) -> Outcome {
    let answer = unavailable(call.services().master_name())?;
    Ok(Value::Str(
        answer.unwrap_or_else(|| "<no master>".to_string()),
    ))
}

pub(crate) fn is_1d(call: &mut Call<'_>) -> Outcome {
    boolean(unavailable(call.services().is_1d())?)
}

pub(crate) fn rand(call: &mut Call<'_>) -> Outcome {
    plain(unavailable(call.services().random())?.clamp(0.0, 1.0))
}
