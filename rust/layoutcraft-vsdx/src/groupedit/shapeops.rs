//! Shape operations of Visio's Shape tab (Operations): Union, Combine, Fragment, Intersect and Subtract on shapes made of
//! straight lines (rectangles, polygons, lines closed to a polygon) and ellipses (drawn as 72-sided polygons).
//!
//! Measured on the installed Visio (visio-test-harness/probe-shape-ops.ps1, two overlapping rectangles A = (1,1)-(4,3) and
//! B = (3,2)-(6,5)): the result replaces the selected shapes, takes its look, text and place in the drawing order from the
//! FIRST selected shape (a different selection order gives the other shape's look) and is written as one closed
//! `RelMoveTo` / `RelLineTo` path in the box of the result (the fractions of its width and height). Union = one outline;
//! Combine = one geometry section per input shape (the overlap becomes a hole, as the sections fill even-odd);
//! Intersect = the common part; Subtract = the first shape without the others; Fragment = one text-less shape per piece
//! (B only, A only, A and B), every piece with the look of the first shape.
use super::*;
use crate::shapes::{Section, SectionRow};
use crate::writer::{find_shape_range, set_attr};

type Pt = (f64, f64);
type Ring = Vec<Pt>;

/// The operation of the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeOp {
    Union,
    Combine,
    Fragment,
    Intersect,
    Subtract,
    /// Join (step 286): lines and open paths whose ends touch become ONE path; see `join_paths`.
    Join,
    /// Trim (step 287): the paths of the selected shapes are cut at the points where they cross or touch one another; see `trim_operation`.
    Trim,
}

const EPS: f64 = 1e-9;
/// Distance of the probe points that tell which side of an edge is inside which shape.
const PROBE: f64 = 1e-5;

fn key(p: Pt) -> (i64, i64) {
    ((p.0 * 1e7).round() as i64, (p.1 * 1e7).round() as i64)
}

fn area(ring: &Ring) -> f64 {
    let mut a = 0.0;
    for i in 0..ring.len() {
        let (p, q) = (ring[i], ring[(i + 1) % ring.len()]);
        a += p.0 * q.1 - q.0 * p.1;
    }
    a / 2.0
}

/// Even-odd point in polygon set test.
fn inside(rings: &[Ring], p: Pt) -> bool {
    let mut count = 0;
    for ring in rings {
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            if (a.1 > p.1) != (b.1 > p.1) {
                let x = a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0);
                if x > p.0 {
                    count += 1;
                }
            }
        }
    }
    count % 2 == 1
}

/// The rings of a shape in page inches, one list per geometry section (so Combine can keep the sections apart).
fn shape_sections(node: &ResolvedShape) -> Result<Vec<Vec<Ring>>> {
    if node.shape_type.as_deref() == Some("Group") || !node.children.is_empty() {
        return Err(Error(format!("shape {} is a group; ungroup it first", node.id)));
    }
    if node.is_master_derived() {
        return Err(Error(format!("shape {} is a stencil shape; shape operations need shapes of their own geometry", node.id)));
    }
    let frame = frame_of(node)?;
    let (w, h) = frame.size;
    let mut sections: Vec<&Section> = node.sections.iter().filter(|s| s.name == "Geometry").collect();
    sections.sort_by_key(|s| s.index.as_deref().and_then(|i| i.parse::<i64>().ok()).unwrap_or(0));
    if sections.is_empty() {
        return Err(Error(format!("shape {} has no outline", node.id)));
    }
    let cell = |row: &SectionRow, name: &str| row.cells.get(name).and_then(|c| c.value.as_deref()).and_then(|v| v.parse::<f64>().ok());
    let mut out = Vec::new();
    for section in sections {
        if section.cells.get("NoFill").and_then(|c| c.value.as_deref()) == Some("1") {
            continue; // an open path or a decoration without fill takes no part
        }
        let mut rows: Vec<&SectionRow> = section.rows.iter().filter(|r| !r.deleted).collect();
        rows.sort_by_key(|r| r.index.as_deref().and_then(|i| i.parse::<i64>().ok()).unwrap_or(0));
        let mut rings: Vec<Ring> = Vec::new();
        let mut current: Ring = Vec::new();
        let finish = |current: &mut Ring, rings: &mut Vec<Ring>| {
            let mut ring = std::mem::take(current);
            if ring.len() > 1 && key(ring[0]) == key(ring[ring.len() - 1]) {
                ring.pop();
            }
            if ring.len() >= 3 {
                rings.push(ring);
            }
        };
        for row in rows {
            let kind = row.row_type.as_deref().unwrap_or("");
            let (x, y) = (cell(row, "X").unwrap_or(0.0), cell(row, "Y").unwrap_or(0.0));
            let local = |x: f64, y: f64| if kind.starts_with("Rel") { (x * w, y * h) } else { (x, y) };
            match kind {
                "MoveTo" | "RelMoveTo" => {
                    finish(&mut current, &mut rings);
                    current.push(frame.to_parent(local(x, y)));
                }
                "LineTo" | "RelLineTo" => current.push(frame.to_parent(local(x, y))),
                "Ellipse" => {
                    finish(&mut current, &mut rings);
                    let (ax, ay) = (cell(row, "A").unwrap_or(x), cell(row, "B").unwrap_or(y));
                    let (cx, cy) = (cell(row, "C").unwrap_or(x), cell(row, "D").unwrap_or(y));
                    let (rx, ry) = ((ax - x, ay - y), (cx - x, cy - y));
                    for k in 0..72 {
                        let t = std::f64::consts::TAU * k as f64 / 72.0;
                        current.push(frame.to_parent((x + rx.0 * t.cos() + ry.0 * t.sin(), y + rx.1 * t.cos() + ry.1 * t.sin())));
                    }
                    finish(&mut current, &mut rings);
                }
                other => return Err(Error(format!("shape {} has a {other} row; only straight lines and ellipses can be combined", node.id))),
            }
        }
        finish(&mut current, &mut rings);
        if !rings.is_empty() {
            out.push(rings);
        }
    }
    if out.is_empty() {
        return Err(Error(format!("shape {} has no filled outline", node.id)));
    }
    Ok(out)
}

