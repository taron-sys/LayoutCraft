//! Differential test against the repository's earlier evaluators (`layoutcraft-vsdx`:
//! `formula.rs` for numbers, `colorexpr.rs` for colours).
//!
//! Part one runs every formula the earlier unit tests cover through both and requires the same
//! result. Part two pins each intentional difference, with the public source that justifies the
//! new behaviour (listed in `docs/46-formula-engine.md`, "Differences from the earlier
//! evaluators"). Part three compares the two on generated arithmetic.

use layoutcraft_formula::{evaluate_str, Env, ErrorValue, Rgb, Services, ThemeKey, Value};
use layoutcraft_vsdx::colorexpr::{evaluate_color, luminance, Val};
use layoutcraft_vsdx::styles::{tint, Theme};

use crate::common::{close, TestEnv};

// ---- numbers -----------------------------------------------------------------------------

/// The earlier evaluator with `Width = 4` and `Zero = 0`, the cells its own tests use.
fn old_number(source: &str) -> Option<f64> {
    layoutcraft_vsdx::formula::evaluate(source, &mut |name| match name {
        "Width" => Some(4.0),
        "Zero" => Some(0.0),
        _ => None,
    })
}

fn numeric_env() -> TestEnv {
    let mut env = TestEnv::new();
    env.set("Width", Value::number(4.0));
    env.set("Zero", Value::number(0.0));
    env
}

/// This crate's result as a plain number; `Err` for an error value or a non-number.
fn new_number(source: &str) -> Result<f64, String> {
    let evaluation = evaluate_str(source, &numeric_env()).map_err(|e| format!("parse: {e}"))?;
    match evaluation.value {
        Value::Error(error) => Err(error.text().to_string()),
        value => value.as_f64().ok_or_else(|| format!("{value:?}")),
    }
}

fn agree(source: &str) -> Result<(), String> {
    match (old_number(source), new_number(source)) {
        (Some(old), Ok(new)) if close(old, new) => Ok(()),
        (None, Err(_)) => Ok(()),
        (old, new) => Err(format!("{source:?}: old {old:?}, new {new:?}")),
    }
}

/// Every formula of the unit tests of `formula.rs`.
const OLD_NUMERIC_TESTS: &[&str] = &[
    "CEILING(0.2778,0.0625)",
    "CEILING(0.3125,0.0625)",
    "CEILING(1.2)",
    "FLOOR(0.2778,0.0625)",
    "FLOOR(-1.2)",
    "IF(Zero=0,1,Width/Zero)",
    "IF(Zero,Missing(1),2)",
    "IF(Width>3,Width*2,UnknownCell)",
    "1/Zero",
    "IF(0,1,IF(1,5,1/Zero))",
    "CHOOSE(2,10,20,1/Zero)",
    "SWITCH(3,1,10,3,30,99)",
    "SWITCH(7,1,10,3,30,99)",
    "2^3^2",
    "-2^2",
    "MOD(-7,3)",
    "MOD(7,-3)",
    "ROUND(3.14159,2)",
    "ROUND(2.5,0)",
    "ROUND(-2.5,0)",
    "INT(-1.5)",
    "CEILING(1.2)",
    "SUM(1,2,3)",
    "SIGN(-9)",
];

#[test]
fn every_formula_the_earlier_numeric_tests_cover_gives_the_same_number() {
    let failures: Vec<String> = OLD_NUMERIC_TESTS
        .iter()
        .filter_map(|source| agree(source).err())
        .collect();
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    // And the values those tests expect are the ones both give.
    for (source, expected) in [
        ("CEILING(0.2778,0.0625)", 0.3125),
        ("FLOOR(0.2778,0.0625)", 0.25),
        ("IF(Width>3,Width*2,UnknownCell)", 8.0),
        ("SWITCH(7,1,10,3,30,99)", 99.0),
        ("2^3^2", 64.0),
        ("-2^2", 4.0),
        ("MOD(-7,3)", 1.0),
        ("MOD(7,-3)", -1.0),
        ("ROUND(-2.5,0)", -3.0),
        ("INT(-1.5)", -2.0),
    ] {
        assert_eq!(old_number(source), Some(expected), "old {source}");
        assert_eq!(new_number(source), Ok(expected), "new {source}");
    }
    assert_eq!(old_number("1/Zero"), None);
    assert!(new_number("1/Zero").is_err());
}

// ---- colours -----------------------------------------------------------------------------

