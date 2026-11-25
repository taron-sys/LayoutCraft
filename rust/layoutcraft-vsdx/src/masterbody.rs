//! Resolve a master body assembled from child shapes into inline parts.
//!
//! Evidence: nested master shapes and their cell formulas are read from stencil and template files; placement and scaling rules were measured on the installed Visio
//! (probe-pdu-drop.ps1, probe-tall-1d-turn.ps1, probe-flat-1d-turn.ps1: dropping 1:10 masters on 1:1 pages, 1-D group masters) and are pinned by `tests/master_pictures.rs`
//! and `tests/group_edit.rs` (private corpus).
//!
//! Master-derived groups (Visio's table/list rows, UML attributes, 3-D blocks)
//! keep their real body in nested master shapes. This module evaluates the
//! cached ShapeSheet formulas against the page instance and maps each child's
//! first Geometry section into the instance's local box.
use std::collections::BTreeMap;

use layoutcraft_model::{ShapePart, SubText};

use crate::formula;
use crate::geometry;
use crate::shapes::{Cell, MasterShape, ResolvedShape, Section};
use crate::styles::{is_bare_themed, is_concrete, tint, DocumentStyles, Theme};

const DPI: f64 = 96.0;
const MIN_BODY_STROKE: f64 = 0.9;

struct NestedCells {
    parent: BTreeMap<String, f64>,
    children: BTreeMap<String, BTreeMap<String, f64>>,
    top_id: Option<String>,
}

struct BodyPaint {
    fill: String,
    stroke: String,
    stroke_width: f64,
    dash: Option<String>,
    line_visible: bool,
    fill_opacity: f64,
    stroke_opacity: f64,
    fill_gradient: Option<serde_json::Value>,
    fill_pattern: Option<layoutcraft_model::FillPattern>,
}

#[derive(Clone, Copy)]
struct Affine {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Affine {
    fn local(
        pin_x: f64,
        pin_y: f64,
        loc_x: f64,
        loc_y: f64,
        angle: f64,
        flip_x: bool,
        flip_y: bool,
    ) -> Self {
        let (sin, cos) = angle.sin_cos();
        let fx = if flip_x { -1.0 } else { 1.0 };
        let fy = if flip_y { -1.0 } else { 1.0 };
        let a = cos * fx;
        let b = sin * fx;
        let c = -sin * fy;
        let d = cos * fy;
        Self {
            a,
            b,
            c,
            d,
            e: pin_x - a * loc_x - c * loc_y,
            f: pin_y - b * loc_x - d * loc_y,
        }
    }

    fn after(self, parent: Self) -> Self {
        Self {
            a: parent.a * self.a + parent.c * self.b,
            b: parent.b * self.a + parent.d * self.b,
            c: parent.a * self.c + parent.c * self.d,
            d: parent.b * self.c + parent.d * self.d,
            e: parent.a * self.e + parent.c * self.f + parent.e,
            f: parent.b * self.e + parent.d * self.f + parent.f,
        }
    }

