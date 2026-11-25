//! Every operator: values from the "About Operators" page, the order of evaluation, operand
//! conversion, tolerance, dimension rules, error propagation and date arithmetic.

use crate::common::{check, err, eval_in, quantity, TestEnv, X};
use layoutcraft_formula::{ErrorValue, Value};
use X::*;

#[test]
fn every_operator_gives_the_value_the_operators_page_lists() {
    check(&[
        ("+37", N(37.0)),
        ("-37", N(-37.0)),
        ("37%", N(0.37)),
        ("5 ^ 2", N(25.0)),
        ("5 * 2", N(10.0)),
        ("5 / 2", N(2.5)),
        ("5 + 2", N(7.0)),
        ("5 - 2", N(3.0)),
        ("5 > 2", B(true)),
        ("5 < 2", B(false)),
        ("5 >= 2", B(true)),
        ("5 <= 2", B(false)),
        ("5 = 2", B(false)),
        ("5 <> 2", B(true)),
        ("5 GT 2", B(true)),
        ("5 LT 2", B(false)),
        ("5 GE 2", B(true)),
        ("5 LE 2", B(false)),
        ("5 EQ 2", B(false)),
        ("5 NE 2", B(true)),
        ("\"dog\" & \"house\"", S("doghouse")),
        ("4 + 5 * 6", N(34.0)),
        ("(4 + 5) * 6", N(54.0)),
    ]);
}

#[test]
fn operators_of_different_levels_follow_the_order_of_evaluation_table() {
    check(&[
        // Unary signs and percent come first in the table, before ^.
        ("-2^2", N(4.0)),
        ("2^-1", N(0.5)),
        ("-3%", N(-0.03)),
        ("200%^2", N(4.0)),
        // ^ before * and /, those before + and -.
        ("2 * 3 ^ 2", N(18.0)),
        ("10 - 6 / 2", N(7.0)),
        // + and - before &, & before the ordering comparisons, those before = and <>.
        ("1 + 2 & 3 + 4", S("37")),
        ("1 & 2 < 13", B(true)),
        ("0 = 1 < 2", B(false)),
        ("1 < 2 = 1 < 2", B(true)),
        ("2 > 1 <> 3 > 1", B(false)),
    ]);
}

#[test]
fn operators_of_one_level_group_from_the_left() {
    check(&[
        ("10 - 3 - 2", N(5.0)),
        ("100 / 10 / 2", N(5.0)),
        ("1 & 2 & 3", S("123")),
        ("7 - 2 + 1", N(6.0)),
        // The sources do not say how ^ groups; the engine groups it from the left like the other
        // binary operators (docs/46, decision list).
        ("2 ^ 3 ^ 2", N(64.0)),
        ("1 < 2 < 3", B(true)),
        ("3 > 2 > 1", B(false)),
    ]);
}

#[test]
fn operands_are_converted_to_what_the_operator_needs() {
    check(&[
        ("2 * 5 & \" cents\"", S("10 cents")),
        ("5 * \"2\"", N(10.0)),
        ("5 * \"sheep\"", N(0.0)),
        ("\"sheep\" + 1", N(1.0)),
        ("TRUE + TRUE", N(2.0)),
        ("TRUE & FALSE", S("TRUEFALSE")),
        ("#ff0000 & \"\"", S("#ff0000")),
        ("1/4 & \"\"", S("0.25")),
        ("1e21 & \"\"", S("1000000000000000000000")),
        ("0.1 + 0.2 & \"\"", S("0.3")),
        ("-\"4\"", N(-4.0)),
        ("\"50\"%", N(0.5)),
        ("\"inf\" + 1", N(1.0)),
        ("\"NaN\" + 1", N(1.0)),
        ("\" 7 \" * 2", N(14.0)),
    ]);
}

#[test]
fn comparing_text_converts_it_to_numbers_first() {
    // "Text strings that cannot be converted return a value of 0": two different words are equal.
    check(&[
        ("\"cat\" = \"dog\"", B(true)),
        ("\"cat\" = 0", B(true)),
        ("\"2\" < \"10\"", B(true)),
        ("\"b\" > \"a\"", B(false)),
        ("STRSAME(\"cat\", \"dog\")", B(false)),
    ]);
}

