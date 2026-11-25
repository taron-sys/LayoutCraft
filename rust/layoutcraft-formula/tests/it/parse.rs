//! Lexing and parsing: precedence, references, units, spans, errors and limits.

use layoutcraft_formula::{
    parse, BinaryOp, CellRef, CmpOp, Container, Expr, ExprKind, ParseErrorKind, Precision,
    SheetTarget, Span, UnaryOp, MAX_DEPTH, MAX_FORMULA_BYTES,
};

fn sexp(expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Number { value, unit: None } => format!("{value}"),
        ExprKind::Number {
            value,
            unit: Some(unit),
        } => format!("{value}{unit:?}"),
        ExprKind::Str(text) => format!("{text:?}"),
        ExprKind::Bool(flag) => format!("{flag}"),
        ExprKind::Color(rgb) => rgb.to_hex(),
        ExprKind::Ref(reference) => reference.to_string(),
        ExprKind::Empty => "_".to_string(),
        ExprKind::Unary { op, operand } => {
            let op = match op {
                UnaryOp::Plus => "+u",
                UnaryOp::Minus => "-u",
                UnaryOp::Percent => "%",
            };
            format!("({op} {})", sexp(operand))
        }
        ExprKind::Binary { op, lhs, rhs } => {
            let op = match op {
                BinaryOp::Add => "+".to_string(),
                BinaryOp::Sub => "-".to_string(),
                BinaryOp::Mul => "*".to_string(),
                BinaryOp::Div => "/".to_string(),
                BinaryOp::Pow => "^".to_string(),
                BinaryOp::Concat => "&".to_string(),
                BinaryOp::Compare(cmp, precision) => {
                    let symbol = match cmp {
                        CmpOp::Eq => "=",
                        CmpOp::Ne => "<>",
                        CmpOp::Lt => "<",
                        CmpOp::Le => "<=",
                        CmpOp::Gt => ">",
                        CmpOp::Ge => ">=",
                    };
                    if *precision == Precision::Exact {
                        format!("exact{symbol}")
                    } else {
                        symbol.to_string()
                    }
                }
            };
            format!("({op} {} {})", sexp(lhs), sexp(rhs))
        }
        ExprKind::Call { name, args, .. } => {
            let args: Vec<String> = args.iter().map(sexp).collect();
            format!("({name} {})", args.join(" "))
        }
    }
}

fn tree(source: &str) -> String {
    match parse(source) {
        Ok(expr) => sexp(&expr),
        Err(error) => panic!("{source:?} should parse: {error}"),
    }
}

fn fails(source: &str) -> (ParseErrorKind, Span) {
    match parse(source) {
        Ok(expr) => panic!("{source:?} should not parse, got {}", sexp(&expr)),
        Err(error) => (error.kind, error.span),
    }
}

fn reference(source: &str) -> CellRef {
    match parse(source).map(|e| e.kind) {
        Ok(ExprKind::Ref(reference)) => reference,
        other => panic!("{source:?} should be a reference, got {other:?}"),
    }
}

#[test]
fn multiplication_binds_tighter_than_addition_and_parentheses_override() {
    assert_eq!(tree("4 + 5 * 6"), "(+ 4 (* 5 6))");
    assert_eq!(tree("(4 + 5) * 6"), "(* (+ 4 5) 6)");
}

#[test]
fn binary_operators_of_one_level_associate_to_the_left() {
    assert_eq!(tree("8 - 3 - 2"), "(- (- 8 3) 2)");
    assert_eq!(tree("8 / 4 / 2"), "(/ (/ 8 4) 2)");
    assert_eq!(tree("2 ^ 3 ^ 2"), "(^ (^ 2 3) 2)");
}

#[test]
fn unary_signs_and_percent_bind_tighter_than_power() {
    assert_eq!(tree("-2 ^ 2"), "(^ (-u 2) 2)");
    assert_eq!(tree("2 ^ -3"), "(^ 2 (-u 3))");
    assert_eq!(tree("50% * 4"), "(* (% 50) 4)");
    assert_eq!(tree("--5"), "(-u (-u 5))");
}

#[test]
fn concatenation_sits_between_addition_and_the_comparisons() {
    assert_eq!(tree("1 + 2 & 3 < 4"), "(< (& (+ 1 2) 3) 4)");
}

#[test]
fn equality_is_looser_than_ordering() {
    assert_eq!(tree("a < b = c > d"), "(= (< a b) (> c d))");
    assert_eq!(tree("a <> b"), "(<> a b)");
    assert_eq!(tree("a <= b"), "(<= a b)");
    assert_eq!(tree("a >= b"), "(>= a b)");
}