    fn apply(self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }
}

fn page_child_transform(page: &ResolvedShape) -> Option<Affine> {
    if std::env::var("LAYOUTCRAFT_DEBUG_FLOOR").is_ok() && page.id == "3" {
        eprintln!(
            "      transform cells pin {:?} {:?} loc {:?} {:?}",
            number_option(&page.cells, "PinX"),
            number_option(&page.cells, "PinY"),
            number_option(&page.cells, "LocPinX"),
            number_option(&page.cells, "LocPinY")
        );
    }
    Some(Affine::local(
        number_option(&page.cells, "PinX")?,
        number_option(&page.cells, "PinY")?,
        number_option(&page.cells, "LocPinX")?,
        number_option(&page.cells, "LocPinY")?,
        number_option(&page.cells, "Angle").unwrap_or(0.0),
        number_option(&page.cells, "FlipX").unwrap_or(0.0) != 0.0,
        number_option(&page.cells, "FlipY").unwrap_or(0.0) != 0.0,
    ))
}

pub fn body_parts(
    node: &ResolvedShape,
    instance_w: f64,
    instance_h: f64,
    theme: &Theme,
    styles: &DocumentStyles,
) -> (Vec<ShapePart>, Vec<SubText>) {
    if node.master_children.is_empty() {
        return (Vec::new(), Vec::new());
    }
    // Children without a page child of their own are authored in the master's space and scaled into the instance below
    // (`master_map`): their formulas (`Sheet.5!Width*0.9`) must see the MASTER's size, or a resized instance draws them at the
    // square of the scale (APC rack stencils, report 18). Children with a page child take the instance's size as before.
    let nested_instance = resolve_cells(node, false);
    let nested_master = resolve_cells(node, true);
    let bevel = ["BevelTopType", "BevelBottomType", "BevelDepthSize"]
        .iter()
        .any(|name| {
            node.cells
                .get(*name)
                .and_then(|cell| cell.value.as_deref())
                .and_then(|value| value.parse::<f64>().ok())
                .is_some_and(|value| value.abs() > 1e-9)
        });
    let large = instance_w * DPI * instance_h * DPI > 60000.0;
    let mut parts = Vec::new();
    let mut texts = Vec::new();
    for child in &node.master_children {
        let parts_before = parts.len();
        // Visio leaves shapes with NonPrinting set out of its image export (the CHART bar graph's
        // dotted unit-bar guide).
        if number_option(&child.cells, "NonPrinting").is_some_and(|value| value != 0.0) {
            continue;
        }
        // A ruler guide in the master (an InfiniteLine, e.g. EXCOBJ Message delivery) is page
        // furniture, never drawn.
        if child.shape_type.as_deref() == Some("Guide") {
            continue;
        }
        let page_child = page_child_for(node, &child.id);
        let nested = if page_child.is_none() { &nested_master } else { &nested_instance };
        // A page child supplies the instance's box and text cells even when
        // its Geometry is inherited from the master (e.g. a resized stencil
        // icon). Keep the master geometry in that case but position it with
        // the page cells.
        let use_page = !bevel && page_child.is_some();
        let page_values = if use_page { page_child } else { None };
        let values = nested.children.get(&child.id);
        let sub_w = page_values
            .and_then(|child| number_option(&child.cells, "Width"))
            .filter(|width| *width > 1e-9)
            .or_else(|| values.and_then(|values| values.get("Width").copied()))
            .unwrap_or_else(|| number(&child.cells, "Width", 1.0))
            .max(1e-9);
        let sub_h = page_values
            .and_then(|child| number_option(&child.cells, "Height"))
            .filter(|height| *height > 1e-9)
            .or_else(|| values.and_then(|values| values.get("Height").copied()))
            .unwrap_or_else(|| number(&child.cells, "Height", 1.0))
            .max(1e-9);
        let pin_x = page_values
            .and_then(|child| number_option(&child.cells, "PinX"))
            .or_else(|| values.and_then(|values| values.get("PinX").copied()))
            .unwrap_or(sub_w / 2.0);
        let pin_y = page_values
            .and_then(|child| number_option(&child.cells, "PinY"))
            .or_else(|| values.and_then(|values| values.get("PinY").copied()))
            .unwrap_or(sub_h / 2.0);
        let loc_x = page_values
            .and_then(|child| number_option(&child.cells, "LocPinX"))
            .or_else(|| values.and_then(|values| values.get("LocPinX").copied()))
            .unwrap_or(sub_w / 2.0);
        let loc_y = page_values
            .and_then(|child| number_option(&child.cells, "LocPinY"))
            .or_else(|| values.and_then(|values| values.get("LocPinY").copied()))
            .unwrap_or(sub_h / 2.0);
        let ox = pin_x - loc_x;
        let oy = pin_y - loc_y;
        let project = move |y: f64| if bevel && y < 0.0 { y * 0.5 } else { y };
        // Page children use page/local coordinates (origin bottom-left), while
        // master children are authored top-down. Flip the page-child Y axis
        // into the master body's local top-down space.
        let parent_h = number(&node.cells, "Height", instance_h).max(1e-9);
        // Nested page children position themselves in their parent's local
        // box, so compose the offsets up the master-child tree. All levels use
        // inches in the same page coordinate space (no scaling).
        // Nested page children position themselves in their parent's local
        // box; compose pin/loc-pin, rotation and flip up the tree. Rotated wall
        // groups in floor plans depend on this, not just translated offsets.
        if std::env::var("LAYOUTCRAFT_DEBUG_FLOOR").is_ok() && node.id == "1" {
            eprintln!(
                "child {} page {:?} geoms {}",
                child.id,
                page_child.map(|p| p.id.as_str()),
                page_child
                    .map(|p| p.sections.iter().filter(|s| s.name == "Geometry").count())
                    .unwrap_or(0)
            );
        }
        let chain_transform = page_values.and_then(|page| {
            let initial = page_child_transform(page)?;
            if std::env::var("LAYOUTCRAFT_DEBUG_FLOOR").is_ok() && child.id == "7" {
                let master = node
                    .master_children
                    .iter()
                    .find(|candidate| candidate.id == child.id);
                eprintln!(
                    "   initial ({},{}) master parent {:?}",
                    initial.e,
                    initial.f,
                    master.and_then(|m| m.parent.clone())
                );
            }
            let mut total = initial;
            let mut id = node
                .master_children
                .iter()
                .find(|candidate| candidate.id == child.id)?
                .parent
                .clone();
            let mut guard = 0;
            while let Some(current) = id {
                let Some(master_child) = node
                    .master_children
                    .iter()
                    .find(|candidate| candidate.id == current)
                else {
                    break;
                };
                let parent_page = page_child_for(node, &current);
                if std::env::var("LAYOUTCRAFT_DEBUG_FLOOR").is_ok() && child.id == "7" {
                    eprintln!(
                        "   parent {} page {:?} transform {:?}",
                        current,
                        parent_page.map(|p| p.id.as_str()),
                        parent_page
                            .and_then(page_child_transform)
                            .map(|t| (t.e, t.f))
                    );
                }
                let parent_page = parent_page?;
                total = total.after(page_child_transform(parent_page)?);
                id = master_child.parent.clone();
                guard += 1;
                if guard > 32 {
                    break;
                }
            }
            Some(total)
        });
        if std::env::var("LAYOUTCRAFT_DEBUG_FLOOR").is_ok() && node.id == "1" {
            eprintln!(
                "  child {} chain {:?}",
                child.id,
                chain_transform.map(|t| (t.a, t.b, t.c, t.d, t.e, t.f))
            );
        }
        // Some Visio add-ins (ImageNode network stencils) author child
        // geometry around the box centre instead of its lower-left corner.
        let center_based = node
            .master_name
            .as_deref()
            .is_some_and(|name| name.starts_with("ImageNode"));
        let shift_x = if center_based { sub_w / 2.0 } else { 0.0 };
        let shift_y = if center_based { -sub_h / 2.0 } else { 0.0 };
        let page_map = {
            let transform = chain_transform;
            move |x: f64, y: f64| {
                let (px, py) = match transform {
                    Some(transform) => transform.apply(x, y),
                    None => (ox + x, oy + y),
                };
                ((px + shift_x) * DPI, ((parent_h - py) + shift_y) * DPI)
            }
        };
        let page_rel = {
            let transform = chain_transform;
            move |x: f64, y: f64| {
                let (px, py) = match transform {
                    Some(transform) => transform.apply(x * sub_w, y * sub_h),
                    None => (ox + x * sub_w, oy + y * sub_h),
                };
                ((px + shift_x) * DPI, ((parent_h - py) + shift_y) * DPI)
            }
        };
        // Master-only children have no page instance. Their geometry is
        // authored bottom-up inside the child box; mirror it inside that box
        // without moving the box itself.
        // Master-only children are authored in the master shape's local
        // space. Scale that space into the instance, then convert bottom-up
        // y to the page's top-down axis.
        let master_w = node.master_width.unwrap_or(instance_w).max(1e-9);
        let master_h = node.master_height.unwrap_or(instance_h).max(1e-9);
        let scale_x = instance_w / master_w;
        let scale_y = instance_h / master_h;
        // A master-only child (or one of its ancestors below the top shape) can be rotated by its
        // own `Angle`: CALOUT's annotation text is turned -90 inside a group turned 90, and the
        // VENNDIAG captions likewise. Compose pin, loc-pin, angle and flip up the master tree;
        // only when something in the chain is turned, so every other child keeps the plain offset.
        let master_chain: Option<Affine> = if chain_transform.is_none() {
            let mut total: Option<Affine> = None;
            let mut turned = false;
            let mut id = Some(child.id.clone());
            let mut ok = true;
            let mut guard = 0;
            while let Some(current) = id {
                let (Some(master_child), Some(v)) = (
                    node.master_children.iter().find(|candidate| candidate.id == current),
                    nested.children.get(&current),
                ) else {
                    ok = false;
                    break;
                };
                let (Some(pin_x), Some(pin_y), Some(loc_x), Some(loc_y)) = (
                    v.get("PinX").copied(),
                    v.get("PinY").copied(),
                    v.get("LocPinX").copied(),
                    v.get("LocPinY").copied(),
                ) else {
                    ok = false;
                    break;
                };
                let angle = v.get("Angle").copied().unwrap_or(0.0);
                turned |= angle.abs() > 1e-9;
                let local = Affine::local(
                    pin_x,
                    pin_y,
                    loc_x,
                    loc_y,
                    angle,
                    v.get("FlipX").copied().unwrap_or(0.0) != 0.0,
                    v.get("FlipY").copied().unwrap_or(0.0) != 0.0,
                );
                total = Some(match total {
                    None => local,
                    Some(inner) => inner.after(local),
                });
                id = master_child
                    .parent
                    .clone()
                    .filter(|parent| node.master_children.iter().any(|candidate| &candidate.id == parent));
                guard += 1;
                if guard > 32 {
                    ok = false;
                    break;
                }
            }
            if ok && turned { total } else { None }
        } else {
            None
        };
        let master_map = move |x: f64, y: f64| {
            let (px, py) = match master_chain {
                Some(chain) => chain.apply(x, y),
                None => (ox + x, oy + project(y)),
            };
            (
                px * scale_x * DPI + shift_x * DPI,
                (master_h - py) * scale_y * DPI + shift_y * DPI,
            )
        };
        let master_rel = move |x: f64, y: f64| {
            let (px, py) = match master_chain {
                Some(chain) => chain.apply(x * sub_w, y * sub_h),
                None => (ox + x * sub_w, oy + project(y * sub_h)),
            };
            (
                px * scale_x * DPI + shift_x * DPI,
                (master_h - py) * scale_y * DPI + shift_y * DPI,
            )
        };
        // Paint cells live on the master child; the page child only overrides
        // the ones it actually authors. Merge both so QuickStyle* cells are
        // not lost (UML state borders take their colour from the master's
        // QuickStyleLineColor).
        let mut paint_cells = child.cells.clone();
        if let Some(page) = page_child {
            for (name, cell) in &page.cells {
                paint_cells.insert(name.clone(), cell.clone());
            }
        }
        let child_fill_style = page_child
            .and_then(|page| page.fill_style.as_deref())
            .or(child.fill_style.as_deref());
        let child_line_style = page_child
            .and_then(|page| page.line_style.as_deref())
            .or(child.line_style.as_deref());
        let paint = child_paint(
            node,
            &paint_cells,
            child_fill_style,
            child_line_style,
            theme,
            styles,
            large,
        );
        let gradient_cells = page_child.map_or(&child.cells, |child| &child.cells);
        let gradient_sections = page_child
            .filter(|page| {
                page.sections
                    .iter()
                    .any(|section| section.name == "FillGradient")
            })
            .map(|page| page.sections.as_slice())
            .unwrap_or(child.sections.as_slice());
        let gradient = crate::import::fill_gradient_value(
            gradient_sections,
            gradient_cells
                .get("FillGradientAngle")
                .and_then(|cell| numeric(Some(cell))),
            gradient_cells
                .get("FillGradientDir")
                .and_then(|cell| numeric(Some(cell))),
            gradient_cells
                .get("FillGradientEnabled")
                .and_then(|cell| numeric(Some(cell)))
                // Not set on the shape: the style says THEMEVAL(), so the theme's fill matrix
                // decides (no gradient without one: SDCONT Waves in a theme-less document).
                .unwrap_or(if paint.fill_gradient.is_none() { 0.0 } else { 1.0 })
                != 0.0,
            theme,
            &styles.fill_gradient_sections(child_fill_style),
        )
        .or_else(|| paint.fill_gradient.clone());
        // A corner-anchored rectangular gradient turns with its shape (MRKTDG matrix faces are
        // -90 degree shapes: the highlight of a top-left gradient ends up top-right).
        let gradient = gradient.map(|mut value| {
            let dir = value.get("dir").and_then(serde_json::Value::as_u64).unwrap_or(0);
            let angle = number_option(&child.cells, "Angle").unwrap_or(0.0);
            let quarter_turns_cw = (-(angle / std::f64::consts::FRAC_PI_2).round() as i64).rem_euclid(4) as usize;
            if quarter_turns_cw != 0 {
                const CYCLE: [u64; 4] = [12, 11, 8, 9]; // top-left, top-right, bottom-right, bottom-left
                if let Some(at) = CYCLE.iter().position(|candidate| *candidate == dir) {
                    if let Some(object) = value.as_object_mut() {
                        object.insert("dir".to_string(), serde_json::json!(CYCLE[(at + quarter_turns_cw) % 4]));
                    }
                }
            }
            value
        });
        let line_gradient_sections = page_child
            .filter(|page| {
                page.sections
                    .iter()
                    .any(|section| section.name == "LineGradient")
            })
            .map(|page| page.sections.as_slice())
            .unwrap_or(child.sections.as_slice());
        let stroke_gradient = crate::import::gradient_value(
            line_gradient_sections,
            "LineGradient",
            gradient_cells
                .get("LineGradientAngle")
                .and_then(|cell| numeric(Some(cell))),
            gradient_cells
                .get("LineGradientDir")
                .and_then(|cell| numeric(Some(cell))),
            gradient_cells
                .get("LineGradientEnabled")
                .and_then(|cell| numeric(Some(cell)))
                .unwrap_or(0.0)
                != 0.0,
            theme,
            &styles.line_gradient_sections(child_line_style),
        );
        // Bevel masters keep their authored body geometry so the projected
        // rim/front face stay intact. Other direct page children carry the
        // instance's concrete stencil details and overrides; nested bodies
        // fall back to the master descendants and their evaluated positions.
        let page_geometry = page_values.filter(|page| {
            page.sections
                .iter()
                .any(|section| section.name == "Geometry")
        });
        let (geometry_sections, scope_id, geometry_from_page) = if let Some(page) = page_geometry {
            (
                page.sections
                    .iter()
                    .filter(|section| section.name == "Geometry")
                    .collect::<Vec<_>>(),
                // Formula references resolve against the master shape's own
                // cells (the nested map is keyed by master shape ID); the page
                // child's ID is a different ID space and can collide with an
                // unrelated master shape.
                child.id.as_str(),
                true,
            )
        } else {
            (
                child
                    .sections
                    .iter()
                    .filter(|section| section.name == "Geometry")
                    .collect::<Vec<_>>(),
                child.id.as_str(),
                false,
            )
        };
        // A body shape can carry several Geometry sections: stencil details
        // (monitor screen, keyboard, arrow line) use separate sections, while
        // other masters use them as alternatives selected by NoShow. Render
        // every section whose evaluated NoShow is clear.
        let mut fill_path = String::new();
        let mut stroke_path = String::new();
        let mut tint_fill_path = String::new();
        let mut tint_nofill_path = String::new();
        for section in geometry_sections {
            let section = resolved_section(section, scope_id, &nested);
            if section_hidden(&section, nested.children.get(&child.id), &nested.parent, nested.top_id.as_deref()) {
                continue;
            }
            let no_fill = paint.fill == "none" || section_value(&section, "NoFill") != 0.0;
            let no_line = section_value(&section, "NoLine") != 0.0;
            let close = !no_fill;
            let path = if geometry_from_page {
                geometry::path_from_section_with(
                    &section,
                    close,
                    page_map,
                    page_rel,
                    (sub_w, sub_h),
                )
            } else {
                geometry::path_from_section_with(
                    &section,
                    close,
                    master_map,
                    master_rel,
                    (sub_w, sub_h),
                )
            };
            if path.is_empty() {
                continue;
            }
            if !no_fill {
                fill_path.push_str(&path);
            }
            // All visible Geometry sections of a child form one compound
            // path. Nested outlines have opposite winding, so merging them
            // makes Visio-style holes instead of solid discs.
            if bevel {
                if no_fill {
                    tint_nofill_path.push_str(&path);
                } else {
                    tint_fill_path.push_str(&path);
                }
            } else if paint.line_visible && !no_line {
                stroke_path.push_str(&path);
            }
        }
        // A legacy shadow (`ShdwPattern` set, no `ShapeShdwType`) is a flat copy of the child's fill
        // in the shadow colour, offset by the page's `ShdwOffsetX/Y`, drawn under the child.
        if !fill_path.is_empty() {
            let pattern = paint_cells.get("ShdwPattern").and_then(|cell| numeric(Some(cell))).unwrap_or(0.0);
            let kind = paint_cells.get("ShapeShdwType").and_then(|cell| numeric(Some(cell))).unwrap_or(0.0);
            if let (true, Some((dx, dy))) = (pattern != 0.0 && kind == 0.0, theme.legacy_shadow_offset) {
                let color = paint_cells
                    .get("ShdwForegnd")
                    .and_then(|cell| theme.color_of(cell))
                    .unwrap_or_else(|| "#000000".to_string());
                let transparency = paint_cells
                    .get("ShdwForegndTrans")
                    .and_then(|cell| numeric(Some(cell)))
                    .unwrap_or(0.0);
                parts.push(ShapePart {
                    d: translate_path(&fill_path, dx * DPI, -dy * DPI),
                    fill: Some(color),
                    fill_opacity: Some((1.0 - transparency).clamp(0.0, 1.0)),
                    stroke: Some("none".to_string()),
                    stroke_opacity: None,
                    stroke_width: Some(0.0),
                    dash: None,
                    fill_gradient: None,
                    stroke_gradient: None,
                    fill_pattern: None,
                    bottom_edge: None,
                    begin_arrow: None,
                    end_arrow: None,
                    source: part_source(child, page_child, "shadow"),
                    extra: Default::default(),
                });
            }
        }
        if !fill_path.is_empty() {
            parts.push(ShapePart {
                d: fill_path,
                fill: Some(paint.fill.clone()),
                fill_opacity: Some(paint.fill_opacity),
                stroke: Some("none".to_string()),
                stroke_opacity: None,
                stroke_width: Some(0.0),
                dash: None,
                fill_gradient: gradient.clone(),
                stroke_gradient: None,
                fill_pattern: paint.fill_pattern.clone(),
                bottom_edge: None,
                begin_arrow: None,
                end_arrow: None,
                source: part_source(child, page_child, "fill"),
                extra: Default::default(),
            });
        }
        if !stroke_path.is_empty() {
            let mut stroke_part = ShapePart {
                d: stroke_path,
                fill: Some("none".to_string()),
                fill_opacity: None,
                stroke: Some(paint.stroke.clone()),
                stroke_opacity: (paint.stroke_opacity < 1.0).then_some(paint.stroke_opacity),
                stroke_width: Some(paint.stroke_width),
                dash: paint.dash.clone(),
                fill_gradient: None,
                stroke_gradient: stroke_gradient.clone(),
                fill_pattern: None,
                bottom_edge: None,
                begin_arrow: None,
                end_arrow: None,
                source: part_source(child, page_child, "stroke"),
                extra: Default::default(),
            };
            let arrow = |names: [&str; 2]| {
                names
                    .iter()
                    .find_map(|name| paint_cells.get(*name).and_then(|cell| numeric(Some(cell))))
                    .unwrap_or(0.0)
            };
            crate::import::apply_line_ends(
                &mut stroke_part,
                arrow(["LineBeginArrow", "BeginArrow"]),
                arrow(["LineEndArrow", "EndArrow"]),
                &paint_cells,
            );
            parts.push(stroke_part);
        }
        let tint_width = paint.stroke_width.max(MIN_BODY_STROKE);
        if !tint_fill_path.is_empty() {
            parts.push(ShapePart {
                d: tint_fill_path,
                fill: Some("none".to_string()),
                fill_opacity: None,
                stroke: Some(tint(&paint.fill, 80.0)),
                stroke_opacity: (paint.stroke_opacity < 1.0).then_some(paint.stroke_opacity),
                stroke_width: Some(tint_width),
                dash: paint.dash.clone(),
                fill_gradient: None,
                stroke_gradient: None,
                fill_pattern: None,
                bottom_edge: None,
                begin_arrow: None,
                end_arrow: None,
                source: part_source(child, page_child, "tint"),
                extra: Default::default(),
            });
        }
        if !tint_nofill_path.is_empty() {
            parts.push(ShapePart {
                d: tint_nofill_path,
                fill: Some("none".to_string()),
                fill_opacity: None,
                stroke: Some(paint.stroke.clone()),
                stroke_opacity: (paint.stroke_opacity < 1.0).then_some(paint.stroke_opacity),
                stroke_width: Some(tint_width),
                dash: paint.dash.clone(),
                fill_gradient: None,
                stroke_gradient: None,
                fill_pattern: None,
                bottom_edge: None,
                begin_arrow: None,
                end_arrow: None,
                source: part_source(child, page_child, "tint"),
                extra: Default::default(),
            });
        }
        // A visible master child carries its own text (UML PK/FK markers,
        // list headings). Emit it relative to the instance's local box.
        if !child_text_hidden(child, &child.id, &nested) {
            let text_source = match page_child {
                // An empty page text is intentional (master group labels are
                // placeholders); do not fall back to the master's "Generic
                // Group" text.
                Some(page) => page.text.clone().unwrap_or_default(),
                None => child.text.clone().unwrap_or_default(),
            };
            let mut text = text_source.trim().to_string();
            // The master's text is a field whose cached result belongs to the master (HVACD duct
            // size "24in. x 24in."); the instance stores its own result in its Field row.
            if !text.is_empty() && text == field_text(child) {
                if let Some(own) = page_child.and_then(resolved_field_text) {
                    text = own;
                }
            }
            if text.is_empty() || matches!(text.as_str(), "?" | "Abc" | "Label") {
                text = page_child
                    .and_then(resolved_field_text)
                    .unwrap_or_else(|| field_text(child));
            }
            if !text.is_empty() && text != "?" {
                let text_value = |name: &str| -> Option<f64> {
                    page_values
                        .and_then(|page| page.cells.get(name))
                        .and_then(|cell| numeric(Some(cell)))
                        .or_else(|| values.and_then(|values| values.get(name).copied()))
                };
                let txt_w = text_value("TxtWidth").unwrap_or(sub_w);
                let txt_h = text_value("TxtHeight").unwrap_or(sub_h * 0.5);
                let txt_pin_x = text_value("TxtPinX").unwrap_or(0.0);
                let txt_loc_x = text_value("TxtLocPinX").unwrap_or(0.0);
                let txt_pin_y = text_value("TxtPinY");
                let txt_loc_y = text_value("TxtLocPinY").unwrap_or(0.0);
                let mut x = ox + txt_pin_x - txt_loc_x;
                let mut text_rot = -text_value("TxtAngle").unwrap_or(0.0);
                // In a turned chain the block sits where the chain carries its centre, and turns
                // with it (net angle = chain angle + TxtAngle, usually 0 for such labels).
                let mut chain_centre = None;
                if let Some(chain) = chain_transform
                    .or(master_chain)
                    .filter(|chain| chain.b.atan2(chain.a).abs() > 1e-9)
                {
                    // The block turns by TxtAngle about its text pin (y up here).
                    let (sin, cos) = text_value("TxtAngle").unwrap_or(0.0).sin_cos();
                    let off = (txt_w / 2.0 - txt_loc_x, txt_h / 2.0 - txt_loc_y);
                    let (cx, cy) = chain.apply(
                        txt_pin_x + off.0 * cos - off.1 * sin,
                        txt_pin_y.unwrap_or(0.0) + off.0 * sin + off.1 * cos,
                    );
                    chain_centre = Some((cx, cy));
                    text_rot = -(chain.b.atan2(chain.a) + text_value("TxtAngle").unwrap_or(0.0));
                }
                let y = match txt_pin_y {
                    // Cell pins are bottom-up; convert the text rectangle into
                    // the instance's top-down local space.
                    Some(pin_y) => instance_h - oy - (pin_y - txt_loc_y) - txt_h,
                    None => instance_h - (oy + sub_h) + (sub_h - txt_h) / 2.0,
                };
                let mut y = y;
                if text_rot.abs() > 1e-9 {
                    // Visio turns the block about its text pin; the renderer turns it about
                    // its centre, so move the centre to where the turn puts it.
                    if let Some(pin_y) = txt_pin_y {
                        let off = (txt_w / 2.0 - txt_loc_x, -(txt_h / 2.0 - txt_loc_y));
                        let (sin, cos) = text_rot.sin_cos();
                        let pin = (ox + txt_pin_x, instance_h - oy - pin_y);
                        let centre = (
                            pin.0 + off.0 * cos - off.1 * sin,
                            pin.1 + off.0 * sin + off.1 * cos,
                        );
                        x = centre.0 - txt_w / 2.0;
                        y = centre.1 - txt_h / 2.0;
                    }
                }
                if let Some((cx, cy)) = chain_centre {
                    x = cx - txt_w / 2.0;
                    y = instance_h - cy - txt_h / 2.0;
                }
                let (font_size, font_family, bold, italic, color) =
                    sub_text_style(child, page_child, &text, &paint.fill, theme, styles);
                // The text block's own margins: left-aligned text starts after the left margin
                // (DVCALL labels, 1 pt), and the box the text fills shrinks by both.
                let margin = |name: &str| {
                    page_child
                        .and_then(|page| number_option(&page.cells, name))
                        .or_else(|| number_option(&child.cells, name))
                        .unwrap_or(0.0)
                        .max(0.0)
                };
                let (margin_left, margin_right) = (margin("LeftMargin"), margin("RightMargin"));
                let x = x + margin_left;
                let txt_w = (txt_w - margin_left - margin_right).max(0.0);
                // a saved instance says only `Inh`: the formula is the master child's
                let fit_line = page_child
                    .and_then(|page| page.cells.get("TxtWidth"))
                    .and_then(|cell| cell.formula.as_deref())
                    .filter(|formula| *formula != "Inh")
                    .or_else(|| child.cells.get("TxtWidth").and_then(|cell| cell.formula.as_deref()))
                    .is_some_and(crate::import::formula_fits_text_line);
                texts.push(SubText {
                    x: x * DPI,
                    y: y * DPI,
                    w: (txt_w * DPI).max(4.0),
                    h: (txt_h * DPI).max(4.0),
                    text: text.to_string(),
                    font_size,
                    font_family,
                    color,
                    bold,
                    italic,
                    underline: false,
                    align: sub_text_align(page_child, child),
                    v_align: "middle".to_string(),
                    indent: sub_text_indent(page_child, child, styles),
                    pad: parts.len() == parts_before,
                    text_rot,
                    fit_line,
                });
            }
        }
    }
    (parts, texts)
}

/// Character style of a master-body child, used for its own text.
fn character_row<'a>(sections: &'a [Section], ix: &str) -> Option<&'a crate::shapes::SectionRow> {
    // A shape that stores only a later row (IX 1 = the trailing empty run)
    // must not hand its size to a first run that uses a different row.
    sections
        .iter()
        .find(|section| section.name == "Character")
        .and_then(|section| {
            section
                .rows
                .iter()
                .find(|row| row.index.as_deref().unwrap_or("0") == ix)
        })
}

