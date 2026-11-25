//! Hostile input: mutated and random formulas, deep nesting, huge numbers, long strings, unicode,
//! and random engine edits. The invariant is that nothing panics or hangs, no non-finite number
//! escapes, references display as text that parses back, and an incremental recalculation agrees
//! with a calculation from scratch.
//!
//! The generator is a fixed-seed PRNG (no new crates). `CRAFT_HOSTILE_DIVISOR` divides the number
//! of cases, as the shared testing rules describe for the stress budget; a hang fails the test
//! through a deadline that is not itself asserted.

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::Duration;

use layoutcraft_formula::{
    evaluate, evaluate_str, function, functions, parse, ErrorValue, ExprKind, Status, Value,
};

use crate::cases::all_cases;
use crate::common::TestEnv;
use crate::host::{engine, DocEngine};

/// Runs `work` on its own thread with a modest stack; fails the test if it does not finish.
fn with_deadline(seconds: u64, work: impl FnOnce() + Send + 'static) {
    let (done, wait) = mpsc::channel();
    let handle = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            work();
            let _ = done.send(());
        })
        .expect("spawn");
    match wait.recv_timeout(Duration::from_secs(seconds)) {
        Ok(()) => handle.join().expect("no panic"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            // The worker ended without reporting: it panicked. Surface its message.
            match handle.join() {
                Err(payload) => std::panic::resume_unwind(payload),
                Ok(()) => panic!("worker ended without finishing"),
            }
        }
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("no result after {seconds} seconds: hang"),
    }
}

fn budget(base: usize) -> usize {
    let divisor = std::env::var("CRAFT_HOSTILE_DIVISOR")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1);
    (base / divisor).max(1)
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// Characters that stress a lexer: quotes, brackets, signs, control and multi-byte characters.
const HOSTILE_CHARS: &[char] = &[
    '"',
    '\'',
    '#',
    '[',
    ']',
    '(',
    ')',
    ',',
    '!',
    '.',
    '%',
    '&',
    '^',
    '=',
    '<',
    '>',
    '-',
    '+',
    '*',
    '/',
    ' ',
    '\t',
    '\n',
    '\r',
    '\0',
    '\u{7f}',
    '\u{b0}',
    '\u{e9}',
    '\u{301}',
    '\u{200b}',
    '\u{feff}',
    '\u{202e}',
    '\u{2018}',
    '\u{65e5}',
    '\u{1f600}',
    '\u{10ffff}',
    'e',
    'E',
    '0',
    '9',
    'x',
    '_',
    '$',
    '@',
    '{',
    '}',
    '\\',
    ';',
    ':',
    '?',
    '~',
    '`',
    '|',
];

fn mutate(rng: &mut Rng, seed: &str) -> String {
    let mut chars: Vec<char> = seed.chars().collect();
    for _ in 0..=rng.below(4) {
        if chars.is_empty() {
            chars.push(*rng.pick(HOSTILE_CHARS));
            continue;
        }
        let at = rng.below(chars.len());
        match rng.below(8) {
            0 => chars.truncate(at),
            1 => {
                chars.remove(at);
            }
            2 => chars.insert(at, *rng.pick(HOSTILE_CHARS)),
            3 => chars[at] = *rng.pick(HOSTILE_CHARS),
            4 => {
                let end = (at + rng.below(12)).min(chars.len());
                let piece: Vec<char> = chars[at..end].to_vec();
                for (offset, c) in piece.into_iter().enumerate() {
                    chars.insert(at + offset, c);
                }
            }
            5 => {
                let other = rng.below(chars.len());
                chars.swap(at, other);
            }
            6 => {
                chars = chars.split_off(at);
            }
            _ => {
                let c = chars[at];
                chars[at] = if c.is_uppercase() {
                    c.to_ascii_lowercase()
                } else {
                    c.to_ascii_uppercase()
                };
            }
        }
    }
    chars.into_iter().collect()
}