#[test]
fn word_comparisons_are_the_exact_forms_of_the_symbols() {
    assert_eq!(tree("a GT b"), "(exact> a b)");
    assert_eq!(tree("a le b"), "(exact<= a b)");
    assert_eq!(tree("a EQ b"), "(exact= a b)");
    assert_eq!(tree("a NE b"), "(exact<> a b)");
    // Looser than the symbols they replace only in spelling, not in precedence.
    assert_eq!(tree("a LT b + 1"), "(exact< a (+ b 1))");
}

#[test]
fn a_leading_equals_sign_is_ignored() {
    assert_eq!(tree("=Width*2"), tree("Width*2"));
}

#[test]
fn literals_parse_to_their_kinds() {
    assert_eq!(tree("\"a \"\"b\"\"\""), "\"a \\\"b\\\"\"");
    assert_eq!(tree("#FFaa00"), "#ffaa00");
    assert_eq!(tree("true & FALSE"), "(& true false)");
    assert_eq!(tree("1.5e2 + .5 + 7."), "(+ (+ 150 0.5) 7)");
}

#[test]
fn units_attach_with_or_without_a_space_and_dot() {
    assert_eq!(tree("1.25 in"), "1.25Inch");
    assert_eq!(tree("45deg"), "45Degree");
    assert_eq!(tree("0.7 pt."), "0.7Point");
    assert_eq!(tree("45\u{b0}"), "45Degree");
    assert_eq!(tree("30'"), "30ArcMinute");
    assert_eq!(tree("2em"), "2ElapsedMinute");
    assert_eq!(tree("1e3m"), "1000Meter");
}

#[test]
fn an_exponent_needs_digits_so_e_can_still_start_a_unit() {
    assert_eq!(tree("2 ed"), "2ElapsedDay");
    assert_eq!(tree("2e-1"), "0.2");
}

#[test]
fn implicit_units_use_brackets_or_the_internal_unit_names() {
    assert_eq!(tree("5[deg,A]"), "5Degree");
    assert_eq!(tree("5[in,P]"), "5Inch");
    assert_eq!(tree("2 DL"), "2Inch");
    assert_eq!(tree("2 DA"), "2Radian");
    assert_eq!(tree("2 DE"), "2ElapsedDay");
}

#[test]
fn a_word_after_a_number_that_is_not_a_unit_is_left_for_the_operators() {
    assert_eq!(tree("5 GT 2"), "(exact> 5 2)");
}

#[test]
fn calls_take_empty_arguments_and_allow_space_before_the_parenthesis() {
    assert_eq!(tree("NA( )"), "(NA )");
    assert_eq!(tree("LEFT (\"abc\", 2)"), "(LEFT \"abc\" 2)");
    assert_eq!(
        tree("INDEX(54,\";1;2;3;\",,\"ERROR\")"),
        "(INDEX 54 \";1;2;3;\" _ \"ERROR\")"
    );
    assert_eq!(tree("f(a,)"), "(f a _)");
}

#[test]
fn a_bare_pi_means_the_function() {
    assert_eq!(tree("PI * 2"), "(* (PI ) 2)");
}

#[test]
fn local_cells_and_section_cells_parse_as_dotted_paths() {
    let r = reference("Width");
    assert_eq!((r.path, r.index), (vec!["Width".to_string()], None));
    let r = reference("Geometry1.X1");
    assert_eq!(r.path, ["Geometry1", "X1"]);
    let r = reference("Prop.Cost");
    assert_eq!(r.path, ["Prop", "Cost"]);
    let r = reference("User.Row_1");
    assert_eq!(r.path, ["User", "Row_1"]);
    let r = reference("Scratch.A1");
    assert_eq!(r.path, ["Scratch", "A1"]);
    let r = reference("Connections.X2");
    assert_eq!(r.path, ["Connections", "X2"]);
    let r = reference("Controls.Row_1");
    assert_eq!(r.path, ["Controls", "Row_1"]);
    assert_eq!(r.sheet.container, Container::Current);
    assert_eq!(r.sheet.target, SheetTarget::Itself);
}

#[test]
fn an_indexed_row_keeps_its_index_apart_from_the_name() {
    let r = reference("Char.Size[2]");
    assert_eq!(
        (r.path, r.index),
        (vec!["Char".to_string(), "Size".to_string()], Some(2))
    );
}

