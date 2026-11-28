//! Containers and lists: which shapes belong to which, read from the `Relationships` cell of each shape.
//!
//! The cell's formula is a sum of terms `DEPENDSON(kind,Sheet.7!SheetRef(),...)` (`docs/04-visio-import.md`): on a container
//! its members (kind 1), on a list its items (2), on a member its containers (4), on a list item its list (5), and between
//! lists that belong together (11). Both ends of a membership state it; either one is enough here.
use std::collections::{BTreeMap, BTreeSet};

use layoutcraft_model::{Connector, Shape};
use serde_json::{json, Value};

use crate::shapes::ResolvedShape;
use crate::{Error, Package, Result};

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

/// `xml` (a page, or one shape) with the references that `keep` refuses taken out of its `Relationships` cells; the empty
/// term stays, as Visio leaves it (`docs/45-containers.md`). `None` when nothing was taken out.
pub(crate) fn without_references(xml: &str, keep: impl Fn(&str) -> bool) -> Option<String> {
    const REF: &str = ",Sheet.";
    const END: &str = "!SheetRef()";
    let mut out = String::with_capacity(xml.len());
    let (mut done, mut from, mut changed) = (0usize, 0usize, false);
    while let Some(found) = xml[from..].find("Relationships") {
        let at = from + found;
        from = at + "Relationships".len();
        // only the cell of that name: its element, up to the end of the tag
        if !(xml[..at].ends_with("N='") || xml[..at].ends_with("N=\"")) {
            continue;
        }
        let end = xml[at..].find('>').map_or(xml.len(), |n| at + n);
        let mut cell = String::with_capacity(end - at);
        let mut rest = &xml[at..end];
        while let Some(next) = rest.find(REF) {
            let after = &rest[next + REF.len()..];
            let digits = after.len() - after.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            if digits > 0 && after[digits..].starts_with(END) && !keep(&after[..digits]) {
                cell.push_str(&rest[..next]);
                rest = &after[digits + END.len()..];
                changed = true;
            } else {
                cell.push_str(&rest[..next + REF.len()]);
                rest = after;
            }
        }
        cell.push_str(rest);
        out.push_str(&xml[done..at]);
        out.push_str(&cell);
        done = end;
        from = end;
    }
    out.push_str(&xml[done..]);
    changed.then_some(out)
}

/// `formula` (a `Relationships` formula, or `None` for a shape that has none) with sheet `id` put into, or taken out of, the
/// term of `kind`: 1 on a container for a member, 4 on a member for its container. A new reference goes first, an emptied
/// term stays, as Visio writes them (`docs/45-containers.md`).
pub fn with_membership(formula: Option<&str>, kind: u32, id: &str, present: bool) -> String {
    let reference = format!("Sheet.{id}!SheetRef()");
    let current = formula.filter(|text| text.contains("DEPENDSON(")).unwrap_or("");
    let term = [format!("DEPENDSON({kind},"), format!("DEPENDSON({kind})")].iter().find_map(|head| current.find(head.as_str()));
    let Some(start) = term else {
        if !present {
            return current.to_string();
        }
        let new = format!("DEPENDSON({kind},{reference})");
        return match current.strip_prefix("SUM(") {
            Some(rest) => format!("SUM({new},{rest}"),
            None => format!("SUM({new})"),
        };
    };
    // the term ends at the bracket that closes it: `SheetRef()` opens and closes its own
    let open = start + "DEPENDSON(".len();
    let mut depth = 0usize;
    let end = current[open..]
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
        .map_or(current.len(), |(index, _)| open + index);
    let mut parts: Vec<&str> = current[open..end].split(',').map(str::trim).filter(|part| !part.is_empty()).collect();
    parts.retain(|part| *part != reference);
    if present {
        parts.insert(1.min(parts.len()), &reference);
    }
    format!("{}{}{}", &current[..open], parts.join(","), &current[end..])
}

