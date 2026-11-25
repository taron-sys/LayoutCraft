//! The function cases: one row per formula, with the function it exercises, a normal or edge/error
//! role and the result the public function pages give.
//!
//! This file is plain data. It is also compiled into `examples/function_table.rs`, which prints
//! the function table of `docs/46-formula-engine.md`, so it must not use the test helpers.
//!
//! Expected values come from the examples and remarks of the per-function pages (the page is named
//! in the group comments); where a page prints four decimals the case uses `Near*`. A case whose
//! expectation is an engine decision rather than a page statement says so in the formula's group.

use layoutcraft_formula::ErrorValue;

/// An expected result.
#[derive(Clone, Debug)]
pub enum X {
    /// A dimensionless number.
    N(f64),
    /// A length in inches.
    L(f64),
    /// An angle in radians.
    A(f64),
    /// A duration in days.
    D(f64),
    /// An area in square inches.
    Sq(f64),
    /// A dimensionless number the pages print with four decimals.
    Near(f64),
    /// An angle in radians printed with four decimals.
    NearA(f64),
    /// An angle printed in degrees with four decimals.
    NearDeg(f64),
    S(&'static str),
    B(bool),
    E(ErrorValue),
    /// A colour as `#rrggbb`.
    C(&'static str),
}

/// One formula and what it must evaluate to.
#[derive(Clone, Debug)]
pub struct Case {
    /// The function the case is about (upper case).
    pub function: &'static str,
    /// The test function that runs it.
    pub test: &'static str,
    pub formula: &'static str,
    pub want: X,
    /// A documented edge or error case (otherwise a normal one).
    pub edge: bool,
    /// Evaluate in a host that offers no services.
    pub bare_host: bool,
}

use ErrorValue as Err;
use X::*;

struct Builder {
    test: &'static str,
    cases: Vec<Case>,
}

impl Builder {
    fn new(test: &'static str) -> Self {
        Builder {
            test,
            cases: Vec::new(),
        }
    }

    fn push(&mut self, function: &'static str, formula: &'static str, want: X, edge: bool) {
        self.cases.push(Case {
            function,
            test: self.test,
            formula,
            want,
            edge,
            bare_host: false,
        });
    }

    /// A normal case.
    fn n(&mut self, function: &'static str, formula: &'static str, want: X) {
        self.push(function, formula, want, false);
    }

    /// An edge or error case.
    fn e(&mut self, function: &'static str, formula: &'static str, want: X) {
        self.push(function, formula, want, true);
    }

    /// An edge case in a host without services.
    fn bare(&mut self, function: &'static str, formula: &'static str, want: X) {
        self.cases.push(Case {
            function,
            test: self.test,
            formula,
            want,
            edge: true,
            bare_host: true,
        });
    }
}

/// Arithmetic, rounding, trigonometry, bits, points: the pages ABS ... PNTY.
pub fn math_cases() -> Vec<Case> {
    let mut b = Builder::new("math_functions_give_the_reference_values_and_errors");
    b.n("ABS", "ABS(-1.24)", N(1.24));
    b.n("ABS", "ABS(-2 in)", L(2.0));
    b.e("ABS", "ABS()", E(Err::Value));
    b.e("ABS", "ABS(1, 2)", E(Err::Value));
    b.e("ABS", "ABS(1/0)", E(Err::DivZero));
    // Operator functions named in the grammar: the same results as the operators.
    b.n("ADD", "ADD(1, 2)", N(3.0));
    b.e("ADD", "ADD(1 in, 1 deg)", E(Err::Dim));
    b.n("SUB", "SUB(5, 2)", N(3.0));
    b.e("SUB", "SUB(1, 1/0)", E(Err::DivZero));
    b.n("MUL", "MUL(3, 4)", N(12.0));
    b.e("MUL", "MUL(1 in, 1 in)", Sq(1.0));
    b.n("DIV", "DIV(8, 2)", N(4.0));
    b.e("DIV", "DIV(1, 0)", E(Err::DivZero));
    b.n("UMINUS", "UMINUS(3)", N(-3.0));
    b.e("UMINUS", "UMINUS(NA())", E(Err::Na));
    b.n("UPLUS", "UPLUS(3)", N(3.0));
    b.e("UPLUS", "UPLUS(2 in)", L(2.0));
    b.n("PCT", "PCT(50)", N(0.5));
    b.e("PCT", "PCT(PNT(1, 2))", E(Err::Value));
    // CEILING and FLOOR pages.
    b.n("CEILING", "CEILING(3.7)", N(4.0));
    b.n("CEILING", "CEILING(-3.7)", N(-4.0));
    b.n("CEILING", "CEILING(3.7, 0.25)", N(3.75));
    b.e("CEILING", "CEILING(3.7, -0.25)", E(Err::Num));
    b.e("CEILING", "CEILING(-3.7, 0.25)", E(Err::Num));
    b.e("CEILING", "CEILING(0, 5)", N(0.0));
    b.e("CEILING", "CEILING(3.7, 0)", N(0.0));
    b.e("CEILING", "CEILING(0.3, 0.1)", N(0.3));
    b.e("CEILING", "CEILING(1.1 in, 0.5 in)", L(1.5));
    b.n("FLOOR", "FLOOR(3.7)", N(3.0));
    b.n("FLOOR", "FLOOR(-3.7)", N(-3.0));
    b.n("FLOOR", "FLOOR(3.7, 0.5)", N(3.5));
    b.e("FLOOR", "FLOOR(-3.7, 0.5)", E(Err::Num));
    b.e("FLOOR", "FLOOR(3.7, 0)", N(0.0));
    b.e("FLOOR", "FLOOR(0.9 in, 0.25 in)", L(0.75));
    b.n("INT", "INT(7.2)", N(7.0));
    b.n("INT", "INT(-7.2)", N(-8.0));
    b.e("INT", "INT(3)", N(3.0));
    b.e("INT", "INT(2.5 in)", L(2.0));
    b.n("INTUP", "INTUP(3.2)", N(4.0));
    b.n("INTUP", "INTUP(-3.2)", N(-3.0));
    b.e("INTUP", "INTUP(3)", N(3.0));
    b.n("ROUND", "ROUND(123.654, 2)", N(123.65));
    b.n("ROUND", "ROUND(123.654, 0)", N(124.0));
    b.n("ROUND", "ROUND(123.654, -1)", N(120.0));
    b.e("ROUND", "ROUND(-123.654, -1)", N(-120.0));
    b.e("ROUND", "ROUND(1)", E(Err::Value));
    b.e("ROUND", "ROUND(1.5, 400)", N(1.5));
    b.e("ROUND", "ROUND(1.5, -400)", N(0.0));
    b.e("ROUND", "ROUND(1e300, 5)", N(1e300));
    b.e("ROUND", "ROUND(2.345 in, 1)", L(2.3));
    b.n("TRUNC", "TRUNC(123.654, 2)", N(123.65));
    b.n("TRUNC", "TRUNC(123.654, 0)", N(123.0));
    b.n("TRUNC", "TRUNC(123.654, -1)", N(120.0));
    b.e("TRUNC", "TRUNC(-123.654, 0)", N(-123.0));
    b.e("TRUNC", "TRUNC(1.5, 400)", N(1.5));
    b.n("SIGN", "SIGN(-5)", N(-1.0));
    b.n("SIGN", "SIGN(0)", N(0.0));
    b.n("SIGN", "SIGN(7)", N(1.0));
    b.e("SIGN", "SIGN(0.00000000001)", N(0.0));
    b.e("SIGN", "SIGN(0.00000000001, 0)", N(1.0));
    b.e("SIGN", "SIGN(0.5, 1)", N(0.0));
    b.e("SIGN", "SIGN(1/0)", E(Err::DivZero));
    b.n("SQRT", "SQRT(2)", Near(std::f64::consts::SQRT_2));
    b.n("SQRT", "SQRT(16 in * 1 in)", L(4.0));
    b.e("SQRT", "SQRT(-1)", E(Err::Num));
    b.e("SQRT", "SQRT(0)", N(0.0));
    b.e("SQRT", "SQRT(4 in)", E(Err::Dim));
    b.n("POW", "POW(5, 2)", N(25.0));
    b.n("POW", "POW(2, -1)", N(0.5));
    b.e("POW", "POW(2, 0)", N(1.0));
    b.e("POW", "POW(0, 0)", N(0.0));
    b.e("POW", "POW(0, -1)", N(0.0));
    b.e("POW", "POW(-2, 0.5)", N(0.0));
    b.e("POW", "POW(-2, -3)", E(Err::Num));
    b.e("POW", "POW(-2, 3)", N(-8.0));
    b.e("POW", "POW(2 in, 2)", Sq(4.0));
    b.e("POW", "POW(2 in, 0.5)", E(Err::Dim));
    b.e("POW", "POW(10, 400)", E(Err::Num));
    b.n("LN", "LN(42)", Near(3.7377));
    b.e("LN", "LN(0)", E(Err::Num));
    b.e("LN", "LN(-1)", E(Err::Num));
    b.n("LOG10", "LOG10(42)", Near(1.6232));
    b.e("LOG10", "LOG10(100)", N(2.0));
    b.e("LOG10", "LOG10(0)", E(Err::Num));
    b.n("PI", "PI()", N(std::f64::consts::PI));
    b.n(
        "PI",
        "ROUND((14 in / 2) ^ 2 * PI(), 4) = 153.938 in^2",
        B(true),
    );
    b.e("PI", "PI(1)", E(Err::Value));
    b.e("PI", "PI * 2", N(2.0 * std::f64::consts::PI));
    // MODULUS page: the result has the sign of the divisor.
    b.n("MODULUS", "MODULUS(5, 1.4)", Near(0.8));
    b.n("MODULUS", "MODULUS(5, -1.4)", Near(-0.6));
    b.n("MODULUS", "MODULUS(-5, 1.4)", Near(0.6));
    b.n("MODULUS", "MODULUS(-5, -1.4)", Near(-0.8));
    b.e("MODULUS", "MODULUS(5, 0)", E(Err::DivZero));
    b.e("MODULUS", "MODULUS(5 in, 2 in)", L(1.0));
    b.e("MODULUS", "MODULUS(6, 3)", N(0.0));
    b.n("MAX", "MAX(13 in, 1 ft, 20 cm)", L(13.0));
    b.n("MAX", "MAX(1, 5, 3)", N(5.0));
    b.e("MAX", "MAX()", E(Err::Value));
    b.e("MAX", "MAX(1 in, 1 deg)", E(Err::Dim));
    b.e("MAX", "MAX(7)", N(7.0));
    b.n("MIN", "MIN(13 in, 1 ft, 20 cm)", L(20.0 / 2.54));
    b.n("MIN", "MIN(4, 2, 3)", N(2.0));
    b.e("MIN", "MIN(-1, NA())", E(Err::Na));
    b.n("SUM", "SUM(5, 7, 12)", N(24.0));
    b.e("SUM", "SUM(1 in, 2 in)", L(3.0));
    b.e("SUM", "SUM(1 in, 1 ed)", E(Err::Dim));
    b.e("SUM", "SUM(1)", N(1.0));
    b.n("MAGNITUDE", "MAGNITUDE(1, 3, 1, 4)", N(5.0));
    b.e("MAGNITUDE", "MAGNITUDE(1, 3 in, 1, 4 in)", L(5.0));
    b.e("MAGNITUDE", "MAGNITUDE(2, 3, 0, 4)", N(6.0));
    b.e("MAGNITUDE", "MAGNITUDE(1, 3 in, 1, 4 deg)", E(Err::Dim));
    b.n("BITAND", "BITAND(12, 6)", N(4.0));
    b.e("BITAND", "BITAND(65537, 3)", N(1.0));
    b.e("BITAND", "BITAND(-1, 5)", N(5.0));
    b.n("BITOR", "BITOR(12, 6)", N(14.0));
    b.e("BITOR", "BITOR(0, 0)", N(0.0));
    b.n("BITXOR", "BITXOR(12, 6)", N(10.0));
    b.e("BITXOR", "BITXOR(5, 5)", N(0.0));
    b.n("BITNOT", "BITNOT(6)", N(65529.0));
    b.e("BITNOT", "BITNOT(0)", N(65535.0));
    b.e("BITNOT", "BITNOT(1 in)", E(Err::Dim));
    // Legacy names kept for the earlier evaluator (not on a public page): relations only.
    b.n("MOD", "MOD(7, 3)", N(1.0));
    b.e("MOD", "MOD(7, 3) = MODULUS(7, 3)", B(true));
    b.e("MOD", "MOD(-5, 1.4) = MODULUS(-5, 1.4)", B(false));
    b.e("MOD", "MOD(5, 0)", E(Err::DivZero));
    b.n("EXP", "EXP(LN(5))", N(5.0));
    b.e("EXP", "EXP(0)", N(1.0));
    b.e("EXP", "EXP(1000)", E(Err::Num));
    b.n("LOG", "LOG(1000)", N(3.0));
    b.e("LOG", "LOG(0)", E(Err::Num));
    b.n("AVG", "AVG(1, 2, 3)", N(2.0));
    b.e("AVG", "AVG()", E(Err::Value));
    b.n("AVERAGE", "AVERAGE(2 in, 4 in)", L(3.0));
    b.e("AVERAGE", "AVERAGE(1 in, 1 deg)", E(Err::Dim));
    // Trigonometry pages: radians unless the argument carries a unit.
    b.n("SIN", "SIN(45 deg)", Near(std::f64::consts::FRAC_1_SQRT_2));
    b.e("SIN", "SIN(0)", N(0.0));
    b.e("SIN", "SIN(PI() / 2)", N(1.0));
    b.e("SIN", "SIN(1 in)", E(Err::Dim));
    b.n("COS", "COS(45 deg)", Near(std::f64::consts::FRAC_1_SQRT_2));
    b.e("COS", "COS(0)", N(1.0));
    b.n("TAN", "TAN(45 deg)", Near(1.0));
    b.e("TAN", "TAN(1 ed)", E(Err::Dim));
    b.n("SINH", "SINH(45 deg)", Near(0.8687));
    b.e("SINH", "SINH(0)", N(0.0));
    b.n("COSH", "COSH(45 deg)", Near(1.3246));
    b.e("COSH", "COSH(0)", N(1.0));
    // The page prints TANH(-45) = -0.6558, which is the value for -45 degrees.
    b.n("TANH", "TANH(-45 deg)", Near(-0.6558));
    b.e("TANH", "TANH(-45)", N(-1.0));
    b.n("ASIN", "ASIN(1)", A(std::f64::consts::FRAC_PI_2));
    b.e("ASIN", "ASIN(2)", E(Err::Num));
    b.e("ASIN", "ASIN(-1)", A(-std::f64::consts::FRAC_PI_2));
    b.n("ACOS", "ACOS(0)", A(std::f64::consts::FRAC_PI_2));
    b.e("ACOS", "ACOS(-1.0001)", E(Err::Num));
    b.e("ACOS", "ACOS(1)", A(0.0));
    b.n("ATAN", "ATAN(1)", A(std::f64::consts::FRAC_PI_4));
    b.e("ATAN", "ATAN(1 in)", E(Err::Dim));
    // The page prints 29.0456; atan(1.25 / 2.25) is 29.0546 degrees (two digits transposed).
    b.n("ATAN2", "ATAN2(1.25, 2.25)", NearDeg(29.0546));
    b.n("ATAN2", "ATAN2(1, SQRT(3))", NearDeg(30.0));
    b.n("ATAN2", "ATAN2(1, 1)", NearDeg(45.0));
    b.e("ATAN2", "ATAN2(0, 0)", A(0.0));
    b.e("ATAN2", "ATAN2(1 in, 1 in)", NearDeg(45.0));
    b.e("ATAN2", "ATAN2(1 in, 1 deg)", E(Err::Dim));
    b.n("DEG", "DEG(PI() / 4)", N(45.0));
    b.e("DEG", "DEG(90 deg)", N(90.0));
    b.e("DEG", "DEG(1 in)", E(Err::Dim));
    b.n("RAD", "RAD(45)", Near(std::f64::consts::FRAC_PI_4));
    b.e("RAD", "RAD(45 deg)", NearDeg(45.0));
    b.e("RAD", "RAD(1 in)", E(Err::Dim));
    b.n("ANG360", "ANG360(395 deg)", NearDeg(35.0));
    b.n("ANG360", "ANG360(-9.8 rad)", NearA(2.7664));
    b.n("ANG360", "ANG360(45)", NearDeg(58.31));
    b.e("ANG360", "ANG360(360 deg)", A(0.0));
    b.e("ANG360", "ANG360(-1e-20)", A(0.0));
    b.e("ANG360", "ANG360(1 in)", E(Err::Dim));
    b.n("GRAVITY", "GRAVITY(2)", A(std::f64::consts::PI));
    b.n("GRAVITY", "GRAVITY(Angle * 2)", A(std::f64::consts::PI));
    b.e("GRAVITY", "GRAVITY(100 deg, 110 deg, 290 deg)", A(0.0));
    b.e("GRAVITY", "GRAVITY(100 deg, 290 deg, 110 deg)", A(0.0));
    b.e("GRAVITY", "GRAVITY(0)", A(0.0));
    b.e("GRAVITY", "GRAVITY(540 deg)", A(std::f64::consts::PI));
    b.e("GRAVITY", "GRAVITY(1 in)", E(Err::Dim));
    b.n("PNT", "PNTX(PNT(7, 12))", N(7.0));
    b.e("PNT", "PNT(1)", E(Err::Value));
    b.n("PNTX", "PNTX(PNT(7, 12))", N(7.0));
    b.e("PNTX", "PNTX(5)", E(Err::Value));
    b.e("PNTX", "PNTX(PNT(1 in, 2 in))", L(1.0));
    b.n("PNTY", "PNTY(PNT(7, 12))", N(12.0));
    b.e("PNTY", "PNTY(\"x\")", E(Err::Value));
    b.cases
}

/// The logic, error and selection functions.
pub fn logic_cases() -> Vec<Case> {
    let mut b = Builder::new("logic_functions_select_lazily_and_report_errors");
    b.n("IF", "IF(Height > 1.25 in, 5, 7)", N(5.0));
    b.n("IF", "IF(Height > 3 in, 5, 7)", N(7.0));
    b.e("IF", "IF(0, 1, 2)", N(2.0));
    b.e("IF", "IF(\"TRUE\", 1, 2)", N(1.0));
    b.e("IF", "IF(\"sheep\", 1, 2)", N(2.0));
    b.e("IF", "IF(1, 1, 1/0)", N(1.0));
    b.e("IF", "IF(0, Missing, 2)", N(2.0));
    b.e("IF", "IF(0, Bogus(1), 2)", N(2.0));
    b.e("IF", "IF(NA(), 1, 2)", E(Err::Na));
    b.e("IF", "IF(PNT(1, 2), 1, 2)", E(Err::Value));
    b.e("IF", "IF(1, 2)", E(Err::Value));
    b.n("IFERROR", "IFERROR(1/0, 5)", N(5.0));
    b.e("IFERROR", "IFERROR(3, 1/0)", N(3.0));
    b.e("IFERROR", "IFERROR(NA(), REF())", E(Err::Ref));
    b.e("IFERROR", "IFERROR(Missing, \"x\")", S("x"));
    b.e("IFERROR", "IFERROR(1)", E(Err::Value));
    b.n("AND", "AND(Height > 1, Width > 1)", B(true));
    b.e("AND", "AND(1, 0)", B(false));
    b.e("AND", "AND(\"TRUE\", 2)", B(true));
    b.e("AND", "AND(1, 1/0)", E(Err::DivZero));
    b.n("OR", "OR(Height > 1, Width > 9)", B(true));
    b.e("OR", "OR(0, 0)", B(false));
    b.e("OR", "OR(0, \"FALSE\", 0.5)", B(true));
    b.e("OR", "OR(1, NA())", E(Err::Na));
    b.n("NOT", "NOT(Height > 0.75 in)", B(false));
    b.e("NOT", "NOT(0)", B(true));
    b.e("NOT", "NOT()", E(Err::Value));
    b.n("ISERR", "ISERR(1/0)", B(true));
    b.e("ISERR", "ISERR(NA())", B(false));
    b.e("ISERR", "ISERR(5)", B(false));
    b.e("ISERR", "ISERR(Missing)", B(true));
    b.e("ISERR", "ISERR(Bogus(1))", B(true));
    b.n("ISERRNA", "ISERRNA(NA())", B(true));
    b.e("ISERRNA", "ISERRNA(1/0)", B(false));
    b.e("ISERRNA", "ISERRNA(\"8\")", B(false));
    b.n("ISERROR", "ISERROR(NA())", B(true));
    b.e("ISERROR", "ISERROR(1/0)", B(true));
    b.e("ISERROR", "ISERROR(1)", B(false));
    b.n("ISERRVALUE", "ISERRVALUE(PNT(1, 2) + 1)", B(true));
    b.e("ISERRVALUE", "ISERRVALUE(1/0)", B(false));
    b.e("ISERRVALUE", "ISERRVALUE(NA())", B(false));
    b.e("ISERRVALUE", "IF(ISERRVALUE(PNT(1, 2) + 1), 2, 9)", N(2.0));
    b.n("NA", "NA()", E(Err::Na));
    b.e("NA", "NA(1)", E(Err::Value));
    b.e("NA", "ISERRNA(NA())", B(true));
    b.n("REF", "REF()", E(Err::Ref));
    b.e("REF", "REF(1)", E(Err::Value));
    b.e("REF", "IFERROR(REF(), 3)", N(3.0));
    b.n("CHOOSE", "CHOOSE(2, 10, 20, 1/0)", N(20.0));
    b.e("CHOOSE", "CHOOSE(0, 1, 2)", E(Err::Value));
    b.e("CHOOSE", "CHOOSE(3, 1, 2)", E(Err::Value));
    b.e("CHOOSE", "CHOOSE(2.9, 1, 2, 3)", N(2.0));
    b.n("SWITCH", "SWITCH(3, 1, 10, 3, 30, 99)", N(30.0));
    b.e("SWITCH", "SWITCH(7, 1, 10, 3, 30, 99)", N(99.0));
    b.e("SWITCH", "SWITCH(7, 1, 10, 3, 30)", E(Err::Na));
    b.e("SWITCH", "SWITCH(1, 1, 5, 1/0, 6)", N(5.0));
    b.n("GUARD", "GUARD(5)", N(5.0));
    b.e("GUARD", "GUARD(1/0)", E(Err::DivZero));
    b.e("GUARD", "GUARD(Width + 0.5 in)", L(4.5));
    b.n("BOUND", "BOUND(5, 0, FALSE, 2, 8)", N(5.0));
    b.e(
        "BOUND",
        "BOUND(, 0, FALSE, 2 in, 2 in, FALSE, 4 in, 4 in, FALSE, 6 in, 6 in)",
        N(0.0),
    );
    b.e(
        "BOUND",
        "BOUND(Width * 0.5, 0, FALSE, Width * 0, Width * 1)",
        L(2.0),
    );
    b.e("BOUND", "BOUND(5, 0, FALSE, 2)", E(Err::Value));
    b.e("BOUND", "BOUND(5, 0, FALSE, 2, 8, FALSE)", E(Err::Value));
    b.n("DEPENDSON", "DEPENDSON(Width, Height)", B(false));
    b.e("DEPENDSON", "DEPENDSON(Missing, Bogus)", B(false));
    b.e("DEPENDSON", "DEPENDSON()", E(Err::Value));
    b.n("SETATREF", "SETATREF(Width)", L(4.0));
    b.e("SETATREF", "SETATREF(Width, 1, TRUE)", N(0.0));
    b.e("SETATREF", "SETATREF(Width, Missing, FALSE)", L(4.0));
    b.e("SETATREF", "SETATREF(Missing)", E(Err::Ref));
    b.n("SETATREFEVAL", "SETATREFEVAL(2 + 3)", N(5.0));
    b.e("SETATREFEVAL", "SETATREFEVAL()", E(Err::Value));
    b.n("SETATREFEXPR", "SETATREFEXPR(5)", N(5.0));
    b.e("SETATREFEXPR", "SETATREFEXPR()", N(0.0));
    b.e("SETATREFEXPR", "SETATREFEXPR(,)", E(Err::Value));
    b.e("SETATREFEXPR", "MAX(7, SETATREFEXPR())", N(7.0));
    b.n("FORMULAEXISTS", "FORMULAEXISTS(Width)", B(true));
    b.e("FORMULAEXISTS", "FORMULAEXISTS(Height)", B(true));
    b.e("FORMULAEXISTS", "FORMULAEXISTS(Zero)", B(false));
    b.e("FORMULAEXISTS", "FORMULAEXISTS(Missing)", E(Err::Ref));
    b.e("FORMULAEXISTS", "FORMULAEXISTS(1 + 1)", E(Err::Value));
    b.n("LOCALFORMULAEXISTS", "LOCALFORMULAEXISTS(Width)", B(true));
    b.e("LOCALFORMULAEXISTS", "LOCALFORMULAEXISTS(Height)", B(false));
    b.e("LOCALFORMULAEXISTS", "LOCALFORMULAEXISTS(Zero)", B(false));
    b.e(
        "LOCALFORMULAEXISTS",
        "LOCALFORMULAEXISTS(Missing)",
        E(Err::Ref),
    );
    b.cases
}

/// The string functions: the pages LEFT ... TEXTHEIGHT.
pub fn text_cases() -> Vec<Case> {
    let mut b = Builder::new("text_functions_give_the_reference_strings_and_errors");
    b.n("LEFT", "LEFT(\"January 1, 2004\", 3)", S("Jan"));
    b.e("LEFT", "LEFT(\"abc\")", S("a"));
    b.e("LEFT", "LEFT(\"abc\", 10)", S("abc"));
    b.e("LEFT", "LEFT(\"abc\", 0)", S(""));
    b.e("LEFT", "LEFT(\"abc\", -1)", E(Err::Value));
    b.e("LEFT", "LEFT(\"h\u{e9}llo\", 2)", S("h\u{e9}"));
    b.e("LEFT", "LEFT(\"\u{1f600}x\", 1)", S("\u{1f600}"));
    b.n("RIGHT", "RIGHT(\"January 1, 2004\", 4)", S("2004"));
    b.e("RIGHT", "RIGHT(\"abc\")", S("c"));
    b.e("RIGHT", "RIGHT(\"abc\", 0)", S(""));
    b.e("RIGHT", "RIGHT(\"abc\", 9)", S("abc"));
    b.e("RIGHT", "RIGHT(\"abc\", -1)", E(Err::Value));
    b.n("MID", "MID(\"SSN 999-99-9999\", 5, 11)", S("999-99-9999"));
    b.e("MID", "MID(\"abc\", 4, 1)", S(""));
    b.e("MID", "MID(\"abc\", 0, 1)", E(Err::Value));
    b.e("MID", "MID(\"abcdef\", 2, -1)", S("bcdef"));
    b.e("MID", "MID(\"abcdef\", 2, 100)", S("bcdef"));
    b.e("MID", "MID(\"abc\", 2, 0)", S(""));
    b.n("LEN", "LEN(\"Seattle, WA\")", N(11.0));
    b.e("LEN", "LEN(\"\")", N(0.0));
    b.e("LEN", "LEN(\"\u{65e5}\u{672c}\u{8a9e}\")", N(3.0));
    b.e("LEN", "LEN(12.5)", N(4.0));
    b.n("LOWER", "LOWER(\"mIxEd CAse\")", S("mixed case"));
    b.e("LOWER", "LOWER(5)", S("5"));
    b.e("LOWER", "LOWER(\"\u{c4}\")", S("\u{e4}"));
    b.n("UPPER", "UPPER(\"mIxEd CAse\")", S("MIXED CASE"));
    b.e("UPPER", "UPPER(TRUE)", S("TRUE"));
    b.n("TRIM", "TRIM(\"January 1, 2003 \")", S("January 1, 2003"));
    b.e("TRIM", "TRIM(\"  a   b  \")", S("a b"));
    b.e("TRIM", "TRIM(\"\")", S(""));
    b.n("FIND", "FIND(\"2003\", \"January 1, 2003\")", N(12.0));
    b.e("FIND", "FIND(\"x\", \"abc\")", E(Err::Value));
    b.e("FIND", "FIND(\"\", \"abc\")", N(1.0));
    b.e("FIND", "FIND(\"\", \"abc\", 3)", N(3.0));
    b.e("FIND", "FIND(\"a\", \"abc\", 0)", E(Err::Value));
    b.e("FIND", "FIND(\"a\", \"abc\", 4)", E(Err::Value));
    b.e("FIND", "FIND(\"A\", \"abc\")", E(Err::Value));
    b.e("FIND", "FIND(\"A\", \"abc\", 1, TRUE)", N(1.0));
    b.e("FIND", "FIND(\"b\", \"abcb\", 3)", N(4.0));
    b.e("FIND", "FIND(\"a\", \"\")", E(Err::Value));
    b.n(
        "SUBSTITUTE",
        "SUBSTITUTE(\"1 January 2003\", \"January\", \"JAN\")",
        S("1 JAN 2003"),
    );
    b.e(
        "SUBSTITUTE",
        "SUBSTITUTE(\"1 January 2003\", \"january\", \"JAN\")",
        S("1 January 2003"),
    );
    b.e(
        "SUBSTITUTE",
        "SUBSTITUTE(\"1 January 2003\", \"january\", \"JAN\", , TRUE)",
        S("1 JAN 2003"),
    );
    b.e(
        "SUBSTITUTE",
        "SUBSTITUTE(\"aaa\", \"a\", \"b\", 2)",
        S("aba"),
    );
    b.e(
        "SUBSTITUTE",
        "SUBSTITUTE(\"aaa\", \"a\", \"b\", 0)",
        E(Err::Value),
    );
    b.e(
        "SUBSTITUTE",
        "SUBSTITUTE(\"aaa\", \"a\", \"b\", 9)",
        S("aaa"),
    );
    b.e("SUBSTITUTE", "SUBSTITUTE(\"abc\", \"\", \"x\")", S("abc"));
    b.e("SUBSTITUTE", "SUBSTITUTE(\"aaa\", \"aa\", \"b\")", S("ba"));
    b.n(
        "REPLACE",
        "REPLACE(\"01/03/2002\", 9, 2, \"03\")",
        S("01/03/2003"),
    );
    b.e("REPLACE", "REPLACE(\"abc\", 1, 0, \"X\")", S("Xabc"));
    b.e("REPLACE", "REPLACE(\"abc\", 0, 1, \"X\")", E(Err::Value));
    b.e("REPLACE", "REPLACE(\"abc\", 2, -1, \"\")", E(Err::Value));
    b.e("REPLACE", "REPLACE(\"abc\", 10, 1, \"X\")", S("abcX"));
    b.n("REPT", "REPT(\"*\", 5)", S("*****"));
    b.e("REPT", "REPT(\"ab\", 0)", S(""));
    b.e("REPT", "REPT(\"ab\", 2.9)", S("abab"));
    b.e("REPT", "REPT(\"ab\", -1)", E(Err::Value));
    b.e("REPT", "REPT(\"x\", 2000000)", E(Err::Value));
    b.e("REPT", "REPT(\"\", 1e9)", S(""));
    b.n("CHAR", "CHAR(9)", S("\t"));
    b.e("CHAR", "CHAR(65)", S("A"));
    b.e("CHAR", "CHAR(0)", E(Err::Value));
    b.e("CHAR", "CHAR(256)", E(Err::Value));
    b.e("CHAR", "CHAR(255)", S("\u{ff}"));
    b.n("UNICHAR", "UNICHAR(65)", S("A"));
    b.e("UNICHAR", "UNICHAR(8364)", S("\u{20ac}"));
    b.e("UNICHAR", "UNICHAR(0)", E(Err::Value));
    b.e("UNICHAR", "UNICHAR(65536)", E(Err::Value));
    b.e("UNICHAR", "UNICHAR(55296)", E(Err::Value));
    b.n("INDEX", "INDEX(3, \"cat;rat;;goat\")", S("goat"));
    b.n("INDEX", "INDEX(54, \";1;2;3;\", , \"ERROR\")", S("ERROR"));
    b.e("INDEX", "INDEX(2, \"cat;rat;;goat\")", S(""));
    b.e("INDEX", "INDEX(0, \";a\")", S(""));
    b.e("INDEX", "INDEX(1, \"a|b\", \"|\")", S("b"));
    b.e("INDEX", "INDEX(-1, \"a;b\")", S(""));
    b.e("INDEX", "INDEX(5, \"a;b\", , 7)", N(7.0));
    b.e("INDEX", "INDEX(1, \"a--b--c\", \"--\")", S("b"));
    b.n("LOOKUP", "LOOKUP(\"rat\", \"cat;rat;;goat\")", N(1.0));
    b.n("LOOKUP", "LOOKUP(\"\", \";cat;rat;;goat\")", N(0.0));
    b.n("LOOKUP", "LOOKUP(\"t\", \"cat;rat;;goat\", \"a\")", N(3.0));
    b.e("LOOKUP", "LOOKUP(\"RAT\", \"cat;rat\")", N(1.0));
    b.e("LOOKUP", "LOOKUP(\"x\", \"a;b\")", N(-1.0));
    b.e("LOOKUP", "LOOKUP(\"a;b\", \"a;b\")", N(-1.0));
    b.n("STRSAME", "STRSAME(\"cat\", \"cat\")", B(true));
    b.n("STRSAME", "STRSAME(\"cat\", \"dog\")", B(false));
    b.e("STRSAME", "STRSAME(\"cat\", \"CAT\")", B(false));
    b.e("STRSAME", "STRSAME(\"cat\", \"CAT\", TRUE)", B(true));
    b.e("STRSAME", "STRSAME(\"c\u{e4}t\", \"CAT\", TRUE)", B(false));
    b.e(
        "STRSAME",
        "STRSAME(\"c\u{e4}t\", \"C\u{c4}T\", TRUE)",
        B(true),
    );
    b.n("STRSAMEEX", "STRSAMEEX(\"cat\", \"CAT\", 0, 1)", B(true));
    b.e("STRSAMEEX", "STRSAMEEX(\"cat\", \"CAT\", 0, 0)", B(false));
    b.e(
        "STRSAMEEX",
        "STRSAMEEX(\"cat\", \"CAT\", 0, 2)",
        E(Err::Unsupported),
    );
    b.e(
        "STRSAMEEX",
        "STRSAMEEX(\"cat\", \"cat\", 0, 65536)",
        E(Err::Unsupported),
    );
    b.n("SHAPETEXT", "SHAPETEXT(TheText)", S("Hello"));
    b.e("SHAPETEXT", "SHAPETEXT(Messy, 84)", S("a b c d"));
    b.e("SHAPETEXT", "SHAPETEXT(Messy, 64)", S("a b c d"));
    b.e("SHAPETEXT", "SHAPETEXT(Messy, 4)", S("a  b\r\n c   d"));
    b.e("SHAPETEXT", "SHAPETEXT(Messy, 16)", S("a\t b   c   d"));
    b.e("SHAPETEXT", "SHAPETEXT(Quotes, 32)", S("'x' \"y\""));
    b.e(
        "SHAPETEXT",
        "SHAPETEXT(Quotes, 0)",
        S("\u{2018}x\u{2019} \u{201c}y\u{201d}"),
    );
    b.e("SHAPETEXT", "SHAPETEXT(TheText, 1)", E(Err::Unsupported));
    b.e("SHAPETEXT", "SHAPETEXT(TheText, 8)", E(Err::Unsupported));
    b.e("SHAPETEXT", "SHAPETEXT(Missing)", E(Err::Ref));
    b.n("TEXTWIDTH", "TEXTWIDTH(TheText)", L(0.5));
    b.e("TEXTWIDTH", "TEXTWIDTH(TheText, 0.2 in)", L(0.2));
    b.bare("TEXTWIDTH", "TEXTWIDTH(TheText)", E(Err::Unsupported));
    b.n("TEXTHEIGHT", "TEXTHEIGHT(TheText, Width - 0.5 in)", L(0.2));
    b.bare("TEXTHEIGHT", "TEXTHEIGHT(TheText)", E(Err::Unsupported));
    b.cases
}

/// The colour and theme functions.
pub fn colour_cases() -> Vec<Case> {
    let mut b = Builder::new("colour_functions_follow_the_240_scale_and_report_errors");
    // RGB / RED / GREEN / BLUE pages.
    b.n("RGB", "RGB(0, 0, 255)", C("#0000ff"));
    b.e("RGB", "RGB(300, -5, 0)", C("#ff0000"));
    b.e("RGB", "RGB(1 in, 0, 0)", E(Err::Dim));
    b.n("RED", "RED(RGB(10, 20, 30))", N(10.0));
    b.e("RED", "RED(\"zzz\")", N(0.0));
    b.e("RED", "RED(22)", N(51.0));
    b.e("RED", "RED(\"#102030\")", N(16.0));
    b.n("GREEN", "GREEN(RGB(10, 20, 30))", N(20.0));
    b.e("GREEN", "GREEN(11)", N(128.0));
    b.e("GREEN", "GREEN(9999)", N(0.0));
    b.n("BLUE", "BLUE(RGB(10, 20, 30))", N(30.0));
    b.e("BLUE", "BLUE(13)", N(128.0));
    b.e("BLUE", "BLUE(PNT(1, 2))", N(0.0));
    // HSL page: its table of standard colours (black's luminosity is printed as 24 there, which
    // does not match the 0..240 scale of the other rows; black is tested at 0).
    b.n("HSL", "HSL(160, 240, 120)", C("#0000ff"));
    b.n("HSL", "HSL(80, 240, 120)", C("#00ff00"));
    b.n("HSL", "HSL(120, 240, 120)", C("#00ffff"));
    b.n("HSL", "HSL(0, 240, 120)", C("#ff0000"));
    b.n("HSL", "HSL(200, 240, 120)", C("#ff00ff"));
    b.n("HSL", "HSL(40, 240, 120)", C("#ffff00"));
    b.n("HSL", "HSL(0, 0, 240)", C("#ffffff"));
    b.e("HSL", "HSL(0, 0, 0)", C("#000000"));
    b.e("HSL", "HSL(400, 999, 120) = HSL(160, 240, 120)", B(true));
    b.e("HSL", "HSL(0, 240, 9999)", C("#ffffff"));
    b.e("HSL", "HSL(0, -5, 120)", C("#808080"));
    b.n("HUE", "HUE(HSL(10, 240, 120))", N(10.0));
    b.e("HUE", "ABS(HUE(HSL(10, 20, 30)) - 10) <= 5", B(true));
    b.n("HUE", "HUE(RGB(0, 0, 255))", N(160.0));
    b.e("HUE", "HUE(7)", N(120.0));
    b.e("HUE", "HUE(9999)", N(0.0));
    b.e("HUE", "HUE(\"nonsense\")", N(0.0));
    b.n("SAT", "SAT(HSL(10, 240, 120))", N(240.0));
    b.e("SAT", "ABS(SAT(HSL(10, 20, 30)) - 20) <= 5", B(true));
    b.e("SAT", "SAT(8)", N(240.0));
    b.e("SAT", "SAT(RGB(128, 128, 128))", N(0.0));
    b.n("LUM", "LUM(HSL(10, 20, 30))", N(30.0));
    b.e("LUM", "LUM(6)", N(120.0));
    b.e("LUM", "LUM(RGB(255, 255, 255))", N(240.0));
    b.e("LUM", "LUM(RGB(0, 0, 0))", N(0.0));
    b.n(
        "HUEDIFF",
        "HUEDIFF(RGB(0, 0, 255), RGB(255, 0, 0))",
        N(160.0),
    );
    b.e(
        "HUEDIFF",
        "HUEDIFF(RGB(255, 0, 0), RGB(0, 0, 255))",
        N(-160.0),
    );
    b.e("HUEDIFF", "HUEDIFF(\"zzz\", RGB(0, 0, 0))", E(Err::Value));
    b.n(
        "SATDIFF",
        "SATDIFF(RGB(0, 0, 255), RGB(128, 128, 128))",
        N(240.0),
    );
    b.e("SATDIFF", "SATDIFF(1/0, RGB(0, 0, 0))", E(Err::DivZero));
    b.n(
        "LUMDIFF",
        "LUMDIFF(RGB(255, 255, 255), RGB(0, 0, 0))",
        N(240.0),
    );
    b.e(
        "LUMDIFF",
        "LUMDIFF(RGB(0, 0, 0), RGB(255, 255, 255))",
        N(-240.0),
    );
    b.e("LUMDIFF", "LUMDIFF(7, 7)", N(0.0));
    // TINT / SHADE / TONE pages: luminosity (saturation) moves by `int` and stays in 0..=240.
    b.n("TINT", "LUM(TINT(RGB(255, 0, 0), 60))", N(180.0));
    b.n("TINT", "HUE(TINT(RGB(255, 0, 0), 60))", N(0.0));
    b.n("TINT", "SAT(TINT(RGB(255, 0, 0), 60))", N(240.0));
    b.e("TINT", "TINT(RGB(255, 0, 0), 1000)", C("#ffffff"));
    b.e("TINT", "TINT(RGB(255, 0, 0), -1000)", C("#000000"));
    b.e("TINT", "TINT(RGB(255, 0, 0), 0)", C("#ff0000"));
    b.e(
        "TINT",
        "TINT(RGB(255, 0, 0), 1.9) = TINT(RGB(255, 0, 0), 1)",
        B(true),
    );
    b.e("TINT", "TINT(\"zzz\", 5)", E(Err::Value));
    b.n("SHADE", "LUM(SHADE(RGB(255, 0, 0), 60))", N(60.0));
    b.e("SHADE", "SHADE(RGB(255, 0, 0), 1000)", C("#000000"));
    b.e(
        "SHADE",
        "SHADE(RGB(255, 0, 0), -60) = TINT(RGB(255, 0, 0), 60)",
        B(true),
    );
    b.n("TONE", "SAT(TONE(RGB(255, 0, 0), 120))", N(120.0));
    b.n("TONE", "LUM(TONE(RGB(255, 0, 0), 120))", N(120.0));
    b.e("TONE", "TONE(RGB(255, 0, 0), 1000)", C("#808080"));
    b.e("TONE", "TONE(RGB(255, 0, 0), -1000)", C("#ff0000"));
    // MSOTINT / MSOSHADE: the percentage towards white (black); the pages give no values.
    b.n("MSOTINT", "MSOTINT(RGB(100, 100, 100), 100)", C("#ffffff"));
    b.e("MSOTINT", "MSOTINT(RGB(100, 100, 100), 0)", C("#646464"));
    b.e(
        "MSOTINT",
        "RED(MSOTINT(RGB(100, 100, 100), 50)) > 100",
        B(true),
    );
    b.e(
        "MSOTINT",
        "RED(MSOTINT(RGB(100, 100, 100), 50)) < 255",
        B(true),
    );
    b.e("MSOTINT", "MSOTINT(RGB(100, 100, 100), 500)", C("#ffffff"));
    b.e(
        "MSOTINT",
        "RED(MSOTINT(RGB(240, 240, 240), 50)) - 240 < RED(MSOTINT(RGB(128, 128, 128), 50)) - 128",
        B(true),
    );
    b.n(
        "MSOSHADE",
        "MSOSHADE(RGB(100, 100, 100), 100)",
        C("#000000"),
    );
    b.e(
        "MSOSHADE",
        "RED(MSOSHADE(RGB(100, 100, 100), 50)) < 100",
        B(true),
    );
    b.e(
        "MSOSHADE",
        "MSOSHADE(RGB(100, 100, 100), -100)",
        C("#ffffff"),
    );
    // BLEND page: 0.25 is 75 % of the first colour and 25 % of the second.
    b.n(
        "BLEND",
        "BLEND(RGB(0, 0, 0), RGB(255, 255, 255), 0)",
        C("#000000"),
    );
    b.n(
        "BLEND",
        "BLEND(RGB(0, 0, 0), RGB(255, 255, 255), 1)",
        C("#ffffff"),
    );
    b.e(
        "BLEND",
        "RED(BLEND(RGB(0, 0, 0), RGB(255, 255, 255), 0.25)) < RED(BLEND(RGB(0, 0, 0), RGB(255, 255, 255), 0.5))",
        B(true),
    );
    b.e("BLEND", "BLEND(6, 7, 0)", C("#ff00ff"));
    b.e(
        "BLEND",
        "BLEND(RGB(0, 0, 0), RGB(255, 255, 255), 1.5)",
        E(Err::Num),
    );
    b.e(
        "BLEND",
        "BLEND(RGB(0, 0, 0), RGB(255, 255, 255), -0.1)",
        E(Err::Num),
    );
    // THEMEVAL page.
    b.n("THEMEVAL", "THEMEVAL()", C("#202020"));
    b.n("THEMEVAL", "THEMEVAL(\"Light\")", C("#f0f0f0"));
    b.n("THEMEVAL", "THEMEVAL(5)", C("#010203"));
    b.n("THEMEVAL", "THEMEVAL(\"5\")", C("#010203"));
    b.n("THEMEVAL", "THEMEVAL(\"LineWeight\")", L(0.01));
    b.e("THEMEVAL", "THEMEVAL(\"Nope\", 7)", N(7.0));
    b.e("THEMEVAL", "THEMEVAL(\"Nope\")", E(Err::Na));
    b.e("THEMEVAL", "THEMEVAL(9)", E(Err::Value));
    b.e("THEMEVAL", "THEMEVAL(2.5)", E(Err::Value));
    b.e("THEMEVAL", "THEMEVAL(TRUE)", E(Err::Value));
    b.bare(
        "THEMEVAL",
        "THEMEVAL(\"LineWeight\", \"0.7 pt.\")",
        S("0.7 pt."),
    );
    b.n("THEME", "THEME(\"Light\")", C("#f0f0f0"));
    b.e("THEME", "THEME(\"Nope\")", E(Err::Na));
    b.n("THEMEGUARD", "THEMEGUARD(THEME(\"Light\"))", C("#f0f0f0"));
    b.e("THEMEGUARD", "THEMEGUARD(5)", N(5.0));
    b.n("ISTHEMED", "ISTHEMED()", B(true));
    b.e("ISTHEMED", "IF(ISTHEMED, 1, 2)", N(1.0));
    b.bare("ISTHEMED", "ISTHEMED()", E(Err::Unsupported));
    b.n("CELLISTHEMED", "CELLISTHEMED()", B(true));
    b.bare("CELLISTHEMED", "CELLISTHEMED()", E(Err::Unsupported));
    b.cases
}

/// The functions that read the shape being evaluated.
pub fn host_cases() -> Vec<Case> {
    let mut b = Builder::new("host_functions_read_the_services_or_report_unsupported");
    b.n("ID", "ID()", N(7.0));
    b.bare("ID", "ID()", E(Err::Unsupported));
    b.n("NAME", "NAME()", S("Box.7"));
    b.e("NAME", "NAME(750)", S("Box.7"));
    b.bare("NAME", "NAME()", E(Err::Unsupported));
    b.n("PAGENAME", "PAGENAME()", S("Page-1"));
    b.bare("PAGENAME", "PAGENAME()", E(Err::Unsupported));
    b.n("MASTERNAME", "MASTERNAME()", S("<no master>"));
    b.bare("MASTERNAME", "MASTERNAME()", E(Err::Unsupported));
    b.n("IS1D", "IS1D()", B(false));
    b.bare("IS1D", "IS1D()", E(Err::Unsupported));
    b.n("RAND", "RAND()", N(0.25));
    b.bare("RAND", "RAND()", E(Err::Unsupported));
    b.cases
}

/// Every case of every group.
pub fn all_cases() -> Vec<Case> {
    let mut all = math_cases();
    all.extend(logic_cases());
    all.extend(text_cases());
    all.extend(colour_cases());
    all.extend(host_cases());
    all
}