#[test]
fn other_shapes_are_named_by_id_by_name_or_by_name_and_id() {
    assert_eq!(reference("Sheet.5!Width").sheet.target, SheetTarget::Id(5));
    assert_eq!(
        reference("sheet.12!Geometry1.X1").sheet.target,
        SheetTarget::Id(12)
    );
    assert_eq!(
        reference("Star!Angle").sheet.target,
        SheetTarget::Name("Star".to_string())
    );
    assert_eq!(
        reference("Executive.2!Height").sheet.target,
        SheetTarget::NamedId {
            name: "Executive".to_string(),
            id: 2
        }
    );
    assert_eq!(
        reference("'1-D'!LineColor").sheet.target,
        SheetTarget::Name("1-D".to_string())
    );
    assert_eq!(
        reference("'it''s'!X").sheet.target,
        SheetTarget::Name("it's".to_string())
    );
}

#[test]
fn page_document_master_and_style_qualifiers_are_recognised() {
    let r = reference("ThePage!User.Vanishing_Point");
    assert_eq!(
        (r.sheet.container, r.sheet.target),
        (Container::ThePage, SheetTarget::Itself)
    );
    assert_eq!(
        reference("TheDoc!PreviewQuality").sheet.container,
        Container::TheDoc
    );
    let r = reference("Pages[Page-3]!Sheet.4!BeginX");
    assert_eq!(
        (r.sheet.container, r.sheet.target),
        (Container::Page("Page-3".to_string()), SheetTarget::Id(4))
    );
    let r = reference("Masters[Gear]!Shaft!Geometry1.X1");
    assert_eq!(
        (r.sheet.container, r.sheet.target),
        (
            Container::Master("Gear".to_string()),
            SheetTarget::Name("Shaft".to_string())
        )
    );
    let r = reference("Styles!Manager!LineColor");
    assert_eq!(
        (r.sheet.container, r.sheet.target),
        (Container::Styles, SheetTarget::Name("Manager".to_string()))
    );
}

#[test]
fn a_reference_displays_as_text_that_parses_back_to_itself() {
    for source in [
        "Width",
        "Geometry1.X1",
        "Char.Size[2]",
        "Sheet.5!Width",
        "Star!Angle",
        "Executive.2!Height",
        "'1-D'!LineColor",
        "ThePage!User.Row_1",
        "TheDoc!PreviewQuality",
        "Pages[Page-3]!Sheet.4!BeginX",
        "Masters[Gear]!Shaft!Geometry1.X1",
        "Styles!Manager!LineColor",
    ] {
        let first = reference(source);
        assert_eq!(reference(&first.to_string()), first, "{source}");
    }
}

#[test]
fn names_may_use_unicode_letters_and_a_leading_underscore() {
    assert_eq!(reference("Größe").path, ["Größe"]);
    assert_eq!(
        reference("\u{5E7B}\u{5E95}.Wert").path,
        ["\u{5E7B}\u{5E95}", "Wert"]
    );
    assert_eq!(
        tree("_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)"),
        "(_WALKGLUE EndTrigger BegTrigger WalkPreference)"
    );
}

#[test]
fn every_node_spans_the_text_it_came_from() {
    let source = "IF(Width > 2 in, \"big\" & Name, Sheet.5!Height * 50%)";
    let expr = parse(source).expect("parses");
    assert_eq!(expr.span, Span::new(0, source.len()));
    fn check(expr: &Expr, source: &str) {
        let text = source
            .get(expr.span.start..expr.span.end)
            .expect("span is a valid slice");
        assert!(
            !text.is_empty() || matches!(expr.kind, ExprKind::Empty),
            "{:?}",
            expr.kind
        );
        match &expr.kind {
            ExprKind::Unary { operand, .. } => {
                assert!(expr.span.start <= operand.span.start && operand.span.end <= expr.span.end);
                check(operand, source);
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                assert_eq!(expr.span, lhs.span.to(rhs.span));
                check(lhs, source);
                check(rhs, source);
            }
            ExprKind::Call {
                args, name_span, ..
            } => {
                assert_eq!(source.get(name_span.start..name_span.end), Some("IF"));
                args.iter().for_each(|arg| check(arg, source));
            }
            _ => {}
        }
    }
    check(&expr, source);
    let ExprKind::Call { args, .. } = &expr.kind else {
        panic!("a call")
    };
    let third = args.get(2).expect("third argument");
    assert_eq!(
        source.get(third.span.start..third.span.end),
        Some("Sheet.5!Height * 50%")
    );
}

#[test]
fn spans_are_byte_offsets_even_after_multibyte_text() {
    let source = "\"\u{e9}\u{e9}\" & ]";
    let (kind, span) = fails(source);
    assert_eq!(kind, ParseErrorKind::UnexpectedChar(']'));
    assert_eq!(source.get(span.start..span.end), Some("]"));
}