/// What a page's XML says of its shapes for containment: the `Relationships` formula of each shape that has one, and the
/// shapes that call themselves containers, each with whether it is locked against changes of membership.
pub fn page_membership(page_xml: &str) -> (BTreeMap<String, String>, BTreeMap<String, bool>) {
    let (mut formulas, mut declared) = (BTreeMap::new(), BTreeMap::new());
    let Ok(tree) = roxmltree::Document::parse(page_xml) else { return (formulas, declared) };
    for shape in tree.descendants().filter(|node| node.tag_name().name() == "Shape") {
        let Some(id) = shape.attribute("ID") else { continue };
        let cell = |node: roxmltree::Node<'_, '_>, name: &str| node.children().find(|c| c.tag_name().name() == "Cell" && c.attribute("N") == Some(name)).and_then(|c| c.attribute("V").map(str::to_string));
        if let Some(formula) = shape.children().find(|c| c.tag_name().name() == "Cell" && c.attribute("N") == Some("Relationships")).and_then(|c| c.attribute("F")) {
            formulas.insert(id.to_string(), formula.to_string());
        }
        let rows: Vec<_> = shape.children().filter(|c| c.tag_name().name() == "Section" && c.attribute("N") == Some("User")).flat_map(|section| section.children()).collect();
        let value = |name: &str| rows.iter().find(|row| row.attribute("N") == Some(name)).and_then(|row| cell(*row, "Value"));
        if value("msvStructureType").is_some_and(|kind| kind.trim_matches('"') == "Container") {
            let locked = value("msvSDContainerLocked").is_some_and(|v| !matches!(v.as_str(), "0" | "FALSE"));
            declared.insert(id.to_string(), locked);
        }
    }
    (formulas, declared)
}

/// The room a new container leaves round what it holds, and above it for its heading, in inches.
pub const MARGIN: f64 = 0.118_110_236_220_472;
pub const HEADING: f64 = 0.3;

