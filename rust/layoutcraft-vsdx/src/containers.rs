//! Containers and lists: which shapes belong to which, read from the `Relationships` cell of each shape.
//!
//! The cell's formula is a sum of terms `DEPENDSON(kind,Sheet.7!SheetRef(),...)` (`docs/04-visio-import.md`): on a container
//! its members (kind 1), on a list its items (2), on a member its containers (4), on a list item its list (5), and between
//! lists that belong together (11). Both ends of a membership state it; either one is enough here.
use std::collections::{BTreeMap, BTreeSet};

use layoutcraft_model::{Connector, Shape};
use serde_json::{json, Value};

use crate::shapes::ResolvedShape;

const MEMBERS: u32 = 1;
const ITEMS: u32 = 2;
const CONTAINED_IN: u32 = 4;
const LISTED_IN: u32 = 5;
const RELATED: u32 = 11;

/// The terms of a `Relationships` formula: each kind with the sheet IDs it names. Anything else in the formula is passed over.
pub(crate) fn parse_relationships(formula: &str) -> Vec<(u32, Vec<String>)> {
    let mut terms = Vec::new();
    let mut rest = formula;
    while let Some(at) = rest.find("DEPENDSON(") {
        rest = &rest[at + "DEPENDSON(".len()..];
        // up to the bracket that closes this term: `SheetRef()` opens and closes its own
        let mut depth = 0usize;
        let end = rest
            .char_indices()
            .find(|(_, c)| match c {
                '(' => {
                    depth += 1;
                    false
                }
                ')' if depth == 0 => true,
                ')' => {
                    depth -= 1;
                    false
                }
                _ => false,
            })
            .map_or(rest.len(), |(index, _)| index);
        let mut parts = rest[..end].split(',');
        if let Some(kind) = parts.next().and_then(|kind| kind.trim().parse::<u32>().ok()) {
            let ids = parts
                .filter_map(|part| {
                    let id: String = part.trim().strip_prefix("Sheet.")?.chars().take_while(char::is_ascii_digit).collect();
                    (!id.is_empty()).then_some(id)
                })
                .collect();
            terms.push((kind, ids));
        }
        rest = &rest[end..];
    }
    terms
}

#[derive(Default)]
struct Record {
    list: bool,
    members: Vec<String>,
    items: Vec<String>,
    related: Vec<String>,
}

fn add(into: &mut Vec<String>, id: &str) {
    if !into.iter().any(|known| known == id) {
        into.push(id.to_string());
    }
}

fn every<'a>(nodes: &'a [ResolvedShape], out: &mut Vec<&'a ResolvedShape>) {
    for node in nodes {
        out.push(node);
        every(&node.children, out);
    }
}

/// Mark the shapes and connectors of a page with the containers and lists they belong to, and return the page's records:
/// `{visioId, kind: "container" | "list", members, items, related}`. A reference to a shape the page does not have is left out.
pub(crate) fn apply(resolved: &[ResolvedShape], shapes: &mut [Shape], connectors: &mut [Connector]) -> Vec<Value> {
    let mut nodes = Vec::new();
    every(resolved, &mut nodes);
    let ids: BTreeSet<&str> = nodes.iter().map(|node| node.id.as_str()).collect();
    let mut records: BTreeMap<String, Record> = BTreeMap::new();
    for node in &nodes {
        let Some(formula) = node.cells.get("Relationships").and_then(|cell| cell.formula.as_deref()) else { continue };
        for (kind, named) in parse_relationships(formula) {
            for other in named.iter().filter(|other| ids.contains(other.as_str()) && **other != node.id) {
                match kind {
                    MEMBERS => add(&mut records.entry(node.id.clone()).or_default().members, other),
                    CONTAINED_IN => add(&mut records.entry(other.clone()).or_default().members, &node.id),
                    ITEMS | LISTED_IN => {
                        let (list, item) = if kind == ITEMS { (&node.id, other) } else { (other, &node.id) };
                        let record = records.entry(list.clone()).or_default();
                        record.list = true;
                        add(&mut record.items, item);
                    }
                    RELATED => add(&mut records.entry(node.id.clone()).or_default().related, other),
                    _ => {}
                }
            }
        }
    }
    // a shape that only names a related list is not a container of anything
    records.retain(|_, record| !record.members.is_empty() || !record.items.is_empty());
    for (container, record) in &records {
        for shape in shapes.iter_mut() {
            let Some(id) = shape.visio_id.as_deref() else { continue };
            let (member, item) = (record.members.iter().any(|m| m == id), record.items.iter().any(|m| m == id));
            if member {
                add(&mut shape.containers, container);
            }
            if item {
                add(&mut shape.list_of, container);
            }
        }
        for connector in connectors.iter_mut() {
            if connector.visio_id.as_deref().is_some_and(|id| record.members.iter().any(|m| m == id)) {
                add(&mut connector.containers, container);
            }
        }
    }
    records
        .into_iter()
        .map(|(id, record)| {
            json!({ "visioId": id, "kind": if record.list { "list" } else { "container" }, "members": record.members, "items": record.items, "related": record.related })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_relationships;

    #[test]
    fn each_term_keeps_its_kind_and_sheets_and_other_text_is_passed_over() {
        let terms = parse_relationships("SUM(DEPENDSON(1,Sheet.7!SheetRef(),Sheet.16!SheetRef()),DEPENDSON(4,Sheet.1!SheetRef()))");
        assert_eq!(terms, [(1, vec!["7".to_string(), "16".to_string()]), (4, vec!["1".to_string()])]);
        assert_eq!(parse_relationships("DEPENDSON(2,Sheet.3!SheetRef()"), [(2, vec!["3".to_string()])], "an unclosed term");
        for nothing in ["", "SUM()", "GUARD(1)", "DEPENDSON(x,Sheet.1!SheetRef())"] {
            assert!(parse_relationships(nothing).is_empty(), "{nothing}");
        }
    }
}