fn first_character_row(sections: &[Section]) -> Option<&crate::shapes::SectionRow> {
    sections
        .iter()
        .find(|section| section.name == "Character")
        .and_then(|section| section.rows.first())
}

/// `IX` of the `cp` that styles the first run of a shape's text.
fn first_run_ix(tokens: &[crate::shapes::TextToken]) -> &str {
    for token in tokens {
        match token {
            crate::shapes::TextToken::Char(ix) => return ix,
            crate::shapes::TextToken::Text(text) if !text.trim().is_empty() => break,
            _ => {}
        }
    }
    "0"
}

fn sub_text_style(
    child: &crate::shapes::MasterShape,
    page_child: Option<&ResolvedShape>,
    text: &str,
    fill: &str,
    theme: &Theme,
    styles: &DocumentStyles,
) -> (f64, String, bool, bool, String) {
    let run_ix = page_child.map_or("0", |page| first_run_ix(&page.text_tokens));
    // A stencil pinned to a built-in theme of its own does not take this document's theme colours.
    let foreign = page_child.is_some_and(crate::import::foreign_theme_index);
    let mut size = 13.0;
    let mut font ="\"Segoe UI\", Calibri, Helvetica, Arial, sans-serif".to_string();
    let mut bold = false;
    let mut italic = false;
    let mut color: Option<String> = None;
    // The page child's own Character row carries the instance overrides
    // (themed title colour, behaviour-text colour); the merged row and the
    // master child supply the defaults. The rows are kept separate here
    // because Visio can store a master row and an instance row side by side.
    let sources: Vec<&[Section]> = page_child
        .map(|page| [page.own_sections.as_slice(), page.sections.as_slice()])
        .into_iter()
        .flatten()
        .chain(std::iter::once(child.sections.as_slice()))
        .collect();
    let rows: Vec<&crate::shapes::SectionRow> =
        sources.iter().filter_map(|sections| first_character_row(sections)).collect();
    // Size must come from the row of the text's first run: a shape that only
    // stores the trailing run's row (IX 1) would otherwise override the
    // master's run-0 size.
    let size_rows: Vec<&crate::shapes::SectionRow> = sources
        .iter()
        .filter_map(|sections| character_row(sections, run_ix))
        .collect();

    let mut size_found = false;
    for row in size_rows.iter().chain(rows.iter()) {
        // `Size = Themed` (`THEMEVAL()`) is the theme's text size, 12 pt, whatever the style says.
        if row
            .cells
            .get("Size")
            .and_then(|cell| cell.value.as_deref())
            .is_some_and(|value| value.eq_ignore_ascii_case("Themed"))
        {
            size = crate::import::font_px(1.0 / 6.0);
            size_found = true;
            break;
        }
        if let Some(value) = row
            .cells
            .get("Size")
            .and_then(|cell| cell.value.as_deref())
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| *value > 0.0)
        {
            size = crate::import::font_px(value);
            size_found = true;
            break;
        }
    }
    // No Character row on the shape: the TextStyle chain sets the size (a PERT cell is 8 pt
    // because its style says so, not 13 px).
    if !size_found {
        let style_id = page_child
            .and_then(|page| page.text_style.as_deref())
            .or(child.text_style.as_deref());
        for row in styles.text_rows(style_id) {
            if let Some(value) = row
                .get("Size")
                .and_then(|cell| cell.value.as_deref())
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|value| *value > 0.0)
            {
                size = crate::import::font_px(value);
                break;
            }
        }
    }
    let mut literal_font = false;
    for row in &rows {
        if let Some(value) = row
            .cells
            .get("Font")
            .and_then(|cell| cell.value.as_deref())
            .filter(|value| !value.is_empty() && *value != "Themed")
        {
            font = value.to_string();
            literal_font = true;
            break;
        }
    }
    // No Font on the shape: a concrete face on its TextStyle chain (Org Normal: Calibri, written as
    // `THEMEVAL("LatinFont",4)` with the face cached) is the one Visio draws.
    if !literal_font && !theme.present {
        let style_id = page_child
            .and_then(|page| page.text_style.as_deref())
            .or(child.text_style.as_deref());
        for row in styles.text_rows(style_id) {
            if let Some(value) = row
                .get("Font")
                .and_then(|cell| cell.value.as_deref())
                .filter(|value| !value.is_empty() && *value != "Themed")
            {
                font = value.to_string();
                literal_font = true;
                break;
            }
        }
    }
    if !literal_font && !theme.present {
        if let Some(root) = styles.root_font() {
            font = root;
        }
    } else if !literal_font {
        if let Some(face) = theme.body_font_family() {
            font = face;
        }
    }
    for row in &rows {
        if let Some(style) = row
            .cells
            .get("Style")
            .and_then(|cell| cell.value.as_deref())
            .and_then(|value| value.parse::<f64>().ok())
        {
            bold = style as u32 & 1 != 0;
            italic = style as u32 & 2 != 0;
            break;
        }
    }
    for row in &rows {
        let Some(cell) = row.cells.get("Color") else {
            continue;
        };
        let value = cell.value.as_deref().unwrap_or("");
        if value.starts_with('#') {
            color = Some(value.to_string());
            break;
        }
        if value.eq_ignore_ascii_case("Themed") {
            // A themed title on a filled header resolves to the theme's light
            // text slot (Visio's UML state labels are white on green).
            color = if foreign { Some("#000000".to_string()) } else { theme.themed_text(fill != "none") };
            break;
        }
        if let Some(resolved) = theme.color_of(cell) {
            color = Some(resolved);
            break;
        }
    }
    // No Character colour on the shape: the TextStyle chain decides, which is
    // how a themed header title becomes white on its coloured fill.
    if color.is_none() {
        let style_id = page_child
            .and_then(|page| page.text_style.as_deref())
            .or(child.text_style.as_deref());
        for row in styles.text_rows(style_id) {
            let Some(cell) = row.get("Color") else {
                continue;
            };
            let value = cell.value.as_deref().unwrap_or("");
            if value.starts_with('#') {
                color = Some(value.to_string());
            } else if value.eq_ignore_ascii_case("Themed") {
                color = if foreign { Some("#000000".to_string()) } else { theme.themed_text(fill != "none") };
            } else {
                color = theme.color_of(cell);
            }
            if color.is_some() {
                break;
            }
        }
    }
    let text = text.trim();
    // PK/FK marker children are painted with their own marker fill in Visio;
    // ordinary master-child labels use their Character colour.
    let color = if text == "PK" || text == "FK" {
        fill.to_string()
    } else {
        color.unwrap_or_else(|| fill.to_string())
    };
    (size, font, bold, italic, color)
}