/// Put a new container round shapes of page `page_index`, behind them, in LayoutCraft's own plain look: the box (`left`,
/// `bottom`, `width`, `height`, page inches, y up) with its heading at the top. The container and `members` name each
/// other as Visio writes it (`docs/45-containers.md`). Returns the new package and the container's ID.
pub fn create(package: &Package, page_index: usize, members: &[String], left: f64, bottom: f64, width: f64, height: f64, heading: &str) -> Result<(Vec<u8>, String)> {
    let part = package.pages()?.get(page_index).ok_or_else(|| Error(format!("page {page_index} is out of range")))?.part.clone();
    let id = crate::writer::next_shape_id(package, page_index)?;
    let (mut formulas, _) = page_membership(&package.text(&part)?);
    let mut own = None;
    for member in members.iter().rev() {
        own = Some(with_membership(own.as_deref(), MEMBERS, member, true));
    }
    let own = own.ok_or_else(|| Error("a new container needs a shape to hold".to_string()))?;
    let outline = crate::draw::polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], width, height);
    let shape = crate::draw::closed_shape_xml(id, &format!("Container.{id}"), left, bottom, width, height, false, None, &outline, "", None)
        .replace("V='#ffffff'", "V='#f1f4f8'")
        .replace("<Cell N='LineColor' V='#000000'/>", "<Cell N='LineColor' V='#4a5a70'/>");
    let user = |name: &str, value: &str, unit: &str| format!("<Row N='{name}'><Cell N='Value' V='{value}'{unit}/></Row>");
    let cells = format!(
        "<Cell N='VerticalAlign' V='0'/><Cell N='Relationships' V='0' F='{own}'/><Section N='User'>{}{}{}{}{}</Section>",
        user("msvStructureType", "Container", " U='STR'"),
        user("msvShapeCategories", "ContainerStyleDefaults", " U='STR'"),
        user("msvSDContainerMargin", &MARGIN.to_string(), " U='MM'"),
        user("msvSDContainerResize", "1", ""),
        user("msvSDContainerLocked", "0", " U='BOOL'"),
    );
    let escaped = heading.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    // the cells go before the first section, the text last
    let at = shape.find("<Section ").ok_or_else(|| Error("the container shape has no section".to_string()))?;
    let end = shape.rfind("</Shape>").ok_or_else(|| Error("the container shape is not closed".to_string()))?;
    let shape = format!("{}{cells}{}<Text>{escaped}</Text></Shape>", &shape[..at], &shape[at..end]);
    let mut xml = package.text(&part)?.to_string();
    // first among the shapes: behind everything it holds
    let open = xml.find("<Shapes>").map(|n| n + "<Shapes>".len()).ok_or_else(|| Error("page has no root Shapes element".to_string()))?;
    xml.insert_str(open, &shape);
    let mut extras = BTreeMap::new();
    extras.insert(part, xml.into_bytes());
    let with_shape = Package::from_bytes(&crate::writer::write_package(package, extras)?)?;
    let container = id.to_string();
    let patches: Vec<crate::writer::ShapeCellPatch> = members
        .iter()
        .map(|member| {
            let formula = with_membership(formulas.remove(member).as_deref(), CONTAINED_IN, &container, true);
            crate::writer::ShapeCellPatch::new(member.clone()).set_with_formula("Relationships", "0", formula)
        })
        .collect();
    Ok((crate::writer::patch_page_cells(&with_shape, page_index, &patches)?, container))
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
    use super::{parse_relationships, with_membership, without_references};

    #[test]
    fn a_membership_is_added_first_and_removed_from_its_term_only() {
        let one = with_membership(None, 4, "1", true);
        assert_eq!(one, "SUM(DEPENDSON(4,Sheet.1!SheetRef()))");
        assert_eq!(with_membership(Some("FALSE"), 4, "1", true), one);
        let two = with_membership(Some(&one), 4, "9", true);
        assert_eq!(parse_relationships(&two), [(4, vec!["9".to_string(), "1".to_string()])]);
        assert_eq!(with_membership(Some(&two), 4, "9", true), two, "named once");
        let both = with_membership(Some(&two), 1, "7", true);
        assert_eq!(parse_relationships(&both).len(), 2);
        let gone = with_membership(Some(&with_membership(Some(&both), 4, "1", false)), 4, "9", false);
        assert!(gone.contains("DEPENDSON(4)") && gone.contains("Sheet.7!"), "{gone}");
        assert_eq!(with_membership(Some(&one), 1, "5", false), one);
    }

    #[test]
    fn a_page_says_which_shapes_call_themselves_containers_and_which_are_locked() {
        let row = |name: &str, value: &str| format!("<Row N='{name}'><Cell N='Value' V='{value}'/></Row>");
        let shape = |id: u32, rows: String| format!("<Shape ID='{id}'><Section N='User'>{rows}</Section></Shape>");
        let xml = format!(
            "<PageContents><Shapes>{}{}{}<Shape ID='4'><Cell N='Relationships' V='0' F='SUM(DEPENDSON(4,Sheet.1!SheetRef()))'/></Shape></Shapes></PageContents>",
            shape(1, row("msvStructureType", "Container")),
            shape(2, row("msvStructureType", "Container") + &row("msvSDContainerLocked", "1")),
            shape(3, row("msvStructureType", "List")),
        );
        let (formulas, declared) = super::page_membership(&xml);
        assert_eq!(declared.into_iter().collect::<Vec<_>>(), [("1".to_string(), false), ("2".to_string(), true)]);
        assert_eq!(formulas.keys().collect::<Vec<_>>(), ["4"]);
    }

    #[test]
    fn only_the_refused_references_leave_the_relationships_cells() {
        let xml = "<Shape ID='1'><Cell N='Relationships' V='0' F='SUM(DEPENDSON(1,Sheet.7!SheetRef(),Sheet.70!SheetRef()),DEPENDSON(4,Sheet.7!SheetRef()))'/><Cell N='Width' F='SUM(1,Sheet.7!SheetRef())'/></Shape>";
        let pruned = without_references(xml, |id| id != "7").unwrap();
        assert!(pruned.contains("F='SUM(DEPENDSON(1,Sheet.70!SheetRef()),DEPENDSON(4))'"), "{pruned}");
        assert!(pruned.contains("N='Width' F='SUM(1,Sheet.7!SheetRef())'"), "another cell is left alone");
        assert!(without_references(xml, |_| true).is_none());
        let double = xml.replace('\'', "\"");
        assert!(without_references(&double, |id| id != "70").is_some_and(|out| !out.contains("Sheet.70!")));
    }

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