#[test]
fn symbolic_comparisons_absorb_noise_that_the_word_forms_see() {
    check(&[
        ("0.1 + 0.2 = 0.3", B(true)),
        ("1 + 1e-12 = 1", B(true)),
        ("1 + 1e-12 EQ 1", B(false)),
        ("1 + 1e-12 NE 1", B(true)),
        ("1 + 1e-12 > 1", B(false)),
        ("1 + 1e-12 GT 1", B(true)),
        ("1 + 1e-12 >= 1", B(true)),
        ("1 + 1e-12 <= 1", B(true)),
        ("1 + 1e-12 < 1", B(false)),
        ("1 + 1e-12 LT 1", B(false)),
        ("1 + 1e-12 LE 1", B(false)),
        ("1 + 1e-12 GE 1", B(true)),
        ("1 + 1e-10 > 1", B(false)),
        ("1 + 1e-8 > 1", B(true)),
        ("1 + 1e-8 <> 1", B(true)),
        // The word forms compare the 15 stored digits: noise beyond them does not count.
        ("1 + 1e-16 EQ 1", B(true)),
        ("0.1 + 0.2 EQ 0.3", B(true)),
    ]);
}

#[test]
fn colours_compare_for_equality_only() {
    check(&[
        ("#ff0000 = #FF0000", B(true)),
        ("#ff0000 <> #00ff00", B(true)),
        ("#ff0000 < #00ff00", E(ErrorValue::Value)),
        ("#ff0000 = RGB(255, 0, 0)", B(true)),
    ]);
}

#[test]
fn error_values_propagate_through_every_operator() {
    let errors = [
        ("1/0", ErrorValue::DivZero),
        ("NA()", ErrorValue::Na),
        ("REF()", ErrorValue::Ref),
        ("SQRT(-1)", ErrorValue::Num),
        ("1 in + 1 deg", ErrorValue::Dim),
        ("Missing", ErrorValue::Ref),
    ];
    let operators = [
        "+", "-", "*", "/", "^", "&", "=", "<>", "<", ">", "<=", ">=", "EQ", "LT",
    ];
    for (error_text, error) in errors {
        for op in operators {
            for formula in [
                format!("({error_text}) {op} 1"),
                format!("1 {op} ({error_text})"),
            ] {
                assert_eq!(err(&formula), error, "{formula}");
            }
        }
        for formula in [
            format!("-({error_text})"),
            format!("+({error_text})"),
            format!("({error_text})%"),
        ] {
            assert_eq!(err(&formula), error, "{formula}");
        }
    }
}

#[test]
fn the_left_operand_error_wins_when_both_operands_fail() {
    assert_eq!(err("1/0 + NA()"), ErrorValue::DivZero);
    assert_eq!(err("NA() + 1/0"), ErrorValue::Na);
}

#[test]
fn each_documented_error_value_shows_the_text_of_the_error_page() {
    let table = [
        (ErrorValue::DivZero, "#DIV/0!"),
        (ErrorValue::Value, "#VALUE!"),
        (ErrorValue::Ref, "#REF!"),
        (ErrorValue::Num, "#NUM!"),
        (ErrorValue::Na, "#N/A!"),
        (ErrorValue::Dim, "#DIM!"),
    ];
    for (error, shown) in table {
        assert_eq!(error.text(), shown);
        assert_eq!(error.to_string(), shown);
        assert!(error.is_documented());
    }
    assert!(!ErrorValue::Unsupported.is_documented());
    assert!(!ErrorValue::Circular.is_documented());
    assert_eq!(ErrorValue::Circular.text(), "#CIRC!");
}

#[test]
fn each_documented_error_is_produced_by_its_documented_cause() {
    check(&[
        // Division by 0 (the page's own example).
        ("10/0", E(ErrorValue::DivZero)),
        // A reference to a cell that does not exist.
        ("Missing", E(ErrorValue::Ref)),
        // The square root of a negative number.
        ("SQRT(-4)", E(ErrorValue::Num)),
        // NA() is "not available".
        ("NA()", E(ErrorValue::Na)),
        // The page's two #DIM! examples.
        ("1in^100 * 1in^100", E(ErrorValue::Dim)),
        ("5.2cm^1.5", E(ErrorValue::Dim)),
        // A wrong-type operand: a point used as a number.
        ("PNT(1, 2) + 1", E(ErrorValue::Value)),
    ]);
}