#[test]
fn malformed_input_is_reported_with_the_span_of_the_problem() {
    let cases: [(&str, ParseErrorKind, Span); 14] = [
        ("", ParseErrorKind::Empty, Span::new(0, 0)),
        ("   ", ParseErrorKind::Empty, Span::new(3, 3)),
        (
            "1 +",
            ParseErrorKind::Expected { what: "a value" },
            Span::new(3, 3),
        ),
        (
            "(1",
            ParseErrorKind::Expected { what: "')'" },
            Span::new(2, 2),
        ),
        ("1 2", ParseErrorKind::TrailingInput, Span::new(2, 3)),
        (
            "f(1 2)",
            ParseErrorKind::Expected { what: "',' or ')'" },
            Span::new(4, 5),
        ),
        ("\"abc", ParseErrorKind::UnterminatedString, Span::new(0, 4)),
        (
            "'abc",
            ParseErrorKind::UnterminatedQuotedName,
            Span::new(0, 4),
        ),
        (
            "Char.Size[2",
            ParseErrorKind::UnterminatedBracket,
            Span::new(9, 11),
        ),
        ("#12345", ParseErrorKind::BadColor, Span::new(0, 6)),
        ("5 xyz1", ParseErrorKind::TrailingInput, Span::new(2, 6)),
        (
            "5xyz",
            ParseErrorKind::UnknownUnit("xyz".to_string()),
            Span::new(1, 4),
        ),
        (
            "1 @ 2",
            ParseErrorKind::UnexpectedChar('@'),
            Span::new(2, 3),
        ),
        ("1e999", ParseErrorKind::BadNumber, Span::new(0, 5)),
    ];
    for (source, kind, span) in cases {
        assert_eq!(fails(source), (kind, span), "{source:?}");
    }
}

#[test]
fn malformed_references_are_rejected_not_guessed() {
    for (source, why) in [
        ("A!B!C!D", "too many sheet qualifiers before '!'"),
        ("'q'", "a cell name cannot be quoted"),
        ("Char.Size[x]", "a row index is a whole number"),
        ("Char.Size[-1]", "a row index is a whole number"),
        ("Foo.Bar!X", "a sheet name with dots must be quoted"),
        ("Pages[]!X", ""),
    ] {
        let (kind, _) = fails(source);
        match (kind, why) {
            (ParseErrorKind::BadReference { why: actual }, expected) if !expected.is_empty() => {
                assert_eq!(actual, expected, "{source}");
            }
            (ParseErrorKind::BadReference { .. } | ParseErrorKind::Expected { .. }, "") => {}
            (other, _) => panic!("{source:?}: unexpected {other:?}"),
        }
    }
}

#[test]
fn a_formula_over_the_length_limit_is_refused_before_it_is_read() {
    let long = "1+".repeat(MAX_FORMULA_BYTES / 2) + "1";
    let (kind, span) = fails(&long);
    assert_eq!(
        kind,
        ParseErrorKind::TooLong {
            max: MAX_FORMULA_BYTES
        }
    );
    assert_eq!(span.end, long.len());
}

#[test]
fn nesting_beyond_the_limit_is_an_error_at_every_kind_of_nesting() {
    let depth = MAX_DEPTH + 10;
    let parens = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
    let calls = format!("{}1{}", "f(".repeat(depth), ")".repeat(depth));
    let signs = format!("{}1", "-".repeat(depth));
    let chain = vec!["1"; depth + 5].join("+");
    for source in [parens, calls, signs, chain] {
        let (kind, _) = fails(&source);
        assert!(matches!(kind, ParseErrorKind::TooDeep { .. }), "{kind:?}");
    }
}

#[test]
fn a_formula_just_inside_the_limits_parses_and_evaluates_on_a_small_stack() {
    // The parser, the evaluator and the drop of the tree must all stay within a 1 MiB stack.
    let handle = std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let nesting = layoutcraft_formula::MAX_NESTING - 2;
            let source = format!("{}1{}", "(".repeat(nesting), ")".repeat(nesting));
            let chain = vec!["1"; MAX_DEPTH - 2].join("+");
            for source in [source, chain] {
                let expr = parse(&source).expect("within limits");
                let env = super::common::TestEnv::new();
                let value = layoutcraft_formula::evaluate(&expr, &env).value;
                assert!(matches!(value, layoutcraft_formula::Value::Number(_)));
            }
        })
        .expect("thread starts");
    handle.join().expect("no stack overflow");
}