/// Intersections of two segments: the parameters on the first one at which the second one touches it.
fn cuts(a: (Pt, Pt), b: (Pt, Pt)) -> Vec<f64> {
    let d = (a.1 .0 - a.0 .0, a.1 .1 - a.0 .1);
    let e = (b.1 .0 - b.0 .0, b.1 .1 - b.0 .1);
    let len2 = d.0 * d.0 + d.1 * d.1;
    let denom = d.0 * e.1 - d.1 * e.0;
    let mut out = Vec::new();
    let w = (b.0 .0 - a.0 .0, b.0 .1 - a.0 .1);
    if denom.abs() > 1e-12 * (len2.sqrt() * (e.0 * e.0 + e.1 * e.1).sqrt()).max(1e-300) {
        let t = (w.0 * e.1 - w.1 * e.0) / denom;
        let u = (w.0 * d.1 - w.1 * d.0) / denom;
        if t > EPS && t < 1.0 - EPS && u > -EPS && u < 1.0 + EPS {
            out.push(t);
        }
    } else if (w.0 * d.1 - w.1 * d.0).abs() <= 1e-9 * len2.sqrt().max(1.0) {
        // parallel and on one line: the other segment's ends that lie inside this one
        for p in [b.0, b.1] {
            let t = ((p.0 - a.0 .0) * d.0 + (p.1 - a.0 .1) * d.1) / len2;
            if t > EPS && t < 1.0 - EPS {
                out.push(t);
            }
        }
    }
    out
}

