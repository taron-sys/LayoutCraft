//! Which connector ends a binary drawing says are glued. The links are not a list of their own: they are the formulas of the
//! connector's `BeginX` .. `EndY` cells and of its two trigger cells, stored as tokens behind the fixed fields of two records.
//! Only the three exact forms below are read; any other formula leaves the end loose. Evidence: `docs/47-legacy-formats.md`.
use super::super::drawing::{Connect, Shape};
use crate::groupedit::glue_formula;
use std::collections::{HashMap, HashSet};

/// A formula block is `length u32, type u8, cell u8, tokens`; type 2 holds a formula.
const FORMULA: u8 = 2;
/// The blocks start shortly after the record's fixed fields.
const MAX_BLOCK_START: usize = 128;
const MAX_BODY: usize = 64 * 1024;
/// Cell numbers of the trigger cells in the event record.
const BEGIN_TRIGGER: u8 = 11;
const END_TRIGGER: u8 = 12;

/// What the coordinate cells of one end say.
#[derive(Debug, Clone, Copy, PartialEq)]
enum End {
    /// `PAR(PNT(Sheet.s!Connections.Xr, Sheet.s!Connections.Yr))`, the row counted from 0.
    Point(u32, u32),
    /// `_WALKGLUE(..)`: glued to a whole shape, which the end's trigger cell names.
    Walk,
}

#[derive(Debug, Default, Clone)]
pub(super) struct Links {
    /// The X and Y cell of the begin end, then of the end end.
    cells: [Option<End>; 4],
    /// The shape each end's `_XFTRIGGER(Sheet.s!EventXFMod)` names.
    triggers: [Option<u32>; 2],
}

/// The formulas of a record's body as `(cell number, tokens)`: the first position from which formula blocks reach the end.
fn formulas(body: &[u8]) -> Vec<(u8, &[u8])> {
    if body.len() > MAX_BODY {
        return Vec::new();
    }
    for start in 0..body.len().min(MAX_BLOCK_START) {
        let mut at = start;
        let mut found = Vec::new();
        let mut blocks = 0;
        while let Some(head) = body.get(at..at + 6) {
            let length = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
            if length < 6 || length > body.len() - at || !(1..=6).contains(&head[4]) {
                break;
            }
            if head[4] == FORMULA {
                found.push((head[5], &body[at + 6..at + length]));
            }
            at += length;
            blocks += 1;
        }
        // A few zero bytes may follow the last block.
        if blocks > 0 && body.len() - at <= 4 && body[at..].iter().all(|&b| b == 0) {
            return found;
        }
    }
    Vec::new()
}

/// The shape and row of `PAR(PNT(..))`: two references to section 7 (connection points), cells 0 (X) and 1 (Y) of one row.
fn point(tokens: &[u8]) -> Option<End> {
    const FUNCTIONS: &[u8] = &[0x80, 0x0b, 0, 0, 0x40, 0x7a, 0x33, 0, 0, 0x40, 0xfe];
    if tokens.len() != 24 + FUNCTIONS.len() || &tokens[24..] != FUNCTIONS {
        return None;
    }
    let (x, y) = (&tokens[..12], &tokens[12..24]);
    let reference = x[0] == 0x70 && x[9] == 7 && x[10..] == [0, 0] && y[..10] == x[..10] && y[10..] == [1, 0];
    let word = |at: usize| u32::from_le_bytes([x[at], x[at + 1], x[at + 2], x[at + 3]]);
    reference.then(|| End::Point(word(1), word(5)))
}

/// `_WALKGLUE(own trigger, other trigger, WalkPreference)`; the triggers are the local cells 0x66 (begin) and 0x67 (end).
fn walk(tokens: &[u8], end: usize) -> Option<End> {
    let (own, other) = if end == 0 { (0x66, 0x67) } else { (0x67, 0x66) };
    let expected = [0x75, own, 0, 0, 0x60, 0x75, other, 0, 0, 0x60, 0x75, 0x65, 0, 0, 0x60, 0x7a, 0x70, 0, 0, 0x40, 0xfe];
    (tokens == expected).then_some(End::Walk)
}

/// The shape of `_XFTRIGGER(Sheet.s!EventXFMod)`.
fn trigger(tokens: &[u8]) -> Option<u32> {
    match tokens {
        [0x74, 0x4e, 0, 0, 0x60, a, b, c, d, 0x7a, 0x73, 0, 0, 0x40, 0xfe] => Some(u32::from_le_bytes([*a, *b, *c, *d])),
        _ => None,
    }
}

impl Links {
    /// The record with the end coordinates of a 1-D shape.
    pub fn transform_1d(&mut self, body: &[u8]) {
        for (cell, tokens) in formulas(body) {
            if let Some(slot) = self.cells.get_mut(cell as usize) {
                *slot = point(tokens).or_else(|| walk(tokens, cell as usize / 2));
            }
        }
    }

    /// The record with the event cells.
    pub fn events(&mut self, body: &[u8]) {
        for (cell, tokens) in formulas(body) {
            match cell {
                BEGIN_TRIGGER => self.triggers[0] = trigger(tokens),
                END_TRIGGER => self.triggers[1] = trigger(tokens),
                _ => {}
            }
        }
    }

    /// The target of one end as `(shape, connection point counted from 1, or none for the whole shape)`.
    fn target(&self, end: usize) -> Option<(u32, Option<u32>)> {
        let (x, y) = (self.cells[end * 2]?, self.cells[end * 2 + 1]?);
        match x {
            _ if x != y => None,
            End::Point(shape, row) => Some((shape, Some(row.checked_add(1)?))),
            End::Walk => Some((self.triggers[end]?, None)),
        }
    }
}