/// Parses and evaluates `source`, checking the invariants that must hold for any input.
fn exercise(source: &str, env: &TestEnv) {
    let Ok(expr) = parse(source) else {
        return;
    };
    // A reference prints as text that parses back to the same reference.
    for reference in expr.references() {
        let shown = reference.to_string();
        match parse(&shown).map(|e| e.kind) {
            Ok(ExprKind::Ref(again)) => {
                assert_eq!(
                    &again, reference,
                    "{source:?}: {shown:?} reads back differently"
                )
            }
            other => panic!(
                "{source:?}: reference {reference:?} shows as {shown:?}, which parses to {other:?}"
            ),
        }
    }
    let first = evaluate(&expr, env);
    let second = evaluate(&expr, env);
    assert_eq!(first, second, "{source:?} is not deterministic");
    if let Value::Number(q) = &first.value {
        assert!(q.value.is_finite(), "{source:?} gave {:?}", first.value);
    }
    if let Value::Point(x, y) = &first.value {
        assert!(x.value.is_finite() && y.value.is_finite(), "{source:?}");
    }
    if let Value::Str(text) = &first.value {
        assert!(
            text.len() <= layoutcraft_formula::MAX_STRING_BYTES + 4,
            "{source:?}"
        );
    }
}

fn seeds() -> Vec<String> {
    let mut seeds: Vec<String> = all_cases().iter().map(|c| c.formula.to_string()).collect();
    seeds.extend(
        [
            "Masters[Gear]!Shaft!Geometry1.X1",
            "'ThePage'!X + 'thedoc'!Y + 'STYLES'!Z + 'Sheet.5'!W + 'a b'.7!Q",
            "Pages[Page-3]!Sheet.4!BeginX + 'It''s'!X",
            "IF(ABS(LUMDIFF(THEMEVAL(\"BackgroundColor\"),THEMEVAL()))<40,#d80073,THEMEVAL())",
            "5[in,P] + 2.5e-3 deg + 90\u{b0} + 3600\" + 60' + 1 sq ft + 2 m^2",
            "1 EQ (2) GT 3 & \"x\" LE -4%",
            "Char.Font[3] & Scratch.A5 & ThePage!User.X & TheDoc!Y & Styles!S!LineColor",
        ]
        .map(String::from),
    );
    seeds
}

#[test]
fn mutated_formulas_never_panic_and_keep_the_invariants() {
    with_deadline(120, || {
        let env = TestEnv::new();
        let seeds = seeds();
        let mut rng = Rng(0xf0_0d);
        let mut parsed = 0usize;
        let total = budget(40_000);
        for round in 0..total {
            let seed = rng.pick(&seeds).clone();
            let source = if round % 7 == 0 {
                seed
            } else {
                mutate(&mut rng, &seed)
            };
            if parse(&source).is_ok() {
                parsed += 1;
            }
            exercise(&source, &env);
        }
        // The sweep is only worth something if a good share of mutants still parse.
        assert!(
            parsed * 10 > total,
            "only {parsed} of {total} mutants parsed"
        );
    });
}

#[test]
fn every_prefix_of_every_seed_parses_or_fails_cleanly() {
    with_deadline(120, || {
        let env = TestEnv::new();
        for seed in seeds() {
            let boundaries: Vec<usize> = seed
                .char_indices()
                .map(|(i, _)| i)
                .chain(std::iter::once(seed.len()))
                .collect();
            for end in boundaries {
                exercise(&seed[..end], &env);
            }
        }
    });
}

