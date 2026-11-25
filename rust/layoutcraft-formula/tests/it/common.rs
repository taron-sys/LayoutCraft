//! Shared helpers: a map-backed environment and short evaluation shortcuts.

use std::collections::HashMap;

use layoutcraft_formula::{
    evaluate_str, CellRef, Env, ErrorValue, FormulaKind, Quantity, Rgb, Services, ThemeKey, Value,
};

#[derive(Default)]
pub struct TestServices {
    pub palette: Vec<(i64, Rgb)>,
}

impl Services for TestServices {
    fn palette_color(&self, index: i64) -> Option<Rgb> {
        self.palette
            .iter()
            .find(|(i, _)| *i == index)
            .map(|(_, c)| *c)
    }

    fn theme_value(&self, key: &ThemeKey) -> Option<Value> {
        match key {
            ThemeKey::HostCell => Some(Value::Color(Rgb::new(0x20, 0x20, 0x20))),
            ThemeKey::Name(name) if name == "Light" => {
                Some(Value::Color(Rgb::new(0xf0, 0xf0, 0xf0)))
            }
            ThemeKey::Name(name) if name == "LineWeight" => Some(Value::length(0.01)),
            ThemeKey::ColorIndex(5) => Some(Value::Color(Rgb::new(1, 2, 3))),
            _ => None,
        }
    }

    fn is_themed(&self) -> Option<bool> {
        Some(true)
    }

    fn shape_id(&self) -> Option<u32> {
        Some(7)
    }

    fn shape_name(&self) -> Option<String> {
        Some("Box.7".to_string())
    }

    fn master_name(&self) -> Option<Option<String>> {
        Some(None)
    }

    fn text_size(&self, text: &str, max_width: Option<f64>) -> Option<(f64, f64)> {
        // One tenth of an inch per character, 0.2 inch per line.
        let width = text.chars().count() as f64 * 0.1;
        Some((max_width.map_or(width, |m| width.min(m)), 0.2))
    }
}

/// Cells are looked up by their canonical spelling, lower-cased.
pub struct TestEnv {
    cells: HashMap<String, Value>,
    local: HashMap<String, FormulaKind>,
    services: TestServices,
}

impl TestEnv {
    pub fn new() -> Self {
        let mut env = TestEnv {
            cells: HashMap::new(),
            local: HashMap::new(),
            services: TestServices::default(),
        };
        env.set("Width", Value::length(4.0));
        env.set("Height", Value::length(2.0));
        env.set("Angle", Value::angle(std::f64::consts::FRAC_PI_2));
        env.set("Zero", Value::number(0.0));
        env.set("Name", Value::string("box"));
        env.set("Sheet.5!Width", Value::length(10.0));
        env
    }

    pub fn set(&mut self, name: &str, value: Value) -> &mut Self {
        self.cells.insert(name.to_ascii_lowercase(), value);
        self
    }
}

impl Env for TestEnv {
    fn cell(&self, reference: &CellRef) -> Result<Value, ErrorValue> {
        self.cells
            .get(&reference.to_string().to_ascii_lowercase())
            .cloned()
            .ok_or(ErrorValue::Ref)
    }

    fn formula_kind(&self, reference: &CellRef) -> Result<FormulaKind, ErrorValue> {
        let key = reference.to_string().to_ascii_lowercase();
        match self.local.get(&key) {
            Some(kind) => Ok(*kind),
            None if self.cells.contains_key(&key) => Ok(FormulaKind::Absent),
            None => Err(ErrorValue::Ref),
        }
    }

    fn services(&self) -> &dyn Services {
        &self.services
    }
}

/// Evaluates against the default environment; a formula that does not parse fails the test.
pub fn eval(source: &str) -> Value {
    eval_in(source, &TestEnv::new())
}

pub fn eval_in(source: &str, env: &TestEnv) -> Value {
    match evaluate_str(source, env) {
        Ok(evaluation) => evaluation.value,
        Err(error) => panic!("{source:?} does not parse: {error}"),
    }
}

/// The internal-unit number of the result; fails the test if it is not a number.
pub fn num(source: &str) -> f64 {
    match eval(source) {
        Value::Number(q) => q.value,
        other => panic!("{source:?} gave {other:?}, not a number"),
    }
}

pub fn quantity(source: &str) -> Quantity {
    match eval(source) {
        Value::Number(q) => q,
        other => panic!("{source:?} gave {other:?}, not a number"),
    }
}

pub fn err(source: &str) -> ErrorValue {
    match eval(source) {
        Value::Error(error) => error,
        other => panic!("{source:?} gave {other:?}, not an error"),
    }
}

pub fn text(source: &str) -> String {
    match eval(source) {
        Value::Str(text) => text,
        other => panic!("{source:?} gave {other:?}, not text"),
    }
}

pub fn boolean(source: &str) -> bool {
    match eval(source) {
        Value::Bool(flag) => flag,
        other => panic!("{source:?} gave {other:?}, not a boolean"),
    }
}

pub fn close(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= 1e-9 * expected.abs().max(1.0)
}