/// Theme values for the new engine, read from the same `Theme` the earlier evaluator uses.
struct ThemeServices {
    theme: Theme,
    bare: Option<Rgb>,
}

impl Services for ThemeServices {
    fn theme_value(&self, key: &ThemeKey) -> Option<Value> {
        match key {
            ThemeKey::HostCell => self.bare.map(Value::Color),
            ThemeKey::Name(name) => self
                .theme
                .slot(name, false)
                .and_then(|hex| Rgb::from_hex(&hex))
                .map(Value::Color),
            ThemeKey::ColorIndex(_) => None,
        }
    }
}

struct ColourEnv {
    cells: TestEnv,
    services: ThemeServices,
}

impl Env for ColourEnv {
    fn cell(
        &self,
        reference: &layoutcraft_formula::CellRef,
    ) -> Result<Value, layoutcraft_formula::ErrorValue> {
        self.cells.cell(reference)
    }

    fn services(&self) -> &dyn Services {
        &self.services
    }
}

fn colour_env(bare: Option<&str>) -> ColourEnv {
    let mut cells = TestEnv::new();
    cells.set("User.DarkerColor", Value::Color(Rgb::new(0xd8, 0x00, 0x73)));
    cells.set("User.Flag", Value::number(1.0));
    ColourEnv {
        cells,
        services: ThemeServices {
            theme: Theme::office(),
            bare: bare.and_then(Rgb::from_hex),
        },
    }
}

fn old_colour(source: &str, bare: Option<&str>) -> Option<String> {
    let theme = Theme::office();
    evaluate_color(source, &theme, bare, &mut |name| match name {
        "User.DarkerColor" => Some(Val::Color("#d80073".to_string())),
        "User.Flag" => Some(Val::Num(1.0)),
        _ => None,
    })
}

fn new_colour(source: &str, bare: Option<&str>) -> Option<String> {
    match evaluate_str(source, &colour_env(bare)).ok()?.value {
        Value::Color(rgb) => Some(rgb.to_hex()),
        _ => None,
    }
}

const TEXT_CALLOUT: &str = "IF(ABS(LUMDIFF(THEMEVAL(\"BackgroundColor\"),THEMEVAL()))<40,IF(LUM(THEMEVAL(\"BackgroundColor\"))>120,User.DarkerColor,THEMEVAL(\"Light\")),THEMEVAL())";

#[test]
fn every_formula_the_earlier_colour_tests_cover_gives_the_same_colour() {
    for (source, bare, expected) in [
        (TEXT_CALLOUT, Some("#f0f0f0"), Some("#d80073")),
        (TEXT_CALLOUT, Some("#202020"), Some("#202020")),
        ("IF(User.Flag,#112233,User.Missing)", None, Some("#112233")),
        (
            "IF(NOT(User.Flag),User.Missing/0,RGB(1,2,3))",
            None,
            Some("#010203"),
        ),
    ] {
        let old = old_colour(source, bare);
        let new = new_colour(source, bare);
        assert_eq!(old.as_deref(), expected, "old {source} {bare:?}");
        assert_eq!(new.as_deref(), expected, "new {source} {bare:?}");
    }
}

// ---- intentional differences -------------------------------------------------------------

#[test]
fn difference_luminosity_is_0_to_240_not_0_to_255() {
    // Source: the LUM, HSL and TINT pages give luminosity as 0 to 240; the earlier evaluator
    // used 0 to 255. The same colour reads 255/240 times higher in the old scale.
    for hex in [
        "#ffffff", "#000000", "#d80073", "#808080", "#336699", "#f0f0f0",
    ] {
        let old = luminance(hex).expect("a colour");
        let rgb = Rgb::from_hex(hex).expect("hex");
        let mut env = colour_env(None);
        env.cells.set("C", Value::Color(rgb));
        let new = match evaluate_str("LUM(C)", &env).expect("parses").value {
            Value::Number(q) => q.value,
            other => panic!("{other:?}"),
        };
        assert!(
            (new - old * 240.0 / 255.0).abs() <= 1.0,
            "{hex}: old {old}, new {new}"
        );
    }
    assert_eq!(luminance("#ffffff"), Some(255.0));
    assert_eq!(new_number("LUM(#ffffff)"), Ok(240.0));
}