/// The outline rings (the result on the left of every edge) of the points where `keep(membership)` holds, `membership[i]`
/// telling whether the point is inside shape `i` (`shapes[i]` = all its rings, filled even-odd).
fn overlay(shapes: &[Vec<Ring>], keep: &dyn Fn(&[bool]) -> bool) -> Vec<Ring> {
    let mut edges: Vec<(Pt, Pt)> = Vec::new();
    for rings in shapes {
        for ring in rings {
            for i in 0..ring.len() {
                let (p, q) = (ring[i], ring[(i + 1) % ring.len()]);
                if key(p) != key(q) {
                    edges.push((p, q));
                }
            }
        }
    }
    // split the edges where they cross or touch
    let mut points: std::collections::HashMap<(i64, i64), Pt> = std::collections::HashMap::new();
    let mut pieces: Vec<((i64, i64), (i64, i64))> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (i, &(p, q)) in edges.iter().enumerate() {
        let mut ts = vec![0.0, 1.0];
        for (j, &other) in edges.iter().enumerate() {
            if i != j {
                ts.extend(cuts((p, q), other));
            }
        }
        ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let at = |t: f64| (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t);
        for pair in ts.windows(2) {
            let (a, b) = (at(pair[0]), at(pair[1]));
            let (ka, kb) = (key(a), key(b));
            if ka == kb {
                continue;
            }
            points.entry(ka).or_insert(a);
            points.entry(kb).or_insert(b);
            let id = if ka < kb { (ka, kb) } else { (kb, ka) };
            if seen.insert(id) {
                pieces.push((ka, kb));
            }
        }
    }
    // keep the pieces that separate the result from the rest
    let member = |p: Pt| -> Vec<bool> { shapes.iter().map(|rings| inside(rings, p)).collect() };
    let mut boundary: Vec<((i64, i64), (i64, i64))> = Vec::new();
    for &(ka, kb) in &pieces {
        let (a, b) = (points[&ka], points[&kb]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt();
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let probe = PROBE.min(len * 0.01);
        let normal = (-dy / len * probe, dx / len * probe);
        let left = keep(&member((mid.0 + normal.0, mid.1 + normal.1)));
        let right = keep(&member((mid.0 - normal.0, mid.1 - normal.1)));
        if left != right {
            boundary.push(if left { (ka, kb) } else { (kb, ka) });
        }
    }
    // chain the edges into rings
    let mut from: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::new();
    for (i, &(a, _)) in boundary.iter().enumerate() {
        from.entry(a).or_default().push(i);
    }
    let mut used = vec![false; boundary.len()];
    let mut rings = Vec::new();
    for start in 0..boundary.len() {
        if used[start] {
            continue;
        }
        let mut ring = vec![points[&boundary[start].0]];
        used[start] = true;
        let mut at = boundary[start].1;
        let mut closed = false;
        for _ in 0..boundary.len() + 1 {
            if at == boundary[start].0 {
                closed = true;
                break;
            }
            ring.push(points[&at]);
            let Some(next) = from.get(&at).and_then(|list| list.iter().copied().find(|&n| !used[n])) else { break };
            used[next] = true;
            at = boundary[next].1;
        }
        if closed {
            rings.push(simplify(ring));
        }
    }
    rings.retain(|r| r.len() >= 3 && area(r).abs() > 1e-9);
    rings
}

/// Drops the corners that lie on the line between their neighbours.
fn simplify(mut ring: Ring) -> Ring {
    loop {
        let n = ring.len();
        if n < 4 {
            return ring;
        }
        let drop = (0..n).find(|&i| {
            let (p, c, q) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
            let cross = (c.0 - p.0) * (q.1 - c.1) - (c.1 - p.1) * (q.0 - c.0);
            let dot = (c.0 - p.0) * (q.0 - c.0) + (c.1 - p.1) * (q.1 - c.1);
            cross.abs() < 1e-9 && dot > 0.0
        });
        match drop {
            Some(i) => {
                ring.remove(i);
            }
            None => return ring,
        }
    }
}

/// The rings split into connected pieces: every outer ring (counter-clockwise) with the holes (clockwise) it contains.
fn pieces_of(rings: Vec<Ring>) -> Vec<Vec<Ring>> {
    let (outer, holes): (Vec<Ring>, Vec<Ring>) = rings.into_iter().partition(|r| area(r) > 0.0);
    let mut pieces: Vec<Vec<Ring>> = outer.into_iter().map(|r| vec![r]).collect();
    for hole in holes {
        let target = pieces
            .iter()
            .enumerate()
            .filter(|(_, piece)| inside(&piece[..1], hole[0]))
            .min_by(|a, b| area(&a.1[0]).partial_cmp(&area(&b.1[0])).unwrap())
            .map(|(i, _)| i);
        match target {
            Some(i) => pieces[i].push(hole),
            None => pieces.push(vec![hole.into_iter().rev().collect()]),
        }
    }
    pieces
}

/// The XML of the new shape: the first shape's own XML (look, text, data) with the frame, the outline and the connection
/// points rewritten for the result.
fn result_xml(source: &str, id: u64, sections: &[Vec<Ring>], keep_text: bool) -> Result<String> {
    let paths: Vec<Vec<(Ring, bool)>> = sections.iter().map(|s| s.iter().map(|r| (r.clone(), true)).collect()).collect();
    result_xml_paths(source, id, &paths, keep_text)
}

/// `result_xml` for paths that may be open: every section holds `(points, closed)`; an all-closed section is filled (NoFill 0), a section with an open path is not (NoFill 1).
fn result_xml_paths(source: &str, id: u64, sections: &[Vec<(Ring, bool)>], keep_text: bool) -> Result<String> {
    let all: Vec<&Ring> = sections.iter().flatten().map(|(ring, _)| ring).collect();
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in all.iter().flat_map(|r| r.iter()) {
        x0 = x0.min(p.0);
        y0 = y0.min(p.1);
        x1 = x1.max(p.0);
        y1 = y1.max(p.1);
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let any_closed = sections.iter().flatten().any(|(_, closed)| *closed);
    if (any_closed && !(w > 1e-6 && h > 1e-6)) || !(w > 1e-6 || h > 1e-6) {
        return Err(Error("the result has no area".to_string()));
    }
    // a path along one axis only (a horizontal line) has a box without height: the fractions are then 0
    let (dw, dh) = (if w > 1e-6 { w } else { 1.0 }, if h > 1e-6 { h } else { 1.0 });
    let open_end = source.find('>').ok_or_else(|| Error("shape XML is damaged".to_string()))? + 1;
    let mut tag = source[..open_end].to_string();
    tag = set_attr(&tag, "ID", &id.to_string());
    for name in ["NameU", "Name"] {
        while let Some(at) = tag.find(&format!(" {name}=")) {
            let quote = tag[at + name.len() + 2..].chars().next().unwrap_or('\'');
            let end = tag[at + name.len() + 3..].find(quote).map_or(tag.len(), |n| at + name.len() + 3 + n + 1);
            tag.replace_range(at..end, "");
        }
    }
    let rest = &source[open_end..];
    // the shape's own cells come first, then its sections, text and sub-shapes
    let head_end = ["<Section ", "<Text", "<Shapes", "<Data1", "</Shape>"].iter().filter_map(|m| rest.find(m)).min().unwrap_or(rest.len());
    let (mut head, tail) = (rest[..head_end].to_string(), &rest[head_end..]);
    for name in ["PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY", "Angle", "FlipX", "FlipY", "TxtPinX", "TxtPinY", "TxtLocPinX", "TxtLocPinY", "TxtWidth", "TxtHeight", "TxtAngle", "BeginX", "BeginY", "EndX", "EndY", "ObjType"] {
        while let Some(at) = head.find(&format!("<Cell N='{name}'")) {
            let end = head[at..].find("/>").map(|n| at + n + 2).ok_or_else(|| Error("cell XML is damaged".to_string()))?;
            head.replace_range(at..end, "");
        }
    }
    let mut out = tag;
    out.push_str(&format!(
        "<Cell N='PinX' V='{}'/><Cell N='PinY' V='{}'/><Cell N='Width' V='{}'/><Cell N='Height' V='{}'/><Cell N='LocPinX' V='{}' F='Width*0.5'/><Cell N='LocPinY' V='{}' F='Height*0.5'/><Cell N='Angle' V='0'/><Cell N='FlipX' V='0'/><Cell N='FlipY' V='0'/>",
        fmt((x0 + x1) / 2.0),
        fmt((y0 + y1) / 2.0),
        fmt(w),
        fmt(h),
        fmt(w / 2.0),
        fmt(h / 2.0)
    ));
    out.push_str(&head);
    // the old sections: everything except the outline and the connection points
    let mut tail = tail.to_string();
    for name in ["Geometry", "Connection", "Controls"] {
        while let Some(at) = tail.find(&format!("<Section N='{name}'")) {
            let end = tail[at..].find("</Section>").map(|n| at + n + "</Section>".len()).ok_or_else(|| Error("section XML is damaged".to_string()))?;
            tail.replace_range(at..end, "");
        }
    }
    if !keep_text {
        if let Some(at) = tail.find("<Text") {
            if let Some(end) = tail[at..].find("</Text>") {
                tail.replace_range(at..at + end + "</Text>".len(), "");
            }
        }
    }
    let mut geometry = String::new();
    let mut ix = 0;
    for rings in sections {
        let nofill = if rings.iter().all(|(_, closed)| *closed) { 0 } else { 1 };
        geometry.push_str(&format!("<Section N='Geometry' IX='{ix}'><Cell N='NoFill' V='{nofill}'/><Cell N='NoLine' V='0'/><Cell N='NoShow' V='0'/><Cell N='NoSnap' V='0'/><Cell N='NoQuickDrag' V='0'/>"));
        ix += 1;
        let mut row = 1;
        for (ring, closed) in rings {
            let rel = |p: Pt| (fmt((p.0 - x0) / dw), fmt((p.1 - y0) / dh));
            let walk: Vec<Pt> = if *closed { ring.iter().cloned().chain(std::iter::once(ring[0])).collect() } else { ring.clone() };
            for (n, p) in walk.iter().enumerate() {
                let (rx, ry) = rel(*p);
                let kind = if n == 0 { "RelMoveTo" } else { "RelLineTo" };
                geometry.push_str(&format!("<Row T='{kind}' IX='{row}'><Cell N='X' V='{rx}'/><Cell N='Y' V='{ry}'/></Row>"));
                row += 1;
            }
        }
        geometry.push_str("</Section>");
    }
    // the outline goes before the text, after the other sections
    let at = tail.find("<Text").or_else(|| tail.find("<Shapes")).or_else(|| tail.rfind("</Shape>")).unwrap_or(tail.len());
    tail.insert_str(at, &geometry);
    out.push_str(&tail);
    Ok(out)
}

/// Runs `op` on the page-level shapes `ids` (the first one is the one whose look the result keeps). Returns the new package
/// and the ids of the new shapes; the selected shapes are gone and connectors glued to them keep their ends in place.
pub fn shape_operation(package: &Package, page_index: usize, op: ShapeOp, ids: &[String]) -> Result<(Vec<u8>, Vec<String>)> {
    if op == ShapeOp::Join {
        return join_operation(package, page_index, ids);
    }
    if op == ShapeOp::Trim {
        return trim_operation(package, page_index, ids);
    }
    if ids.len() < 2 {
        return Err(Error("select at least two shapes".to_string()));
    }
    let mut distinct = ids.to_vec();
    distinct.sort();
    distinct.dedup();
    if distinct.len() != ids.len() {
        return Err(Error("a shape is listed twice".to_string()));
    }
    let editor = GroupEditor::open(package, page_index)?;
    let mut sections_of: Vec<Vec<Vec<Ring>>> = Vec::new();
    for id in ids {
        let node = editor.roots.iter().find(|n| n.id == *id).ok_or_else(|| Error(format!("shape {id} is not on the page itself")))?;
        sections_of.push(shape_sections(node)?);
    }
    let shapes: Vec<Vec<Ring>> = sections_of.iter().map(|s| s.iter().flatten().cloned().collect()).collect();
    let n = shapes.len();
    // the result: a list of (sections of the new shape, keeps the text)
    let mut results: Vec<(Vec<Vec<Ring>>, bool)> = Vec::new();
    match op {
        ShapeOp::Combine => results.push((sections_of.iter().flatten().cloned().collect(), true)),
        ShapeOp::Union | ShapeOp::Intersect | ShapeOp::Subtract => {
            let rings = match op {
                ShapeOp::Union => overlay(&shapes, &|m| m.iter().any(|&v| v)),
                ShapeOp::Intersect => overlay(&shapes, &|m| m.iter().all(|&v| v)),
                _ => overlay(&shapes, &|m| m[0] && !m[1..].iter().any(|&v| v)),
            };
            if rings.is_empty() {
                return Err(Error(match op {
                    ShapeOp::Intersect => "the shapes do not overlap".to_string(),
                    _ => "nothing is left of the shapes".to_string(),
                }));
            }
            results.push((vec![rings], true));
        }
        ShapeOp::Join | ShapeOp::Trim => unreachable!("Join and Trim are handled before"),
        ShapeOp::Fragment => {
            if n > 6 {
                return Err(Error("Fragment works on at most six shapes at a time".to_string()));
            }
            // pieces in Visio's order for two shapes: B only, A only, A and B; singles last to first, then the overlaps
            let mut signatures: Vec<u32> = (0..n).rev().map(|i| 1u32 << i).collect();
            signatures.extend((1u32..(1 << n)).filter(|s| s.count_ones() > 1));
            for sig in signatures {
                let rings = overlay(&shapes, &|m| m.iter().enumerate().fold(0u32, |acc, (i, &v)| acc | ((v as u32) << i)) == sig);
                for piece in pieces_of(rings) {
                    results.push((vec![piece], false));
                }
            }
            if results.is_empty() {
                return Err(Error("nothing to fragment".to_string()));
            }
        }
    }
    let results: Vec<(Vec<Vec<(Ring, bool)>>, bool)> = results.into_iter().map(|(s, k)| (s.into_iter().map(|rings| rings.into_iter().map(|r| (r, true)).collect()).collect(), k)).collect();
    finish_operation(package, page_index, ids, &results)
}

/// Replace the shapes `ids` by the new shapes of `results` (sections of paths, keeps the text), which take the look of the first shape.
fn finish_operation(package: &Package, page_index: usize, ids: &[String], results: &[(Vec<Vec<(Ring, bool)>>, bool)]) -> Result<(Vec<u8>, Vec<String>)> {
    replace_shapes(package, page_index, ids, &|xml, first_id| {
        let (start, end) = find_shape_range(xml, &ids[0]).ok_or_else(|| Error(format!("shape {} not found", ids[0])))?;
        let source = xml[start..end].to_string();
        results.iter().enumerate().map(|(k, (sections, keep_text))| result_xml_paths(&source, first_id + k as u64, sections, *keep_text)).collect()
    })
}

/// Replace the page-level shapes `gone_ids` by new shapes: `make` builds the XML of the new shapes from the page XML and the first free shape id (the k-th gets `first_id + k`);
/// they take the place of the first removed shape in the drawing order, and connectors glued to the removed shapes keep their ends in place.
fn replace_shapes(package: &Package, page_index: usize, gone_ids: &[String], make: &dyn Fn(&str, u64) -> Result<Vec<String>>) -> Result<(Vec<u8>, Vec<String>)> {
    let part = package.pages()?.get(page_index).ok_or_else(|| Error(format!("page {page_index} does not exist")))?.part.clone();
    let mut xml = package.text(&part)?.to_string();
    let (start, _end) = find_shape_range(&xml, &gone_ids[0]).ok_or_else(|| Error(format!("shape {} not found", gone_ids[0])))?;
    let first_id = crate::writer::next_shape_id(package, page_index)?;
    let pieces = make(&xml, first_id)?;
    let new_ids: Vec<String> = (0..pieces.len()).map(|k| (first_id + k as u64).to_string()).collect();
    xml.insert_str(start, &pieces.concat());
    let staged = Package::from_bytes(&crate::writer::with_part(package, &part, xml.into_bytes())?)?;
    let editor = GroupEditor::open(&staged, page_index)?;
    let mut gone: Vec<String> = Vec::new();
    for id in gone_ids {
        gone.extend(editor.subtree_ids(id)?);
    }
    let connects = staged.connects(&part)?;
    let patches = editor.detach_patches(&gone, &connects);
    let bytes = crate::writer::patch_page(&staged, page_index, &patches, &[], gone_ids)?;
    Ok((bytes, new_ids))
}

// ---- Join (step 286) -------------------------------------------------------------------------------------------------------------------------------------------
// Measured on the installed Visio 16.0.14332 (probe-join-trim.ps1, probe-join-gap.ps1): Join on lines / paths whose ends touch (gaps of 1e-6, 1e-4 and 2e-4 in join, 5e-4 and 1e-3 in do not) makes ONE shape
// (2-D, written with RelMoveTo / RelLineTo rows, no fill when the chain is open, filled when it closes into a polygon); the direction of the lines does not matter; the result
// has the look of the first selected shape; paths that do not touch stay in the same shape as separate geometry sections (so does a closed shape such as a rectangle);
// a single line becomes a 2-D shape. Only straight segments (MoveTo / LineTo / PolylineTo and their relative rows) are joined here.

/// The tolerance of Join in inches (probe-join-gap.ps1: Visio joined a gap of 2e-4 in and not one of 5e-4 in; the largest measured join is used).
const JOIN_TOL: f64 = 2e-4;

/// The paths of a shape in page inches: `(points, closed)`, one per MoveTo; a path whose last point is its first (4 or more points) is closed and loses the repeated point.
fn shape_paths(node: &ResolvedShape) -> Result<Vec<(Ring, bool)>> {
    if node.shape_type.as_deref() == Some("Group") || !node.children.is_empty() {
        return Err(Error(format!("shape {} is a group; ungroup it first", node.id)));
    }
    if node.is_master_derived() {
        return Err(Error(format!("shape {} is a stencil shape; Join needs shapes of their own geometry", node.id)));
    }
    let frame = frame_of(node)?;
    let (w, h) = frame.size;
    let mut sections: Vec<&Section> = node.sections.iter().filter(|s| s.name == "Geometry").collect();
    sections.sort_by_key(|s| s.index.as_deref().and_then(|i| i.parse::<i64>().ok()).unwrap_or(0));
    if sections.is_empty() {
        return Err(Error(format!("shape {} has no outline", node.id)));
    }
    let cell = |row: &SectionRow, name: &str| row.cells.get(name).and_then(|c| c.value.as_deref()).and_then(|v| v.parse::<f64>().ok());
    let mut out: Vec<(Ring, bool)> = Vec::new();
    let flush = |current: &mut Ring, out: &mut Vec<(Ring, bool)>| {
        let mut ring = std::mem::take(current);
        if ring.len() < 2 {
            return;
        }
        let closed = ring.len() >= 4 && key(ring[0]) == key(ring[ring.len() - 1]);
        if closed {
            ring.pop();
        }
        out.push((ring, closed));
    };
    for section in sections {
        let mut rows: Vec<&SectionRow> = section.rows.iter().filter(|r| !r.deleted).collect();
        rows.sort_by_key(|r| r.index.as_deref().and_then(|i| i.parse::<i64>().ok()).unwrap_or(0));
        let mut current: Ring = Vec::new();
        for row in rows {
            let kind = row.row_type.as_deref().unwrap_or("");
            let (x, y) = (cell(row, "X").unwrap_or(0.0), cell(row, "Y").unwrap_or(0.0));
            let local = |x: f64, y: f64| if kind.starts_with("Rel") { (x * w, y * h) } else { (x, y) };
            match kind {
                "MoveTo" | "RelMoveTo" => {
                    flush(&mut current, &mut out);
                    current.push(frame.to_parent(local(x, y)));
                }
                "LineTo" | "RelLineTo" => current.push(frame.to_parent(local(x, y))),
                "PolylineTo" => {
                    let a = row.cells.get("A").ok_or_else(|| Error(format!("shape {} has a polyline row without points", node.id)))?;
                    let text = a.formula.as_deref().or(a.value.as_deref()).unwrap_or("");
                    let inner = super::vertices::polyline_points(text, (w, h)).ok_or_else(|| Error(format!("shape {} has POLYLINE points Join cannot read", node.id)))?;
                    for p in inner {
                        current.push(frame.to_parent(p));
                    }
                    current.push(frame.to_parent((x, y)));
                }
                other => return Err(Error(format!("shape {} has a {other} row; Join works on straight line segments", node.id))),
            }
        }
        flush(&mut current, &mut out);
    }
    if out.is_empty() {
        return Err(Error(format!("shape {} has no path", node.id)));
    }
    Ok(out)
}

/// Chain the open paths whose ends touch (in the order of the selection, either direction) into one path each; a chain that comes back to its start is closed; closed shapes and
/// open paths that touch nothing keep their own section. One section per result path.
fn join_paths(paths: Vec<(Ring, bool)>) -> Vec<Vec<(Ring, bool)>> {
    let close = |a: Pt, b: Pt| (a.0 - b.0).abs() < JOIN_TOL && (a.1 - b.1).abs() < JOIN_TOL;
    let mut used = vec![false; paths.len()];
    let mut sections: Vec<Vec<(Ring, bool)>> = Vec::new();
    for i in 0..paths.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        if paths[i].1 {
            sections.push(vec![paths[i].clone()]);
            continue;
        }
        let mut chain = paths[i].0.clone();
        loop {
            let mut extended = false;
            for j in 0..paths.len() {
                if used[j] || paths[j].1 {
                    continue;
                }
                let p = &paths[j].0;
                let (pf, pl) = (p[0], p[p.len() - 1]);
                let (cf, cl) = (chain[0], chain[chain.len() - 1]);
                if close(cl, pf) {
                    chain.extend(p[1..].iter().cloned());
                } else if close(cl, pl) {
                    chain.extend(p[..p.len() - 1].iter().rev().cloned());
                } else if close(cf, pl) {
                    let mut joined = p.clone();
                    joined.extend(chain[1..].iter().cloned());
                    chain = joined;
                } else if close(cf, pf) {
                    let mut joined: Vec<Pt> = p.iter().rev().cloned().collect();
                    joined.extend(chain[1..].iter().cloned());
                    chain = joined;
                } else {
                    continue;
                }
                used[j] = true;
                extended = true;
                break;
            }
            if !extended {
                break;
            }
        }
        let closed = chain.len() >= 4 && close(chain[0], chain[chain.len() - 1]);
        if closed {
            chain.pop();
        }
        sections.push(vec![(chain, closed)]);
    }
    sections
}

/// Join on the page-level shapes `ids` (one or more; the first gives the look): the selected shapes are replaced by ONE new shape. Returns the new package and the new shape's id.
fn join_operation(package: &Package, page_index: usize, ids: &[String]) -> Result<(Vec<u8>, Vec<String>)> {
    if ids.is_empty() {
        return Err(Error("select at least one shape".to_string()));
    }
    let mut distinct = ids.to_vec();
    distinct.sort();
    distinct.dedup();
    if distinct.len() != ids.len() {
        return Err(Error("a shape is listed twice".to_string()));
    }
    let editor = GroupEditor::open(package, page_index)?;
    let mut paths: Vec<(Ring, bool)> = Vec::new();
    for id in ids {
        let node = editor.roots.iter().find(|n| n.id == *id).ok_or_else(|| Error(format!("shape {id} is not on the page itself")))?;
        paths.extend(shape_paths(node)?);
    }
    let sections = join_paths(paths);
    finish_operation(package, page_index, ids, &[(sections, true)])
}

// ---- Trim (step 287) ---------------------------------------------------------------------------------------------------------------------------------------------
// Measured on the installed Visio (probe-trim2.ps1, 1534): Trim cuts every path of the selected shapes at the points where it crosses or touches a path of ANOTHER selected shape
// (the end of a line that touches the middle of another line cuts the other line, the touching line itself is not cut). A closed outline is cut as the path it was drawn
// as, from its first point back to its first point, so the seam stays and N cuts give N + 1 pieces (a rectangle crossed twice gives 3 pieces, crossed once 2). A piece of ONE segment
// becomes a one-dimensional line, a longer piece an open 2-D path (no fill); a shape that is not cut is left as it is (Visio re-creates it unchanged). Every piece keeps the look of
// the shape it comes from. Here: straight segments only; a cut exactly at a corner of a path is not made.

/// The value of the attribute `name` of an XML tag.
fn tag_attr(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!(" {name}='"))? + name.len() + 3;
    let end = tag[at..].find('\'')?;
    Some(tag[at..at + end].to_string())
}