/// Horizontal alignment of a master-body child's text box. CFF container
/// titles use `HorzAlign` 8 for left-aligned labels; an absent row follows
/// Visio's centered default.
fn sub_text_align(
    page_child: Option<&ResolvedShape>,
    child: &crate::shapes::MasterShape,
) -> String {
    for sections in [
        page_child
            .map(|page| page.sections.as_slice())
            .unwrap_or(&[]),
        child.sections.as_slice(),
    ] {
        let Some(section) = sections.iter().find(|section| section.name == "Paragraph") else {
            continue;
        };
        let Some(row) = section.rows.first() else {
            continue;
        };
        if let Some(value) = row
            .cells
            .get("HorzAlign")
            .and_then(|cell| cell.value.as_deref())
            .and_then(|value| value.parse::<f64>().ok())
        {
            let value = value.round() as i64;
            return if value == 0 || value == 8 {
                "left"
            } else if value == 2 {
                "right"
            } else {
                "center"
            }
            .to_string();
        }
    }
    "center".to_string()
}

/// `Para.IndLeft` of a master-body child's text, in pixels: the shape's own Paragraph row, else its
/// text style's (ALARM labels indent 4 pt, which moves their centred text 2 pt to the right).
fn sub_text_indent(
    page_child: Option<&ResolvedShape>,
    child: &crate::shapes::MasterShape,
    styles: &DocumentStyles,
) -> f64 {
    let own = [
        page_child.map(|page| page.sections.as_slice()).unwrap_or(&[]),
        child.sections.as_slice(),
    ]
    .into_iter()
    .find_map(|sections| {
        sections
            .iter()
            .find(|section| section.name == "Paragraph")
            .and_then(|section| section.rows.first())
            .and_then(|row| row.cells.get("IndLeft"))
            .and_then(|cell| numeric(Some(cell)))
    });
    own.or_else(|| {
        styles
            .paragraph_rows(child.text_style.as_deref())
            .iter()
            .find_map(|cells| cells.get("IndLeft").and_then(|cell| numeric(Some(cell))))
    })
    .map_or(0.0, |value| (value * DPI).max(0.0))
}

