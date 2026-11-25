//! Cell references of every form, resolved through a test host (the forms of "About Cell
//! References"), and the services a host provides per cell.

use crate::cases::X::{self, *};
use crate::common::matches;
use crate::host::{engine, formula, set, value, DocEngine};
use layoutcraft_formula::{ErrorValue, Value};

fn document() -> DocEngine {
    let mut e = engine();
    {
        let names = &mut e.host_mut().names;
        names.insert("star".into(), vec![2]);
        names.insert("executive".into(), vec![3, 4]);
        names.insert("shaft".into(), vec![7]);
        names.insert("manager".into(), vec![9]);
        names.insert("1-d".into(), vec![10]);
        names.insert("it's".into(), vec![11]);
    }
    set(&mut e, "Sheet.1!Width", Value::length(4.0));
    set(&mut e, "Sheet.1!Height", Value::length(2.0));
    set(&mut e, "Sheet.1!Char.Font[3]", Value::number(33.0));
    set(&mut e, "Sheet.1!Scratch.A5", Value::number(55.0));
    set(&mut e, "Sheet.1!User.Cost", Value::number(12.5));
    set(&mut e, "Sheet.1!Geometry1.X1", Value::length(0.5));
    set(&mut e, "Sheet.2!Angle", Value::angle(1.0));
    set(&mut e, "Sheet.3!Height", Value::length(1.0));
    set(&mut e, "Sheet.4!Height", Value::length(2.5));
    set(&mut e, "Sheet.8!FillForegnd", Value::number(5.0));
    set(
        &mut e,
        "Masters[Gear].Sheet.7!Geometry1.X1",
        Value::length(0.25),
    );
    set(&mut e, "Page-1!User.Vanishing_Point", Value::number(77.0));
    set(&mut e, "Pages[Page-3].Sheet.4!BeginX", Value::length(9.0));
    set(&mut e, "Style.9!LineColor", Value::number(3.0));
    set(&mut e, "Doc!PreviewQuality", Value::number(2.0));
    set(&mut e, "Sheet.10!LineColor", Value::number(6.0));
    set(&mut e, "Sheet.11!X", Value::number(11.0));
    formula(&mut e, "Sheet.1!Out", "0");
    e.recalculate();
    e
}

fn eval_in(e: &mut DocEngine, source: &str) -> Value {
    formula(e, "Sheet.1!Out", source);
    e.recalculate();
    value(e, "Sheet.1!Out")
}

fn check(cases: &[(&str, X)]) {
    let mut e = document();
    let mut failures = Vec::new();
    for (source, expected) in cases {
        let actual = eval_in(&mut e, source);
        if !matches(&actual, expected) {
            failures.push(format!("{source:?}: expected {expected:?}, got {actual:?}"));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn every_reference_form_of_the_references_page_reaches_its_cell() {
    check(&[
        // The same shape.
        ("Width", L(4.0)),
        ("width", L(4.0)),
        ("WIDTH", L(4.0)),
        // A shape by name, by ID and by name plus ID.
        ("Star!Angle", A(1.0)),
        ("STAR!angle", A(1.0)),
        ("Sheet.2!Angle", A(1.0)),
        ("sheet.2!Angle", A(1.0)),
        ("Executive.3!Height", L(1.0)),
        ("Executive.4!Height", L(2.5)),
        ("Sheet.8!FillForegnd", N(5.0)),
        // Section cells: named column with a row index, unnamed column.
        ("Char.Font[3]", N(33.0)),
        ("Char.Font[ 3 ]", N(33.0)),
        ("Scratch.A5", N(55.0)),
        ("User.Cost", N(12.5)),
        ("Geometry1.X1", L(0.5)),
        // A master, the page, another page, a style, the document.
        ("Masters[Gear]!Shaft!Geometry1.X1", L(0.25)),
        ("Masters[Gear]!Sheet.7!Geometry1.X1", L(0.25)),
        ("ThePage!User.Vanishing_Point", N(77.0)),
        ("Pages[Page-3]!Sheet.4!BeginX", L(9.0)),
        ("Styles!Manager!LineColor", N(3.0)),
        ("TheDoc!PreviewQuality", N(2.0)),
        // A nonstandard name in quotes, with a doubled quote for a quote inside.
        ("'1-D'!LineColor", N(6.0)),
        ("'It''s'!X", N(11.0)),
        // References inside expressions.
        ("Star!Angle + 1 rad", A(2.0)),
        ("Width * User.Cost", L(50.0)),
    ]);
}

#[test]
fn a_reference_that_does_not_name_a_cell_is_ref_error() {
    check(&[
        ("Nobody!Width", E(ErrorValue::Ref)),
        ("Sheet.99!Width", E(ErrorValue::Ref)),
        // Two shapes carry the name: the ID is required.
        ("Executive!Height", E(ErrorValue::Ref)),
        ("Executive.9!Height", E(ErrorValue::Ref)),
        ("NoSuchCell", E(ErrorValue::Ref)),
        ("Char.Font[4]", E(ErrorValue::Ref)),
        ("Masters[Pump]!Shaft!Geometry1.X1", E(ErrorValue::Ref)),
        ("Pages[Page-9]!Sheet.4!BeginX", E(ErrorValue::Ref)),
        ("ThePage!Nothing", E(ErrorValue::Ref)),
    ]);
}

#[test]
fn services_come_from_the_host_per_cell() {
    check(&[("ID()", N(1.0)), ("PAGENAME()", E(ErrorValue::Unsupported))]);
}

#[test]
fn formulaexists_tells_local_formulas_from_inherited_ones_and_constants() {
    let mut e = document();
    formula(&mut e, "Sheet.1!Computed", "2 in * 2");
    e.host_mut().inherited.push("sheet.1!height".to_string());
    for (source, expected) in [
        ("FORMULAEXISTS(Computed)", B(true)),
        ("LOCALFORMULAEXISTS(Computed)", B(true)),
        ("FORMULAEXISTS(Width)", B(false)),
        ("LOCALFORMULAEXISTS(Width)", B(false)),
        // The host reports an inherited formula for Height.
        ("FORMULAEXISTS(Height)", B(true)),
        ("LOCALFORMULAEXISTS(Height)", B(false)),
        ("FORMULAEXISTS(Nobody!Width)", E(ErrorValue::Ref)),
        ("FORMULAEXISTS(Star!Angle)", B(false)),
    ] {
        let actual = eval_in(&mut e, source);
        assert!(matches(&actual, &expected), "{source}: {actual:?}");
    }
}

#[test]
fn the_dependencies_of_a_formula_are_the_cells_its_references_resolve_to() {
    let mut e = document();
    formula(
        &mut e,
        "Sheet.1!Out",
        "Width + Sheet.1!Width + WIDTH + Star!Angle + User.Cost + Nobody!X",
    );
    // Three spellings of one cell count once; a reference that resolves to nothing is no edge.
    assert_eq!(
        e.dependencies(&"sheet.1!out".to_string()),
        vec![
            "sheet.1!width".to_string(),
            "sheet.2!angle".to_string(),
            "sheet.1!user.cost".to_string()
        ]
    );
    assert_eq!(
        e.dependents(&"sheet.1!width".to_string()),
        vec!["sheet.1!out".to_string()]
    );
}
