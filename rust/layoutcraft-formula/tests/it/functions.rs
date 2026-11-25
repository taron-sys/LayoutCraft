//! The function table and every implemented function, through the case lists of `cases.rs`.

use std::collections::BTreeSet;

use layoutcraft_formula::{evaluate_str, function, functions, ErrorValue, Status, Value};

use crate::cases::{
    all_cases, colour_cases, host_cases, logic_cases, math_cases, text_cases, Case,
};
use crate::common::{matches, TestEnv};

/// Runs the cases; `test` is the name of the calling test, which `docs/46` lists for each function.
fn run(cases: Vec<Case>, test: &str) {
    let with = TestEnv::new();
    let without = TestEnv::without_services();
    let mut failures = Vec::new();
    for case in &cases {
        assert_eq!(
            case.test, test,
            "{} {:?} names the wrong test",
            case.function, case.formula
        );
        let env = if case.bare_host { &without } else { &with };
        match evaluate_str(case.formula, env) {
            Ok(evaluation) if matches(&evaluation.value, &case.want) => {}
            Ok(evaluation) => failures.push(format!(
                "{} {:?}: expected {:?}, got {:?}",
                case.function, case.formula, case.want, evaluation.value
            )),
            Err(error) => failures.push(format!(
                "{} {:?}: does not parse: {error}",
                case.function, case.formula
            )),
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn math_functions_give_the_reference_values_and_errors() {
    run(
        math_cases(),
        "math_functions_give_the_reference_values_and_errors",
    );
}

#[test]
fn logic_functions_select_lazily_and_report_errors() {
    run(
        logic_cases(),
        "logic_functions_select_lazily_and_report_errors",
    );
}

#[test]
fn text_functions_give_the_reference_strings_and_errors() {
    run(
        text_cases(),
        "text_functions_give_the_reference_strings_and_errors",
    );
}

#[test]
fn colour_functions_follow_the_240_scale_and_report_errors() {
    run(
        colour_cases(),
        "colour_functions_follow_the_240_scale_and_report_errors",
    );
}

#[test]
fn host_functions_read_the_services_or_report_unsupported() {
    run(
        host_cases(),
        "host_functions_read_the_services_or_report_unsupported",
    );
}

#[test]
fn every_implemented_function_has_a_normal_case_and_an_edge_case() {
    let cases = all_cases();
    let mut missing = Vec::new();
    for info in functions() {
        if info.status == Status::Unsupported {
            continue;
        }
        let normal = cases.iter().any(|c| c.function == info.name && !c.edge);
        let edge = cases.iter().any(|c| c.function == info.name && c.edge);
        if !normal || !edge {
            missing.push(format!("{} (normal {normal}, edge {edge})", info.name));
        }
    }
    assert!(missing.is_empty(), "functions without cases: {missing:?}");
    // And no case names a function that is not in the table.
    let unknown: Vec<&str> = cases
        .iter()
        .map(|c| c.function)
        .filter(|name| function(name).is_none())
        .collect();
    assert!(
        unknown.is_empty(),
        "cases for unknown functions: {unknown:?}"
    );
}

#[test]
fn the_function_table_has_one_row_per_name_in_upper_case() {
    let mut seen = BTreeSet::new();
    for info in functions() {
        assert_eq!(info.name, info.name.to_ascii_uppercase());
        assert!(seen.insert(info.name), "duplicate row {}", info.name);
        assert!(info.min_args <= info.max_args, "{}", info.name);
        assert_eq!(
            !info.note.is_empty(),
            info.status != Status::Full,
            "{}: a note says why a function is not Full, and only then",
            info.name
        );
        if info.status == Status::Unsupported {
            let arity = usize::from(info.min_args);
            let call = format!("{}({})", info.name, vec!["1"; arity].join(","));
            let outcome = evaluate_str(&call, &TestEnv::new()).expect("parses").value;
            assert_eq!(outcome, Value::Error(ErrorValue::Unsupported), "{call}");
        }
    }
    assert!(functions().len() > 200);
}

#[test]
fn the_table_covers_every_function_token_of_the_formula_grammar() {
    // The function tokens of the ABNF in [MS-VSDX] 2.5.1.
    let grammar = "ABS ACOS ADD AND ANG360 ANGLETOLOC ANGLETOPAR ASIN ATAN ATAN2 BITAND BITNOT BITOR \
        BITXOR BKGPAGENAME BLEND BOUND CAT CATEGORY CEILING CELLISTHEMED CHAR COMPANY COS COSH CREATOR \
        CY DATE DATETIME DATEVALUE DAY DAYOFYEAR DEG DEPENDSON DESCRIPTION DIRECTORY DIV DOCCREATION \
        DOCLASTEDIT DOCLASTPRINT DOCLASTSAVE EEQ EGE EGT ELE ELT ENE FEQ FGE FGT FIELDPICTURE FILENAME \
        FIND FLE FLOOR FLT FNE FORMAT FORMATEX FORMULAEXISTS GRAVITY GUARD HASCATEGORY HOUR HSL HUE \
        HUEDIFF HYPERLINKBASE ID IF IFERROR INDEX INT INTERSECTX INTERSECTY INTUP IS1D ISERR ISERRNA \
        ISERROR ISERRVALUE ISTHEMED KEYWORDS LANGUAGE LEFT LEN LN LOC LOCALFORMULAEXISTS LOCTOLOC \
        LOCTOPAR LOG10 LOOKUP LOWER LUM LUMDIFF MAGNITUDE MANAGER MASTERNAME MAX MID MIN MINUTE MODULUS \
        MONTH MSOSHADE MSOTINT MUL NA NAME NOT NOW NURBS OR PAGECOUNT PAGENAME PAGENUMBER PAR PCT PI PNT \
        PNTX PNTY POLYLINE POW RAD RAND REF REPLACE RGB RIGHT ROUND SAT SATDIFF SECOND SETATREF \
        SETATREFEVAL SETATREFEXPR SHADE SHAPETEXT SIGN SIN SINH SQRT STRSAME STRSAMEEX SUB SUBJECT \
        SUBSTITUTE SUM TAN TANH TEXTHEIGHT TEXTWIDTH THEME THEMECBV THEMEGUARD THEMEPROP THEMERESTORE \
        THEMEVAL TIME TIMEVALUE TINT TITLE TONE TRIM TRUNC UMINUS UNICHAR UPLUS UPPER USE VERSION \
        WEEKDAY YEAR";
    let missing: Vec<&str> = grammar
        .split_whitespace()
        .filter(|name| function(name).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "grammar functions missing from the table: {missing:?}"
    );
}

#[test]
fn function_names_are_found_whatever_their_case() {
    assert_eq!(function("abs").map(|f| f.name), Some("ABS"));
    assert_eq!(function("Abs").map(|f| f.name), Some("ABS"));
    assert!(function("NoSuchFunction").is_none());
    let env = TestEnv::new();
    for source in ["abs(-1)", "Abs(-1)", "ABS (-1)"] {
        let value = evaluate_str(source, &env).expect("parses").value;
        assert_eq!(value, Value::number(1.0), "{source}");
    }
}

#[test]
fn wrong_argument_counts_are_value_errors_for_every_implemented_function() {
    let env = TestEnv::new();
    for info in functions() {
        if info.status == Status::Unsupported {
            continue;
        }
        if info.max_args != u8::MAX {
            let args = vec!["1"; usize::from(info.max_args) + 1].join(",");
            let value = evaluate_str(&format!("{}({args})", info.name), &env)
                .expect("parses")
                .value;
            assert_eq!(
                value,
                Value::Error(ErrorValue::Value),
                "{} too many",
                info.name
            );
        }
        if info.min_args > 0 {
            let args = vec!["1"; usize::from(info.min_args) - 1].join(",");
            let value = evaluate_str(&format!("{}({args})", info.name), &env)
                .expect("parses")
                .value;
            assert_eq!(
                value,
                Value::Error(ErrorValue::Value),
                "{} too few",
                info.name
            );
        }
    }
}

#[test]
fn eager_functions_return_the_first_argument_error_and_the_iserr_family_inspects_it() {
    let env = TestEnv::new();
    let value = evaluate_str("MAX(1/0, NA())", &env).expect("parses").value;
    assert_eq!(value, Value::Error(ErrorValue::DivZero));
    let value = evaluate_str("ISERROR(1/0)", &env).expect("parses").value;
    assert_eq!(value, Value::Bool(true));
}

#[test]
fn calling_an_unsupported_or_unknown_function_is_reported_once_per_name() {
    let env = TestEnv::new();
    let evaluation = evaluate_str(
        "IF(1, DATE(1,2,3), 0) + Bogus(1) + date(4) + Bogus(2)",
        &env,
    )
    .expect("parses");
    assert_eq!(evaluation.value, Value::Error(ErrorValue::Unsupported));
    assert_eq!(
        evaluation.unsupported,
        vec!["DATE".to_string(), "BOGUS".to_string()]
    );
    let evaluation =
        evaluate_str("IFERROR(Bogus(1), IFERROR(date(4), Bogus(2)))", &env).expect("parses");
    assert_eq!(evaluation.value, Value::Error(ErrorValue::Unsupported));
    assert_eq!(
        evaluation.unsupported,
        vec!["BOGUS".to_string(), "DATE".to_string()]
    );
    // A branch that is not taken reports nothing.
    let evaluation = evaluate_str("IF(0, DATE(1,2,3), 5)", &env).expect("parses");
    assert_eq!(evaluation.value, Value::number(5.0));
    assert!(evaluation.unsupported.is_empty());
}