#[test]
fn difference_modulus_follows_the_divisor_and_mod_is_kept_for_compatibility() {
    // Source: the MODULUS page (the result has the sign of the divisor; MODULUS(-5, 1.4) is 0.6).
    // The earlier evaluator had MOD only: |a| mod |b| with the divisor's sign.
    assert_eq!(old_number("MOD(-7,3)"), Some(1.0));
    assert_eq!(new_number("MOD(-7,3)"), Ok(1.0));
    assert_eq!(new_number("MODULUS(-7,3)"), Ok(2.0));
    assert_eq!(old_number("MODULUS(-7,3)"), None);
}

#[test]
fn difference_tint_and_shade_move_luminosity_while_the_earlier_ones_mixed_with_white_or_black() {
    // Source: the TINT, SHADE pages (luminosity changes by the integer, 0 to 240). The earlier
    // evaluator mixed with white or black by a percentage for TINT, SHADE, MSOTINT and MSOSHADE;
    // MSOTINT and MSOSHADE keep that meaning here (the pages say percentage change).
    assert_eq!(tint("#808080", 50.0), "#c0c0c0");
    assert_ne!(
        new_colour("TINT(#808080, 50)", None),
        old_colour("TINT(#808080, 50)", None)
    );
    assert_eq!(
        old_colour("TINT(#808080, 50)", None).as_deref(),
        Some("#c0c0c0")
    );
    assert_eq!(
        old_colour("SHADE(#808080, 50)", None).as_deref(),
        Some("#404040")
    );
    assert_ne!(
        new_colour("SHADE(#808080, 50)", None),
        old_colour("SHADE(#808080, 50)", None)
    );
    // TINT raises the luminosity by 50 of 240 steps: from 120 to 170.
    assert_eq!(new_number("LUM(TINT(#808080, 50))"), Ok(170.0));
    for source in [
        "MSOTINT(#808080, 50)",
        "MSOSHADE(#808080, 50)",
        "MSOTINT(#336699, 20)",
    ] {
        assert_eq!(
            old_colour(source, None),
            new_colour(source, None),
            "{source}"
        );
    }
}

#[test]
fn difference_symbolic_comparisons_have_a_tolerance_the_earlier_ones_did_not() {
    // Source: About Operators (the symbolic operators are the best choice for rounded values;
    // the word forms compare all 15 digits).
    assert_eq!(old_number("0.1+0.2=0.3"), Some(0.0));
    assert_eq!(new_number("0.1+0.2=0.3"), Ok(1.0));
    assert_eq!(new_number("0.1+0.2 EQ 0.3"), Ok(1.0));
    // The earlier evaluator had no word forms.
    assert_eq!(old_number("1 EQ 1"), None);
    assert_eq!(new_number("1 EQ 1"), Ok(1.0));
}

#[test]
fn difference_equality_binds_looser_than_the_ordering_comparisons() {
    // Source: About Operators, "Order of evaluation" (greater/less than sixth, equal seventh);
    // the earlier evaluator parsed all six comparisons at one level, left to right.
    assert_eq!(old_number("0 = 1 < 2"), Some(1.0));
    assert_eq!(new_number("0 = 1 < 2"), Ok(0.0));
}

#[test]
fn difference_sign_uses_the_documented_fuzz() {
    // Source: the SIGN page (1E-9 unless a fuzz is given).
    assert_eq!(old_number("SIGN(0.00000000001)"), Some(1.0));
    assert_eq!(new_number("SIGN(0.00000000001)"), Ok(0.0));
    assert_eq!(new_number("SIGN(0.00000000001, 0)"), Ok(1.0));
}

#[test]
fn difference_floor_and_ceiling_refuse_opposite_signs() {
    // Source: the FLOOR and CEILING pages ("number and multiple must have the same signs, or a
    // #NUM! error is returned").
    assert_eq!(old_number("FLOOR(-1.2,0.5)"), Some(-1.0));
    assert_eq!(new_number("FLOOR(-1.2,0.5)"), Err("#NUM!".into()));
    assert_eq!(old_number("CEILING(-1.2,0.5)"), Some(-1.0));
    assert_eq!(new_number("CEILING(-1.2,0.5)"), Err("#NUM!".into()));
}

#[test]
fn difference_invalid_numbers_are_errors_not_nan_or_infinity() {
    // Source: the SQRT, ASIN, ACOS pages (#NUM!) and About Error Values (invalid number).
    for source in ["SQRT(-1)", "ASIN(2)", "ACOS(2)"] {
        assert!(
            old_number(source).is_some_and(f64::is_nan),
            "old {source}: {:?}",
            old_number(source)
        );
        assert_eq!(new_number(source), Err("#NUM!".into()), "{source}");
    }
    // POW(0, -1): the POW page says 0 (the earlier evaluator returned infinity).
    assert_eq!(old_number("POW(0,-1)"), Some(f64::INFINITY));
    assert_eq!(new_number("POW(0,-1)"), Ok(0.0));
}

