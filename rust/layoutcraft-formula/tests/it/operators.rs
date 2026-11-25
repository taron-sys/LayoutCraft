//! Operators, unit arithmetic and error values (values are the documented examples).

use crate::common::{boolean, close, err, eval, eval_in, num, quantity, text, TestEnv};
use layoutcraft_formula::{Dim, ErrorValue, Value};

#[test]
fn precedence_gives_the_documented_results() {
    assert_eq!(num("4 + 5 * 6"), 34.0);
    assert_eq!(num("(4 + 5) * 6"), 54.0);
    assert_eq!(num("5 ^ 2"), 25.0);
    assert_eq!(num("37%"), 0.37);
    assert_eq!(num("-2^2"), 4.0);
    assert_eq!(num("2^3^2"), 64.0);
}

#[test]
fn operands_convert_to_what_the_operator_needs() {
    assert_eq!(text("2 * 5 & \" cents\""), "10 cents");
    assert_eq!(num("5 * \"2\""), 10.0);
    assert_eq!(num("5 * \"sheep\""), 0.0);
}

#[test]
fn symbolic_comparison_tolerates_noise_and_the_word_form_does_not() {
    assert!(boolean("1 + 1e-12 = 1"));
    assert!(!boolean("1 + 1e-12 EQ 1"));
    assert!(boolean("5 > 2") && !boolean("5 < 2") && boolean("5 <> 2"));
}

#[test]
fn division_by_zero_overflow_and_invalid_powers_are_error_values() {
    assert_eq!(err("1/0"), ErrorValue::DivZero);
    assert_eq!(err("1e308 * 10"), ErrorValue::Num);
    assert_eq!(err("(-8)^(1/3)"), ErrorValue::Num);
    assert_eq!(err("Missing + 1"), ErrorValue::Ref);
    assert_eq!(num("IFERROR(1/0, 5)"), 5.0);
}

#[test]
fn lengths_divide_to_plain_numbers_and_multiply_to_areas() {
    assert_eq!(num("Width / Height"), 2.0);
    assert_eq!(quantity("Width / Height").dim, Dim::NONE);
    assert_eq!(quantity("Width * Height").dim, Dim::AREA);
    assert!(close(num("2.54 cm"), 1.0));
    assert!(close(num("180 deg"), std::f64::consts::PI));
}

#[test]
fn mixing_dimensions_or_fractional_powers_is_dim_error() {
    assert_eq!(err("1 in + 1 deg"), ErrorValue::Dim);
    assert_eq!(err("5.2cm^1.5"), ErrorValue::Dim);
    assert_eq!(err("1in^100 * 1in^100"), ErrorValue::Dim);
    assert_eq!(quantity("1 in + 1").dim, Dim::LENGTH);
}

#[test]
fn unsupported_and_unknown_functions_are_explicit_errors() {
    assert_eq!(err("DATE(2020,1,1)"), ErrorValue::Unsupported);
    assert_eq!(err("Bogus(1)"), ErrorValue::Unsupported);
    let mut env = TestEnv::new();
    env.set("Flag", Value::Bool(true));
    assert_eq!(eval_in("Flag + 1", &env), Value::number(2.0));
    assert_eq!(eval("MAX(13 in, 1 ft, 20 cm)"), Value::length(13.0));
}