#[test]
fn token_soup_never_panics() {
    with_deadline(120, || {
        let env = TestEnv::new();
        let names: Vec<&str> = functions().iter().map(|f| f.name).collect();
        let atoms: Vec<String> = [
            "0",
            "1",
            "2.5",
            "1e3",
            "-7",
            ".5",
            "1e308",
            "1e-320",
            "\"abc\"",
            "\"\"",
            "\"\u{65e5}\"",
            "TRUE",
            "FALSE",
            "#ff8000",
            "Width",
            "Height",
            "Zero",
            "Name",
            "Sheet.5!Width",
            "'x y'!Z",
            "Char.Font[2]",
            "5 in",
            "3 deg",
            "2 ed",
            "1 sq m",
            "7[ft,D]",
            "50%",
            "Missing",
            "ThePage!X",
            "PI",
            "ISTHEMED",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let operators = [
            " + ", " - ", " * ", " / ", " ^ ", " & ", " = ", " <> ", " < ", " <= ", " > ", " >= ",
            " EQ ", " NE ", " LT ", " LE ", " GT ", " GE ", ", ",
        ];
        let mut rng = Rng(0xbeef);
        for _ in 0..budget(30_000) {
            let mut text = String::new();
            let mut open = 0usize;
            for _ in 0..rng.below(14) + 1 {
                match rng.below(6) {
                    0 => {
                        text.push_str(rng.pick(&names));
                        text.push('(');
                        open += 1;
                    }
                    1 if open > 0 => {
                        text.push(')');
                        open -= 1;
                    }
                    2 => text.push('('),
                    3 => text.push_str(rng.pick(&operators)),
                    _ => text.push_str(rng.pick(&atoms)),
                }
                if rng.below(3) == 0 {
                    text.push_str(rng.pick(&atoms));
                }
            }
            for _ in 0..open {
                text.push(')');
            }
            exercise(&text, &env);
        }
    });
}

#[test]
fn every_function_takes_random_arguments_without_panicking() {
    with_deadline(120, || {
        let env = TestEnv::new();
        let args = [
            "0",
            "1",
            "-1",
            "2.5",
            "1e300",
            "-1e300",
            "1e-300",
            "255",
            "256",
            "65535",
            "65536",
            "\"\"",
            "\"abc\"",
            "\"a;b;c\"",
            "\"#336699\"",
            "\"1e5\"",
            "\"\u{1f600}\u{301}\"",
            "TRUE",
            "#336699",
            "Width",
            "Angle",
            "5 in",
            "90 deg",
            "1 ed",
            "PNT(1, 2)",
            "NA()",
            "1/0",
            "Missing",
            "REPT(\"ab\", 100000)",
        ];
        let mut rng = Rng(0xa11);
        for info in functions() {
            for _ in 0..budget(200) {
                let count = if info.max_args == u8::MAX {
                    rng.below(6) + usize::from(info.min_args)
                } else {
                    usize::from(info.min_args)
                        + rng.below(usize::from(info.max_args - info.min_args) + 2)
                };
                let list: Vec<&str> = (0..count)
                    .map(|_| {
                        if rng.below(12) == 0 {
                            ""
                        } else {
                            *rng.pick(&args)
                        }
                    })
                    .collect();
                exercise(&format!("{}({})", info.name, list.join(", ")), &env);
            }
        }
        // Unsupported functions never produce a value of their own.
        for info in functions()
            .iter()
            .filter(|f| f.status == Status::Unsupported)
        {
            let call = format!("{}(1)", info.name);
            let value = evaluate_str(&call, &env).expect("parses").value;
            assert_eq!(value, Value::Error(ErrorValue::Unsupported), "{call}");
        }
        assert!(function("REPT").is_some());
    });
}

#[test]
fn deep_and_wide_nesting_is_refused_or_evaluated_on_a_small_stack() {
    with_deadline(120, || {
        let env = TestEnv::new();
        let shapes: Vec<String> = vec![
            format!("{}1{}", "(".repeat(100_000), ")".repeat(100_000)),
            format!("{}1", "-".repeat(100_000)),
            format!("{}1", "+".repeat(100_000)),
            format!("{}1{}", "ABS(".repeat(10_000), ")".repeat(10_000)),
            format!("{}1{}", "IF(1,".repeat(5_000), ",2)".repeat(5_000)),
            "1+".repeat(100_000) + "1",
            "1^".repeat(30_000) + "1",
            "1&".repeat(30_000) + "1",
            "1=".repeat(30_000) + "1",
            "1%".repeat(30_000),
            "(".repeat(63) + "1" + &")".repeat(63),
            "ABS(".repeat(63) + "1" + &")".repeat(63),
            "-".repeat(63) + "1",
            "'".repeat(50_000),
            "\"".repeat(50_001),
            "[".repeat(50_000),
            "Sheet.1!".repeat(8_000) + "X",
            "A.".repeat(30_000) + "B",
            "MAX(".to_string() + &"1,".repeat(30_000) + "1)",
            "PNTX(".repeat(60) + "PNT(1,2)" + &")".repeat(60),
        ];
        for shape in &shapes {
            exercise(shape, &env);
        }
        // A tree just inside the limits evaluates; one just outside is an error, never a crash.
        let inside = "1+".repeat(200) + "1";
        assert_eq!(
            evaluate_str(&inside, &env).expect("parses").value,
            Value::number(201.0)
        );
        assert!(parse(&("1+".repeat(300) + "1")).is_err());
    });
}

#[test]
fn huge_numbers_and_lengths_give_errors_not_overflows() {
    with_deadline(120, || {
        let env = TestEnv::new();
        let digits_60k = "9".repeat(60_000);
        let formulas: Vec<String> = [
            "1e308 * 1e308",
            "(1e308) ^ (1e308)",
            "(-1e308) ^ 3",
            "10 ^ 308 * 10",
            "1e-400",
            "1e400",
            "0.0000000000000000000000000000000000000000000000000000000000001e1",
            "ROUND(1e308, 300)",
            "ROUND(-1e308, -308)",
            "ROUND(123, 1e300)",
            "ROUND(123, -1e300)",
            "TRUNC(1e300, -300)",
            "CEILING(1e308, 1e-308)",
            "FLOOR(1e308, 1e-308)",
            "CEILING(1e-308, 1e308)",
            "INT(1e308)",
            "MODULUS(1e308, 1e-308)",
            "MODULUS(1, 1e-320)",
            "SIGN(1e308, 1e308)",
            "POW(1e308, 2)",
            "POW(2, 1e308)",
            "POW(10 in, 1e9)",
            "SQRT(1e308)",
            "LN(1e-320)",
            "EXP(1e308)",
            "MAGNITUDE(1e308, 1e308, 1e308, 1e308)",
            "BITAND(1e300, -1e300)",
            "BITNOT(1e300)",
            "CHAR(1e300)",
            "CHAR(-1e300)",
            "UNICHAR(1e300)",
            "LEFT(\"abc\", 1e300)",
            "RIGHT(\"abc\", 1e300)",
            "MID(\"abc\", 1e300, 1e300)",
            "MID(\"abc\", 2, 1e300)",
            "MID(\"abc\", -1e300, 1)",
            "REPLACE(\"abc\", 1e300, 1e300, \"x\")",
            "REPLACE(\"abc\", 2, 1e300, \"x\")",
            "REPT(\"a\", 1e300)",
            "REPT(\"abc\", 349525)",
            "REPT(\"abc\", 349526)",
            "SUBSTITUTE(\"a\", \"a\", REPT(\"b\", 1000000))",
            "SUBSTITUTE(REPT(\"a\", 1000000), \"a\", \"bb\")",
            "REPT(\"x\", 1000000) & REPT(\"y\", 1000000)",
            "LEN(REPT(\"\u{1f600}\", 200000))",
            "UPPER(REPT(\"\u{df}\", 500000))",
            "FIND(\"a\", REPT(\"b\", 1000000))",
            "INDEX(1e300, \"a;b\")",
            "INDEX(0, REPT(\"a;\", 400000))",
            "LOOKUP(\"z\", REPT(\"a;\", 400000))",
            "FIND(\"a\", \"b\", 1e300)",
            "SUBSTITUTE(\"aaa\", \"a\", \"b\", 1e300)",
            "GRAVITY(1e300)",
            "ANG360(1e300)",
            "ANG360(-1e300)",
            "SIN(1e300)",
            "TAN(1e300)",
            "ATAN2(1e308, 1e-308)",
            "RGB(1e300, -1e300, 1e-300)",
            "HSL(1e300, 1e300, 1e300)",
            "HSL(-1e300, -1e300, -1e300)",
            "TINT(#336699, 1e300)",
            "TONE(#336699, -1e300)",
            "BLEND(#000000, #ffffff, 1e-300)",
            "THEMEVAL(1e300)",
            "1e308 in * 1e308 in",
            "1e308 ft",
            "1e308 mi",
            "1e308 km + 1e308 km",
            "1e308 acres",
            "1e308 ha",
            "(1e154 in)^2",
            "(1e155 in)^2",
        ]
        .iter()
        .map(|s| s.to_string())
        .chain([
            digits_60k.clone(),
            format!("{digits_60k}.5"),
            format!("0.{digits_60k}"),
        ])
        .collect();
        for formula in &formulas {
            exercise(formula, &env);
        }
        // Over the length limit: refused before it is read.
        assert!(parse(&"9".repeat(70_000)).is_err());
        assert!(parse(&"\"".repeat(70_000)).is_err());
        // The string cap is real: a result longer than the cap is an error, not 2 MB of text.
        let value = evaluate_str("REPT(\"abc\", 349526)", &env)
            .expect("parses")
            .value;
        assert_eq!(value, Value::Error(ErrorValue::Value));
        let value = evaluate_str("REPT(\"abc\", 349525)", &env)
            .expect("parses")
            .value;
        assert!(matches!(value, Value::Str(text) if text.len() == 3 * 349_525));
    });
}

#[test]
fn unicode_text_is_counted_in_characters_and_never_split() {
    with_deadline(60, || {
        let env = TestEnv::new();
        let words = [
            "e\u{301}",
            "\u{1f600}",
            "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}",
            "\u{202e}abc",
            "\u{0}",
            "\u{feff}x",
            "\u{10ffff}",
            "\u{df}",
            "\u{130}",
            "\u{3a3}",
            "\u{fb01}",
            "caf\u{e9}",
        ];
        for word in words {
            for template in [
                "LEN(\"W\")",
                "LEFT(\"W\", 1)",
                "RIGHT(\"W\", 1)",
                "MID(\"W\", 1, 1)",
                "UPPER(\"W\")",
                "LOWER(\"W\")",
                "TRIM(\" W  W \")",
                "FIND(\"W\", \"xWx\")",
                "FIND(\"w\", \"xWx\", 1, TRUE)",
                "SUBSTITUTE(\"WW\", \"W\", \"-\")",
                "REPLACE(\"xWx\", 2, 1, \"\")",
                "REPT(\"W\", 3)",
                "STRSAME(\"W\", \"W\")",
                "STRSAME(\"W\", \"w\", TRUE)",
                "INDEX(1, \"a;W\")",
                "LOOKUP(\"W\", \"a;W\")",
                "\"W\" & \"W\"",
                "'W'!X",
                "W",
                "5 W",
            ] {
                let formula = template.replace('W', word);
                exercise(&formula, &env);
            }
        }
        // Whole characters: a combining sequence is two characters, an emoji is one.
        let length = |text: &str| match evaluate_str(&format!("LEN(\"{text}\")"), &env) {
            Ok(evaluation) => evaluation.value,
            Err(error) => panic!("{error}"),
        };
        assert_eq!(length("e\u{301}"), Value::number(2.0));
        assert_eq!(length("\u{1f600}"), Value::number(1.0));
    });
}

#[test]
fn random_engine_edits_agree_with_a_calculation_from_scratch() {
    with_deadline(120, || {
        const CELLS: usize = 14;
        let formulas = [
            "N{a} + 1",
            "N{a} * N{b}",
            "IF(N{a}, N{b}, N{c})",
            "MAX(N{a}, N{b}, 2)",
            "N{a} & N{b}",
            "N{a} / (N{b} - N{b})",
            "IFERROR(N{a} / N{b}, 0)",
            "Missing + N{a}",
            "N{a} < N{b}",
            "DATE(1,2,3)",
            "SUM(N{a}, N{b}, N{c})",
            "N{a} +",
            "ISERR(N{a})",
            "NA()",
            "N{a}",
            "N0",
        ];
        let key = |i: usize| format!("Sheet.1!N{i}").to_ascii_lowercase();
        let mut rng = Rng(0x5eed1);
        for round in 0..budget(1500) {
            let mut live = engine();
            // The model: each cell is a constant, a formula, or absent.
            let mut model: BTreeMap<usize, Result<Value, String>> = BTreeMap::new();
            let mut log: Vec<String> = Vec::new();
            for _ in 0..rng.below(60) + 10 {
                let cell = rng.below(CELLS);
                match rng.below(6) {
                    0 => {
                        let value = Value::number(rng.below(5) as f64);
                        log.push(format!("set N{cell} = {value:?}"));
                        live.set_value(key(cell), value.clone());
                        model.insert(cell, Ok(value));
                    }
                    1 => {
                        log.push(format!("remove N{cell}"));
                        live.remove(&key(cell));
                        model.remove(&cell);
                    }
                    _ => {
                        let template = rng.pick(&formulas);
                        let text = template
                            .replace("{a}", &rng.below(CELLS).to_string())
                            .replace("{b}", &rng.below(CELLS).to_string())
                            .replace("{c}", &rng.below(CELLS).to_string());
                        log.push(format!("N{cell} = {text}"));
                        let _ = live.set_formula(key(cell), &text);
                        model.insert(cell, Err(text));
                    }
                }
                if rng.below(4) == 0 {
                    log.push("recalculate".to_string());
                    live.recalculate();
                }
            }
            live.recalculate();
            let mut fresh: DocEngine = engine();
            for (cell, definition) in &model {
                match definition {
                    Ok(value) => fresh.set_value(key(*cell), value.clone()),
                    Err(text) => {
                        let _ = fresh.set_formula(key(*cell), text);
                    }
                }
            }
            fresh.recalculate();
            for cell in 0..CELLS {
                assert_eq!(
                    live.value(&key(cell)),
                    fresh.value(&key(cell)),
                    "round {round}, cell N{cell}, model {model:?}, log {log:#?}"
                );
            }
        }
    });
}
