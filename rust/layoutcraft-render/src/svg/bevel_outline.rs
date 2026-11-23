//! Bevels on outlines that are not plain rectangles (ellipse, rounded rectangle, triangle, diamond, freeform), probed against Visio in
//! visio-test-harness/probe-bevel-shapes.ps1: the same measured shading as `bevel_profile_overlay`, laid along the outline in mitred bands.
use super::*;

/// The closed outline of a path made of M, L, H, V, C, Q, A and Z (absolute commands, as the importer writes them) as a polyline in
/// the shape's own box; curves are cut into short pieces. `None` when the path has several subpaths or a command not handled.
pub(super) fn flatten_closed_path(d: &str) -> Option<Vec<(f64, f64)>> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    for c in d.chars() {
        if c.is_ascii_alphabetic() && c != 'e' {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            tokens.push(c.to_string());
        } else if c == ',' || c == ' ' {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else if c == '-' && !current.is_empty() && !current.ends_with('e') {
            tokens.push(std::mem::take(&mut current));
            current.push(c);
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut at = (0.0, 0.0);
    let mut i = 0;
    let mut cmd = ' ';
    let mut moves = 0;
    let num_at = |tokens: &[String], i: usize| tokens.get(i).and_then(|t| t.parse::<f64>().ok());
    while i < tokens.len() {
        if tokens[i].chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            cmd = tokens[i].chars().next().unwrap();
            i += 1;
            if cmd == 'Z' || cmd == 'z' {
                continue;
            }
        }
        match cmd {
            'M' => {
                moves += 1;
                if moves > 1 {
                    return None;
                }
                at = (num_at(&tokens, i)?, num_at(&tokens, i + 1)?);
                points.push(at);
                i += 2;
                cmd = 'L';
            }
            'L' => {
                at = (num_at(&tokens, i)?, num_at(&tokens, i + 1)?);
                points.push(at);
                i += 2;
            }
            'H' => {
                at.0 = num_at(&tokens, i)?;
                points.push(at);
                i += 1;
            }
            'V' => {
                at.1 = num_at(&tokens, i)?;
                points.push(at);
                i += 1;
            }
            'C' => {
                let c1 = (num_at(&tokens, i)?, num_at(&tokens, i + 1)?);
                let c2 = (num_at(&tokens, i + 2)?, num_at(&tokens, i + 3)?);
                let end = (num_at(&tokens, i + 4)?, num_at(&tokens, i + 5)?);
                for k in 1..=12 {
                    let t = k as f64 / 12.0;
                    let u = 1.0 - t;
                    points.push((
                        u * u * u * at.0 + 3.0 * u * u * t * c1.0 + 3.0 * u * t * t * c2.0 + t * t * t * end.0,
                        u * u * u * at.1 + 3.0 * u * u * t * c1.1 + 3.0 * u * t * t * c2.1 + t * t * t * end.1,
                    ));
                }
                at = end;
                i += 6;
            }
            'Q' => {
                let c1 = (num_at(&tokens, i)?, num_at(&tokens, i + 1)?);
                let end = (num_at(&tokens, i + 2)?, num_at(&tokens, i + 3)?);
                for k in 1..=8 {
                    let t = k as f64 / 8.0;
                    let u = 1.0 - t;
                    points.push((u * u * at.0 + 2.0 * u * t * c1.0 + t * t * end.0, u * u * at.1 + 2.0 * u * t * c1.1 + t * t * end.1));
                }
                at = end;
                i += 4;
            }
            'A' => {
                let (mut rx, mut ry) = (num_at(&tokens, i)?.abs(), num_at(&tokens, i + 1)?.abs());
                let phi = num_at(&tokens, i + 2)?.to_radians();
                let (large, sweep) = (num_at(&tokens, i + 3)? != 0.0, num_at(&tokens, i + 4)? != 0.0);
                let end = (num_at(&tokens, i + 5)?, num_at(&tokens, i + 6)?);
                i += 7;
                if rx < 1e-9 || ry < 1e-9 || (end.0 - at.0).abs() + (end.1 - at.1).abs() < 1e-9 {
                    points.push(end);
                    at = end;
                    continue;
                }
                // endpoint to centre parametrisation (SVG implementation notes F.6.5)
                let (cp, sp) = (phi.cos(), phi.sin());
                let (dx, dy) = ((at.0 - end.0) / 2.0, (at.1 - end.1) / 2.0);
                let (x1, y1) = (cp * dx + sp * dy, -sp * dx + cp * dy);
                let lambda = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
                if lambda > 1.0 {
                    rx *= lambda.sqrt();
                    ry *= lambda.sqrt();
                }
                let numer = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
                let denom = rx * rx * y1 * y1 + ry * ry * x1 * x1;
                let mut coef = (numer / denom).max(0.0).sqrt();
                if large == sweep {
                    coef = -coef;
                }
                let (cx1, cy1) = (coef * rx * y1 / ry, -coef * ry * x1 / rx);
                let (cx, cy) = (cp * cx1 - sp * cy1 + (at.0 + end.0) / 2.0, sp * cx1 + cp * cy1 + (at.1 + end.1) / 2.0);
                let angle = |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
                let theta1 = angle(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
                let mut delta = angle((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
                if !sweep && delta > 0.0 {
                    delta -= std::f64::consts::TAU;
                } else if sweep && delta < 0.0 {
                    delta += std::f64::consts::TAU;
                }
                let steps = ((delta.abs() / std::f64::consts::TAU * 64.0).ceil() as usize).max(4);
                for k in 1..=steps {
                    let t = theta1 + delta * k as f64 / steps as f64;
                    points.push((cx + rx * t.cos() * cp - ry * t.sin() * sp, cy + rx * t.cos() * sp + ry * t.sin() * cp));
                }
                at = end;
            }
            _ => return None,
        }
    }
    // a closing point equal to the first one is dropped
    if points.len() > 1 {
        let (f, l) = (points[0], points[points.len() - 1]);
        if (f.0 - l.0).abs() < 1e-6 && (f.1 - l.1).abs() < 1e-6 {
            points.pop();
        }
    }
    points.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6);
    (points.len() >= 3).then_some(points)
}

/// A bevel on any single closed outline: the measured shading of `bevel_profile_overlay` along the outline in mitred bands. The side of
/// a band is the direction of its outward normal (light from the top left): the gain of every sample is the mix of the four sides'
/// gains by how much the normal points at them. `None` for several outlines, outlines with more than 260 points and shapes under 6 px.
pub(super) fn bevel_outline_overlay(shape: &SceneShape, bevel: &SceneBevel, index: usize) -> Option<String> {
    use crate::bevel_table::{BEVEL_GAIN, BEVEL_INTERIOR_GAIN, BEVEL_SAMPLES};
    if !(1..=12).contains(&bevel.kind) || shape.w < 6.0 || shape.h < 6.0 {
        return None;
    }
    let (w, h) = (shape.w, shape.h);
    // the outline: the one part's path, or the box (rounded when the shape has a corner radius)
    let (outline_d, mut ring) = match shape.parts.as_slice() {
        [part] if shape.rounding <= 0.1 || !path_is_box(&part.d, w, h) => (part.d.clone(), flatten_closed_path(&part.d)?),
        [] | [_] => {
            let r = shape.rounding.max(0.0).min(w / 2.0).min(h / 2.0);
            let d = format!(
                "M{} 0H{}A{r} {r} 0 0 1 {w} {r}V{}A{r} {r} 0 0 1 {} {h}H{r}A{r} {r} 0 0 1 0 {}V{r}A{r} {r} 0 0 1 {r} 0Z",
                num(r),
                num(w - r),
                num(h - r),
                num(w - r),
                num(h - r)
            );
            let points = flatten_closed_path(&d)?;
            (d, points)
        }
        _ => return None,
    };
    if ring.len() > 260 {
        return None;
    }
    // clockwise on screen (y down) = positive area; the inward normal is then to the right of the direction of travel
    let area: f64 = (0..ring.len()).map(|k| ring[k].0 * ring[(k + 1) % ring.len()].1 - ring[(k + 1) % ring.len()].0 * ring[k].1).sum::<f64>() / 2.0;
    if area < 0.0 {
        ring.reverse();
    }
    let n = ring.len();
    let width = bevel.width.clamp(2.0, w.min(h) / 2.0);
    let fill = shape.parts.first().map_or(shape.fill.as_str(), |part| part.fill.as_str());
    let rgb = (fill.len() == 7 && fill.starts_with('#'))
        .then(|| (u8::from_str_radix(&fill[1..3], 16), u8::from_str_radix(&fill[3..5], 16), u8::from_str_radix(&fill[5..7], 16)))
        .and_then(|(r, g, b)| Some((r.ok()?, g.ok()?, b.ok()?)));
    let table = &BEVEL_GAIN[bevel.kind as usize - 1];
    let strength = (bevel.height / bevel.width.max(0.1)).clamp(0.05, 1.0).powf(BEVEL_RATIO_POWER);
    let paint = |gain: f64| -> (String, f64) {
        let gain = (100.0 + (gain - 100.0) * strength).round().max(0.0);
        match rgb {
            Some((r, g, b)) => {
                let scale = |c: u8| ((c as f64 * gain / 100.0).round().clamp(0.0, 255.0)) as u8;
                (format!("#{:02x}{:02x}{:02x}", scale(r), scale(g), scale(b)), 1.0)
            }
            None if gain >= 100.0 => ("#ffffff".to_string(), ((gain - 100.0) / 100.0 * 0.55).min(0.9)),
            None => ("#000000".to_string(), (100.0 - gain) / 100.0),
        }
    };
    // unit direction along every edge and its inward normal (right of the direction of travel with y down)
    let dir: Vec<(f64, f64)> = (0..n)
        .map(|k| {
            let (a, b) = (ring[k], ring[(k + 1) % n]);
            let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt().max(1e-9);
            ((b.0 - a.0) / len, (b.1 - a.1) / len)
        })
        .collect();
    let inward = |k: usize| (-dir[k].1, dir[k].0);
    // the inner corner of every vertex: the mitre of the bands of its two edges
    let inner: Vec<(f64, f64)> = (0..n)
        .map(|k| {
            let (np, nn) = (inward((k + n - 1) % n), inward(k));
            let dot = np.0 * nn.0 + np.1 * nn.1;
            let scale = width / (1.0 + dot).max(0.12);
            (ring[k].0 + (np.0 + nn.0) * scale, ring[k].1 + (np.1 + nn.1) * scale)
        })
        .collect();
    let mut defs = String::new();
    let mut body = String::new();
    // the interior, lit by the interior gain, inside the inner corners
    let (interior, interior_opacity) = paint(BEVEL_INTERIOR_GAIN as f64);
    let inner_points: Vec<String> = inner.iter().map(|p| format!("{},{}", num(p.0), num(p.1))).collect();
    body.push_str(&format!(r#"<polygon points="{}" fill="{interior}" fill-opacity="{}"/>"#, inner_points.join(" "), num(interior_opacity)));
    for k in 0..n {
        let (a, b) = (ring[k], ring[(k + 1) % n]);
        let (ia, ib) = (inner[k], inner[(k + 1) % n]);
        let nin = inward(k);
        // sides: top, left, bottom, right; the weight of a side is how far the outward normal (-nin) points at it
        let outward = (-nin.0, -nin.1);
        let weights = [(-outward.1).max(0.0), (-outward.0).max(0.0), outward.1.max(0.0), outward.0.max(0.0)];
        let total: f64 = weights.iter().sum::<f64>().max(1e-9);
        let mix = |sample: usize| -> f64 { (0..4).map(|side| weights[side] * table[side][sample] as f64).sum::<f64>() / total };
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let mut stops = String::new();
        let mut push = |offset: f64, gain: f64| {
            let (colour, opacity) = paint(gain);
            stops.push_str(&format!(r#"<stop offset="{}" stop-color="{colour}" stop-opacity="{}"/>"#, num(offset), num(opacity)));
        };
        push(0.0, mix(0));
        for sample in 0..BEVEL_SAMPLES {
            push((6 + 2 * sample) as f64 / 24.0, mix(sample));
        }
        defs.push_str(&format!(
            r#"<linearGradient id="bevelO{index}e{k}" gradientUnits="userSpaceOnUse" x1="{}" y1="{}" x2="{}" y2="{}">{stops}</linearGradient>"#,
            num(mid.0),
            num(mid.1),
            num(mid.0 + nin.0 * width),
            num(mid.1 + nin.1 * width)
        ));
        body.push_str(&format!(
            r#"<polygon points="{},{} {},{} {},{} {},{}" fill="url(#bevelO{index}e{k})"/>"#,
            num(a.0),
            num(a.1),
            num(b.0),
            num(b.1),
            num(ib.0),
            num(ib.1),
            num(ia.0),
            num(ia.1)
        ));
    }
    let outline = match shape.parts.as_slice() {
        [part] if part.stroke != "none" && part.stroke_width > 0.0 => {
            format!(r#"<path d="{}" fill="none" stroke="{}" stroke-width="{}"/>"#, escape(&outline_d), escape(&part.stroke), num(part.stroke_width))
        }
        _ => String::new(),
    };
    Some(format!(
        r#"<defs>{defs}<clipPath id="bevelOc{index}"><path d="{}"/></clipPath></defs><g clip-path="url(#bevelOc{index})">{body}</g>{outline}"#,
        escape(&outline_d)
    ))
}