fn resolved_field_text(node: &ResolvedShape) -> Option<String> {
    for sections in [node.own_sections.as_slice(), node.sections.as_slice()] {
        let Some(section) = sections.iter().find(|section| section.name == "Field") else {
            continue;
        };
        for row in &section.rows {
            let value = row
                .cells
                .get("Value")
                .and_then(|cell| cell.value.as_deref())
                .map(|value| value.trim_matches('"').trim().to_string())
                .unwrap_or_default();
            if !value.is_empty() && !matches!(value.as_str(), "Abc" | "Label" | "?") {
                return Some(value);
            }
        }
    }
    None
}

/// Text stored in a master shape's first Field row (UML markers use a field
/// so the value can be `PK`/`FK`).
fn field_text(child: &crate::shapes::MasterShape) -> String {
    child
        .sections
        .iter()
        .find(|section| section.name == "Field")
        .and_then(|section| section.rows.first())
        .and_then(|row| row.cells.get("Value"))
        .and_then(|cell| cell.value.as_deref())
        .unwrap_or("")
        .trim_matches('"')
        .trim()
        .to_string()
}

/// `HideText` is how item-master children suppress themselves (e.g. the FK
/// marker is only shown when `User.ForeignKey` is true).
fn child_text_hidden(child: &crate::shapes::MasterShape, id: &str, nested: &NestedCells) -> bool {
    let Some(cell) = child.cells.get("HideText") else {
        return false;
    };
    let values = nested.children.get(id);
    let value = if let Some(text) = usable_formula(cell) {
        let evaluated = formula::evaluate(text, &mut |reference| {
            // `Sheet.5!Width` names the group (the master's top shape), not this child.
            if let Some((sheet, cell)) = reference.rsplit_once('!') {
                if sheet.strip_prefix("Sheet.").is_some_and(|id| Some(id) == nested.top_id.as_deref()) {
                    return nested.parent.get(cell).copied();
                }
            }
            let name = reference.rsplit('!').next().unwrap_or(reference);
            values
                .and_then(|values| values.get(name).copied())
                .or_else(|| nested.parent.get(name).copied())
        });
        // `HideText = Geometry1.NoShow` (MRKTDG matrix axis labels) names the shape's own section
        // cell, which we do not evaluate: Visio's cached result stands for it.
        if evaluated.is_none() && text.starts_with("Geometry") && text.ends_with(".NoShow") {
            cell.value
                .as_deref()
                .and_then(|value| value.parse::<f64>().ok())
        } else {
            evaluated
        }
    } else {
        cell.value
            .as_deref()
            .and_then(|value| value.parse::<f64>().ok())
    };
    value.is_some_and(|value| value != 0.0)
}