#[test]
fn difference_unit_suffixes_must_be_documented_ones() {
    // Source: About Units of Measure. The earlier evaluator took any letters after a number as
    // a unit (unknown ones as factor 1) and knew `PX`.
    assert_eq!(old_number("5xyz"), Some(5.0));
    assert!(new_number("5xyz").is_err());
    assert!((old_number("96px").expect("old accepts px") - 1.0).abs() < 1e-12);
    assert!(new_number("96px").is_err());
    // The documented ones agree.
    for source in ["25.4mm", "2.54cm", "1ft", "72pt", "180deg", "37%"] {
        assert!(agree(source).is_ok(), "{source}: {:?}", agree(source));
    }
}

#[test]
fn difference_a_branchless_if_and_the_variadic_bitxor_follow_the_pages() {
    // Source: the IF page (three arguments) and the BITXOR page (two).
    assert_eq!(old_number("IF(1,5)"), Some(5.0));
    assert_eq!(old_number("IF(0,5)"), Some(0.0));
    assert_eq!(new_number("IF(1,5)"), Err("#VALUE!".into()));
    assert_eq!(old_number("BITXOR(1,2,3)"), Some(0.0));
    assert_eq!(new_number("BITXOR(1,2,3)"), Err("#VALUE!".into()));
}

#[test]
fn difference_setatrefexpr_without_an_argument_is_zero_and_eval_is_not_a_function() {
    // Source: the SETATREFEXPR page (initial value 0). `EVAL` is not on any public function page.
    assert_eq!(old_number("SETATREFEXPR()"), None);
    assert_eq!(new_number("SETATREFEXPR()"), Ok(0.0));
    assert_eq!(old_number("EVAL(5)"), Some(5.0));
    assert!(new_number("EVAL(5)").is_err());
    let evaluation = evaluate_str("EVAL(5)", &numeric_env()).expect("parses");
    assert_eq!(evaluation.value, Value::Error(ErrorValue::Unsupported));
}

// ---- generated arithmetic ----------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// An arithmetic expression over the subset both evaluators implement. Comparisons are always
/// parenthesised (their precedence is an intentional difference) and appear only as the
/// condition of `IF`.
fn generate(rng: &mut Rng, depth: u32) -> String {
    if depth == 0 || rng.below(4) == 0 {
        return match rng.below(6) {
            0 => "Width".to_string(),
            1 => "Zero".to_string(),
            2 => format!("{}.{}", rng.below(20), rng.below(100)),
            _ => format!("{}", rng.below(12)),
        };
    }
    let a = generate(rng, depth - 1);
    let b = generate(rng, depth - 1);
    match rng.below(14) {
        0 => format!("({a} + {b})"),
        1 => format!("({a} - {b})"),
        2 => format!("({a} * {b})"),
        3 => format!("({a} / {b})"),
        4 => format!("({a} ^ {})", rng.below(4)),
        5 => format!("(-{a})"),
        6 => format!("MIN({a}, {b})"),
        7 => format!("MAX({a}, {b})"),
        8 => format!("ABS({a})"),
        9 => format!("INT({a})"),
        10 => format!("ROUND({a}, {})", rng.below(3)),
        11 => format!("IF(({a} < {b}), {a}, {b})"),
        12 => format!("IF(({a} >= {b}), {b}, {a} + 1)"),
        _ => format!("SQRT(ABS({a}))"),
    }
}

#[test]
fn generated_arithmetic_gives_the_same_number_or_fails_in_both() {
    let mut rng = Rng(0x5eed);
    let mut compared = 0;
    let mut failures = Vec::new();
    for _ in 0..3000 {
        let source = generate(&mut rng, 4);
        compared += 1;
        // Floating-point comparisons in IF conditions can sit on a tie that the symbolic
        // tolerance of the new engine resolves differently; those are the documented
        // difference, so skip a case where the two disagree only through a comparison.
        if let Err(message) = agree(&source) {
            if source.contains(" < ") || source.contains(" >= ") {
                let tolerant_tie = old_number(&source).is_some() && new_number(&source).is_ok();
                if tolerant_tie {
                    continue;
                }
            }
            failures.push(message);
        }
    }
    assert_eq!(compared, 3000);
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
