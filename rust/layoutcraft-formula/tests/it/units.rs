//! Units of measure, implicit units, dimensions of the results, and number display.

use crate::common::{boolean, check, X};
use layoutcraft_formula::{format_number, parse, Dim, ErrorValue, ParseErrorKind, Unit};
use std::f64::consts::PI;
use X::*;

const METER: f64 = 1.0 / 0.0254;

#[test]
fn length_units_convert_to_inches() {
    check(&[
        ("1 in", L(1.0)),
        ("2.54 cm", L(1.0)),
        ("25.4 mm", L(1.0)),
        ("1 ft", L(12.0)),
        ("1 yd", L(36.0)),
        ("1 mi", L(63_360.0)),
        ("72 pt", L(1.0)),
        ("6 p", L(1.0)),
        ("1 m", L(METER)),
        ("1 km", L(1000.0 * METER)),
        ("1 nm", L(1852.0 * METER)),
        ("1 IN_F", L(1.0)),
        ("1 MI_F", L(63_360.0)),
        ("0.7 pt.", L(0.7 / 72.0)),
        ("1.25in", L(1.25)),
        ("1.25IN", L(1.25)),
        ("1e1 in", L(10.0)),
        (".5 in", L(0.5)),
    ]);
}

#[test]
fn didot_and_cicero_follow_the_decision_of_the_unit_table() {
    // Unspecified in the sources: 2660 didots to the metre, 12 didots to the cicero.
    assert!(boolean("2660 d = 1 m"));
    assert!(boolean("1 c = 12 d"));
    assert!(boolean("1 c > 1 p"));
}

#[test]
fn angle_units_convert_to_radians() {
    check(&[
        ("180 deg", A(PI)),
        ("1 rad", A(1.0)),
        ("60'", A(PI / 180.0)),
        ("3600\"", A(PI / 180.0)),
        ("90\u{b0}", A(PI / 2.0)),
        ("90 \u{b0}", A(PI / 2.0)),
        ("45DEG", A(PI / 4.0)),
    ]);
}

#[test]
fn duration_units_convert_to_days() {
    check(&[
        ("1 ew", D(7.0)),
        ("1 ed", D(1.0)),
        ("24 eh", D(1.0)),
        ("1440 em", D(1.0)),
        ("86400 es", D(1.0)),
        ("2 eday", D(2.0)),
        ("2 eweek", D(14.0)),
        ("12 ehour", D(0.5)),
        ("30 eminute", D(30.0 / 1440.0)),
        ("90 esecond", D(90.0 / 86_400.0)),
        ("3 ed.", D(3.0)),
        ("2em", D(2.0 / 1440.0)),
    ]);
}

#[test]
fn area_units_have_the_area_dimension() {
    let metre_squared = METER * METER;
    check(&[
        ("1 acres", Sq(43_560.0 * 144.0)),
        ("1 ha", Sq(10_000.0 * metre_squared)),
        ("2 hectares", Sq(2.0 * 10_000.0 * metre_squared)),
        ("1 hectare", Sq(10_000.0 * metre_squared)),
        ("1 HA.", Sq(10_000.0 * metre_squared)),
        ("3 sq ft", Sq(3.0 * 144.0)),
        ("2 SQ. FT.", Sq(2.0 * 144.0)),
        ("2 SQ IN", Sq(2.0)),
        ("3 in^2", Sq(3.0)),
        ("3 IN.^2", Sq(3.0)),
        ("1 yd^2", Sq(1296.0)),
        ("1 mi^2", Sq(63_360.0 * 63_360.0)),
        ("1 m ^2", Sq(metre_squared)),
        ("1 sq km", Sq(1_000_000.0 * metre_squared)),
        ("1 cm^2", Sq(metre_squared / 10_000.0)),
        ("1 mm^2", Sq(metre_squared / 1_000_000.0)),
        ("1 sq m", Sq(metre_squared)),
        ("1 sq mi", Sq(63_360.0 * 63_360.0)),
        ("1 sq yd", Sq(1296.0)),
        // An area unit adds to a plain number like any dimensioned value.
        ("1 ft^2 + 1 in * 1 in", Sq(145.0)),
    ]);
}

#[test]
fn only_the_exponent_two_makes_an_area_unit_other_powers_stay_powers() {
    check(&[
        ("(2 ft)^2", Sq(576.0)),
        ("2 ft ^ 3 / 1 ft", Sq(1152.0)),
        ("5.2cm^1.5", E(ErrorValue::Dim)),
    ]);
    // `^2` right after a unit is the unit; the same text after a reference is a power.
    check(&[("Width ^ 2", Sq(16.0)), ("2 in^2 / 1 in", L(2.0))]);
}

#[test]
fn implicit_units_name_the_unit_and_the_measurement_system() {
    check(&[
        ("2 DL", L(2.0)),
        ("2 DP", L(2.0)),
        ("2 DT", L(2.0)),
        ("3 DA", A(3.0)),
        ("2 DE", D(2.0)),
        ("2[ft,D]", L(24.0)),
        ("5[in,P]", L(5.0)),
        ("5[pt,T]", L(5.0 / 72.0)),
        ("5[deg,A]", A(5.0 * PI / 180.0)),
        ("5[eh,E]", D(5.0 / 24.0)),
        ("5[in,d]", L(5.0)),
        ("5[in]", L(5.0)),
        ("5 [in,P]", L(5.0)),
    ]);
}