/// Resolve the fill and line cells of one body shape the way the page shape
/// does: the instance's own cells first, then the body shape, then the
/// document's fill/line style chain. `LinePattern` 0 (or a themed pattern that
/// resolves to no line) leaves Visio drawing a light tint of the fill.
fn child_paint(
    node: &ResolvedShape,
    cells: &BTreeMap<String, Cell>,
    child_fill_style: Option<&str>,
    child_line_style: Option<&str>,
    theme: &Theme,
    styles: &DocumentStyles,
    large: bool,
) -> BodyPaint {
    // The master child carries its own FillStyle/LineStyle (Visio's stencil
    // libraries put the real paint there); only afterwards fall back to the
    // instance group's style chain.
    let mut fill_styles = styles.fill_row_refs(child_fill_style);
    if child_fill_style != node.fill_style.as_deref() {
        fill_styles.extend(styles.fill_row_refs(node.fill_style.as_deref()));
    }
    let mut line_styles = styles.line_row_refs(child_line_style);
    if child_line_style != node.line_style.as_deref() {
        line_styles.extend(styles.line_row_refs(node.line_style.as_deref()));
    }
    // A master-body child is an independent shape: its own cells and style
    // chain must win over the instance group's cells. The group is only the
    // final fallback (some stencils rely on it for monochrome bodies).
    let chain = [cells];
    let mut fill_extras = fill_styles.clone();
    fill_extras.push(&node.cells);
    let mut line_extras = line_styles.clone();
    line_extras.push(&node.cells);
    let quick_extras: Vec<&BTreeMap<String, Cell>> = fill_extras
        .iter()
        .chain(line_extras.iter())
        .copied()
        .collect();
    let own_fill = find_chain(&chain, "FillForegnd");
    let fill_cell = own_fill.or_else(|| find_extra("FillForegnd", &fill_extras));
    let bkgnd_cell = pick_with(&chain, "FillBkgnd", &fill_extras);
    let fill_pattern = pick_with(&chain, "FillPattern", &fill_extras);
    let line_color = pick_with(&chain, "LineColor", &line_extras);
    let line_weight = pick_with(&chain, "LineWeight", &line_extras);
    let line_pattern = pick_with(&chain, "LinePattern", &line_extras);
    let line_cap = pick_with(&chain, "LineCap", &line_extras).and_then(|cell| numeric(Some(cell)));
    let line_transparency = pick_with(&chain, "LineColorTrans", &line_extras)
        .and_then(|cell| numeric(Some(cell)))
        .unwrap_or(0.0);
    let fill_transparency = pick_with(&chain, "FillForegndTrans", &fill_extras)
        .and_then(|cell| numeric(Some(cell)))
        .unwrap_or(0.0);
    let fill_opacity = (1.0 - fill_transparency).clamp(0.0, 1.0);
    let stroke_opacity = (1.0 - line_transparency).clamp(0.0, 1.0);
    let neutral_theme = !theme.present
        && node.cells.keys().chain(cells.keys()).any(|name| {
            name.starts_with("QuickStyle") && (name.ends_with("Matrix") || name.ends_with("Color"))
        });
    let quick_fill = if neutral_theme {
        None
    } else {
        numeric(pick_with(&chain, "QuickStyleFillColor", &quick_extras))
    };
    let quick_line = if neutral_theme {
        None
    } else {
        numeric(pick_with(&chain, "QuickStyleLineColor", &quick_extras))
    };
    let quick_fill_matrix = if neutral_theme {
        None
    } else {
        numeric(pick_with(&chain, "QuickStyleFillMatrix", &quick_extras))
    };
    let quick_line_matrix = if neutral_theme {
        None
    } else {
        numeric(pick_with(&chain, "QuickStyleLineMatrix", &quick_extras))
    };
    let quick_fill_paint = theme.quick_fill_paint(
        quick_fill_matrix,
        quick_fill.filter(|value| *value != 100.0),
    );
    let variant_line = numeric(pick_with(&chain, "QuickStyleVariation", &quick_extras))
        .is_some_and(|value| (value as i64) & 4 != 0);
    let quick_line_paint = theme.quick_line_paint(
        quick_line_matrix,
        quick_line.filter(|value| *value != 100.0 && *value != 0.0),
        variant_line,
    );

    let mut fill = fill_cell
        .filter(|cell| is_concrete(cell))
        .and_then(|cell| theme.color_of(cell));
    let quick_fill = quick_fill.filter(|value| *value != 100.0);
    let bare_themed_fill = fill_cell.is_some_and(is_bare_themed);
    let mut fill_gradient = None;
    if fill.is_none() && (!bare_themed_fill || quick_fill_paint.is_some()) {
        if let Some(quick) = &quick_fill_paint {
            fill = Some(if large {
                tint(&quick.color, 85.0)
            } else {
                quick.color.clone()
            });
            fill_gradient = quick.gradient.clone();
        } else if let Some(quick) = quick_fill.and_then(|value| theme.quick_color(Some(value))) {
            fill = Some(if large { tint(&quick, 85.0) } else { quick });
        }
    }
    let fill_role = "fill";
    if fill.is_none() && !neutral_theme {
        fill = theme.themed_color(fill_cell, fill_role);
    }
    if fill.is_none() && !neutral_theme {
        fill = theme.themed_color(bkgnd_cell, fill_role);
    }
    let fill = fill.unwrap_or_else(|| "#ffffff".to_string());
    let no_fill = numeric(fill_pattern).is_some_and(|value| value == 0.0);
    // Legacy gradient patterns 25-40 blend the foreground into FillBkgnd.
    let pattern_kind = numeric(fill_pattern).map_or(1, |value| value.round() as u32);
    if !no_fill && fill_gradient.is_none() && (25..=40).contains(&pattern_kind) {
        let back = bkgnd_cell
            .and_then(|cell| theme.color_of(cell))
            .unwrap_or_else(|| "#ffffff".to_string());
        let mut legacy = crate::import::legacy_pattern_gradient(pattern_kind, &fill, &back);
        // A legacy pattern gradient that also carries an explicit rectangular/radial type
        // (ENTAPP laptop screens: FillPattern 31 with FillGradientDir 12) is drawn with that type.
        let explicit_dir = pick_with(&chain, "FillGradientDir", &fill_extras)
            .and_then(|cell| numeric(Some(cell)))
            .filter(|dir| (8.0..=13.0).contains(dir) && (31..=34).contains(&pattern_kind));
        if let (Some(dir), Some(object)) = (explicit_dir, legacy.as_object_mut()) {
            object.insert("dir".to_string(), serde_json::json!(dir as u64));
            object.insert("far_end".to_string(), serde_json::json!(true));
        }
        fill_gradient = Some(legacy);
    }

    // Hatch and dither patterns (2-24) draw the foreground over the background colour.
    let fill_pattern = if !no_fill && (2..=24).contains(&pattern_kind) {
        Some(layoutcraft_model::FillPattern {
            kind: pattern_kind,
            fore: fill.clone(),
            back: bkgnd_cell
                .and_then(|cell| {
                    // Without a theme part a bare `Themed` background falls through to the root
                    // style's FillBkgnd, which is black (Site link's checker is yellow on black).
                    if !theme.present && is_bare_themed(cell) {
                        Some("#000000".to_string())
                    } else {
                        theme.color_of(cell)
                    }
                })
                .unwrap_or_else(|| "#ffffff".to_string()),
        })
    } else {
        None
    };

    let mut stroke = line_color
        .filter(|cell| is_concrete(cell))
        .and_then(|cell| theme.color_of(cell));
    // A legacy shape (QuickStyleLineColor/Matrix 100 and only the bare `THEMEVAL()` of its line
    // style) in a document without a theme part draws the line style's own colour, as in
    // `import.rs` (COM-verified on PEPUMP/NETSME).
    let foreign_theme = crate::import::foreign_theme_index(node) || cells.get("ThemeIndex").and_then(|cell| numeric(Some(cell))).is_some_and(|value| value != 0.0 && value != 65534.0);
    let legacy_themed = (!theme.present || foreign_theme)
        && quick_line.is_some_and(|value| (100.0..=106.0).contains(&value))
        && quick_line_matrix.is_none_or(|value| (100.0..=106.0).contains(&value))
        && line_color.is_some_and(is_bare_themed);
    let style_line = |name: &str| -> Option<&Cell> {
        line_styles
            .iter()
            .filter_map(|cells| cells.get(name))
            .find(|cell| cell.value.as_deref().is_some_and(|v| !v.is_empty() && v != "Themed"))
    };
    if legacy_themed {
        if let Some(cell) = style_line("LineColor") {
            let mut copy = cell.clone();
            copy.formula = None;
            if let Some(color) = theme.color_of(&copy) {
                stroke = Some(color);
            }
        }
    }
    let quick_line = quick_line.filter(|value| *value != 100.0 && *value != 0.0);
    if stroke.is_none() {
        stroke = quick_line_paint
            .as_ref()
            .map(|quick| quick.color.clone())
            .or_else(|| quick_line.and_then(|value| theme.quick_color(Some(value))));
    }
    if stroke.is_none() && !neutral_theme {
        stroke = theme.themed_color(line_color, "line");
    }
    let stroke = stroke.unwrap_or_else(|| "#000000".to_string());

    let weight = line_weight
        .and_then(|cell| numeric(Some(cell)))
        .or_else(|| {
            // A bare `Themed` weight in a document with no theme part falls back to the first
            // concrete weight in the line-style chain (the root style's 0.75 pt), not to the
            // Office theme's 1 pt (COM-verified on NETLME Data Center).
            if theme.present {
                return None;
            }
            style_line("LineWeight").and_then(|cell| numeric(Some(cell)))
        })
        .unwrap_or_else(|| {
            let matrix = numeric(find_chain(&chain, "QuickStyleLineMatrix"));
            theme.themed_line_weight_pt(matrix) / 72.0
        });
    let mut stroke_width = ((weight * DPI * 100.0).round() / 100.0).max(0.2);
    let mut dash = quick_line_paint
        .as_ref()
        .and_then(|quick| quick.dash.clone());
    let mut line_visible = true;
    if let Some(pattern) = line_pattern {
        match pattern.value.as_deref() {
            Some(value) if value.parse::<f64>().is_ok() => {
                let value = value.parse::<f64>().unwrap_or(1.0);
                if value == 0.0 {
                    stroke_width = 0.0;
                    line_visible = false;
                } else if (2.0..=23.0).contains(&value) && value.fract() == 0.0 {
                    dash = Some(format!("pat{}", value as i64));
                }
            }
            // `THEMEVAL()`/`Themed` resolves to the theme's solid line
            // pattern; treating it as "no line" turns every stencil outline
            // into a tint of its fill.
            _ => {}
        }
    }

    if stroke_opacity <= 0.0 {
        stroke_width = 0.0;
        line_visible = false;
    }
    let dash = crate::import::with_line_cap(dash, line_cap);
    BodyPaint {
        fill: if no_fill { "none".to_string() } else { fill },
        stroke,
        stroke_width,
        dash,
        line_visible,
        fill_opacity,
        stroke_opacity,
        fill_gradient,
        fill_pattern,
    }
}