/// The one-dimensional line from `a` to `b` with the look (style references and line cells) of the shape `source`.
fn line_piece_xml(source: &str, id: u64, a: Pt, b: Pt) -> String {
    let mut piece = crate::draw::line_shape_xml(id, &format!("Line.{id}"), a.0, a.1, b.0, b.1, None);
    let open_end = source.find('>').unwrap_or(0) + 1;
    let (src_tag, src_rest) = (&source[..open_end], &source[open_end..]);
    let head = &src_rest[..src_rest.find("<Section ").unwrap_or(src_rest.len())];
    if let Some(close) = piece.find('>') {
        let mut tag = piece[..close + 1].to_string();
        for name in ["LineStyle", "FillStyle", "TextStyle"] {
            if let Some(value) = tag_attr(src_tag, name) {
                tag = set_attr(&tag, name, &value);
            }
        }
        piece = format!("{tag}{}", &piece[close + 1..]);
    }
    let mut carried = String::new();
    for name in ["LineColor", "LinePattern", "LineWeight", "BeginArrow", "EndArrow", "BeginArrowSize", "EndArrowSize", "LineCap"] {
        // the default cell of a drawn line goes; a cell the source does not have is not written (the piece inherits it from the style, as the source does)
        while let Some(old) = piece.find(&format!("<Cell N='{name}'")) {
            let end = piece[old..].find("/>").map(|n| old + n + 2).unwrap_or(piece.len());
            piece.replace_range(old..end, "");
        }
        if let Some(at) = head.find(&format!("<Cell N='{name}'")) {
            if let Some(len) = head[at..].find("/>") {
                carried.push_str(&head[at..at + len + 2]);
            }
        }
    }
    if let Some(at) = piece.find("<Section ") {
        piece.insert_str(at, &carried);
    }
    piece
}