#[test]
fn arithmetic_that_leaves_the_finite_numbers_is_num_error() {
    check(&[
        ("1e308 * 10", E(ErrorValue::Num)),
        ("-1e308 * 10", E(ErrorValue::Num)),
        ("1e308 + 1e308", E(ErrorValue::Num)),
        ("10 ^ 400", E(ErrorValue::Num)),
        ("0 ^ -1", E(ErrorValue::Num)),
        ("(-8) ^ (1/3)", E(ErrorValue::Num)),
        ("1e-320 / 1e10", N(0.0)),
        ("1 / 1e-320", E(ErrorValue::Num)),
        ("0 / 5", N(0.0)),
        ("5 / 0", E(ErrorValue::DivZero)),
        ("0 / 0", E(ErrorValue::DivZero)),
        ("5 / 0 in", E(ErrorValue::DivZero)),
        ("5 / \"x\"", E(ErrorValue::DivZero)),
    ]);
}

#[test]
fn dimensions_follow_the_arithmetic() {
    check(&[
        ("2 in * 3 in", Sq(6.0)),
        ("6 in * 6 in / 2 in", L(18.0)),
        ("Width / Height", N(2.0)),
        ("Width * Height", Sq(8.0)),
        ("Width * Height / Width", L(2.0)),
        ("Width ^ 2", Sq(16.0)),
        ("(2 in) ^ 3 / (1 in) ^ 2", L(8.0)),
        ("Width ^ 0", N(1.0)),
        ("4 in ^ 1", L(4.0)),
        ("(4 in) ^ -1 * 1 in", N(0.25)),
        ("1 in + 1", L(2.0)),
        ("1 + 1 in", L(2.0)),
        ("1 in - 1", L(0.0)),
        ("5 deg * 2", A(10.0 * std::f64::consts::PI / 180.0)),
        ("1 ed + 12 eh", D(1.5)),
        ("1 in < 2", B(true)),
        ("1 in = 1", B(true)),
        ("-(2 in)", L(-2.0)),
        ("50% * 4 in", L(2.0)),
        ("(3 in)%", L(0.03)),
    ]);
}

#[test]
fn dimension_mismatches_and_range_overflow_are_dim_errors() {
    check(&[
        ("1 in + 1 deg", E(ErrorValue::Dim)),
        ("1 in - 1 ed", E(ErrorValue::Dim)),
        ("1 in < 1 deg", E(ErrorValue::Dim)),
        ("1 in = 1 deg", E(ErrorValue::Dim)),
        ("5.2cm^1.5", E(ErrorValue::Dim)),
        ("1in^100 * 1in^100", E(ErrorValue::Dim)),
        ("1 in ^ (2 in)", E(ErrorValue::Dim)),
        ("(2 in) ^ 128", E(ErrorValue::Dim)),
        ("(2 in) ^ 127 * (2 in)", E(ErrorValue::Dim)),
        ("(2 in) ^ -128 / (2 in)", E(ErrorValue::Dim)),
        ("(2 in) ^ -1e300", E(ErrorValue::Dim)),
        ("(1 in) ^ 1e10", E(ErrorValue::Dim)),
    ]);
    // The boundary exponents of the documented range -128..=127 are representable.
    assert_eq!(quantity("(1 in) ^ 127").dim.length, 127);
    assert_eq!(quantity("(1 in) ^ -128").dim.length, -128);
}

#[test]
fn date_and_duration_arithmetic_follows_the_dates_page() {
    let mut env = TestEnv::new();
    env.set("D1", Value::Date(36_000.5));
    env.set("D2", Value::Date(36_010.0));
    env.set("Dur", Value::duration(2.0));
    let value = |source: &str| eval_in(source, &env);
    // date +/- duration = date; duration + date = date; date - date = duration;
    // date + date = date; duration +/- duration = duration.
    assert_eq!(value("D1 + Dur"), Value::Date(36_002.5));
    assert_eq!(value("D1 - Dur"), Value::Date(35_998.5));
    assert_eq!(value("Dur + D1"), Value::Date(36_002.5));
    assert_eq!(value("D2 - D1"), Value::duration(9.5));
    assert_eq!(value("D1 + D2"), Value::Date(72_010.5));
    assert_eq!(value("Dur + Dur"), Value::duration(4.0));
    assert_eq!(value("Dur - 1 ed"), Value::duration(1.0));
    assert_eq!(value("D1 + 1 ew"), Value::Date(36_007.5));
    assert_eq!(value("D1 > D2"), Value::Bool(false));
    assert_eq!(value("D1 & \"\""), Value::string("36000.5"));
    assert_eq!(value("D1 * 2"), Value::number(72_001.0));
}