fn find_chain<'a>(chain: &[&'a BTreeMap<String, Cell>], name: &str) -> Option<&'a Cell> {
    for cells in chain {
        if let Some(cell) = cells.get(name) {
            if cell.formula.as_deref() == Some("Inh") && !is_concrete(cell) {
                continue;
            }
            return Some(cell);
        }
    }
    chain.iter().find_map(|cells| cells.get(name))
}

fn find_extra<'a>(name: &str, rows: &[&'a BTreeMap<String, Cell>]) -> Option<&'a Cell> {
    rows.iter()
        .find_map(|row| {
            row.get(name)
                .filter(|cell| cell.formula.as_deref() != Some("Inh"))
        })
        .or_else(|| rows.iter().find_map(|row| row.get(name)))
}

fn pick_with<'a>(
    chain: &[&'a BTreeMap<String, Cell>],
    name: &str,
    rows: &[&'a BTreeMap<String, Cell>],
) -> Option<&'a Cell> {
    find_chain(chain, name).or_else(|| find_extra(name, rows))
}

fn numeric(cell: Option<&Cell>) -> Option<f64> {
    let value = cell?.value.as_deref()?;
    if value.is_empty() || value == "Themed" {
        return None;
    }
    value.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn part_source(
    child: &MasterShape,
    page_child: Option<&ResolvedShape>,
    role: &str,
) -> Option<layoutcraft_model::PartSource> {
    Some(layoutcraft_model::PartSource {
        master_shape_id: child.id.clone(),
        page_shape_id: page_child.map(|page| page.id.clone()),
        role: role.to_string(),
    })
}

fn page_child_for<'a>(node: &'a ResolvedShape, master_shape_id: &str) -> Option<&'a ResolvedShape> {
    fn walk<'a>(children: &'a [ResolvedShape], id: &str) -> Option<&'a ResolvedShape> {
        for child in children {
            if child.master_shape_id.as_deref() == Some(id) {
                return Some(child);
            }
            if let Some(found) = walk(&child.children, id) {
                return Some(found);
            }
        }
        None
    }
    walk(&node.children, master_shape_id)
}

fn number_option(cells: &BTreeMap<String, Cell>, name: &str) -> Option<f64> {
    cells
        .get(name)
        .and_then(|cell| cell.value.as_deref())
        .and_then(|value| value.parse::<f64>().ok())
}

fn resolve_cells(node: &ResolvedShape, master_space: bool) -> NestedCells {
    let top_id = node.master_top_id.clone();
    let mut parent = cell_map(&node.cells, &node.sections);
    // The children's formulas (`Sheet.5!Width*0.9`) are authored in the MASTER's space; the caller scales that space
    // into the instance (`master_map`). Evaluating them against the instance's own, already resized, Width and
    // Height scaled the art twice (a resized APC rack master drew at the square of the scale).
    if let (Some(master_w), Some(master_h), true) = (node.master_width, node.master_height, master_space) {
        for (size_name, loc_name, master) in [("Width", "LocPinX", master_w), ("Height", "LocPinY", master_h)] {
            if let Some(instance) = parent.get(size_name).copied().filter(|v| v.abs() > 1e-9) {
                let ratio = master / instance;
                if (ratio - 1.0).abs() > 1e-9 {
                    parent.insert(size_name.to_string(), master);
                    if let Some(loc) = parent.get(loc_name).copied() {
                        parent.insert(loc_name.to_string(), loc * ratio);
                    }
                }
            }
        }
    }
    let mut children: BTreeMap<String, BTreeMap<String, f64>> = node
        .master_children
        .iter()
        .map(|child| (child.id.clone(), cell_map(&child.cells, &child.sections)))
        .collect();
    // `SHAPETEXT(TheText)` is only known by length: whether the page shape (or its page child
    // standing for a master child) has text.
    let text_len = |text: &Option<String>| text.as_deref().map_or(0.0, |t| t.chars().count() as f64);
    parent.insert("TheText.len".to_string(), text_len(&node.text));
    for child in &node.master_children {
        // The page child's own text wins; otherwise the master child's.
        let page_text = node
            .children
            .iter()
            .find(|page_child| page_child.master_shape_id.as_deref() == Some(child.id.as_str()))
            .filter(|page_child| page_child.has_text)
            .map(|page_child| text_len(&page_child.text));
        if let Some(values) = children.get_mut(&child.id) {
            values.insert("TheText.len".to_string(), page_text.unwrap_or_else(|| text_len(&child.text)));
        }
    }
    let parent_entries = cell_entries(&node.cells, &node.sections);
    let child_entries: BTreeMap<String, Vec<(String, Cell)>> = node
        .master_children
        .iter()
        .map(|child| {
            (
                child.id.clone(),
                cell_entries(&child.cells, &child.sections),
            )
        })
        .collect();
    for _ in 0..8 {
        let current = children.clone();
        let parent_before = parent.clone();
        let mut next = BTreeMap::new();
        for child in &node.master_children {
            let mut values = current.get(&child.id).cloned().unwrap_or_default();
            for (key, cell) in &child_entries[&child.id] {
                let Some(formula_text) = usable_formula(cell) else {
                    continue;
                };
                let mut lookup = |reference: &str| {
                    reference_value(reference, &child.id, &parent, &current, top_id.as_deref())
                };
                if let Some(value) = formula::evaluate(formula_text, &mut lookup) {
                    values.insert(key.clone(), value);
                }
            }
            next.insert(child.id.clone(), values);
        }
        for (key, cell) in &parent_entries {
            let Some(formula_text) = usable_formula(cell) else {
                continue;
            };
            let mut lookup = |reference: &str| {
                if let Some((sheet, cell)) = reference.split_once('!') {
                    let id = sheet.strip_prefix("Sheet.")?;
                    if Some(id) == top_id.as_deref() {
                        return parent.get(cell).copied();
                    }
                }
                parent.get(reference).copied()
            };
            if let Some(value) = formula::evaluate(formula_text, &mut lookup) {
                parent.insert(key.clone(), value);
            }
        }
        // Once an iteration leaves both maps unchanged, every later iteration
        // would repeat it exactly, so stopping here gives identical results.
        let settled = next == current && parent == parent_before;
        children = next;
        if settled {
            break;
        }
    }
    NestedCells {
        parent,
        children,
        top_id,
    }
}

fn resolved_section(section: &Section, child_id: &str, nested: &NestedCells) -> Section {
    let mut out = section.clone();
    let section_no: usize = section
        .index
        .as_deref()
        .and_then(|index| index.parse::<usize>().ok())
        .map(|index| index + 1)
        .unwrap_or(1);
    let mut geometry: BTreeMap<String, f64> = BTreeMap::new();
    for cell in out.cells.values_mut() {
        resolve_formula(cell, section_no, child_id, nested, &geometry);
    }
    let mut rows: Vec<&mut crate::SectionRow> = out.rows.iter_mut().collect();
    rows.sort_by_key(|row| {
        row.index
            .as_deref()
            .and_then(|index| index.parse::<u64>().ok())
            .unwrap_or(u64::MAX)
    });
    for row in rows {
        for cell in row.cells.values_mut() {
            resolve_formula(cell, section_no, child_id, nested, &geometry);
        }
        for (name, cell) in &row.cells {
            if let Some(value) = cell
                .value
                .as_deref()
                .and_then(|value| value.parse::<f64>().ok())
            {
                geometry.insert(
                    format!("{name}{}", row.index.as_deref().unwrap_or("0")),
                    value,
                );
            }
        }
    }
    out
}

fn resolve_formula(
    cell: &mut Cell,
    section_no: usize,
    child_id: &str,
    nested: &NestedCells,
    geometry: &BTreeMap<String, f64>,
) {
    let Some(formula_text) = usable_formula(cell) else {
        return;
    };
    let prefix = format!("Geometry{section_no}.");
    let mut lookup = |reference: &str| {
        if let Some(key) = reference.strip_prefix(&prefix) {
            return geometry.get(key).copied();
        }
        reference_value(
            reference,
            child_id,
            &nested.parent,
            &nested.children,
            nested.top_id.as_deref(),
        )
    };
    if let Some(value) = formula::evaluate(formula_text, &mut lookup) {
        cell.value = Some(value.to_string());
    }
}