/// Trim on the page-level shapes `ids` (straight segments only; at least one of them must be cut by another).
fn trim_operation(package: &Package, page_index: usize, ids: &[String]) -> Result<(Vec<u8>, Vec<String>)> {
    if ids.len() < 2 {
        return Err(Error("select at least two shapes to trim".to_string()));
    }
    let mut distinct = ids.to_vec();
    distinct.sort();
    distinct.dedup();
    if distinct.len() != ids.len() {
        return Err(Error("a shape is listed twice".to_string()));
    }
    let editor = GroupEditor::open(package, page_index)?;
    let mut shapes: Vec<Vec<(Ring, bool)>> = Vec::new();
    for id in ids {
        let node = editor.roots.iter().find(|n| n.id == *id).ok_or_else(|| Error(format!("shape {id} is not on the page itself")))?;
        shapes.push(shape_paths(node)?);
    }
    let walks: Vec<Vec<Vec<Pt>>> = shapes
        .iter()
        .map(|paths| {
            paths
                .iter()
                .map(|(ring, closed)| {
                    let mut walk = ring.clone();
                    if *closed {
                        walk.push(ring[0]);
                    }
                    walk
                })
                .collect()
        })
        .collect();
    // per shape: None = not cut, Some(pieces) = (points, closed) of every path of the shape after the cuts
    let mut cut_shapes: Vec<Option<Vec<(Ring, bool)>>> = Vec::new();
    for (si, paths) in shapes.iter().enumerate() {
        let mut pieces: Vec<(Ring, bool)> = Vec::new();
        let mut any = false;
        for (pi, (ring, closed)) in paths.iter().enumerate() {
            let walk = &walks[si][pi];
            let mut current: Ring = vec![walk[0]];
            let mut path_pieces: Vec<(Ring, bool)> = Vec::new();
            let mut cut = false;
            for i in 0..walk.len() - 1 {
                let seg = (walk[i], walk[i + 1]);
                let mut ts: Vec<f64> = Vec::new();
                for (sj, other) in walks.iter().enumerate() {
                    if sj == si {
                        continue;
                    }
                    for ow in other {
                        for k in 0..ow.len() - 1 {
                            ts.extend(cuts(seg, (ow[k], ow[k + 1])));
                        }
                    }
                }
                ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
                ts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
                for t in ts {
                    let p = (seg.0 .0 + (seg.1 .0 - seg.0 .0) * t, seg.0 .1 + (seg.1 .1 - seg.0 .1) * t);
                    current.push(p);
                    path_pieces.push((std::mem::take(&mut current), false));
                    current = vec![p];
                    cut = true;
                }
                current.push(seg.1);
            }
            if cut {
                path_pieces.push((current, false));
                pieces.extend(path_pieces);
                any = true;
            } else {
                pieces.push((ring.clone(), *closed));
            }
        }
        cut_shapes.push(if any { Some(pieces) } else { None });
    }
    let affected: Vec<usize> = (0..shapes.len()).filter(|i| cut_shapes[*i].is_some()).collect();
    if affected.is_empty() {
        return Err(Error("the selected shapes do not cross or touch one another: nothing to trim".to_string()));
    }
    let gone: Vec<String> = affected.iter().map(|i| ids[*i].clone()).collect();
    replace_shapes(package, page_index, &gone, &|xml, first_id| {
        let mut out = Vec::new();
        let mut id = first_id;
        for si in &affected {
            let (start, end) = find_shape_range(xml, &ids[*si]).ok_or_else(|| Error(format!("shape {} not found", ids[*si])))?;
            let source = xml[start..end].to_string();
            for (points, closed) in cut_shapes[*si].as_ref().unwrap() {
                if !*closed && points.len() == 2 {
                    out.push(line_piece_xml(&source, id, points[0], points[1]));
                } else {
                    out.push(result_xml_paths(&source, id, &[vec![(points.clone(), *closed)]], false)?);
                }
                id += 1;
            }
        }
        Ok(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two open lines whose ends are `gap` inches apart, as `shape_paths` would deliver them.
    fn two_lines(gap: f64) -> Vec<(Ring, bool)> {
        vec![(vec![(1.0, 1.0), (3.0, 2.0)], false), (vec![(3.0 + gap, 2.0), (5.0, 1.2)], false)]
    }

    fn path_count(sections: &[Vec<(Ring, bool)>]) -> usize {
        sections.iter().map(|section| section.len()).sum()
    }

    // probe-join-gap.ps1 (Visio 16.0.14332): ends 1e-6, 1e-4 and 2e-4 in apart are joined into one path, 5e-4 and 1e-3 in apart are not.
    #[test]
    fn ends_a_hair_apart_are_joined_and_ends_further_apart_are_not() {
        assert_eq!(path_count(&join_paths(two_lines(1e-6))), 1);
        assert_eq!(path_count(&join_paths(two_lines(1.5e-4))), 1);
        assert_eq!(path_count(&join_paths(two_lines(5e-4))), 2);
        assert_eq!(path_count(&join_paths(two_lines(1e-3))), 2);
    }

    #[test]
    fn the_direction_of_the_second_line_does_not_matter() {
        let reversed = vec![(vec![(1.0, 1.0), (3.0, 2.0)], false), (vec![(5.0, 1.2), (3.0, 2.0)], false)];
        assert_eq!(path_count(&join_paths(reversed)), 1);
    }
}