/// Glue the ends of the page's shapes whose target is another shape of the same page: the cells get the formulas, as the
/// editor writes them, and the page gets the `Connect` rows.
pub(super) fn connect(shapes: &mut [Shape], links: &HashMap<u32, Links>) -> Vec<Connect> {
    fn ids(shapes: &[Shape], into: &mut HashSet<u32>) {
        for shape in shapes {
            into.insert(shape.id);
            ids(&shape.children, into);
        }
    }
    fn walk_shapes(shapes: &mut [Shape], present: &HashSet<u32>, links: &HashMap<u32, Links>, out: &mut Vec<Connect>) {
        for shape in shapes {
            walk_shapes(&mut shape.children, present, links, out);
            let Some(found) = links.get(&shape.id) else { continue };
            let mut targets = [found.target(0), found.target(1)].map(|t| t.filter(|(to, _)| *to != shape.id && present.contains(to)));
            // Both ends on one connection point: drawings saved again as `.vsdx` list no connection for this.
            if targets[0] == targets[1] && targets[0].is_some_and(|(_, point)| point.is_some()) {
                targets = [None, None];
            }
            for (end, target) in targets.into_iter().enumerate() {
                let Some((to, point)) = target else { continue };
                let (x, y, trigger) = if end == 0 { ("BeginX", "BeginY", "BegTrigger") } else { ("EndX", "EndY", "EndTrigger") };
                // Without the coordinate cells there is nothing to hold the formula.
                if shape.sheet.cell(x).is_none() || shape.sheet.cell(y).is_none() {
                    continue;
                }
                let to = to.to_string();
                let formula = glue_formula(&(to.as_str(), point.map(|p| p as usize)), end == 0);
                for cell in shape.sheet.cells.iter_mut().filter(|c| c.name == x || c.name == y) {
                    cell.formula = Some(formula.clone());
                }
                shape.sheet.set(trigger, "2");
                for cell in shape.sheet.cells.iter_mut().filter(|c| c.name == trigger) {
                    cell.formula = Some(format!("_XFTRIGGER(Sheet.{to}!EventXFMod)"));
                }
                out.push(Connect {
                    from_sheet: shape.id.to_string(),
                    from_cell: x.to_string(),
                    to_cell: point.map_or_else(|| "PinX".to_string(), |p| format!("Connections.X{p}")),
                    to_sheet: to,
                });
            }
        }
    }
    let mut present = HashSet::new();
    ids(shapes, &mut present);
    let mut out = Vec::new();
    walk_shapes(shapes, &present, links, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(kind: u8, cell: u8, tokens: &[u8]) -> Vec<u8> {
        let mut out = (tokens.len() as u32 + 6).to_le_bytes().to_vec();
        out.extend([kind, cell]);
        out.extend(tokens);
        out
    }

    fn reference(shape: u32, row: u32, column: u8) -> Vec<u8> {
        let mut out = vec![0x70];
        out.extend(shape.to_le_bytes());
        out.extend(row.to_le_bytes());
        out.extend([7, column, 0]);
        out
    }

    fn to_point(shape: u32, row: u32) -> Vec<u8> {
        [reference(shape, row, 0), reference(shape, row, 1), vec![0x80, 0x0b, 0, 0, 0x40, 0x7a, 0x33, 0, 0, 0x40, 0xfe]].concat()
    }

    #[test]
    fn reads_both_kinds_of_glue_and_nothing_else() {
        let walking = [0x75, 0x67, 0, 0, 0x60, 0x75, 0x66, 0, 0, 0x60, 0x75, 0x65, 0, 0, 0x60, 0x7a, 0x70, 0, 0, 0x40, 0xfe];
        let mut transform = vec![0x40; 36];
        transform.extend([1, 3]);
        for (cell, tokens) in [(0, to_point(7, 1)), (1, to_point(7, 1)), (2, walking.to_vec()), (3, walking.to_vec())] {
            transform.extend(block(FORMULA, cell, &tokens));
        }
        let mut events = vec![9; 20];
        events.extend(block(FORMULA, END_TRIGGER, &[0x74, 0x4e, 0, 0, 0x60, 5, 0, 0, 0, 0x7a, 0x73, 0, 0, 0x40, 0xfe]));
        let mut links = Links::default();
        links.transform_1d(&transform);
        assert_eq!(links.target(0), Some((7, Some(2))));
        assert_eq!(links.target(1), None, "a walking end needs its trigger");
        links.events(&events);
        assert_eq!(links.target(1), Some((5, None)));
        // X and Y that disagree, and a formula of another shape, leave the end loose
        let mut other = Links::default();
        let mut transform = block(FORMULA, 0, &to_point(7, 1));
        transform.extend(block(FORMULA, 1, &to_point(8, 1)));
        transform.extend(block(FORMULA, 2, &reference(7, 0, 0)));
        transform.extend(block(FORMULA, 3, &reference(7, 0, 0)));
        other.transform_1d(&transform);
        assert_eq!((other.target(0), other.target(1)), (None, None));
    }

    #[test]
    fn cut_or_foreign_bytes_give_no_formulas() {
        let whole = block(FORMULA, 0, &to_point(3, 0));
        for length in 0..whole.len() {
            assert!(formulas(&whole[..length]).iter().all(|(_, tokens)| point(tokens).is_none()), "{length}");
        }
        assert!(formulas(&[0xff; 300]).is_empty());
    }
}