#[test]
fn a_number_with_no_unit_is_dimensionless() {
    check(&[("5", N(5.0)), ("1.5e3", N(1500.0)), ("5%", N(0.05))]);
}

#[test]
fn units_the_pages_do_not_list_are_parse_errors_not_guesses() {
    for source in [
        "5xyz", "5 inches", "5px", "5[xyz,P]", "5[in,Q]", "5 sq", "5sq xx", "1 sq deg",
    ] {
        let outcome = parse(source);
        assert!(outcome.is_err(), "{source:?} parsed as {outcome:?}");
    }
    // A length unit followed by `^2` is an area unit (D9), so a power written that way leaves
    // the unit behind it dangling.
    assert!(parse("1 in ^ 2 in").is_err());
    let error = parse("5xyz").expect_err("unknown unit");
    assert_eq!(error.kind, ParseErrorKind::UnknownUnit("xyz".into()));
}

#[test]
fn unit_names_are_found_ignoring_case_and_one_trailing_dot() {
    for (name, unit) in [
        ("IN", Unit::Inch),
        ("Ft.", Unit::Foot),
        ("pT", Unit::Point),
        ("EDAY", Unit::ElapsedDay),
        ("ehour.", Unit::ElapsedHour),
        ("MI_F", Unit::Mile),
        ("HA.", Unit::Hectare),
        ("ACRES", Unit::Acre),
        ("RAD", Unit::Radian),
        ("\u{b0}", Unit::Degree),
    ] {
        assert_eq!(Unit::parse(name), Some(unit), "{name}");
    }
    for name in [
        "", "inch", "inches", "feet", "foot", "degree", "acre", "px", "IN..", "x",
    ] {
        assert_eq!(Unit::parse(name), None, "{name:?}");
    }
}

#[test]
fn every_unit_has_one_dimension_and_a_positive_factor() {
    use Unit::*;
    let lengths = [
        Inch,
        Foot,
        Yard,
        Mile,
        NauticalMile,
        Millimeter,
        Centimeter,
        Meter,
        Kilometer,
        Point,
        Pica,
        Didot,
        Cicero,
    ];
    for unit in lengths {
        assert_eq!(unit.dim(), Dim::LENGTH, "{unit:?}");
        assert!(unit.factor() > 0.0);
        if let Some(area) = unit.squared() {
            assert_eq!(area.dim(), Dim::AREA, "{unit:?}");
            assert!((area.factor() - unit.factor() * unit.factor()).abs() < 1e-6 * area.factor());
        }
    }
    for unit in [Degree, Radian, ArcMinute, ArcSecond] {
        assert_eq!(unit.dim(), Dim::ANGLE);
    }
    for unit in [
        ElapsedWeek,
        ElapsedDay,
        ElapsedHour,
        ElapsedMinute,
        ElapsedSecond,
    ] {
        assert_eq!(unit.dim(), Dim::DURATION);
    }
    for unit in [Acre, Hectare] {
        assert_eq!(unit.dim(), Dim::AREA);
    }
    // Lengths without an area form in the units table have no squared unit.
    assert_eq!(Point.squared(), None);
    assert_eq!(Degree.squared(), None);
}

#[test]
fn dimensions_add_subtract_scale_and_stop_at_the_documented_range() {
    let area = Dim::LENGTH.checked_add(Dim::LENGTH).expect("2 fits");
    assert_eq!(area, Dim::AREA);
    assert_eq!(Dim::AREA.checked_sub(Dim::LENGTH), Some(Dim::LENGTH));
    assert_eq!(
        Dim::LENGTH.checked_scale(-128).map(|d| d.length),
        Some(-128)
    );
    assert_eq!(Dim::LENGTH.checked_scale(127).map(|d| d.length), Some(127));
    assert_eq!(Dim::LENGTH.checked_scale(128), None);
    assert_eq!(Dim::LENGTH.checked_scale(-129), None);
    assert_eq!(Dim::LENGTH.checked_scale(i32::MAX), None);
    assert_eq!(Dim::NONE.checked_scale(i32::MAX), Some(Dim::NONE));
    let max = Dim::LENGTH.checked_scale(127).expect("127 fits");
    assert_eq!(max.checked_add(Dim::LENGTH), None);
    let min = Dim::LENGTH.checked_scale(-128).expect("-128 fits");
    assert_eq!(min.checked_sub(Dim::LENGTH), None);
    assert!(Dim::NONE.is_none() && !Dim::LENGTH.is_none());
}

#[test]
fn numbers_are_shown_with_at_most_fifteen_significant_digits() {
    for (value, shown) in [
        (0.0, "0"),
        (-0.0, "0"),
        (1.0, "1"),
        (-2.5, "-2.5"),
        (0.1 + 0.2, "0.3"),
        (1.0 / 3.0, "0.333333333333333"),
        (2.0 / 3.0, "0.666666666666667"),
        (1e21, "1000000000000000000000"),
        (1e-7, "0.0000001"),
        (123_456_789_012_345_678.0, "123456789012346000"),
        (f64::NAN, "#NUM!"),
        (f64::INFINITY, "#NUM!"),
    ] {
        assert_eq!(format_number(value), shown);
    }
}