fn reference_value(
    reference: &str,
    own_id: &str,
    parent: &BTreeMap<String, f64>,
    children: &BTreeMap<String, BTreeMap<String, f64>>,
    top_id: Option<&str>,
) -> Option<f64> {
    if let Some((sheet, cell)) = reference.split_once('!') {
        let target = sheet.strip_prefix("Sheet.").unwrap_or(sheet);
        if Some(target) == top_id {
            return parent.get(cell).copied();
        }
        return children
            .get(target)
            .and_then(|values| values.get(cell))
            .copied();
    }
    children
        .get(own_id)
        .and_then(|values| values.get(reference))
        .copied()
}

fn usable_formula(cell: &Cell) -> Option<&str> {
    cell.formula
        .as_deref()
        .filter(|formula| *formula != "Inh" && *formula != "No Formula" && !formula.is_empty())
}

fn cell_map(cells: &BTreeMap<String, Cell>, sections: &[Section]) -> BTreeMap<String, f64> {
    let mut out: BTreeMap<String, f64> = cells
        .iter()
        .filter_map(|(name, cell)| {
            cell.value
                .as_deref()
                .and_then(|value| value.parse::<f64>().ok())
                .map(|value| (name.clone(), value))
        })
        .collect();
    for section in sections {
        match section.name.as_str() {
            "Scratch" => {
                for row in &section.rows {
                    let row_number = row
                        .index
                        .as_deref()
                        .and_then(|index| index.parse::<u64>().ok())
                        .unwrap_or(0)
                        + 1;
                    for (name, cell) in &row.cells {
                        if let Some(value) = cell
                            .value
                            .as_deref()
                            .and_then(|value| value.parse::<f64>().ok())
                        {
                            out.insert(format!("Scratch.{name}{row_number}"), value);
                        }
                    }
                }
            }
            "Control" => {
                // `Controls.Row_4.Y`; a bare `Controls.Row_4` is the row's X.
                for row in &section.rows {
                    let Some(name) = row.name.as_deref() else {
                        continue;
                    };
                    for (cell_name, cell) in &row.cells {
                        if let Some(value) = cell.value.as_deref().and_then(|value| value.parse::<f64>().ok()) {
                            out.insert(format!("Controls.{name}.{cell_name}"), value);
                            if cell_name == "X" {
                                out.insert(format!("Controls.{name}"), value);
                            }
                        }
                    }
                }
            }
            "User" => {
                for row in &section.rows {
                    let Some(name) = row.name.as_deref() else {
                        continue;
                    };
                    if let Some(value) = row.cells.get("Value").and_then(|cell| {
                        cell.value
                            .as_deref()
                            .and_then(|value| value.parse::<f64>().ok())
                    }) {
                        out.insert(format!("User.{name}"), value);
                    }
                }
            }
            "Geometry" => {
                let section_no = section
                    .index
                    .as_deref()
                    .and_then(|index| index.parse::<usize>().ok())
                    .map(|index| index + 1)
                    .unwrap_or(1);
                for row in &section.rows {
                    let row_index = row.index.as_deref().unwrap_or("0");
                    for (name, cell) in &row.cells {
                        if let Some(value) = cell
                            .value
                            .as_deref()
                            .and_then(|value| value.parse::<f64>().ok())
                        {
                            out.insert(format!("Geometry{section_no}.{name}{row_index}"), value);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn cell_entries(cells: &BTreeMap<String, Cell>, sections: &[Section]) -> Vec<(String, Cell)> {
    let mut out: BTreeMap<String, Cell> = cells
        .iter()
        .map(|(name, cell)| (name.clone(), cell.clone()))
        .collect();
    for section in sections {
        match section.name.as_str() {
            "Scratch" => {
                for row in &section.rows {
                    let row_number = row
                        .index
                        .as_deref()
                        .and_then(|index| index.parse::<u64>().ok())
                        .unwrap_or(0)
                        + 1;
                    for (name, cell) in &row.cells {
                        out.insert(format!("Scratch.{name}{row_number}"), cell.clone());
                    }
                }
            }
            "Control" => {
                for row in &section.rows {
                    let Some(name) = row.name.as_deref() else {
                        continue;
                    };
                    for (cell_name, cell) in &row.cells {
                        out.insert(format!("Controls.{name}.{cell_name}"), cell.clone());
                        if cell_name == "X" {
                            out.insert(format!("Controls.{name}"), cell.clone());
                        }
                    }
                }
            }
            "User" => {
                for row in &section.rows {
                    let Some(name) = row.name.as_deref() else {
                        continue;
                    };
                    if let Some(cell) = row.cells.get("Value") {
                        out.insert(format!("User.{name}"), cell.clone());
                    }
                }
            }
            "Geometry" => {
                let section_no = section
                    .index
                    .as_deref()
                    .and_then(|index| index.parse::<usize>().ok())
                    .map(|index| index + 1)
                    .unwrap_or(1);
                for row in &section.rows {
                    let row_index = row.index.as_deref().unwrap_or("0");
                    for (name, cell) in &row.cells {
                        out.insert(
                            format!("Geometry{section_no}.{name}{row_index}"),
                            cell.clone(),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    out.into_iter().collect()
}

fn section_value(section: &Section, name: &str) -> f64 {
    section
        .cells
        .get(name)
        .and_then(|cell| cell.value.as_deref())
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0)
}

/// Evaluate a master-body child's section `NoShow`, which is often a formula
/// over the instance's `User` cells (`IF(User.Type=1,0,1)`,
/// `NOT(User.Form)`). Child values win over instance values; `Sheet.N!`
/// prefixes are normalised. A formula that cannot be evaluated falls back to
/// the cached cell value, matching the previous behaviour.
fn section_hidden(
    section: &Section,
    child_values: Option<&BTreeMap<String, f64>>,
    parent_values: &BTreeMap<String, f64>,
    top_id: Option<&str>,
) -> bool {
    let Some(cell) = section.cells.get("NoShow") else {
        return false;
    };
    if let Some(text) = cell
        .formula
        .as_deref()
        .filter(|formula| *formula != "Inh" && *formula != "No Formula" && !formula.is_empty())
    {
        if let Some(value) = formula::evaluate(text, &mut |reference| {
            let name = reference.rsplit('!').next().unwrap_or(reference);
            // `Sheet.5!Width` names the group (the master's top shape), not this child.
            if let Some((sheet, cell)) = reference.rsplit_once('!') {
                if sheet.strip_prefix("Sheet.").is_some_and(|id| Some(id) == top_id) {
                    return parent_values.get(cell).copied();
                }
            }
            child_values
                .and_then(|values| values.get(name).copied())
                .or_else(|| parent_values.get(name).copied())
        }) {
            return value != 0.0;
        }
    }
    cell.value
        .as_deref()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0)
        != 0.0
}

fn number(cells: &BTreeMap<String, Cell>, name: &str, fallback: f64) -> f64 {
    cells
        .get(name)
        .and_then(|cell| cell.value.as_deref())
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(fallback)
}


/// The path with every point moved by (`dx`, `dy`). The builder emits absolute `M L C Q A Z`
/// commands only; an arc moves just its end point.
fn translate_path(d: &str, dx: f64, dy: f64) -> String {
    let mut out = String::with_capacity(d.len() + 16);
    let mut command = ' ';
    let mut args: Vec<f64> = Vec::new();
    let mut number = String::new();
    let flush = |command: char, args: &mut Vec<f64>, out: &mut String| {
        if command == ' ' {
            return;
        }
        out.push(command);
        let arc = command == 'A';
        for (index, value) in args.iter().enumerate() {
            let shifted = match (arc, index % 7) {
                (false, i) if i % 2 == 0 => value + dx,
                (false, _) => value + dy,
                (true, 5) => value + dx,
                (true, 6) => value + dy,
                (true, _) => *value,
            };
            if index > 0 {
                out.push(' ');
            }
            out.push_str(&format!("{shifted:.2}"));
        }
        args.clear();
    };
    for ch in d.chars() {
        if ch.is_ascii_alphabetic() {
            if !number.is_empty() {
                args.push(number.parse().unwrap_or(0.0));
                number.clear();
            }
            flush(command, &mut args, &mut out);
            command = ch;
            if ch == 'Z' {
                out.push('Z');
                command = ' ';
            }
        } else if ch == ' ' || ch == ',' {
            if !number.is_empty() {
                args.push(number.parse().unwrap_or(0.0));
                number.clear();
            }
        } else if ch == '-' && !number.is_empty() && !number.ends_with('e') {
            args.push(number.parse().unwrap_or(0.0));
            number.clear();
            number.push(ch);
        } else {
            number.push(ch);
        }
    }
    if !number.is_empty() {
        args.push(number.parse().unwrap_or(0.0));
    }
    flush(command, &mut args, &mut out);
    out
}
