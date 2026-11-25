//! A small document host for engine tests.
//!
//! A cell key is `<sheet>!<path>`, for example `Sheet.1!Width`. Sheets are `Sheet.<id>` (shapes of
//! the page `Page-1`), `Page-1` (the page sheet itself), `Doc`, `Style.<name>`, and the same
//! shapes in other containers (`Pages[X].Sheet.4`, `Masters[M].Sheet.2`). Names and paths are
//! compared without regard to case.

use std::collections::HashMap;

use layoutcraft_formula::{
    CellRef, Container, Engine, FormulaKind, Host, Report, Services, SheetTarget, Value,
};

#[derive(Default)]
pub struct Doc {
    /// Shape names: lower-cased name to the list of ids carrying it.
    pub names: HashMap<String, Vec<u32>>,
    pub services: DocServices,
    /// Cells for which the host reports an inherited formula.
    pub inherited: Vec<String>,
}

#[derive(Default)]
pub struct DocServices;

impl Services for DocServices {
    fn shape_id(&self) -> Option<u32> {
        Some(1)
    }
}

pub fn key(sheet: &str, path: &str) -> String {
    format!("{sheet}!{path}").to_ascii_lowercase()
}

fn path_text(reference: &CellRef) -> String {
    let mut text = reference.path.join(".");
    if let Some(index) = reference.index {
        text.push_str(&format!("[{index}]"));
    }
    text.to_ascii_lowercase()
}

impl Host for Doc {
    type Key = String;

    fn resolve(&self, from: &String, reference: &CellRef) -> Option<String> {
        let own_sheet = from.split('!').next().unwrap_or("");
        let id_of = |name: &str, id: Option<u32>| -> Option<u32> {
            let ids = self.names.get(&name.to_ascii_lowercase())?;
            match id {
                Some(id) => ids.contains(&id).then_some(id),
                // A name that more than one shape carries needs the ID.
                None if ids.len() == 1 => ids.first().copied(),
                None => None,
            }
        };
        let shape = match &reference.sheet.target {
            SheetTarget::Itself => None,
            SheetTarget::Id(id) => Some(*id),
            SheetTarget::Name(name) => Some(id_of(name, None)?),
            SheetTarget::NamedId { name, id } => Some(id_of(name, Some(*id))?),
        };
        let sheet = match (&reference.sheet.container, shape) {
            (Container::Current, None) => own_sheet.to_string(),
            (Container::Current, Some(id)) => format!("Sheet.{id}"),
            (Container::ThePage, None) => "Page-1".to_string(),
            (Container::ThePage, Some(id)) => format!("Sheet.{id}"),
            (Container::TheDoc, _) => "Doc".to_string(),
            (Container::Styles, None) => "Style".to_string(),
            (Container::Styles, Some(id)) => format!("Style.{id}"),
            (Container::Page(page), None) => format!("Pages[{page}]"),
            (Container::Page(page), Some(id)) => format!("Pages[{page}].Sheet.{id}"),
            (Container::Master(master), None) => format!("Masters[{master}]"),
            (Container::Master(master), Some(id)) => format!("Masters[{master}].Sheet.{id}"),
        };
        Some(key(&sheet, &path_text(reference)))
    }

    fn services(&self, _from: &String) -> &dyn Services {
        &self.services
    }

    fn formula_kind(&self, key: &String) -> Option<FormulaKind> {
        self.inherited
            .iter()
            .any(|k| k == key)
            .then_some(FormulaKind::Inherited)
    }
}

pub type DocEngine = Engine<Doc>;

pub fn engine() -> DocEngine {
    Engine::new(Doc::default())
}

/// Sets a constant: `set(&mut e, "Sheet.1!Width", Value::length(4.0))`.
pub fn set(engine: &mut DocEngine, cell: &str, value: Value) {
    engine.set_value(cell.to_ascii_lowercase(), value);
}

/// Sets a formula, which must parse.
pub fn formula(engine: &mut DocEngine, cell: &str, text: &str) {
    if let Err(error) = engine.set_formula(cell.to_ascii_lowercase(), text) {
        panic!("{cell} = {text:?} does not parse: {error}");
    }
}

pub fn value(engine: &DocEngine, cell: &str) -> Value {
    match engine.value(&cell.to_ascii_lowercase()) {
        Some(value) => value.clone(),
        None => panic!("no cell {cell}"),
    }
}

pub fn recomputed(report: &Report<String>) -> Vec<&str> {
    report.recomputed.iter().map(String::as_str).collect()
}
