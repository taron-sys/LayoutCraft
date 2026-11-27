//! Editable-model conversion: 2-D shapes, group flattening, simple line/ellipse
//! geometry, text, Shape Data, theme/quick-style inheritance, pictures,
//! connectors and waypoints.
//!
//! Full rich-text layout, rotated-group label placement and native Visio data
//! graphics are later slices; this module deliberately produces a partial
//! document for comparison.
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::f64::consts::PI;

use layoutcraft_model::{
    Connector, DataRow, Document, Endpoint, Page, Shape, ShapePart, SCHEMA_VERSION,
};

use crate::media::{
    attach_images, collect_foreign, resolve_callout_links, restore_data_graphic, restore_native_data_graphic,
    MediaCollector,
};
use crate::shapes::{Cell, ResolvedShape, Section, TextToken};
use crate::styles::{is_bare_themed, is_concrete, tint, DocumentStyles, Theme};
use crate::{formula, Error, Package, Result};

const DPI: f64 = 96.0;
const FONT_STACK: &str = "\"Segoe UI\", Calibri, Helvetica, Arial, sans-serif";

/// What an import leaves behind for the next one (the media numbering), see `import_document_with`.
#[derive(Debug, Default, Clone)]
pub struct ImportState {
    media: MediaCollector,
    /// The master catalog, document styles and document theme of the package: they come from parts no page owns, so an edit of a page keeps them
    /// (loading them was ~50 ms of every edit of a drawing with masters).
    shared: Option<std::sync::Arc<SharedParts>>,
    /// Per page: what the top-level shapes of the page flattened to. An edit changes a shape or two; the others are taken over as they were
    /// (flattening a master-backed group costs ~10 ms, a page of 20 of them was most of the 300 ms of an edit).
    pages: Vec<Option<std::sync::Arc<PageCache>>>,
}

/// Pages with more top-level shapes than this do not keep the flattened output of their shapes.
const MAX_CACHED_NODES: usize = 400;

#[derive(Debug)]
struct PageCache {
    /// Everything besides the shapes that the flattened output depends on (page height, theme choice of the page): equal key = same inputs.
    key: String,
    nodes: Vec<std::sync::Arc<NodeCache>>,
}

#[derive(Debug)]
struct NodeCache {
    /// The resolved top-level shape this output came from (the input of the flattening).
    source: ResolvedShape,
    /// `source` with the master pictures added (`add_master_pictures`), which `collect_foreign` reads later.
    drawn: ResolvedShape,
    /// The stacking counter before and after the node (the output carries it in each shape).
    z_start: i64,
    z_end: i64,
    shapes: Vec<Shape>,
}

impl ImportState {
    /// A group editor on page `page` of `package`, an edited version of `from` (the package this state was made from): the master catalog of the
    /// import is reused when the parts no page owns are the same, else it is loaded again.
    pub fn open_editor(&self, from: &Package, package: &Package, page: usize) -> Result<crate::groupedit::GroupEditor> {
        match &self.shared {
            Some(shared) if shared_parts_unchanged(from, package) => crate::groupedit::GroupEditor::open_with_catalog(package, page, &shared.catalog),
            _ => crate::groupedit::GroupEditor::open(package, page),
        }
    }
}

#[derive(Debug)]
struct SharedParts {
    catalog: crate::MasterCatalog,
    styles: DocumentStyles,
    document_theme: Theme,
}

/// Parts that belong to one page: `visio/pages/pageN.xml` and its relationships.
fn page_owned_part(name: &str) -> bool {
    if crate::is_page_part(name) {
        return true;
    }
    name.strip_prefix("visio/pages/_rels/page")
        .and_then(|rest| rest.strip_suffix(".xml.rels"))
        .is_some_and(|stem| !stem.is_empty() && stem.chars().all(|c| c.is_ascii_digit()))
}

fn rels_part_name(part: &str) -> String {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    format!("{dir}/_rels/{file}.rels")
}

/// True when every part that is not owned by a single page is byte-identical in both packages (masters, styles, theme,
/// `pages.xml`, media, content types...): then a page whose own parts are unchanged imports to the same result.
fn shared_parts_unchanged(old: &Package, new: &Package) -> bool {
    let shared = |p: &Package| p.names().filter(|n| !page_owned_part(n)).map(str::to_string).collect::<Vec<_>>();
    let names = shared(new);
    names == shared(old) && names.iter().all(|n| old.entry(n) == new.entry(n))
}

fn page_unchanged(old: &Package, new: &Package, before: &crate::PageRef, now: &crate::PageRef) -> bool {
    if before != now {
        return false;
    }
    let same = |part: &str| old.entry(part) == new.entry(part) && old.entry(&rels_part_name(part)) == new.entry(&rels_part_name(part));
    same(&now.part) && now.back_part.as_deref().map_or(true, same)
}

pub fn import_document(package: &Package) -> Result<Document> {
    Ok(import_document_with(package, None)?.0)
}

/// Import `package`, reusing the pages of `previous` (the package, its document and import state) that an edit left alone:
/// a page is taken over as it was when its own parts, its background page's parts and all shared parts are unchanged. Pages
/// that changed are imported again, with the same media numbering. The result equals a full import except that media ids
/// keep the numbers of the earlier import.
pub fn import_document_with(
    package: &Package,
    previous: Option<(&Package, &Document, &ImportState)>,
) -> Result<(Document, ImportState)> {
    let pages = package.pages()?;
    let reuse = previous.and_then(|(old_package, old_document, old_state)| {
        if !shared_parts_unchanged(old_package, package) {
            return None;
        }
        let old_pages = old_package.pages().ok()?;
        (old_pages.len() == old_document.pages.len()).then_some((old_package, old_document, old_state, old_pages))
    });
    let shared: std::sync::Arc<SharedParts> = match reuse.as_ref().and_then(|(_, _, state, _)| state.shared.clone()) {
        Some(shared) => shared,
        None => std::sync::Arc::new(SharedParts {
            catalog: crate::MasterCatalog::load(package)?,
            styles: DocumentStyles::load(package)?,
            document_theme: Theme::load(package)?,
        }),
    };
    let catalog = &shared.catalog;
    let styles = &shared.styles;
    let document_theme = &shared.document_theme;
    // A drawing with several theme parts (a theme per page, Design > Themes on one page): a page follows the part whose schemeEnum is its `ThemeIndex`.
    // Only for drawings whose theme parts each carry ONE scheme number (what Visio writes for Page.SetTheme and what we write): a part that names different numbers for its colour
    // and its effect / font schemes (a real file can say ThemeIndex 34 and ColorSchemeIndex 40) keeps the older rule, the last Visio theme part wins.
    let theme_parts: Vec<&str> = package.names().filter(|n| { let l = n.to_ascii_lowercase(); l.contains("theme/theme") && l.ends_with(".xml") }).collect();
    let uniform_parts = !theme_parts.is_empty() && theme_parts.iter().all(|part| {
        let Ok(text) = package.text(part) else { return false };
        let mut numbers = text.match_indices("schemeEnum=").filter_map(|(at, key)| text[at + key.len() + 1..].split(['"', '\'']).next().map(str::to_string));
        let first = numbers.next();
        first.is_some() && numbers.all(|n| Some(&n) == first.as_ref())
    });
    let several_themes = uniform_parts && theme_parts.len() > 1;
    let mut page_themes: std::collections::HashMap<u32, Theme> = std::collections::HashMap::new();
    // Visio leaves the `ThemeIndex` cell out of a page that has no theme: once a page states a theme, a page without the cell is a page without one
    let any_page_themed = uniform_parts && pages.iter().any(|p| p.theme_index.is_some_and(|index| index != 0));
    let mut out = Vec::new();
    let mut media_collector = reuse.as_ref().map(|(_, _, state, _)| state.media.clone()).unwrap_or_default();
    let mut new_page_caches: Vec<Option<std::sync::Arc<PageCache>>> = Vec::new();
    for (page_index, page) in pages.into_iter().enumerate() {
        if let Some((old_package, old_document, old_state, old_pages)) = &reuse {
            if old_pages.get(page_index).is_some_and(|before| page_unchanged(old_package, package, before, &page)) {
                out.push(old_document.pages[page_index].clone());
                new_page_caches.push(old_state.pages.get(page_index).cloned().flatten());
                continue;
            }
        }
        // A page can pick its own theme colour variation (PageSheet cell).
        let page_theme_index = page.theme_index.or(any_page_themed.then_some(0));
        let base_theme = match page_theme_index.filter(|index| several_themes || (uniform_parts && *index == 0)) {
            Some(index) => {
                if !page_themes.contains_key(&index) {
                    page_themes.insert(index, Theme::load_for(package, Some(index))?);
                }
                page_themes[&index].clone()
            }
            None => document_theme.clone(),
        };
        let mut theme = match page.variation {
            Some(index) => base_theme.with_variation(index),
            None => base_theme,
        };
        theme.legacy_shadow_offset = page.shadow_offset;
        let page_height = match page.height {
            Some(height) => height,
            None => page_height_inches(package, &page.part)?,
        };
        let tree = package.page_shapes(&page.part)?;
        let resolved = catalog.resolve(&tree)?;
        let mut shapes = Vec::new();
        let mut z;
        let mut stack = Vec::new();
        let foreground_scale = page.scale.filter(|scale| *scale > 0.0).unwrap_or(1.0);
        let background_start = shapes.len();
        // Composite a referenced background page beneath the foreground.
        if let Some(back_part) = page.back_part.as_deref() {
            let scales_match = page
                .back_scale
                .map(|scale| (scale - foreground_scale).abs() < 1e-6)
                .unwrap_or(true);
            if scales_match {
                if let Ok(back_tree) = package.page_shapes(back_part) {
                    if let Ok(back_resolved) = catalog.resolve(&back_tree) {
                        let back_height = page.back_height.unwrap_or(page_height);
                        let mut back_z = -1_000_000i64;
                        for node in &back_resolved {
                            flatten_node(
                                node,
                                IDENTITY,
                                back_height,
                                &mut stack,
                                &mut shapes,
                                &mut back_z,
                                &styles,
                                &theme,
                                None,
                                true,
                            );
                        }
                        let back_rels = package.relationships(back_part)?;
                        attach_images(&mut shapes[background_start..], &collect_foreign(&back_resolved), &back_rels, package, &mut media_collector);
                    }
                }
            }
        }
        // from here on the page's own shapes: their IDs may be those of shapes on the background page
        let foreground_start = shapes.len();
        for shape in &mut shapes[background_start..] {
            if shape.label.trim().parse::<u32>().is_ok() {
                shape.label = (page_index + 1).to_string();
                shape.rich = None;
            }
        }
        z = 0;
        // Pictures that are part of a master's group body (a rack unit that is one EMF picture) are drawn as children of the group.
        // A page without a background page whose inputs are those of the previous import takes over the output of the top-level shapes that did not change.
        let cache_key = format!("{page_height:?}|{:?}|{:?}|{:?}|{any_page_themed}|{several_themes}|{}", page.theme_index, page.variation, page.shadow_offset, background_start);
        let old_cache = reuse
            .as_ref()
            .and_then(|(_, _, state, _)| state.pages.get(page_index))
            .and_then(|cache| cache.as_ref())
            .filter(|cache| page.back_part.is_none() && cache.key == cache_key);
        // Keeping the output costs copies of every node: only for pages of a size one edits by hand (a 35 MB page with thousands of shapes opened
        // three times slower with it, and nobody drags those shapes around).
        // It is only built by an import that has a previous one (an edit): opening a file stays as cheap as it was, the first edit of a page pays for
        // the copies and the following ones profit.
        let keep_cache = reuse.is_some() && page.back_part.is_none() && resolved.len() <= MAX_CACHED_NODES;
        let mut drawn: Vec<ResolvedShape> = Vec::with_capacity(resolved.len());
        let mut node_caches: Vec<std::sync::Arc<NodeCache>> = Vec::with_capacity(resolved.len());
        for (index, node) in resolved.iter().enumerate() {
            if let Some(previous) = old_cache.and_then(|cache| cache.nodes.get(index)).filter(|previous| previous.z_start == z && previous.source == *node) {
                shapes.extend(previous.shapes.iter().cloned());
                z = previous.z_end;
                drawn.push(previous.drawn.clone());
                node_caches.push(previous.clone());
                continue;
            }
            let drawn_node = add_master_pictures(node);
            let (z_start, first) = (z, shapes.len());
            flatten_node(
                &drawn_node,
                IDENTITY,
                page_height,
                &mut stack,
                &mut shapes,
                &mut z,
                &styles,
                &theme,
                None,
                false,
            );
            if keep_cache {
                node_caches.push(std::sync::Arc::new(NodeCache { source: node.clone(), drawn: drawn_node.clone(), z_start, z_end: z, shapes: shapes[first..].to_vec() }));
            }
            drawn.push(drawn_node);
        }
        new_page_caches.push(keep_cache.then(|| std::sync::Arc::new(PageCache { key: cache_key, nodes: node_caches })));
        let page_connects = package.connects(&page.part)?;
        let sites = glued_sites(package, page_index, &page_connects);
        let mut connectors = convert_connectors(
            &resolved,
            page_height,
            &page_connects,
            &sites,
            &shapes,
            &styles,
            &theme,
        );
        let page_rels = package.relationships(&page.part)?;
        let foreign = collect_foreign(&drawn);
        attach_images(
            &mut shapes[foreground_start..],
            &foreign,
            &page_rels,
            package,
            &mut media_collector,
        );
        // The drawing-only picture children of a master group (`with_master_pictures`) belong to the group: a click on the picture selects it.
        for shape in &mut shapes {
            if let Some(owner) = shape.visio_id.as_deref().and_then(|id| id.split_once(":m")).map(|(owner, _)| owner.to_string()) {
                shape.visio_id = Some(owner);
            }
        }
        resolve_callout_links(&mut shapes);
        let layers = page.layers.clone();
        apply_layer_visibility(&mut shapes, &resolved, &layers);
        apply_layer_colors(&mut shapes, &resolved, &layers);
        let guides = collect_guides(&resolved);
        let groups = collect_groups(&resolved, &shapes, page_height);
        let containers = crate::containers::apply(&resolved, &mut shapes, &mut connectors);
        out.push(Page {
            id: page.id.clone(),
            name: page.name,
            shapes,
            connectors,
            groups,
            containers,
            layers,
            guides,
            line_jump: page.line_jump.clone(),
            visio: None,
            scale: foreground_scale,
            extra: Default::default(),
        });
    }
    let document = Document {
        schema_version: SCHEMA_VERSION,
        version: 1,
        name: None,
        page_index: 0,
        pages: out,
        theme: Some(document_theme.to_json()),
        media: media_collector.records.clone(),
        extra: Default::default(),
    };
    Ok((document, ImportState { media: media_collector, shared: Some(shared), pages: new_page_caches }))
}

/// Layer indices from a shape's `LayerMember` cell (`"0;2"`).
fn layer_members(node: &ResolvedShape) -> Vec<u32> {
    node.cells
        .get("LayerMember")
        .and_then(|cell| cell.value.as_deref())
        .map(|value| {
            value
                .split(';')
                .filter_map(|part| part.trim().parse::<u32>().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// A shape is drawn when it has no layers, or at least one of its layers is
/// visible (Visio's rule, checked against `Page.Export`). Unknown layer
/// indices count as visible.
fn layers_hide(members: &[u32], layers: &[layoutcraft_model::Layer]) -> bool {
    !members.is_empty()
        && members.iter().all(|index| {
            layers
                .iter()
                .find(|layer| layer.index == *index)
                .is_some_and(|layer| !layer.visible)
        })
}

/// Visio IDs of every node hidden by its own layers or an ancestor group's.
fn hidden_node_ids(
    nodes: &[ResolvedShape],
    layers: &[layoutcraft_model::Layer],
    inherited: bool,
    out: &mut std::collections::HashSet<String>,
) {
    for node in nodes {
        let hidden = inherited || layers_hide(&layer_members(node), layers);
        if hidden {
            out.insert(node.id.clone());
        }
        hidden_node_ids(&node.children, layers, hidden, out);
    }
}

/// The colour of the first layer with a colour that each node is on (a group's members take their group's).
fn colored_node_ids(nodes: &[ResolvedShape], layers: &[layoutcraft_model::Layer], inherited: Option<&str>, out: &mut std::collections::HashMap<String, String>) {
    for node in nodes {
        let own = layer_members(node)
            .iter()
            .find_map(|index| layers.iter().find(|layer| layer.index == *index).and_then(|layer| layer.color.as_deref()));
        let colour = own.or(inherited);
        if let Some(colour) = colour {
            out.insert(node.id.clone(), colour.to_string());
        }
        colored_node_ids(&node.children, layers, colour, out);
    }
}

fn apply_layer_colors(shapes: &mut [Shape], resolved: &[ResolvedShape], layers: &[layoutcraft_model::Layer]) {
    if !layers.iter().any(|layer| layer.color.is_some()) {
        return;
    }
    let mut colours = std::collections::HashMap::new();
    colored_node_ids(resolved, layers, None, &mut colours);
    for shape in shapes.iter_mut() {
        if let Some(colour) = shape.visio_id.as_ref().and_then(|id| colours.get(id)) {
            shape.layer_color = Some(colour.clone());
        }
    }
}

/// Mark shapes on hidden layers (and members of a group on a hidden layer).
fn apply_layer_visibility(
    shapes: &mut [Shape],
    resolved: &[ResolvedShape],
    layers: &[layoutcraft_model::Layer],
) {
    if !layers.iter().all(|layer| layer.visible) {
        let mut hidden = std::collections::HashSet::new();
        hidden_node_ids(resolved, layers, false, &mut hidden);
        for shape in shapes.iter_mut() {
            if shape
                .visio_id
                .as_ref()
                .is_some_and(|id| hidden.contains(id))
            {
                shape.layer_hidden = true;
            }
        }
    }
    if !layers.iter().all(|layer| layer.print) {
        // Same membership rule as visibility, evaluated on the Print flag.
        let as_print: Vec<layoutcraft_model::Layer> = layers
            .iter()
            .map(|layer| layoutcraft_model::Layer {
                visible: layer.print,
                ..layer.clone()
            })
            .collect();
        let mut nonprinting = std::collections::HashSet::new();
        hidden_node_ids(resolved, &as_print, false, &mut nonprinting);
        for shape in shapes.iter_mut() {
            if shape
                .visio_id
                .as_ref()
                .is_some_and(|id| nonprinting.contains(id))
            {
                shape.layer_nonprinting = true;
            }
        }
    }
}

/// Groups that were flattened into world-space member shapes. Master-backed
/// groups are drawn as one shape and need no entry.
///
/// `members` are the Visio IDs of the shapes directly inside the group.
/// `frame` is the group's own box in the same convention as a shape
/// (`x`/`y` top-left of the unrotated box around its world centre, `w`/`h`,
/// `rot` in degrees), so a rotated group keeps its oriented frame; `bounds` is
/// the axis-aligned box of that frame. A group with no usable `Width`/`Height`
/// falls back to the union of its descendants' boxes (`frame.rot` = 0).
fn collect_groups(resolved: &[ResolvedShape], shapes: &[Shape], page_height: f64) -> Vec<Value> {
    fn descendants(node: &ResolvedShape, out: &mut std::collections::HashSet<String>) {
        for child in &node.children {
            out.insert(child.id.clone());
            descendants(child, out);
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn walk(
        nodes: &[ResolvedShape],
        parent_transform: Transform,
        parent: Option<&str>,
        root: Option<&str>,
        depth: usize,
        shapes: &[Shape],
        page_height: f64,
        out: &mut Vec<Value>,
    ) {
        for node in nodes {
            let is_group = node.shape_type.as_deref() == Some("Group");
            let flattened = is_group && (node.master_id.is_none() || is_container_group(node) || made_beside_a_guide(node));
            if !flattened {
                continue;
            }
            let Some(local) = shape_transform(node) else {
                continue;
            };
            let self_transform = compose(&parent_transform, &local);
            let root_id = root.unwrap_or(&node.id);
            let mut ids = std::collections::HashSet::new();
            descendants(node, &mut ids);
            let members: Vec<&str> = shapes
                .iter()
                .filter(|shape| {
                    shape
                        .group
                        .as_ref()
                        .and_then(|group| group.get("id"))
                        .and_then(Value::as_str)
                        == Some(node.id.as_str())
                })
                .filter_map(|shape| shape.visio_id.as_deref())
                .collect();

            // The group's own oriented box, placed like a shape.
            let w_in = cell_num(node, "Width").unwrap_or(0.0);
            let h_in = cell_num(node, "Height").unwrap_or(0.0);
            let mut frame = None;
            if w_in > 0.0 && h_in > 0.0 {
                let centre = apply_transform(&self_transform, w_in / 2.0, h_in / 2.0);
                frame = Some((
                    (centre.0 - w_in / 2.0) * DPI,
                    (page_height - centre.1 - h_in / 2.0) * DPI,
                    w_in * DPI,
                    h_in * DPI,
                    -transform_angle(&self_transform) * 180.0 / PI,
                ));
            }
            if frame.is_none() {
                let boxes: Vec<&Shape> = shapes
                    .iter()
                    .filter(|shape| shape.visio_id.as_ref().is_some_and(|id| ids.contains(id)))
                    .collect();
                if !boxes.is_empty() {
                    let x0 = boxes.iter().map(|s| s.x).fold(f64::INFINITY, f64::min);
                    let y0 = boxes.iter().map(|s| s.y).fold(f64::INFINITY, f64::min);
                    let x1 = boxes.iter().map(|s| s.x + s.w).fold(f64::NEG_INFINITY, f64::max);
                    let y1 = boxes.iter().map(|s| s.y + s.h).fold(f64::NEG_INFINITY, f64::max);
                    frame = Some((x0, y0, x1 - x0, y1 - y0, 0.0));
                }
            }
            let (frame_json, bounds_json) = match frame {
                Some((x, y, w, h, rot)) => {
                    let theta = rot.to_radians();
                    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
                    let half_w = (w * theta.cos().abs() + h * theta.sin().abs()) / 2.0;
                    let half_h = (w * theta.sin().abs() + h * theta.cos().abs()) / 2.0;
                    (
                        json!({"x": x, "y": y, "w": w, "h": h, "rot": rot}),
                        json!({"x": cx - half_w, "y": cy - half_h, "w": half_w * 2.0, "h": half_h * 2.0}),
                    )
                }
                None => (Value::Null, Value::Null),
            };
            out.push(json!({
                "visioId": node.id,
                "parentId": parent,
                "rootId": root_id,
                "depth": depth + 1,
                "kind": if is_container_group(node) { "container" } else { "group" },
                "name": node.name,
                "members": members,
                "frame": frame_json,
                "bounds": bounds_json,
            }));
            walk(
                &node.children,
                self_transform,
                Some(&node.id),
                Some(root_id),
                depth + 1,
                shapes,
                page_height,
                out,
            );
        }
    }
    let mut out = Vec::new();
    walk(resolved, IDENTITY, None, None, 0, shapes, page_height, &mut out);
    out
}

/// Ruler guides: `Type="Guide"` page shapes.
fn collect_guides(resolved: &[ResolvedShape]) -> Vec<layoutcraft_model::Guide> {
    resolved
        .iter()
        .filter(|node| node.shape_type.as_deref() == Some("Guide"))
        .map(|node| layoutcraft_model::Guide {
            visio_id: Some(node.id.clone()),
            x_in: cell_num(node, "PinX").unwrap_or(0.0),
            y_in: cell_num(node, "PinY").unwrap_or(0.0),
            angle: cell_num(node, "Angle").unwrap_or(0.0),
            extra: Default::default(),
        })
        .collect()
}

fn page_height_inches(package: &Package, part: &str) -> Result<f64> {
    let doc = package.xml(part)?;
    let root = doc.root_element();
    let sheet = root
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "PageSheet")
        .or_else(|| {
            root.descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "PageSheet")
        });
    let value = sheet
        .and_then(|s| {
            s.children().find(|n| {
                n.is_element()
                    && n.tag_name().name() == "Cell"
                    && n.attribute("N") == Some("PageHeight")
            })
        })
        .and_then(|c| c.attribute("V"))
        .and_then(|v| v.parse::<f64>().ok());
    value.ok_or_else(|| Error(format!("page {part} has no PageHeight")))
}

fn usable_visibility_formula(cell: &Cell) -> bool {
    cell.formula
        .as_deref()
        .is_some_and(|formula| !formula.is_empty() && formula != "Inh" && formula != "No Formula")
}

/// Visio's legacy gradient fill patterns 25-40 (foreground to background), as the
/// gradient the renderer already draws. Directions read from a Visio export of all
/// sixteen: 25/27 left-right/right-left, 28/30 top-bottom/bottom-top, 26/29 centred
/// bands, 31-34 diagonals from the top-left/top-right/bottom-left/bottom-right corner,
/// 35 and 40 from the centre, 36-39 radiating from a corner.
pub(crate) fn legacy_pattern_gradient(kind: u32, fore: &str, back: &str) -> Value {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    // (angle, radial preset, centred band)
    let (angle, dir, band) = match kind {
        25 => (0.0, 0, false),
        26 => (0.0, 0, true),
        27 => (PI, 0, false),
        28 => (FRAC_PI_2, 0, false),
        29 => (FRAC_PI_2, 0, true),
        30 => (-FRAC_PI_2, 0, false),
        31 => (FRAC_PI_4, 0, false),
        32 => (3.0 * FRAC_PI_4, 0, false),
        33 => (-FRAC_PI_4, 0, false),
        34 => (-3.0 * FRAC_PI_4, 0, false),
        35 | 40 => (0.0, 3, false),
        36 => (0.0, 4, false),
        37 => (0.0, 6, false),
        38 => (0.0, 2, false),
        _ => (0.0, 1, false), // 39: bottom-right corner
    };
    let stop = |color: &str, position: f64| json!({ "color": color, "opacity": 1.0, "position": position });
    let stops = if band {
        vec![stop(back, 0.0), stop(fore, 0.5), stop(back, 1.0)]
    } else {
        vec![stop(fore, 0.0), stop(back, 1.0)]
    };
    let mut gradient = json!({ "angle": angle, "dir": dir, "stops": stops });
    if (25..=29).contains(&kind) {
        gradient["smooth"] = Value::Bool(true);
    }
    gradient
}

/// A font size cell (inches) in pixels, kept to a hundredth of a pixel. Visio draws 8 pt text at
/// 10.67 px; rounding it to 11 px made text 3 % wider, enough to wrap a label that fits in
/// Visio (a Timeline interval date, 123 px of text in a box with 126 px to spare).
pub(crate) fn font_px(inches: f64) -> f64 {
    ((inches * DPI) * 100.0).round() / 100.0
}

fn is_connector(node: &ResolvedShape) -> bool {
    if cell_num(node, "ObjType").map(|v| v == 2.0).unwrap_or(false) {
        return true;
    }
    let Some(name) = node.master_name.as_deref().or(node.name.as_deref()) else {
        return false;
    };
    let canonical: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    const CONNECTOR_NAMES: [&str; 14] = [
        "dynamicconnector",
        "sequenceflow",
        "messageflow",
        "connector",
        "straightconnector",
        "relationship",
        "umlrelationship",
        "association",
        "dependency",
        "generalization",
        "inheritance",
        "realization",
        "aggregation",
        "composition",
    ];
    let named = CONNECTOR_NAMES
        .iter()
        .any(|candidate| canonical.starts_with(candidate));
    // A connector is 1-D: it has begin/end points. Master names alone are
    // ambiguous (an ER "Relationship" is a 2-D diamond).
    named && (node.cells.contains_key("BeginX") || node.cells.contains_key("EndX"))
}

#[derive(Debug, Clone, Copy)]
struct Transform {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

const IDENTITY: Transform = Transform {
    a: 1.0,
    b: 0.0,
    c: 0.0,
    d: 1.0,
    e: 0.0,
    f: 0.0,
};

fn apply_transform(t: &Transform, x: f64, y: f64) -> (f64, f64) {
    (t.a * x + t.c * y + t.e, t.b * x + t.d * y + t.f)
}

fn compose(parent: &Transform, local: &Transform) -> Transform {
    Transform {
        a: parent.a * local.a + parent.c * local.b,
        b: parent.b * local.a + parent.d * local.b,
        c: parent.a * local.c + parent.c * local.d,
        d: parent.b * local.c + parent.d * local.d,
        e: parent.a * local.e + parent.c * local.f + parent.e,
        f: parent.b * local.e + parent.d * local.f + parent.f,
    }
}

fn transform_angle(t: &Transform) -> f64 {
    t.b.atan2(t.a)
}

/// The turn and the mirror of a transform's linear part: `R(angle) * diag(-1 if mirrored, 1)`. A shape inside a flipped
/// group is placed with the angle and carries the mirror as an extra flip of its own (Visio mirrors the members of a
/// flipped group about the group's pin; the importer's shapes only know a turn and flips of their own box).
fn transform_turn_and_mirror(t: &Transform) -> (f64, bool) {
    let det = t.a * t.d - t.c * t.b;
    if det >= 0.0 {
        (t.b.atan2(t.a), false)
    } else {
        ((-t.b).atan2(-t.a), true)
    }
}

/// -1 for an axis the shape itself is flipped on (`FlipX` / `FlipY`), else 1.
fn own_flip_signs(node: &ResolvedShape) -> (f64, f64) {
    (
        if cell_num(node, "FlipX").unwrap_or(0.0) != 0.0 { -1.0 } else { 1.0 },
        if cell_num(node, "FlipY").unwrap_or(0.0) != 0.0 { -1.0 } else { 1.0 },
    )
}

/// `shape_transform` with the shape's own `FlipX` / `FlipY` (what its members see).
fn shape_transform_with_flips(node: &ResolvedShape) -> Option<Transform> {
    let plain = shape_transform(node)?;
    let fx = if cell_num(node, "FlipX").unwrap_or(0.0) != 0.0 { -1.0 } else { 1.0 };
    let fy = if cell_num(node, "FlipY").unwrap_or(0.0) != 0.0 { -1.0 } else { 1.0 };
    if fx > 0.0 && fy > 0.0 {
        return Some(plain);
    }
    let w_in = cell_num(node, "Width")?;
    let h_in = cell_num(node, "Height")?;
    let loc_x = cell_num(node, "LocPinX").unwrap_or(w_in / 2.0);
    let loc_y = cell_num(node, "LocPinY").unwrap_or(h_in / 2.0);
    let pin_x = cell_num(node, "PinX").unwrap_or(0.0);
    let pin_y = cell_num(node, "PinY").unwrap_or(0.0);
    let (a, b, c, d) = (plain.a * fx, plain.b * fx, plain.c * fy, plain.d * fy);
    Some(Transform { a, b, c, d, e: pin_x - (a * loc_x + c * loc_y), f: pin_y - (b * loc_x + d * loc_y) })
}

/// A copy of `node` with its `FlipX` toggled (a mirror inherited from a flipped group, see `transform_turn_and_mirror`).
/// A master-instance group whose master body holds a picture (`ForeignData`: the CyberPower / APC rack units are one EMF each) has no page
/// child for it, so nothing drew it and the shape came out blank. Give such a group a drawing-only child for each picture of its master
/// body, with the master's cells scaled to the instance's size (the master is authored at its own size).
fn with_master_pictures(nodes: &[ResolvedShape]) -> Vec<ResolvedShape> {
    nodes.iter().map(add_master_pictures).collect()
}

fn add_master_pictures(node: &ResolvedShape) -> ResolvedShape {
    let mut out = node.clone();
    out.children = node.children.iter().map(add_master_pictures).collect();
    if node.shape_type.as_deref() != Some("Group") || node.master_id.is_none() || node.master_children.is_empty() {
        return out;
    }
    let value = |cells: &std::collections::BTreeMap<String, crate::shapes::Cell>, name: &str| cells.get(name).and_then(|c| c.value.as_deref()).and_then(|v| v.parse::<f64>().ok());
    let scale = |instance: Option<f64>, master: Option<f64>| match (instance, master) {
        (Some(i), Some(m)) if i > 1e-9 && m > 1e-9 => i / m,
        _ => 1.0,
    };
    let (sx, sy) = (scale(value(&node.cells, "Width"), node.master_width), scale(value(&node.cells, "Height"), node.master_height));
    // Where the origin of a picture's parent group lies in the master top group's frame: the sum of (PinX - LocPinX, PinY - LocPinY) of the
    // groups between the picture and the top group (CyberPower: the picture layers are groups inside the group). `None` when one of them is
    // hidden, turned or mirrored (not handled, the picture is then left out as before).
    let origin_in_top = |child: &crate::shapes::MasterShape| -> Option<(f64, f64)> {
        let (mut ox, mut oy) = (0.0, 0.0);
        let mut parent = child.parent.clone();
        let mut guard = 0;
        while parent != node.master_top_id {
            guard += 1;
            let id = parent.as_deref()?;
            let group = node.master_children.iter().find(|s| s.id == id)?;
            if guard > 32
                || group.shape_type.as_deref() != Some("Group")
                || value(&group.cells, "NoShow").is_some_and(|v| v != 0.0)
                || value(&group.cells, "Angle").is_some_and(|v| v.abs() > 1e-9)
                || value(&group.cells, "FlipX").is_some_and(|v| v != 0.0)
                || value(&group.cells, "FlipY").is_some_and(|v| v != 0.0)
            {
                return None;
            }
            ox += value(&group.cells, "PinX")? - value(&group.cells, "LocPinX")?;
            oy += value(&group.cells, "PinY")? - value(&group.cells, "LocPinY")?;
            parent = group.parent.clone();
        }
        Some((ox, oy))
    };
    // The master shapes the page already carries as its own shapes, at any depth (a resized instance has its sub-shapes written out).
    let mut written_out: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut pending: Vec<&ResolvedShape> = node.children.iter().collect();
    while let Some(c) = pending.pop() {
        if let Some(id) = c.master_shape_id.as_deref() {
            written_out.insert(id);
        }
        pending.extend(c.children.iter());
    }
    for child in &node.master_children {
        if child.foreign.is_none()
            || value(&child.cells, "NoShow").is_some_and(|v| v != 0.0)
            || value(&child.cells, "NonPrinting").is_some_and(|v| v != 0.0)
            || written_out.contains(child.id.as_str())
        {
            continue;
        }
        let Some((ox, oy)) = origin_in_top(child) else { continue };
        let mut cells = child.cells.clone();
        for (name, k, shift) in [("PinX", sx, ox), ("PinY", sy, oy), ("LocPinX", sx, 0.0), ("LocPinY", sy, 0.0), ("Width", sx, 0.0), ("Height", sy, 0.0), ("ImgWidth", sx, 0.0), ("ImgOffsetX", sx, 0.0), ("ImgHeight", sy, 0.0), ("ImgOffsetY", sy, 0.0)] {
            if let Some(v) = value(&child.cells, name) {
                cells.insert(name.to_string(), crate::shapes::Cell { value: Some(format!("{}", (v + shift) * k)), formula: None, unit: None });
            }
        }
        out.children.push(ResolvedShape {
            id: format!("{}:m{}", node.id, child.id),
            name: child.name.clone(),
            shape_type: Some("Foreign".to_string()),
            master_shape_id: Some(child.id.clone()),
            master_part: node.master_part.clone(),
            fill_style: child.fill_style.clone(),
            line_style: child.line_style.clone(),
            text_style: child.text_style.clone(),
            cells,
            sections: child.sections.clone(),
            foreign: child.foreign.clone(),
            parent: Some(node.id.clone()),
            depth: node.depth + 1,
            ..Default::default()
        });
    }
    out
}

fn with_toggled_flip_x(node: &ResolvedShape) -> ResolvedShape {
    let mut copy = node.clone();
    let now = cell_num(node, "FlipX").unwrap_or(0.0) != 0.0;
    let mut cell = copy.cells.get("FlipX").cloned().unwrap_or_default();
    cell.value = Some(if now { "0" } else { "1" }.to_string());
    cell.formula = None;
    copy.cells.insert("FlipX".to_string(), cell);
    copy
}

/// A shape with no width or height whose geometry is drawn in absolute inches around its pin (the
/// EEPATH Junction dot: a 0.05 in circle on a 0 x 0 shape).
fn is_point_shape(node: &ResolvedShape) -> bool {
    cell_num(node, "Width") == Some(0.0)
        && cell_num(node, "Height") == Some(0.0)
        && node.shape_type.as_deref() != Some("Group")
        && node.foreign.is_none()
        && node.sections.iter().any(|section| section.name == "Geometry")
}

fn shape_transform(node: &ResolvedShape) -> Option<Transform> {
    let w_in = cell_num(node, "Width")?;
    let h_in = cell_num(node, "Height")?;
    // A group may be zero wide (a Gantt milestone is a point on the timeline); its children
    // are placed in absolute local inches.
    let zero_width_group = w_in == 0.0 && node.shape_type.as_deref() == Some("Group");
    if (w_in <= 0.0 && !zero_width_group && !is_point_shape(node)) || h_in < 0.0 {
        return None;
    }
    let pin_x = cell_num(node, "PinX").unwrap_or(0.0);
    let pin_y = cell_num(node, "PinY").unwrap_or(0.0);
    let loc_x = cell_num(node, "LocPinX").unwrap_or(w_in / 2.0);
    let loc_y = cell_num(node, "LocPinY").unwrap_or(h_in / 2.0);
    let angle = cell_num(node, "Angle").unwrap_or(0.0);
    let cos = angle.cos();
    let sin = angle.sin();
    Some(Transform {
        a: cos,
        b: sin,
        c: -sin,
        d: cos,
        e: pin_x - (cos * loc_x - sin * loc_y),
        f: pin_y - (sin * loc_x + cos * loc_y),
    })
}

/// The group's own geometry as one shape (a master-backed container, or a plain page group that owns geometry).
#[allow(clippy::too_many_arguments)]
fn own_group_body(
    node: &ResolvedShape,
    parent: &Transform,
    page_height: f64,
    stack: &[String],
    z: i64,
    styles: &DocumentStyles,
    theme: &Theme,
    container: Option<&ResolvedShape>,
    self_mirrored: bool,
    self_angle: f64,
) -> Option<Shape> {
    let w_in = cell_num(node, "Width").unwrap_or(0.0);
    let h_in = cell_num(node, "Height").unwrap_or(0.0);
    let loc_x = cell_num(node, "LocPinX").unwrap_or(w_in / 2.0);
    let loc_y = cell_num(node, "LocPinY").unwrap_or(h_in / 2.0);
    let angle = cell_num(node, "Angle").unwrap_or(0.0);
    let (flip_x, flip_y) = own_flip_signs(node);
    let (cdx, cdy) = ((w_in / 2.0 - loc_x) * flip_x, (h_in / 2.0 - loc_y) * flip_y);
    let (pin_x, pin_y) = (cell_num(node, "PinX").unwrap_or(0.0), cell_num(node, "PinY").unwrap_or(0.0));
    let center = (pin_x + cdx * angle.cos() - cdy * angle.sin(), pin_y + cdx * angle.sin() + cdy * angle.cos());
    let world_center = apply_transform(parent, center.0, center.1);
    let group = group_metadata(stack, transform_angle(parent) * 180.0 / PI);
    let mirrored_node;
    let body_node = if self_mirrored {
        mirrored_node = with_toggled_flip_x(node);
        &mirrored_node
    } else {
        node
    };
    convert_shape(
        body_node,
        &Placement {
            x: (world_center.0 - w_in / 2.0) * DPI,
            y: (page_height - world_center.1 - h_in / 2.0) * DPI,
            w: w_in * DPI,
            h: h_in * DPI,
            rot: -self_angle * 180.0 / PI,
        },
        z,
        group,
        styles,
        theme,
        container,
    )
}

#[allow(clippy::too_many_arguments)]
fn flatten_node(
    node: &ResolvedShape,
    parent: Transform,
    page_height: f64,
    stack: &mut Vec<String>,
    shapes: &mut Vec<Shape>,
    z: &mut i64,
    styles: &DocumentStyles,
    theme: &Theme,
    container: Option<&ResolvedShape>,
    include_inherited: bool,
) {
    // Ruler guides are page furniture (see `collect_guides`), not shapes.
    if node.shape_type.as_deref() == Some("Guide") {
        return;
    }
    let Some(local) = shape_transform(node) else {
        return;
    };
    let self_transform = compose(&parent, &local);
    // What the members see: this shape's frame including its own mirror.
    let children_transform = match shape_transform_with_flips(node) {
        Some(with_flips) => compose(&parent, &with_flips),
        None => self_transform,
    };
    let (self_angle, self_mirrored) = transform_turn_and_mirror(&self_transform);
    let is_group = node.shape_type.as_deref() == Some("Group");
    let recurse_group = is_group && (node.master_id.is_none() || is_container_group(node) || made_beside_a_guide(node));
    if recurse_group {
        // A master-backed container (a UML fragment) paints its own outline under its children. A plain page group that owns
        // geometry is drawn too (Visio, probed in step 219 with make-displaymode.mjs), by its `DisplayMode` cell: 0 = its own geometry
        // is not drawn, 1 = behind the members, 2 or no cell = in front of them. Groups without geometry draw nothing themselves.
        let w_in = cell_num(node, "Width").unwrap_or(0.0);
        let h_in = cell_num(node, "Height").unwrap_or(0.0);
        let mut drew_body = false;
        let display_mode = cell_num(node, "DisplayMode");
        let has_geometry = w_in > 0.0 && h_in > 0.0 && node.sections.iter().any(|section| section.name == "Geometry");
        let plain_body = node.master_id.is_none() && has_geometry && display_mode != Some(0.0);
        let body_behind = node.master_id.is_some() || display_mode == Some(1.0);
        if (node.master_id.is_some() && has_geometry) || (plain_body && body_behind) {
            if let Some(shape) = own_group_body(node, &parent, page_height, stack, *z, styles, theme, container, self_mirrored, self_angle) {
                shapes.push(shape);
                *z += 1;
                drew_body = true;
            }
        }
        stack.push(node.id.clone());
        let next_container = if is_visual_container(node) {
            Some(node)
        } else {
            container
        };
        for child in &node.children {
            // A member written `Del='1'` (with the ID -1) is one the instance took out of its master.
            if made_beside_a_guide(node) && child.id == "4294967295" {
                continue;
            }
            // The body drawn above already holds the master's own sub-shapes; only members that
            // are not part of the master (real container contents) are flattened on top.
            if drew_body && child.master_shape_id.is_some() && !has_foreign(child) {
                continue;
            }
            flatten_node(
                child,
                children_transform,
                page_height,
                stack,
                shapes,
                z,
                styles,
                theme,
                next_container,
                include_inherited,
            );
        }
        stack.pop();
        if plain_body && !body_behind {
            if let Some(shape) = own_group_body(node, &parent, page_height, stack, *z, styles, theme, container, self_mirrored, self_angle) {
                shapes.push(shape);
                *z += 1;
            }
        }
        return;
    }
    if is_connector(node) {
        return;
    }
    let w_in = cell_num(node, "Width").unwrap_or(0.0);
    let h_in = cell_num(node, "Height").unwrap_or(0.0);
    let zero_width_group = w_in == 0.0 && node.shape_type.as_deref() == Some("Group");
    if (w_in <= 0.0 && !zero_width_group && !is_point_shape(node)) || h_in < 0.0 {
        return;
    }
    let loc_x = cell_num(node, "LocPinX").unwrap_or(w_in / 2.0);
    let loc_y = cell_num(node, "LocPinY").unwrap_or(h_in / 2.0);
    let angle = cell_num(node, "Angle").unwrap_or(0.0);
    let cos = angle.cos();
    let sin = angle.sin();
    // A flip mirrors the shape about its pin before the turn (Visio, probed with XYToPage on a FlipX shape whose pin is at 25 % of its width:
    // its box then lies on the other side of the pin): the box centre is on the other side of the pin on the flipped axis.
    let (flip_x, flip_y) = own_flip_signs(node);
    let cdx = (w_in / 2.0 - loc_x) * flip_x;
    let cdy = (h_in / 2.0 - loc_y) * flip_y;
    let pin_x = cell_num(node, "PinX").unwrap_or(0.0);
    let pin_y = cell_num(node, "PinY").unwrap_or(0.0);
    let center = (pin_x + cdx * cos - cdy * sin, pin_y + cdx * sin + cdy * cos);
    let world_center = apply_transform(&parent, center.0, center.1);
    let group_rot = transform_angle(&parent) * 180.0 / PI;
    let group = group_metadata(stack, group_rot);
    // A group that owns no geometry, no text and no picture draws nothing in Visio; its
    // children are flattened below. (It used to come out as a themed rectangle.)
    // Master children that are pictures or empty sub-groups draw nothing of their own (the
    // pictures are flattened separately below).
    let draws_nothing = is_group
        && node.master_children.iter().all(|child| {
            child.foreign.is_some()
                || (child.shape_type.as_deref() == Some("Group")
                    && !child.has_text
                    && !child.sections.iter().any(|section| section.name == "Geometry"))
        })
        && node.foreign.is_none()
        && !node.has_text
        && !node.sections.iter().any(|section| section.name == "Geometry");
    let mirrored_leaf;
    let leaf_node = if self_mirrored {
        mirrored_leaf = with_toggled_flip_x(node);
        &mirrored_leaf
    } else {
        node
    };
    let shape = if draws_nothing {
        None
    } else {
        convert_shape(
        leaf_node,
        &Placement {
            x: (world_center.0 - w_in / 2.0) * DPI,
            y: (page_height - world_center.1 - h_in / 2.0) * DPI,
            w: w_in * DPI,
            h: h_in * DPI,
            rot: -self_angle * 180.0 / PI,
        },
        *z,
        group,
        styles,
        theme,
        container,
    )
    };
    let shape = shape.map(|mut shape| {
        // A group with no geometry of its own has nothing to paint: keep its text and
        // pictures, but not the themed fallback rectangle.
        if is_group
            && shape.parts.is_empty()
            && node.foreign.is_none()
            && !node.sections.iter().any(|section| section.name == "Geometry")
        {
            shape.fill = Some("none".to_string());
            shape.stroke = Some("none".to_string());
            shape.stroke_width = Some(0.0);
        }
        shape
    });
    // Parts of master children that come after a picture in the master's own order are drawn over
    // that picture (the translucent name plate of an org-chart panel), so they become a second
    // shape placed after the picture.
    let mut late_body: Option<Shape> = None;
    let shape = shape.map(|mut shape| {
        let picture_at = node
            .master_children
            .iter()
            .find(|child| child.foreign.is_some())
            .map(|child| child.order);
        if let (true, Some(picture_at)) = (is_group, picture_at) {
            let rank = |part: &ShapePart| {
                part.source.as_ref().and_then(|source| {
                    node.master_children
                        .iter()
                        .find(|child| child.id == source.master_shape_id)
                        .map(|child| child.order)
                })
            };
            let is_late = |part: &ShapePart| rank(part).is_some_and(|at| at > picture_at);
            if shape.parts.iter().any(is_late) && shape.parts.iter().any(|part| !is_late(part)) {
                let mut late = shape.clone();
                late.parts.retain(is_late);
                late.label.clear();
                late.sub_texts.clear();
                late.visio_id = None;
                shape.parts.retain(|part| !is_late(part));
                late_body = Some(late);
            }
        }
        shape
    });
    if let Some(shape) = shape {
        shapes.push(shape);
        *z += 1;
    }
    // Master-backed groups draw their body and normally suppress their page
    // children, but picture children (`ForeignData`) are real standalone
    // content (org-chart photos). Flatten those on top of the group body.
    if is_group {
        for child in &node.children {
            if has_foreign(child)
                || child.master_explicit
                || (include_inherited && child.master_id.is_some())
            {
                // A picture the master supplies and the page writes out (a resized instance): a click on it belongs to the group.
                let member = has_foreign(child) && child.master_shape_id.is_some() && stack.is_empty();
                if member {
                    stack.push(node.id.clone());
                }
                flatten_node(
                    child,
                    self_transform,
                    page_height,
                    stack,
                    shapes,
                    z,
                    styles,
                    theme,
                    container,
                    include_inherited,
                );
                if member {
                    stack.pop();
                }
            }
        }
    }
    if let Some(mut late) = late_body {
        late.z = Some(*z as f64);
        *z += 1;
        shapes.push(late);
    }
}

/// True when the shape's `ThemeIndex` names a built-in theme (not 0 and not 65534, "none").
/// The theme's text colour, except for a stencil pinned to a built-in theme of its own, whose text is
/// plain black here.
fn themed_text_of(node: &ResolvedShape, theme: &Theme, has_fill: bool) -> Option<String> {
    if foreign_theme_index(node) {
        return Some("#000000".to_string());
    }
    theme.themed_text(has_fill)
}

pub(crate) fn foreign_theme_index(node: &ResolvedShape) -> bool {
    numeric_cell(find_chain(&node.cells, "ThemeIndex"))
        .or_else(|| node.master_top_user.get("ThemeIndex").copied())
        .is_some_and(|value| value != 0.0 && value != 65534.0)
}

fn has_foreign(node: &ResolvedShape) -> bool {
    node.foreign.is_some() || node.children.iter().any(has_foreign)
}

/// CFF/swimlane/pool/lane containers paint a white interior over their
/// background children; ordinary groups and UML entity lists do not.
fn is_visual_container(node: &ResolvedShape) -> bool {
    let categories = instance_user_value(node, "msvShapeCategories")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if [
        "cffcontainer",
        "cff container",
        "pool",
        "swimlane",
        "lane",
        "phase",
    ]
    .iter()
    .any(|marker| categories.contains(marker))
    {
        return true;
    }
    node.master_name.as_deref().is_some_and(|name| {
        let name = name.to_ascii_lowercase();
        name.contains("cff") || name.contains("swimlane") || name.contains("pool / lane")
    })
}

fn is_container_background(node: &ResolvedShape, container: &ResolvedShape) -> bool {
    if node.master_id.is_none() || node.master_id != container.master_id {
        return false;
    }
    let structure = instance_user_value(node, "msvStructureType")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if structure == "heading" {
        return false;
    }
    let categories = instance_user_value(node, "msvShapeCategories")
        .unwrap_or_default()
        .to_ascii_lowercase();
    !["dblistitem", "dbattribute", "task"]
        .iter()
        .any(|marker| categories.contains(marker))
}

/// A group made from a master of several shapes that lists a ruler guide first: it took no master shape (see `shapes.rs`),
/// so there is no master body to composite and its members are drawn.
fn made_beside_a_guide(node: &ResolvedShape) -> bool {
    node.master_explicit && node.master_children.is_empty() && node.children.iter().any(|child| child.shape_type.as_deref() == Some("Guide"))
}

fn is_container_group(node: &ResolvedShape) -> bool {
    let categories = instance_user_value(node, "msvShapeCategories")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let category_container = categories.split(';').any(|category| {
        matches!(
            category.trim(),
            "container"
                | "containerstyledefaults"
                | "cffcontainer"
                | "pool"
                | "swimlane"
                | "lane"
                | "phase"
        )
    });
    let has_container_meta = [
        "msvSDContainerStyle",
        "msvSDContainerLocked",
        "msvSDListItemMaster",
    ]
    .iter()
    .any(|name| instance_user_value(node, name).is_some_and(|value| !value.is_empty()));
    category_container || has_container_meta
}

fn group_metadata(stack: &[String], rot: f64) -> Option<Value> {
    let root = stack.first()?;
    let parent = stack.last()?;
    Some(json!({
        "id": parent,
        "parentId": parent,
        "rootId": root,
        "depth": stack.len(),
        "kind": "group",
        "rotated": rot.abs() > 1e-9,
    }))
}

struct Placement {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    rot: f64,
}

struct StyleContext {
    fill_styles: Vec<BTreeMap<String, Cell>>,
    line_styles: Vec<BTreeMap<String, Cell>>,
    text_styles: Vec<BTreeMap<String, Cell>>,
    paragraph_styles: Vec<BTreeMap<String, Cell>>,
    large: bool,
    connector: bool,
    /// Font of the document's root style, used for a `Themed` font when no theme part exists.
    root_font: Option<String>,
}

fn style_context(node: &ResolvedShape, styles: &DocumentStyles, large: bool) -> StyleContext {
    StyleContext {
        fill_styles: styles.fill_rows(node.fill_style.as_deref()),
        line_styles: styles.line_rows(node.line_style.as_deref()),
        text_styles: styles.text_rows(node.text_style.as_deref()),
        paragraph_styles: styles.paragraph_rows(node.text_style.as_deref()),
        large,
        connector: styles.is_connector_style(node.line_style.as_deref()),
        root_font: styles.root_font(),
    }
}

struct Paint {
    fill: String,
    stroke: String,
    stroke_width: f64,
    /// 1 - `LineColorTrans`, so a 100%-transparent line still carries its
    /// authored geometry but is invisible (ImageNode stencil backgrounds).
    stroke_opacity: f64,
    dash: Option<String>,
    /// Gradient supplied by the active QuickStyle fill matrix.
    fill_gradient: Option<Value>,
    fill_pattern: Option<layoutcraft_model::FillPattern>,
    fill_opacity: f64,
}

/// A shape that explicitly carries QuickStyle colour/matrix cells expects the
/// theme to supply them. If the package has no theme part, Visio leaves such a
/// shape at the neutral white/black default; style-only themed colours in
/// documents without a theme still use the Office fallback.
fn ignores_theme_colors(node: &ResolvedShape, theme: &Theme) -> bool {
    !theme.present
        && node.cells.keys().any(|name| {
            name.starts_with("QuickStyle") && (name.ends_with("Matrix") || name.ends_with("Color"))
        })
}

/// Evaluate a line-weight formula such as the UML separator's
/// `IF(THEMEPROP("Embellishment")=3,0.75PT,THEMEVAL())`. Theme properties and
/// the bare theme value are substituted before the arithmetic parser runs.
pub(crate) fn resolve_weight_formula(
    cell: &Cell,
    theme: &Theme,
    matrix: Option<f64>,
) -> Option<f64> {
    let formula = cell
        .formula
        .as_deref()
        .filter(|formula| *formula != "Inh" && *formula != "No Formula" && !formula.is_empty())?;
    let mut text = formula.to_string();
    for name in ["Embellishment", "MultiFormat"] {
        if let Some(value) = theme.property(name) {
            for quoted in [
                format!("THEMEPROP(\"{name}\")"),
                format!("THEMEPROP('{name}')"),
            ] {
                text = text.replace(&quoted, &value.to_string());
            }
        }
    }
    // `THEMEVAL("LineWeight", 0.24 pt)`: the second argument is what applies when the shape has
    // no quick style (matrix 100 or absent) - COM-verified on the Process Engineering valves.
    if matrix.is_none_or(|value| value == 100.0) {
        const CALL: &str = "THEMEVAL(\"LineWeight\",";
        if let Some(start) = text.find(CALL) {
            let rest = &text[start + CALL.len()..];
            if let Some(end) = rest.find(')') {
                if let Some(value) = formula::evaluate(rest[..end].trim(), &mut |_| None) {
                    let replaced = format!("{}{}IN{}", &text[..start], value, &rest[end + 1..]);
                    text = replaced;
                }
            }
        }
    }
    if text.contains("THEMEVAL()") {
        let themed = theme.themed_line_weight_pt(matrix);
        text = text.replace("THEMEVAL()", &format!("{themed}PT"));
    }
    if text.contains("THEMEPROP") || text.contains("THEMEVAL") {
        return None;
    }
    formula::evaluate(&text, &mut |_| None)
}

fn resolve_style(node: &ResolvedShape, ctx: &StyleContext, theme: &Theme) -> Paint {
    let neutral_theme = ignores_theme_colors(node, theme);
    let line_like = numeric_cell(node.cells.get("Height"))
        .is_some_and(|height| height.abs() < 1e-9)
        && numeric_cell(node.cells.get("Width")).is_some_and(|width| width > 1e-9);
    let connector = ctx.connector;
    let own_fill = find_chain(&node.cells, "FillForegnd");
    let fill_from_style = own_fill.is_none();
    let fill_cell = own_fill.or_else(|| find_extra("FillForegnd", &[ctx.fill_styles.as_slice()]));
    let bkgnd_cell = pick_with(&node.cells, "FillBkgnd", &[&ctx.fill_styles]);
    let pattern = pick_with(&node.cells, "FillPattern", &[&ctx.fill_styles]);
    let line_color = pick_with(&node.cells, "LineColor", &[&ctx.line_styles]);
    let line_weight = pick_with(&node.cells, "LineWeight", &[&ctx.line_styles]);
    let line_pattern = pick_with(&node.cells, "LinePattern", &[&ctx.line_styles]);
    let line_cap = pick_with(&node.cells, "LineCap", &[&ctx.line_styles]).and_then(|cell| numeric_cell(Some(cell)));
    let line_transparency = pick_with(&node.cells, "LineColorTrans", &[&ctx.line_styles])
        .and_then(|cell| numeric_cell(Some(cell)))
        .unwrap_or(0.0);
    let quick_fill = if neutral_theme {
        None
    } else {
        numeric_cell(find_chain(&node.cells, "QuickStyleFillColor").or_else(|| {
            find_extra(
                "QuickStyleFillColor",
                &[ctx.fill_styles.as_slice(), ctx.line_styles.as_slice()],
            )
        }))
    };
    let quick_line = if neutral_theme {
        None
    } else {
        numeric_cell(find_chain(&node.cells, "QuickStyleLineColor").or_else(|| {
            find_extra(
                "QuickStyleLineColor",
                &[ctx.line_styles.as_slice(), ctx.fill_styles.as_slice()],
            )
        }))
    };
    let quick_fill_matrix = if neutral_theme {
        None
    } else {
        numeric_cell(find_chain(&node.cells, "QuickStyleFillMatrix").or_else(|| {
            find_extra(
                "QuickStyleFillMatrix",
                &[ctx.fill_styles.as_slice(), ctx.line_styles.as_slice()],
            )
        }))
    };
    let quick_line_matrix = if neutral_theme {
        None
    } else {
        numeric_cell(find_chain(&node.cells, "QuickStyleLineMatrix").or_else(|| {
            find_extra(
                "QuickStyleLineMatrix",
                &[ctx.line_styles.as_slice(), ctx.fill_styles.as_slice()],
            )
        }))
    };
    let quick_fill_paint = theme.quick_fill_paint(
        quick_fill_matrix,
        quick_fill.filter(|value| *value != 100.0),
    );
    let variant_line = numeric_cell(find_chain(&node.cells, "QuickStyleVariation"))
        .is_some_and(|value| (value as i64) & 4 != 0);
    let quick_line_paint = if connector {
        None
    } else {
        quick_line.and_then(|color| {
            theme.quick_line_paint(
                quick_line_matrix,
                Some(color).filter(|value| *value != 100.0 && *value != 0.0),
                variant_line,
            )
        })
    };
    let mut fill = fill_cell
        .filter(|cell| is_concrete(cell))
        .and_then(|cell| theme.color_of(cell));
    if !theme.present && !neutral_theme {
        // Without a theme part, indexed colours cached in the style cell are
        // authoritative (floor-plan styles use THEMEVAL("FillColor", 9)).
        // Do not let the Office fallback inside the formula win over V=9.
        if let Some(color) = fill_cell.and_then(|cell| cached_cell_color(theme, cell)) {
            fill = Some(color);
        }
    }
    let quick_fill = quick_fill.filter(|value| *value != 100.0);
    let bare_themed_fill = fill_cell.is_some_and(is_bare_themed);
    let mut fill_gradient = None;
    if fill.is_none() && (!bare_themed_fill || quick_fill_paint.is_some()) {
        if let Some(quick) = &quick_fill_paint {
            fill = Some(if ctx.large {
                tint(&quick.color, 85.0)
            } else {
                quick.color.clone()
            });
            fill_gradient = quick.gradient.clone();
        } else if let Some(quick) = quick_fill.and_then(|value| theme.quick_color(Some(value))) {
            fill = Some(if ctx.large { tint(&quick, 85.0) } else { quick });
        }
    }
    let fill_role = if fill_from_style {
        "fill_style"
    } else {
        "fill"
    };
    if fill.is_none() && !neutral_theme {
        fill = theme.themed_color(fill_cell, fill_role);
    }
    if fill.is_none() && !neutral_theme {
        fill = theme.themed_color(bkgnd_cell, fill_role);
    }
    let fill = fill.unwrap_or_else(|| "#ffffff".to_string());

    let mut stroke = line_color
        .filter(|cell| is_concrete(cell))
        .and_then(|cell| theme.color_of(cell));
    if !theme.present && !neutral_theme {
        if let Some(color) = line_color.and_then(|cell| cached_cell_color(theme, cell)) {
            stroke = Some(color);
        }
    }
    // A legacy shape (Process Engineering, Electrical) has QuickStyleLineColor/Matrix 100, i.e.
    // no quick style, and only the bare `THEMEVAL()` of its line style; Visio then draws the
    // theme's dark colour, not the accent the quick-style lookup would give (COM-verified on
    // PEPUMP: LineColor evaluates to 0, black, in an Office theme whose accent3 is #a5a5a5).
    let legacy_themed = !connector
        && !line_like
        && quick_line.is_some_and(|value| (100.0..=106.0).contains(&value))
        && quick_line_matrix.is_none_or(|value| (100.0..=106.0).contains(&value))
        && line_color.is_some_and(is_bare_themed)
        // A document without a theme part: THEMEVAL() has nothing to read (the theme-driven
        // Bank Account file draws these outlines white, so this must not apply there). The same
        // goes for a shape that names a built-in theme of its own (`ThemeIndex` 37 on the UML
        // stencil masters): it is not drawn with this document's theme.
        && (!theme.present || foreign_theme_index(node));
    if legacy_themed {
        // The line style's own colour (0 = black).
        if let Some(color) = style_cell(&ctx.line_styles, "LineColor").and_then(|cell| cached_cell_color(theme, cell)) {
            stroke = Some(color);
        }
    }
    let quick_line = quick_line.filter(|value| *value != 100.0 && *value != 0.0);
    if stroke.is_none() {
        // A zero-height QuickStyleType-1 line with the default
        // QuickStyleLineColor (100) has no colour of its own: the matrix entry
        // may hold literal white, so use the connector-scheme colour below
        // (UML list separators). Plain lines without QuickStyleType, such as
        // CFF swimlane separators, are really white in Visio.
        let quick_style_type = numeric_cell(find_chain(&node.cells, "QuickStyleType"));
        let default_line_colour =
            line_like && quick_line.is_none() && quick_style_type == Some(1.0);
        stroke = quick_line_paint
            .as_ref()
            .filter(|_| !default_line_colour)
            .map(|quick| quick.color.clone())
            .or_else(|| quick_line.and_then(|value| theme.quick_color(Some(value))));
    }
    if stroke.is_none() && !neutral_theme {
        stroke = if line_like {
            theme.themed_line_connector_shaded()
        } else if connector {
            theme.themed_line_connector()
        } else {
            theme.themed_color(line_color, "line")
        };
    }
    let stroke = stroke.unwrap_or_else(|| "#000000".to_string());

    // The same shapes take their line style's weight (probed: 0.75 pt on PEPUMP and NETSME, 0.24 pt on PEVALV).
    let bare_weight = legacy_themed
        && line_weight.is_some_and(|cell| cell.formula.as_deref() == Some("THEMEVAL()"));
    let weight = line_weight
        .filter(|_| !bare_weight)
        .map_or_else(
        || {
            if bare_weight {
                // The line style's own weight (0.75 pt in Normal, 0.24 pt in the valve styles).
                let style_weight = style_cell(&ctx.line_styles, "LineWeight");
                // A style weight that is itself `THEMEVAL("LineWeight",default)` (the block styles of
                // CYCLEDIAG) is drawn with the theme's line weight (0.75 pt), not the default.
                if theme.present
                    && style_weight.is_some_and(|cell| {
                        cell.formula.as_deref().is_some_and(|f| f.starts_with("THEMEVAL(\"LineWeight\""))
                    })
                {
                    return theme.themed_line_weight_pt(None) / 72.0;
                }
                return style_weight
                    .and_then(|cell| numeric_cell(Some(cell)))
                    .unwrap_or(0.75 / 72.0);
            }
            if connector {
                return theme.connector_line_weight_pt() / 72.0;
            }
            let matrix = numeric_cell(find_chain(&node.cells, "QuickStyleLineMatrix"));
            theme.themed_line_weight_pt(matrix) / 72.0
        },
        |cell| {
            let matrix = numeric_cell(find_chain(&node.cells, "QuickStyleLineMatrix"));
            numeric_cell(Some(cell))
                .or_else(|| resolve_weight_formula(cell, theme, matrix))
                .unwrap_or(0.01)
        },
    );
    let mut stroke_width = ((weight * DPI * 100.0).round() / 100.0).max(0.2);
    let mut dash = quick_line_paint
        .as_ref()
        .and_then(|quick| quick.dash.clone());
    if let Some(pattern) = numeric_cell(line_pattern) {
        if pattern == 0.0 {
            stroke_width = 0.0;
        } else if (2.0..=23.0).contains(&pattern) && pattern.fract() == 0.0 {
            dash = Some(format!("pat{}", pattern as i64));
        }
    }
    dash = with_line_cap(dash, line_cap);
    let no_fill = numeric_cell(pattern).is_some_and(|value| value == 0.0);
    let stroke_opacity = (1.0 - line_transparency).clamp(0.0, 1.0);
    if stroke_opacity <= 0.0 {
        stroke_width = 0.0;
    }
    let fill_transparency = pick_with(
        &node.cells,
        "FillForegndTrans",
        &[ctx.fill_styles.as_slice()],
    )
    .and_then(|cell| numeric_cell(Some(cell)))
    .unwrap_or(0.0);
    let fill_opacity = (1.0 - fill_transparency).clamp(0.0, 1.0);
    let pattern_kind = numeric_cell(pattern).unwrap_or(1.0).round() as u32;
    let mut fill_gradient = fill_gradient;
    if !no_fill && fill_gradient.is_none() && (25..=40).contains(&pattern_kind) {
        let back = bkgnd_cell
            .and_then(|cell| {
                if theme.present {
                    theme.color_of(cell)
                } else {
                    cached_cell_color(theme, cell)
                }
            })
            .unwrap_or_else(|| "#ffffff".to_string());
        fill_gradient = Some(legacy_pattern_gradient(pattern_kind, &fill, &back));
    }
    let fill_pattern = if !no_fill && (2..=24).contains(&pattern_kind) {
        Some(layoutcraft_model::FillPattern {
            kind: pattern_kind,
            fore: fill.clone(),
            back: bkgnd_cell
                .and_then(|cell| {
                    if theme.present {
                        theme.color_of(cell)
                    } else {
                        cached_cell_color(theme, cell)
                    }
                })
                .unwrap_or_else(|| "#ffffff".to_string()),
        })
    } else {
        None
    };

    Paint {
        fill: if no_fill { "none".to_string() } else { fill },
        stroke,
        stroke_width,
        stroke_opacity,
        dash,
        fill_gradient,
        fill_pattern,
        fill_opacity,
    }
}

struct TextPaint {
    font_size: f64,
    font_family: String,
    text_color: String,
    bold: bool,
    italic: bool,
    underline: bool,
    /// `Char.Pos`: 1 superscript, 2 subscript, 0 normal.
    script: u8,
}

#[derive(Debug, Clone)]
struct ParagraphStyle {
    align: String,
    bullet: bool,
    bullet_str: String,
    indent: f64,
    /// `SpLine`: a negative number is a multiple of the font size (0 = no value: automatic), a positive one an exact distance in pixels.
    line_spacing: f64,
    /// `SpBefore` / `SpAfter` in pixels.
    space_before: f64,
    space_after: f64,
    /// `IndFirst` in pixels (negative = hanging).
    indent_first: f64,
}

impl ParagraphStyle {
    fn to_json(&self) -> Value {
        json!({
            "align": self.align,
            "bullet": self.bullet,
            "bulletStr": self.bullet_str,
            "indent": self.indent,
        })
    }
}

#[derive(Default)]
struct ParagraphAccum {
    pp: String,
    runs: Vec<Value>,
}

fn paragraph_has_content(paragraph: &ParagraphAccum) -> bool {
    paragraph.runs.iter().any(|run| {
        run.get("field").is_some()
            || run
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.is_empty())
    })
}

fn start_paragraph(paragraphs: &mut Vec<ParagraphAccum>, current: &mut ParagraphAccum, pp: &str) {
    if paragraph_has_content(current) {
        let finished = std::mem::replace(
            current,
            ParagraphAccum {
                pp: pp.to_string(),
                runs: Vec::new(),
            },
        );
        paragraphs.push(finished);
    } else {
        current.pp = pp.to_string();
    }
}

fn section_row<'a>(sections: &'a [Section], name: &str, ix: &str) -> Option<&'a crate::SectionRow> {
    sections
        .iter()
        .find(|section| section.name == name)?
        .rows
        .iter()
        .find(|row| row.index.as_deref().unwrap_or("0") == ix)
}

fn first_section_row<'a>(sections: &'a [Section], name: &str) -> Option<&'a crate::SectionRow> {
    sections
        .iter()
        .find(|section| section.name == name)?
        .rows
        .first()
}

fn paragraph_style(
    node: &ResolvedShape,
    style_rows: &[BTreeMap<String, Cell>],
    ix: &str,
    fallback_align: Option<&str>,
) -> ParagraphStyle {
    let mut rows: Vec<&BTreeMap<String, Cell>> = Vec::new();
    for sections in [&node.own_sections, &node.master_sections] {
        if let Some(row) = section_row(sections, "Paragraph", ix) {
            rows.push(&row.cells);
        }
    }
    if ix != "0" {
        for sections in [&node.own_sections, &node.master_sections] {
            if let Some(row) = section_row(sections, "Paragraph", "0") {
                rows.push(&row.cells);
            }
        }
    }
    // Whatever the shape does not set comes from its text style (a left-aligned body text
    // style is why such paragraphs must not default to centred).
    rows.extend(style_rows.iter());
    let pick = |name: &str| -> Option<&Cell> { rows.iter().find_map(|cells| cells.get(name)) };
    let align = pick("HorzAlign")
        .and_then(|cell| numeric_cell(Some(cell)))
        .map(|value| {
            let value = value.round() as i64;
            if value == 0 {
                "left"
            } else if value == 2 {
                "right"
            } else {
                "center"
            }
        })
        .unwrap_or_else(|| fallback_align.unwrap_or("center"))
        .to_string();
    let bullet = pick("Bullet")
        .and_then(|cell| numeric_cell(Some(cell)))
        .is_some_and(|value| value.round() != 0.0);
    let bullet_str = pick("BulletStr")
        .and_then(|cell| cell.value.as_deref())
        .map(strip_quotes)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "•".to_string());
    let indent = pick("IndLeft")
        .and_then(|cell| numeric_cell(Some(cell)))
        .map(|value| (value * DPI).max(0.0))
        .unwrap_or(0.0);
    let number = |name: &str| pick(name).and_then(|cell| numeric_cell(Some(cell)));
    let line_spacing = number("SpLine").map_or(0.0, |value| if value > 0.0 { value * DPI } else if value < 0.0 { value } else { 0.0 });
    ParagraphStyle {
        align,
        bullet,
        bullet_str,
        indent,
        line_spacing,
        space_before: number("SpBefore").map_or(0.0, |value| (value * DPI).max(0.0)),
        space_after: number("SpAfter").map_or(0.0, |value| (value * DPI).max(0.0)),
        indent_first: number("IndFirst").map_or(0.0, |value| value * DPI),
    }
}

fn paragraph_align(node: &ResolvedShape, style_rows: &[BTreeMap<String, Cell>]) -> Option<String> {
    let from_style = || {
        let value = style_rows
            .iter()
            .find_map(|cells| cells.get("HorzAlign"))
            .and_then(|cell| numeric_cell(Some(cell)))?
            .round() as i64;
        Some(
            if value == 0 {
                "left"
            } else if value == 2 {
                "right"
            } else {
                "center"
            }
            .to_string(),
        )
    };
    for sections in [&node.own_sections, &node.master_sections] {
        let Some(row) = section_row(sections, "Paragraph", "0")
            .or_else(|| first_section_row(sections, "Paragraph"))
        else {
            continue;
        };
        let Some(cell) = row.cells.get("HorzAlign") else {
            continue;
        };
        let value = numeric_cell(Some(cell)).unwrap_or(1.0).round() as i64;
        return Some(
            if value == 0 {
                "left"
            } else if value == 2 {
                "right"
            } else {
                "center"
            }
            .to_string(),
        );
    }
    from_style()
}

fn vertical_align(node: &ResolvedShape) -> Option<String> {
    let value = numeric_cell(node.cells.get("VerticalAlign"))?.round() as i64;
    Some(
        if value == 0 {
            "top"
        } else if value == 2 {
            "bottom"
        } else {
            "middle"
        }
        .to_string(),
    )
}

fn text_pin_outside(node: &ResolvedShape) -> bool {
    let width = numeric_cell(node.cells.get("Width")).unwrap_or(0.0);
    let height = numeric_cell(node.cells.get("Height")).unwrap_or(0.0);
    let pin_x = numeric_cell(node.cells.get("TxtPinX"));
    let pin_y = numeric_cell(node.cells.get("TxtPinY"));
    pin_x.is_some_and(|x| x < 0.0 || (width > 0.0 && x > width))
        || pin_y.is_some_and(|y| y < 0.0 || (height > 0.0 && y > height))
}

fn text_block(node: &ResolvedShape, styles: &DocumentStyles, w_px: f64, h_px: f64) -> Value {
    let w = node.cells.get("Width").and_then(|cell| {
        cell.value
            .as_deref()
            .and_then(|value| value.parse::<f64>().ok())
    });
    let h = node.cells.get("Height").and_then(|cell| {
        cell.value
            .as_deref()
            .and_then(|value| value.parse::<f64>().ok())
    });
    let w_in = w.unwrap_or(w_px / DPI);
    let h_in = h.unwrap_or(h_px / DPI);
    // A shape that sets no margin takes the one of its TextStyle chain (the "Normal" style gives
    // 0.0556 in), which is why Visio wraps text narrower than the shape's own text box.
    let margin = |name: &str| {
        node.cells
            .get(name)
            .and_then(|cell| cell.value.as_deref())
            .and_then(|value| value.parse::<f64>().ok())
            .or_else(|| styles.text_style_number(node.text_style.as_deref(), name))
            .unwrap_or(0.0)
    };
    let left = margin("LeftMargin");
    let right = margin("RightMargin");
    let top = margin("TopMargin");
    let bottom = margin("BottomMargin");
    if !node.cells.contains_key("TxtWidth") || !node.cells.contains_key("TxtHeight") {
        return json!({
            "x": left * DPI,
            "y": top * DPI,
            "w": ((w_in - left - right) * DPI).max(4.0),
            "h": ((h_in - top - bottom) * DPI).max(4.0),
        });
    }
    let width = numeric_cell(node.cells.get("TxtWidth")).unwrap_or(w_in);
    let height = numeric_cell(node.cells.get("TxtHeight")).unwrap_or(h_in);
    let pin_x = numeric_cell(node.cells.get("TxtPinX")).unwrap_or(width / 2.0);
    let pin_y = numeric_cell(node.cells.get("TxtPinY")).unwrap_or(height / 2.0);
    let loc_x = numeric_cell(node.cells.get("TxtLocPinX")).unwrap_or(width / 2.0);
    let loc_y = numeric_cell(node.cells.get("TxtLocPinY")).unwrap_or(height / 2.0);
    let x = pin_x - loc_x + left;
    let y = pin_y - loc_y;
    let (bx, by) = (x * DPI, (h_in - (y + height)) * DPI + top * DPI);
    let (bw, bh) = (((width - left - right) * DPI).max(4.0), ((height - top - bottom) * DPI).max(4.0));
    // A flipped shape carries its text block with it (the text itself is not mirrored).
    let bx = if cell_num(node, "FlipX").unwrap_or(0.0) != 0.0 { w_in * DPI - (bx + bw) } else { bx };
    let by = if cell_num(node, "FlipY").unwrap_or(0.0) != 0.0 { h_in * DPI - (by + bh) } else { by };
    json!({ "x": bx, "y": by, "w": bw, "h": bh })
}

/// A `TxtWidth` formula that makes the block exactly as wide as its text (`TEXTWIDTH(TheText)`, `MAX(TEXTWIDTH(TheText),Width)`). A `MIN(...)` cap
/// (`MIN(Width*2.5,TEXTWIDTH(TheText))`) can make the block narrower than the text, and Visio then wraps it (its saved `TxtHeight` is two lines).
pub(crate) fn formula_fits_text_line(formula: &str) -> bool {
    let upper = formula.to_ascii_uppercase();
    upper.contains("TEXTWIDTH(") && !upper.contains("MIN(")
}

/// `TxtWidth` is `TEXTWIDTH(...)`, written on the shape or (a saved instance says only `Inh`) on its master shape.
fn text_width_fits_line(node: &ResolvedShape) -> bool {
    let own = node.cells.get("TxtWidth").and_then(|cell| cell.formula.as_deref());
    let formula = match own {
        Some("Inh") | None => node.master_text_formulas.get("TxtWidth").map(String::as_str),
        other => other,
    };
    formula.is_some_and(formula_fits_text_line)
}

fn text_fit(node: &ResolvedShape) -> Option<Value> {
    let has = |name: &str, function: &str| {
        node.cells
            .get(name)
            .and_then(|cell| cell.formula.as_deref())
            .is_some_and(|formula| formula.to_ascii_uppercase().contains(function))
    };
    let width = has("TxtWidth", "TEXTWIDTH(");
    let height = has("TxtHeight", "TEXTHEIGHT(");
    (width || height).then(|| json!({ "width": width, "height": height }))
}

fn run_style(
    node: &ResolvedShape,
    ctx: &StyleContext,
    theme: &Theme,
    ix: &str,
    has_fill: bool,
) -> Value {
    let mut rows: Vec<&BTreeMap<String, Cell>> = Vec::new();
    for sections in [&node.own_sections, &node.master_sections] {
        if let Some(row) = section_row(sections, "Character", ix) {
            rows.push(&row.cells);
        }
    }
    if ix != "0" {
        for sections in [&node.own_sections, &node.master_sections] {
            if let Some(row) = section_row(sections, "Character", "0") {
                rows.push(&row.cells);
            }
        }
    }
    let own_rows = rows.len();
    rows.extend(ctx.text_styles.iter());

    let pick_entry = |name: &str| -> Option<(&Cell, usize)> {
        for (index, cells) in rows.iter().enumerate() {
            if let Some(cell) = cells.get(name) {
                // A Character row on the shape itself caches the instance's
                // resolved colour (the container heading is grey even though
                // its master caches black). Other inherited character cells
                // defer to the style chain when their formula is inherited.
                if name == "Color" && index < own_rows {
                    return Some((cell, index));
                }
                if cell.formula.as_deref() != Some("Inh") {
                    return Some((cell, index));
                }
            }
        }
        for (index, cells) in rows.iter().enumerate() {
            if let Some(cell) = cells.get(name) {
                return Some((cell, index));
            }
        }
        None
    };
    let pick = |name: &str| pick_entry(name).map(|(cell, _)| cell);
    let mut out = Map::new();

    if let Some(size) = pick("Size").or_else(|| node.cells.get("Char.Size")) {
        if let Some(value) = numeric_cell(Some(size)).filter(|value| *value > 0.0) {
            out.insert(
                "fontSize".to_string(),
                json!(font_px(value)),
            );
        }
    }
    let color_pick = pick_entry("Color");
    let color_cell = color_pick
        .map(|(cell, _)| cell)
        .or_else(|| node.cells.get("Char.Color"));
    let color = match color_cell {
        Some(cell) if is_bare_themed(cell) => themed_text_of(node, theme, has_fill),
        Some(cell) => character_color_expression(cell, node, theme, has_fill)
            .or_else(|| theme.themed_color(Some(cell), "text")),
        None => themed_text_of(node, theme, has_fill),
    };
    if let Some(color) = color {
        out.insert("textColor".to_string(), json!(color));
    }
    if let Some(style) = pick("Style").or_else(|| node.cells.get("Char.Style")) {
        let value = numeric_cell(Some(style)).unwrap_or(0.0).round() as u32;
        out.insert("bold".to_string(), json!(value & 1 != 0));
        out.insert("italic".to_string(), json!(value & 2 != 0));
        out.insert("underline".to_string(), json!(value & 4 != 0));
    }
    // Strike-through, case and letter spacing (inches) of the run: only written when they differ from the plain text.
    if pick("Strikethru").and_then(|cell| numeric_cell(Some(cell))).is_some_and(|value| value > 0.5) {
        out.insert("strike".to_string(), json!(true));
    }
    let script = script_position(pick("Pos"));
    if script != 0 {
        out.insert("scriptPos".to_string(), json!(script));
    }
    if let Some(case) = pick("Case").and_then(|cell| numeric_cell(Some(cell))).map(|value| value.round() as u32).filter(|case| *case == 1 || *case == 2) {
        out.insert("caps".to_string(), json!(case));
    }
    if let Some(spacing) = pick("Letterspace").and_then(|cell| numeric_cell(Some(cell))).filter(|value| value.abs() > 1e-6) {
        out.insert("spacing".to_string(), json!(font_px(spacing)));
    }
    let font_value = pick("Font")
        .or_else(|| node.cells.get("Char.Font"))
        .and_then(|font| font.value.as_deref())
        .filter(|value| !value.is_empty() && *value != "Themed");
    if let Some(value) = font_value {
        out.insert("fontFamily".to_string(), json!(value));
    } else if !theme.present {
        if let Some(root) = &ctx.root_font {
            out.insert("fontFamily".to_string(), json!(root));
        }
    } else if let Some(face) = theme.body_font_family() {
        out.insert("fontFamily".to_string(), json!(face));
    }
    Value::Object(out)
}

/// Colour of a non-concrete Character colour cell whose formula the flat theme
/// evaluator cannot read (`IF(ABS(LUMDIFF(..)) < 40, User.DarkerColor, ..)`).
fn character_color_expression(
    cell: &Cell,
    node: &ResolvedShape,
    theme: &Theme,
    has_fill: bool,
) -> Option<String> {
    if is_concrete(cell) {
        return None;
    }
    let formula = cell.formula.as_deref()?;
    if !formula.contains('(') || formula.eq_ignore_ascii_case("inh") {
        return None;
    }
    // A bare THEMEVAL() in a text cell is the shape's QuickStyle font colour
    // (a variation index 100-106) when it has one, else the theme's text role.
    let quick_font = numeric_cell(find_chain(&node.cells, "QuickStyleFontColor"))
        .filter(|value| *value != 100.0 && *value != 0.0)
        .and_then(|value| theme.quick_color(Some(value)));
    let bare = quick_font.or_else(|| theme.themed_text(has_fill));
    crate::colorexpr::evaluate_color(formula, theme, bare.as_deref(), &mut |reference| {
        // `Sheet.5!User.X` and `User.X` both read the instance's own cell.
        let name = reference.rsplit('!').next().unwrap_or(reference);
        let value = match name.strip_prefix("User.") {
            Some(user) => instance_user_value(node, user)?,
            None => strip_quotes(node.cells.get(name)?.value.as_deref()?),
        };
        Some(match Some(&value).filter(|v| v.starts_with('#')).and_then(|v| crate::styles::normalize_hex_color(v)) {
            Some(hex) => crate::colorexpr::Val::Color(hex),
            None => match value.parse::<f64>() {
                Ok(number) => crate::colorexpr::Val::Num(number),
                Err(_) => crate::colorexpr::Val::Str(value),
            },
        })
    })
}

/// The result the instance itself stores for field `ix` (its own Field section). The
/// master's text only caches the master's placeholder ("New Camera"), so an instance
/// value always wins.
fn instance_field_value(node: &ResolvedShape, ix: &str) -> Option<String> {
    let section = node.own_sections.iter().find(|section| section.name == "Field")?;
    let row = section
        .rows
        .iter()
        .find(|row| row.index.as_deref().unwrap_or("0") == ix)?;
    row.cells.get("Value")?.value.clone()
}

/// The shape's text with instance field results substituted, when it has any.
fn effective_text(node: &ResolvedShape) -> Option<String> {
    let overridden = node.text_tokens.iter().any(|token| {
        matches!(token, TextToken::Field { ix, .. } if instance_field_value(node, ix).is_some())
    });
    if !overridden {
        return None;
    }
    let mut out = String::new();
    for token in &node.text_tokens {
        match token {
            TextToken::Text(raw) => out.push_str(raw),
            TextToken::Field { ix, text } => match instance_field_value(node, ix) {
                Some(own) => out.push_str(&own),
                None => out.push_str(text),
            },
            _ => {}
        }
    }
    Some(out)
}

fn field_rows(node: &ResolvedShape) -> BTreeMap<String, Value> {
    // Master rows first, then the instance's: an instance row inherits the formula/format
    // (`F='Inh'`) but carries its own cached result, which is what Visio shows.
    let mut rows: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    for sections in [&node.master_sections, &node.own_sections] {
        let Some(section) = sections.iter().find(|section| section.name == "Field") else {
            continue;
        };
        for row in &section.rows {
            let ix = row.index.as_deref().unwrap_or("0").to_string();
            let entry = rows.entry(ix).or_default();
            let formula = row
                .cells
                .get("Value")
                .and_then(|cell| cell.formula.clone())
                .filter(|formula| formula != "Inh");
            if let Some(formula) = formula {
                entry.0 = formula;
            }
            if row.cells.contains_key("Value") {
                entry.1 = row_cell_string(row, "Value");
            }
            if let Some(format) = row
                .cells
                .get("Format")
                .and_then(|cell| cell.formula.clone())
                .filter(|format| format != "Inh")
            {
                entry.2 = format;
            }
        }
    }
    rows.into_iter()
        .map(|(ix, (formula, value, format))| {
            (
                ix,
                json!({ "formula": formula, "value": value, "format": format }),
            )
        })
        .collect()
}

fn cached_run_style(
    cache: &mut BTreeMap<String, Value>,
    node: &ResolvedShape,
    ctx: &StyleContext,
    theme: &Theme,
    ix: &str,
    has_fill: bool,
) -> Value {
    if let Some(style) = cache.get(ix) {
        return style.clone();
    }
    let style = run_style(node, ctx, theme, ix, has_fill);
    cache.insert(ix.to_string(), style.clone());
    style
}

fn rich_text(
    node: &ResolvedShape,
    ctx: &StyleContext,
    theme: &Theme,
    fallback_align: Option<&str>,
    has_fill: bool,
) -> Option<Value> {
    if !node.has_text {
        return None;
    }
    let fields = field_rows(node);
    let mut style_cache: BTreeMap<String, Value> = BTreeMap::new();

    let mut paragraphs: Vec<ParagraphAccum> = Vec::new();
    let mut current = ParagraphAccum {
        pp: "0".to_string(),
        runs: Vec::new(),
    };
    let mut cp = "0".to_string();

    for token in &node.text_tokens {
        match token {
            TextToken::Char(ix) => {
                cp = ix.clone();
            }
            TextToken::Para(ix) => {
                start_paragraph(&mut paragraphs, &mut current, ix);
            }
            TextToken::Field { ix, text } => {
                let field = fields
                    .get(ix)
                    .cloned()
                    .unwrap_or_else(|| json!({ "formula": "", "value": "", "format": "" }));
                let value = field
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let text = match instance_field_value(node, ix) {
                    Some(own) => own,
                    None if text.is_empty() => value,
                    None => text.clone(),
                };
                let style = cached_run_style(&mut style_cache, node, ctx, theme, &cp, has_fill);
                current.runs.push(json!({
                    "field": field,
                    "cp": cp,
                    "style": style,
                    "text": text,
                }));
            }
            TextToken::Text(raw) => {
                let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
                for (index, part) in normalized.split('\n').enumerate() {
                    if index > 0 {
                        let pp = current.pp.clone();
                        start_paragraph(&mut paragraphs, &mut current, &pp);
                    }
                    if part.is_empty() {
                        continue;
                    }
                    let style = cached_run_style(&mut style_cache, node, ctx, theme, &cp, has_fill);
                    current.runs.push(json!({
                        "text": part,
                        "cp": cp,
                        "style": style,
                    }));
                }
            }
        }
    }
    if paragraph_has_content(&current) {
        paragraphs.push(current);
    }
    if paragraphs.is_empty() {
        return None;
    }

    let mut out = Vec::new();
    for paragraph in paragraphs {
        let style = paragraph_style(node, &ctx.paragraph_styles, &paragraph.pp, fallback_align);
        let mut entry = json!({
            "pp": paragraph.pp,
            "runs": paragraph.runs,
            "style": style.to_json(),
            "align": style.align,
            "bullet": style.bullet,
            "bulletStr": style.bullet_str,
            "indent": style.indent,
        });
        if let Some(map) = entry.as_object_mut() {
            // taken out again by the caller (`rich_text` keeps the model JSON as it was)
            map.insert("lineSpacing".to_string(), json!(style.line_spacing));
            map.insert("spaceBefore".to_string(), json!(style.space_before));
            map.insert("spaceAfter".to_string(), json!(style.space_after));
            map.insert("indentFirst".to_string(), json!(style.indent_first));
        }
        out.push(entry);
    }
    Some(json!({ "paragraphs": out }))
}

fn resolve_text(
    node: &ResolvedShape,
    ctx: &StyleContext,
    theme: &Theme,
    has_fill: bool,
) -> TextPaint {
    let mut rows: Vec<&BTreeMap<String, Cell>> = Vec::new();
    for sections in [&node.own_sections, &node.master_sections] {
        let row = sections
            .iter()
            .find(|section| section.name == "Character")
            .and_then(|section| {
                section
                    .rows
                    .iter()
                    .find(|row| row.index.as_deref() == Some("0") || row.index.is_none())
                    .or_else(|| section.rows.first())
            });
        if let Some(row) = row {
            rows.push(&row.cells);
        }
    }
    let own_rows = rows.len();
    rows.extend(ctx.text_styles.iter());

    let pick_entry = |name: &str| -> Option<(&Cell, usize)> {
        for (index, cells) in rows.iter().enumerate() {
            if let Some(cell) = cells.get(name) {
                // A Character row on the shape itself caches the instance's
                // resolved colour (the container heading is grey even though
                // its master caches black). Other inherited character cells
                // defer to the style chain when their formula is inherited.
                if name == "Color" && index < own_rows {
                    return Some((cell, index));
                }
                if cell.formula.as_deref() != Some("Inh") {
                    return Some((cell, index));
                }
            }
        }
        for (index, cells) in rows.iter().enumerate() {
            if let Some(cell) = cells.get(name) {
                return Some((cell, index));
            }
        }
        None
    };
    let pick = |name: &str| pick_entry(name).map(|(cell, _)| cell);

    let font_size = pick("Size")
        .or_else(|| node.cells.get("Char.Size"))
        .and_then(|cell| numeric_cell(Some(cell)))
        .filter(|size| *size > 0.0)
        .map(font_px)
        .unwrap_or(12.0);

    let color_pick = pick_entry("Color");
    let color_cell = color_pick
        .map(|(cell, _)| cell)
        .or_else(|| node.cells.get("Char.Color"));
    let text_color = match color_cell {
        Some(cell) if is_bare_themed(cell) => themed_text_of(node, theme, has_fill),
        Some(cell) => character_color_expression(cell, node, theme, has_fill)
            .or_else(|| theme.themed_color(Some(cell), "text")),
        None => themed_text_of(node, theme, has_fill),
    };

    let style = pick("Style")
        .or_else(|| node.cells.get("Char.Style"))
        .and_then(|cell| numeric_cell(Some(cell)))
        .unwrap_or(0.0)
        .round() as u32;
    let font = pick("Font").or_else(|| node.cells.get("Char.Font"));
    let font_family = font
        .and_then(|cell| cell.value.as_deref())
        .filter(|value| !value.is_empty() && *value != "Themed")
        .map(str::to_string)
        .or_else(|| if theme.present { theme.body_font_family() } else { ctx.root_font.clone() })
        .unwrap_or_else(|| FONT_STACK.to_string());

    TextPaint {
        font_size,
        font_family,
        text_color: text_color.unwrap_or_else(|| "#1b2733".to_string()),
        bold: style & 1 != 0,
        italic: style & 2 != 0,
        underline: style & 4 != 0,
        script: script_position(pick("Pos").or_else(|| node.cells.get("Char.Pos"))),
    }
}

/// The colour of the text block's background: `TextBkgnd` when it is a concrete colour (0 means none) and `TextBkgndTrans` leaves it visible.
/// Measured in Visio on the Aruba SFP modules: the white block of the label hides the lower half of the icon; `TextBkgnd = 0` or a transparency of
/// 100 % shows the whole icon.
fn text_background(node: &ResolvedShape) -> Option<String> {
    // Only a group's label matters: it is drawn over the members. A plain shape's block lies over its own fill, which the cell's white default
    // (a themed background colour on most styles) leaves unchanged on a white page.
    if node.children.is_empty() {
        return None;
    }
    let colour = node.cells.get("TextBkgnd")?.value.as_deref()?;
    let hex = colour.starts_with('#') && colour.len() == 7 && colour[1..].chars().all(|c| c.is_ascii_hexdigit());
    let transparent = numeric_cell(node.cells.get("TextBkgndTrans")).is_some_and(|value| value >= 0.995);
    (hex && !transparent).then(|| colour.to_ascii_lowercase())
}

/// `Char.Pos` as 0 (normal), 1 (superscript) or 2 (subscript).
fn script_position(cell: Option<&Cell>) -> u8 {
    match numeric_cell(cell).map(|value| value.round() as i64) {
        Some(1) => 1,
        Some(2) => 2,
        _ => 0,
    }
}

fn find_chain<'a>(cells: &'a BTreeMap<String, Cell>, name: &str) -> Option<&'a Cell> {
    if let Some(cell) = cells.get(name) {
        if cell.formula.as_deref() == Some("Inh") && !is_concrete(cell) {
            // Fall through to the second pass.
        } else {
            return Some(cell);
        }
    }
    cells.get(name)
}

fn find_extra<'a>(name: &str, groups: &[&'a [BTreeMap<String, Cell>]]) -> Option<&'a Cell> {
    for group in groups {
        for cells in *group {
            if let Some(cell) = cells.get(name) {
                if cell.formula.as_deref() != Some("Inh") {
                    return Some(cell);
                }
            }
        }
    }
    for group in groups {
        for cells in *group {
            if let Some(cell) = cells.get(name) {
                return Some(cell);
            }
        }
    }
    None
}

fn pick_with<'a>(
    cells: &'a BTreeMap<String, Cell>,
    name: &str,
    extra: &[&'a [BTreeMap<String, Cell>]],
) -> Option<&'a Cell> {
    find_chain(cells, name).or_else(|| find_extra(name, extra))
}

/// The first cell called `name` in the style chain that holds a real value (not `Themed`).
fn style_cell<'a>(styles: &'a [BTreeMap<String, Cell>], name: &str) -> Option<&'a Cell> {
    styles
        .iter()
        .filter_map(|cells| cells.get(name))
        .find(|cell| cell.value.as_deref().is_some_and(|v| !v.is_empty() && v != "Themed"))
}

fn cached_cell_color(theme: &Theme, cell: &Cell) -> Option<String> {
    let value = cell.value.as_deref()?;
    if value.is_empty() || value == "Themed" {
        return None;
    }
    let mut copy = cell.clone();
    copy.formula = None;
    theme.color_of(&copy)
}

/// Visio's `LineCap` (0 round, 1 flat, 2 square) rides on the stroke style as `+flat` / `+square`.
pub(crate) fn with_line_cap(dash: Option<String>, cap: Option<f64>) -> Option<String> {
    let suffix = match cap {
        Some(v) if v == 1.0 => "+flat",
        Some(v) if v == 2.0 => "+square",
        _ => return dash,
    };
    Some(format!("{}{suffix}", dash.unwrap_or_else(|| "solid".to_string())))
}

fn numeric_cell(cell: Option<&Cell>) -> Option<f64> {
    let value = cell?.value.as_deref()?;
    if value.is_empty() || value == "Themed" {
        return None;
    }
    value.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn convert_shape(
    node: &ResolvedShape,
    place: &Placement,
    z: i64,
    group: Option<Value>,
    styles: &DocumentStyles,
    theme: &Theme,
    container: Option<&ResolvedShape>,
) -> Option<Shape> {
    let w = place.w;
    let h = place.h;
    let style = style_context(node, styles, w * h > 60000.0);
    let paint = resolve_style(node, &style, theme);
    let mut align = paragraph_align(node, &style.paragraph_styles);
    let text_box = text_block(node, styles, w, h);
    let text_fit = text_fit(node);
    let (parts, sub_texts, front_parts) = geometry_parts(
        node,
        w,
        h,
        Some(paint.fill.as_str()),
        Some(paint.stroke.as_str()),
        Some(paint.stroke_width),
        paint.fill_gradient.as_ref(),
        paint.fill_pattern.as_ref(),
        paint.fill_opacity,
        paint.stroke_opacity,
        paint.dash.as_deref(),
        theme,
        styles,
    );
    let has_geometry = node
        .sections
        .iter()
        .any(|section| section.name == "Geometry")
        || (node.master_explicit && !node.master_children.is_empty());
    // Geometry that exists but is entirely hidden by NoShow must not fall
    // back to a filled rectangle in the renderer.
    // A shape taken from a master with no geometry at all (the "[parameters]" text of a UML fragment)
    // paints nothing but its text; it must not become a themed rectangle either.
    let textual_master_child = node.master_id.is_some()
        && node.parent.is_some()
        && node.foreign.is_none()
        && node.shape_type.as_deref() != Some("Group")
        && node.master_children.is_empty()
        && !node.sections.iter().any(|section| section.name == "Geometry");
    let hidden_geometry = parts.is_empty() && (has_geometry || textual_master_child);
    // Neither does a shape or group with no Geometry section at all, whatever its fill and line cells say: only its text and
    // its member shapes show (measured in Visio, docs/04). A picture is the exception: it is its own content.
    let no_outline = !has_geometry && node.foreign.is_none();
    let (fill, stroke, stroke_width, dash) = if hidden_geometry || (no_outline && parts.is_empty()) {
        ("none".to_string(), "none".to_string(), 0.0, None)
    } else if let Some(front) = parts.last() {
        (
            front.fill.clone().unwrap_or_else(|| "none".to_string()),
            front.stroke.clone().unwrap_or_else(|| "none".to_string()),
            front.stroke_width.unwrap_or(0.0),
            front.dash.clone(),
        )
    } else {
        (
            paint.fill.clone(),
            paint.stroke.clone(),
            paint.stroke_width,
            paint.dash.clone(),
        )
    };
    // UML list rows paint their own band behind the marker icon and text.
    let mut parts = parts;
    // The band stands in for the row's own background geometry, so it follows
    // that geometry's visibility: ER attribute rows hide it with `NoShow`.
    if !hidden_geometry && !own_geometry_hidden(node) {
        if let Some(band) = list_band_part(node, w, h) {
            parts.insert(0, band);
        }
    }
    let container_background =
        container.is_some_and(|container| is_container_background(node, container));
    let mut fill = fill;
    if container_background {
        // Visio paints the container/list interior white over the master's
        // background children; heading and member shapes remain themed.
        fill = "#ffffff".to_string();
        for part in &mut parts {
            if part
                .fill
                .as_deref()
                .is_some_and(|fill| !fill.eq_ignore_ascii_case("none"))
            {
                part.fill = Some("#ffffff".to_string());
            }
            part.fill_gradient = None;
        }
    }
    let has_visible_fill =
        parts.iter().any(|part| {
            part.fill
                .as_deref()
                .is_some_and(|fill| !fill.eq_ignore_ascii_case("none"))
        }) || (!hidden_geometry && parts.is_empty() && !fill.eq_ignore_ascii_case("none"));
    // Labels placed outside the shape (BPMN Start/End captions, annotations)
    // sit on the page background, so Visio uses the theme text colour rather
    // than the light-on-fill colour.
    let text_on_background = has_visible_fill && !text_pin_outside(node);
    let text = resolve_text(node, &style, theme, text_on_background);
    let mut rich = rich_text(node, &style, theme, align.as_deref(), text_on_background);
    // paragraph spacing travels next to `rich`, not inside it (the golden models keep their shape)
    let mut paragraph_spacing: Vec<[f64; 4]> = Vec::new();
    if let Some(paragraphs) = rich.as_mut().and_then(|value| value.get_mut("paragraphs")).and_then(Value::as_array_mut) {
        for paragraph in paragraphs.iter_mut() {
            let mut take = |key: &str, default: f64| -> f64 {
                paragraph.as_object_mut().and_then(|map| map.remove(key)).and_then(|value| value.as_f64()).unwrap_or(default)
            };
            paragraph_spacing.push([take("lineSpacing", 0.0), take("spaceBefore", 0.0), take("spaceAfter", 0.0), take("indentFirst", 0.0)]);
        }
        if paragraph_spacing.iter().all(|s| *s == [0.0, 0.0, 0.0, 0.0]) {
            paragraph_spacing.clear();
        }
    }

    let mut data = Vec::new();
    if let Some(section) = section(node, "Property", None) {
        for row in &section.rows {
            let name = row.name.clone().unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            data.push(DataRow {
                name,
                label: row_cell_string(row, "Label"),
                value: row_cell_string(row, "Value"),
                prompt: row_cell_string(row, "Prompt"),
                format: row_cell_string(row, "Format"),
                r#type: row_cell_num(row, "Type").unwrap_or(0.0) as u32,
                invisible: row_cell_num(row, "Invisible").unwrap_or(0.0) as u32,
                extra: Default::default(),
            });
        }
    }

    let mut label = effective_text(node)
        .as_deref()
        .or(node.text.as_deref())
        .unwrap_or("")
        .trim_end_matches(['\r', '\n'])
        .to_string();
    let placeholder = matches!(label.trim(), "" | "Label" | "Abc" | "?");
    if placeholder {
        if let Some(text) =
            instance_user_value(node, "LabelText").filter(|value| !value.trim().is_empty())
        {
            if node.master_name.as_deref() == Some("Bubble callout") || text == "Operating System" {
                label.clear();
            } else {
                label = text;
                align = Some("left".to_string());
            }
            rich = None;
        } else if let Some(fields) = field_label(node) {
            label = fields;
            rich = None;
        }
    }
    if label.trim().is_empty()
        && node
            .name
            .as_deref()
            .is_some_and(|name| name.starts_with("DefaultLabel"))
    {
        if let Some(comment) = node
            .cells
            .get("Comment")
            .and_then(|cell| cell.value.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            label = comment.to_string();
        }
    }

    let mut shape = Shape {
        id: None,
        visio_id: Some(node.id.clone()),
        shape_type: if parts.is_empty() && !hidden_geometry {
            "rect"
        } else {
            "custom"
        }
        .to_string(),
        x: place.x,
        y: place.y,
        w: w.max(2.0),
        h: h.max(2.0),
        rot: place.rot,
        text_rot: text_rotation(node),
        rounding: rounding_value(node),
        flip_x: cell_num(node, "FlipX").unwrap_or(0.0) != 0.0,
        flip_y: cell_num(node, "FlipY").unwrap_or(0.0) != 0.0,
        label,
        fill: Some(fill),
        fill_opacity: Some(paint.fill_opacity),
        stroke_opacity: (paint.stroke_opacity < 1.0).then_some(paint.stroke_opacity),
        stroke: Some(stroke),
        stroke_width: Some(stroke_width),
        stroke_style: dash,
        font_size: Some(text.font_size),
        font_family: Some(text.font_family),
        text_color: Some(text.text_color),
        bold: Some(text.bold),
        italic: Some(text.italic),
        underline: Some(text.underline),
        align: Some(align.unwrap_or_else(|| "center".to_string())),
        v_align: Some(vertical_align(node).unwrap_or_else(|| "middle".to_string())),
        z: Some(z as f64),
        parts,
        sub_texts,
        shadow: shadow_value(node, theme),
        bevel: bevel_value(node),
        glow: glow_value(node, theme),
        soft_edge: soft_edge_value(node),
        reflection: reflection_value(node),
        data,
        text_box: Some(text_box),
        text_box_rel: None,
        rich,
        image: None,
        hyperlinks: Vec::new(),
        alt_text: instance_user_value(node, "visAltDescription").filter(|text| !text.trim().is_empty()),
        text_fit,
        text_fits_line: text_width_fits_line(node),
        text_script: text.script,
        text_bkgnd: text_background(node),
        callout: None,
        native_data_graphic: None,
        layers: layer_members(node),
        layer_hidden: false,
        layer_color: None,
        paragraph_spacing,
        front_parts,
        layer_nonprinting: false,
        data_color: None,
        attached_to: None,
        group,
        containers: Vec::new(),
        list_of: Vec::new(),
        container_info: None,
        container_id: None,
        container_kind: None,
        extra: Default::default(),
    };
    restore_data_graphic(node, &mut shape);
    restore_native_data_graphic(node, &mut shape);
    // `HideText` suppresses the label of a master-backed group (the cached
    // value is what Visio evaluated, even when the formula reads an action
    // row). Nested `Type="Shape"` children inherit it too but still show text
    // carried by their parent group, so they are left alone. A top-level master-backed shape hides
    // its own label the same way (ENTITY Category: `HideText = GUARD(User.DBHideDiscriminator)`).
    if (node.shape_type.as_deref() == Some("Group") || node.parent.is_none())
        && node.master_id.is_some()
        && cell_num(node, "HideText").is_some_and(|value| value != 0.0)
    {
        shape.label.clear();
        shape.rich = None;
    }
    if node
        .name
        .as_deref()
        .is_some_and(|name| name.starts_with("DefaultLabel"))
    {
        shape.fill = Some("none".to_string());
        shape.stroke = Some("none".to_string());
        shape.stroke_width = Some(0.0);
        shape.parts.clear();
    }
    Some(shape)
}

struct ConnectorEnds {
    begin: Option<String>,
    end: Option<String>,
    /// Where the connection point the begin / end is glued to sits on the page (inches, y up), when it is known.
    begin_site: Option<(f64, f64)>,
    end_site: Option<(f64, f64)>,
}

/// The page positions (inches, y up) of the connection points connectors are glued to, from the editor's geometry
/// (`GroupEditor::connection_points`). Empty when nothing is glued to a numbered connection point.
fn glued_sites(package: &Package, page_index: usize, connects: &[crate::Connect]) -> BTreeMap<(String, usize), (f64, f64)> {
    let mut wanted: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for connect in connects {
        if let Some(n) = connect.to_cell.strip_prefix("Connections.X").and_then(|n| n.parse::<usize>().ok()) {
            wanted.entry(connect.to_sheet.clone()).or_default().push(n);
        }
    }
    let mut out = BTreeMap::new();
    if wanted.is_empty() {
        return out;
    }
    let Ok(editor) = crate::groupedit::GroupEditor::open(package, page_index) else { return out };
    for (sheet, ns) in wanted {
        let points = editor.connection_points(&sheet);
        for n in ns {
            if let Some((_, x, y)) = points.iter().find(|p| p.0 == n) {
                out.insert((sheet.clone(), n), (*x, *y));
            }
        }
    }
    out
}

fn convert_connectors(
    nodes: &[ResolvedShape],
    page_height: f64,
    connects: &[crate::Connect],
    sites: &BTreeMap<(String, usize), (f64, f64)>,
    shapes: &[Shape],
    styles: &DocumentStyles,
    theme: &Theme,
) -> Vec<Connector> {
    let targets: BTreeMap<&str, &Shape> = shapes
        .iter()
        .filter_map(|shape| shape.visio_id.as_deref().map(|id| (id, shape)))
        .collect();
    let mut ends: BTreeMap<String, ConnectorEnds> = BTreeMap::new();
    for connect in connects {
        let entry = ends
            .entry(connect.from_sheet.clone())
            .or_insert_with(|| ConnectorEnds {
                begin: None,
                end: None,
                begin_site: None,
                end_site: None,
            });
        let site = connect
            .to_cell
            .strip_prefix("Connections.X")
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|n| sites.get(&(connect.to_sheet.clone(), n)).copied());
        match connect.from_cell.as_str() {
            "BeginX" | "BeginY" => {
                entry.begin = Some(connect.to_sheet.clone());
                entry.begin_site = site;
            }
            "EndX" | "EndY" => {
                entry.end = Some(connect.to_sheet.clone());
                entry.end_site = site;
            }
            _ => {}
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn walk(
        nodes: &[ResolvedShape],
        parent: Transform,
        page_height: f64,
        ends: &BTreeMap<String, ConnectorEnds>,
        targets: &BTreeMap<&str, &Shape>,
        styles: &DocumentStyles,
        theme: &Theme,
        z: &mut f64,
        order: &mut f64,
        out: &mut Vec<Connector>,
    ) {
        for node in nodes {
            let current = *z;
            *z += 1.0;
            // One drawing order for shapes and connectors: Visio draws in shape-tree order. A connector's `order_z` is just above
            // every shape that comes before it in the tree (`order` carries the largest shape z seen so far).
            let order_z = *order + 0.5;
            if !is_connector(node) {
                if let Some(shape_z) = targets.get(node.id.as_str()).and_then(|shape| shape.z) {
                    if shape_z >= 0.0 && shape_z > *order {
                        *order = shape_z;
                    }
                }
            }
            if is_connector(node) {
                // A connector inside a group keeps its cells in the group's own
                // coordinates; `parent` brings its points onto the page.
                let mut connector = convert_connector(
                    node,
                    &parent,
                    page_height,
                    ends.get(&node.id),
                    targets,
                    styles,
                    theme,
                    current,
                );
                connector.order_z = Some(order_z);
                out.push(connector);
                continue;
            }
            // Children of a group live in the group's frame.
            let child_parent = if node.shape_type.as_deref() == Some("Group") {
                shape_transform(node)
                    .map(|local| compose(&parent, &local))
                    .unwrap_or(parent)
            } else {
                parent
            };
            walk(
                &node.children,
                child_parent,
                page_height,
                ends,
                targets,
                styles,
                theme,
                z,
                order,
                out,
            );
        }
    }
    let mut out = Vec::new();
    let mut z = 0.0;
    let mut order = -1.0;
    walk(
        nodes,
        IDENTITY,
        page_height,
        &ends,
        &targets,
        styles,
        theme,
        &mut z,
        &mut order,
        &mut out,
    );
    out
}

/// A point in the connector's own frame (inches, y up) as page pixels (y down); the
/// connector may sit inside a group whose frame `parent` maps onto the page.
fn connector_page_point(parent: &Transform, page_height: f64, x_in: f64, y_in: f64) -> (f64, f64) {
    let (wx, wy) = apply_transform(parent, x_in, y_in);
    (wx * DPI, (page_height - wy) * DPI)
}

#[allow(clippy::too_many_arguments)]
fn convert_connector(
    node: &ResolvedShape,
    parent: &Transform,
    page_height: f64,
    ends: Option<&ConnectorEnds>,
    targets: &BTreeMap<&str, &Shape>,
    styles: &DocumentStyles,
    theme: &Theme,
    z: f64,
) -> Connector {
    let begin_target = ends.and_then(|e| e.begin.as_deref());
    let end_target = ends.and_then(|e| e.end.as_deref());
    let from = endpoint(node, parent, page_height, true, begin_target, ends.and_then(|e| e.begin_site), targets);
    let to = endpoint(node, parent, page_height, false, end_target, ends.and_then(|e| e.end_site), targets);
    let (waypoints, hops) = connector_waypoints(node, parent, page_height);
    let route_style = cell_num(node, "ShapeRouteStyle").map(|v| v as u32);
    let route = if matches!(route_style, Some(2 | 7 | 8)) {
        "straight"
    } else {
        "elbow"
    };
    let mut style = style_context(node, styles, false);
    // Connector shapes can carry the Normal/Dynamic Connector style rather
    // than a style named "Connector"; their line must follow the connector
    // scheme, not the ordinary theme line colour.
    style.connector = true;
    let paint = resolve_style(node, &style, theme);
    // `EndArrow = THEMEVAL()` resolves to a filled arrow only when the package
    // actually carries a theme; themeless templates (Base Electrical) draw
    // plain connectors.
    let themed_end_arrow = if theme.present { 4.0 } else { 0.0 };
    let begin_arrow = arrow_value(node, &["LineBeginArrow", "BeginArrow"], styles, 0.0);
    let end_arrow = arrow_value(
        node,
        &["LineEndArrow", "EndArrow"],
        styles,
        themed_end_arrow,
    );
    let label_point = connector_label_point(node, parent, page_height);
    Connector {
        id: None,
        visio_id: Some(node.id.clone()),
        from,
        to,
        route: route.to_string(),
        waypoints,
        hops,
        label: node
            .text
            .as_deref()
            .unwrap_or("")
            .trim_end_matches(['\r', '\n'])
            .to_string(),
        label_point,
        group: None,
        stroke: Some(paint.stroke.clone()),
        stroke_width: Some(paint.stroke_width.max(1.0)),
        stroke_style: Some(paint.dash.clone().unwrap_or_else(|| "solid".to_string())),
        arrow_start: Some(arrow_kind(begin_arrow)),
        arrow_end: Some(arrow_kind(end_arrow)),
        begin_line_end: gallery_line_end(begin_arrow, node.cells.get("BeginArrowSize")),
        end_line_end: gallery_line_end(end_arrow, node.cells.get("EndArrowSize")),
        route_style,
        con_fixed_code: cell_num(node, "ConFixedCode").map(|v| v as u32),
        line_route_ext: cell_num(node, "ConLineRouteExt").map(|v| v as u32),
        walk_preference: cell_num(node, "WalkPreference").map(|v| v as u32),
        plow_code: cell_num(node, "ShapePlowCode").map(|v| v as u32),
        glue_type: Some(cell_num(node, "GlueType").map(|v| v as u32).unwrap_or(2)),
        dyn_feedback: Some(cell_num(node, "DynFeedback").map(|v| v as u32).unwrap_or(2)),
        containers: Vec::new(),
        z: Some(z),
        order_z: None,
        visio: None,
        extra: Default::default(),
    }
}

fn connector_label_point(node: &ResolvedShape, parent: &Transform, page_height: f64) -> Option<Value> {
    if node.text.as_deref().unwrap_or("").trim().is_empty() {
        return None;
    }
    let pin_x = cell_num(node, "PinX")?;
    let pin_y = cell_num(node, "PinY")?;
    let loc_x = cell_num(node, "LocPinX").unwrap_or(0.0);
    let loc_y = cell_num(node, "LocPinY").unwrap_or(0.0);
    let txt_x = cell_num(node, "TxtPinX").unwrap_or(0.0);
    let txt_y = cell_num(node, "TxtPinY").unwrap_or(0.0);
    let txt_loc_x = cell_num(node, "TxtLocPinX").unwrap_or(0.0);
    let txt_loc_y = cell_num(node, "TxtLocPinY").unwrap_or(0.0);
    let txt_w = cell_num(node, "TxtWidth").unwrap_or(0.0);
    let txt_h = cell_num(node, "TxtHeight").unwrap_or(0.0);
    let left = pin_x - loc_x + txt_x - txt_loc_x;
    let bottom = pin_y - loc_y + txt_y - txt_loc_y;
    let (x, y) = connector_page_point(parent, page_height, left + txt_w / 2.0, bottom + txt_h / 2.0);
    Some(json!({ "x": x, "y": y }))
}

/// Arrow cell value for a connector. The Connector StyleSheet defines
/// `EndArrow` as `THEMEVAL()`; Visio resolves that to a filled arrow (4).
/// `themed` is the value a bare themed cell resolves to (4 for the end arrow,
/// 0 for the begin arrow). A wholly absent cell stays 0 (no arrowhead).
fn arrow_value(
    node: &ResolvedShape,
    names: &[&str],
    styles: &DocumentStyles,
    themed: f64,
) -> f64 {
    for name in names {
        if let Some(cell) = node.cells.get(*name) {
            if let Some(value) = cell.value.as_deref() {
                if value.eq_ignore_ascii_case("Themed") {
                    return themed;
                }
                if let Ok(value) = value.parse::<f64>() {
                    return value;
                }
            }
        }
    }
    for row in styles.line_rows(node.line_style.as_deref()) {
        for name in names {
            let Some(cell) = row.get(*name) else {
                continue;
            };
            let Some(value) = cell.value.as_deref() else {
                continue;
            };
            if value.eq_ignore_ascii_case("Themed") {
                return themed;
            }
            if let Ok(value) = value.parse::<f64>() {
                return value;
            }
        }
    }
    0.0
}

/// The Visio line end of a connector end when its type is a gallery kind (1-35) other than the plain filled
/// triangle (4), which `arrow_kind` and the renderer's connector arrow already draw.
fn gallery_line_end(kind: f64, size_cell: Option<&Cell>) -> Option<layoutcraft_model::LineEnd> {
    let kind = kind as i64;
    if !(1..=35).contains(&kind) || kind == 4 {
        return None;
    }
    let size = size_cell.and_then(|cell| numeric_cell(Some(cell))).map_or(2, |value| value.max(0.0) as u32);
    Some(layoutcraft_model::LineEnd { kind: kind as u32, size })
}

fn arrow_kind(value: f64) -> String {
    match value as i64 {
        0 => "none",
        12 => "arrow",
        14 => "hollowTriangle",
        22 => "hollowDiamond",
        38 => "filledDiamond",
        1 | 2 => "arrow",
        _ => "filled",
    }
    .to_string()
}

fn endpoint(
    node: &ResolvedShape,
    parent: &Transform,
    page_height: f64,
    begin: bool,
    target_id: Option<&str>,
    site: Option<(f64, f64)>,
    targets: &BTreeMap<&str, &Shape>,
) -> Endpoint {
    let x_name = if begin { "BeginX" } else { "EndX" };
    let y_name = if begin { "BeginY" } else { "EndY" };
    let Some(x_in) = cell_num(node, x_name) else {
        return Endpoint::default();
    };
    let y_in = cell_num(node, y_name).unwrap_or(0.0);
    let (mut x, mut y) = connector_page_point(parent, page_height, x_in, y_in);
    // The cached end of a glued connector can be stale: a drawing written by a converter states the end by the glue formula
    // (`PAR(PNT(Sheet.59!Connections.X1, ...))`) but caches another place (computer-network: the corner of the device's
    // box), and Visio draws the end at the connection point it recalculates. When the connection point is known and the
    // cached end is clearly somewhere else, the end goes to the connection point.
    if let Some((sx, sy)) = site {
        let (px, py) = (sx * DPI, (page_height - sy) * DPI);
        if (px - x).hypot(py - y) > 0.08 * DPI {
            (x, y) = (px, py);
        }
    }
    if let Some(target_id) = target_id {
        if let Some(target) = targets.get(target_id) {
            let cx = target.x + target.w / 2.0;
            let cy = target.y + target.h / 2.0;
            let dx_page = x - cx;
            let dy_page = y - cy;
            // `local_to_world` applies the target's rotation and flip. Undo
            // them here, otherwise a connector glued to a rotated symbol
            // (electrical resistors, capacitors) attaches at a doubly-rotated
            // point and the wire runs diagonally.
            let angle = target.rot * std::f64::consts::PI / 180.0;
            let dx_rot = dx_page * angle.cos() + dy_page * angle.sin();
            let dy_rot = -dx_page * angle.sin() + dy_page * angle.cos();
            let fx = if target.flip_x { -1.0 } else { 1.0 };
            let fy = if target.flip_y { -1.0 } else { 1.0 };
            let dx = dx_rot / fx;
            let dy = dy_rot / fy;
            let lx = if target.w.abs() > 1e-9 {
                (dx + target.w / 2.0) / target.w
            } else {
                0.0
            };
            let ly = if target.h.abs() > 1e-9 {
                (dy + target.h / 2.0) / target.h
            } else {
                0.0
            };
            let side = if dx_rot.abs() * (target.h / 2.0)
                > dy_rot.abs() * (target.w / 2.0)
            {
                if dx_rot > 0.0 {
                    "e"
                } else {
                    "w"
                }
            } else if dy_rot > 0.0 {
                "s"
            } else {
                "n"
            };
            return Endpoint {
                shape_id: Some(format!("shape:visio:{target_id}")),
                side: Some(side.to_string()),
                point: None,
                local: Some(json!({ "x": lx, "y": ly })),
                extra: Default::default(),
            };
        }
        return Endpoint {
            shape_id: Some(format!("shape:visio:{target_id}")),
            side: None,
            point: Some(json!({ "x": x, "y": y })),
            local: None,
            extra: Default::default(),
        };
    }
    Endpoint {
        shape_id: None,
        side: None,
        point: Some(json!({ "x": x, "y": y })),
        local: None,
        extra: Default::default(),
    }
}

/// Points of a connector's first Geometry section (without the two ends) and the hops Visio
/// stored where it crosses another connector.
fn connector_waypoints(
    node: &ResolvedShape,
    parent: &Transform,
    page_height: f64,
) -> (Vec<Value>, Vec<layoutcraft_model::ConnectorHop>) {
    let pin_x = cell_num(node, "PinX").unwrap_or(0.0);
    let pin_y = cell_num(node, "PinY").unwrap_or(0.0);
    let loc_x = cell_num(node, "LocPinX").unwrap_or(0.0);
    let loc_y = cell_num(node, "LocPinY").unwrap_or(0.0);
    let w_in = cell_num(node, "Width").unwrap_or(0.0);
    let h_in = cell_num(node, "Height").unwrap_or(0.0);
    let to_page = |x_in: f64, y_in: f64| connector_page_point(parent, page_height, pin_x - loc_x + x_in, pin_y - loc_y + y_in);
    for section in &node.sections {
        if section.name != "Geometry" {
            continue;
        }
        let mut points = Vec::new();
        let mut hops = Vec::new();
        let mut previous: Option<(f64, f64)> = None;
        for row in &section.rows {
            let kind = row.row_type.as_deref().unwrap_or("");
            let (Some(x), Some(y)) = (row_cell_num(row, "X"), row_cell_num(row, "Y")) else {
                continue;
            };
            let (x_in, y_in) = if kind.starts_with("Rel") {
                (x * w_in, y * h_in)
            } else {
                (x, y)
            };
            let here = to_page(x_in, y_in);
            if let Some((px_in, py_in)) = previous {
                let from = to_page(px_in, py_in);
                if kind == "MoveTo" {
                    hops.push(layoutcraft_model::ConnectorHop {
                        from: [from.0, from.1],
                        to: [here.0, here.1],
                        via: Vec::new(),
                        apex: None,
                        gap: true,
                    });
                } else if kind == "ArcTo" {
                    // A is the sagitta: positive bulges to the right of the direction of travel.
                    let bulge = row_cell_num(row, "A").unwrap_or(0.0);
                    let (dx, dy) = (x_in - px_in, y_in - py_in);
                    let len = dx.hypot(dy);
                    if len > 1e-9 && bulge.abs() > 1e-9 {
                        let apex = to_page((px_in + x_in) / 2.0 + bulge * dy / len, (py_in + y_in) / 2.0 - bulge * dx / len);
                        hops.push(layoutcraft_model::ConnectorHop {
                            from: [from.0, from.1],
                            to: [here.0, here.1],
                            via: Vec::new(),
                            apex: Some([apex.0, apex.1]),
                            gap: false,
                        });
                    }
                } else if kind == "PolylineTo" {
                    let formula = row.cells.get("A").and_then(|c| c.formula.clone().or_else(|| c.value.clone())).unwrap_or_default();
                    if let Some(inner) = formula.trim().strip_prefix("POLYLINE(").and_then(|rest| rest.strip_suffix(')')) {
                        let numbers: Vec<f64> = inner.split(',').filter_map(|v| v.trim().parse().ok()).collect();
                        // POLYLINE(xType, yType, x1, y1, ...): type 0 means fractions of Width/Height.
                        if numbers.len() >= 4 {
                            let (xt, yt) = (numbers[0], numbers[1]);
                            let via = numbers[2..]
                                .chunks_exact(2)
                                .map(|pair| {
                                    let px = if xt == 0.0 { pair[0] * w_in } else { pair[0] };
                                    let py = if yt == 0.0 { pair[1] * h_in } else { pair[1] };
                                    let p = to_page(px, py);
                                    [p.0, p.1]
                                })
                                .collect();
                            hops.push(layoutcraft_model::ConnectorHop {
                                from: [from.0, from.1],
                                to: [here.0, here.1],
                                via,
                                apex: None,
                                gap: false,
                            });
                        }
                    }
                }
            }
            previous = Some((x_in, y_in));
            points.push(here);
        }
        if points.len() > 2 {
            return (
                points[1..points.len() - 1]
                    .iter()
                    .map(|(x, y)| json!({ "x": x, "y": y }))
                    .collect(),
                hops,
            );
        }
    }
    (Vec::new(), Vec::new())
}

#[allow(clippy::too_many_arguments)]
fn geometry_parts(
    node: &ResolvedShape,
    w_px: f64,
    h_px: f64,
    fill: Option<&str>,
    stroke: Option<&str>,
    stroke_width: Option<f64>,
    quick_fill_gradient: Option<&Value>,
    quick_fill_pattern: Option<&layoutcraft_model::FillPattern>,
    fill_opacity: f64,
    stroke_opacity: f64,
    dash: Option<&str>,
    theme: &Theme,
    styles: &DocumentStyles,
) -> (Vec<ShapePart>, Vec<layoutcraft_model::SubText>, usize) {
    let w_in = w_px / DPI;
    let h_in = h_px / DPI;
    let has_own_geometry = node
        .own_sections
        .iter()
        .any(|section| section.name == "Geometry");
    let mut body_texts = Vec::new();
    let mut body_children: Vec<ShapePart> = Vec::new();
    if node.master_explicit && !node.master_children.is_empty() {
        let (mut body, mut texts) = crate::masterbody::body_parts(node, w_in, h_in, theme, styles);
        // A flipped master group mirrors its whole body about the instance box (BLOCK3 Right arrow
        // is FlipX, Vertical bar turns -90 degrees with FlipY: the extrusion goes the other way).
        let (group_flip_x, group_flip_y) = (
            cell_num(node, "FlipX").unwrap_or(0.0) != 0.0,
            cell_num(node, "FlipY").unwrap_or(0.0) != 0.0,
        );
        if group_flip_x || group_flip_y {
            for part in body.iter_mut() {
                part.d = crate::geometry::flip_path(&part.d, w_px, h_px, group_flip_x, group_flip_y);
            }
            // The children's text blocks move with them (their glyphs stay upright): PYRAMID
            // "Inverted Pyramid with caption" puts the captions on the other side.
            for text in texts.iter_mut() {
                if group_flip_x {
                    text.x = w_px - (text.x + text.w);
                }
                if group_flip_y {
                    text.y = h_px - (text.y + text.h);
                }
                if group_flip_x != group_flip_y {
                    text.text_rot = -text.text_rot;
                }
            }
        }
        // Most master groups draw their body. Some (floor-plan Space shapes,
        // data-graphic callouts) add their own page Geometry while the body
        // still supplies text; keep that text with the page geometry.
        if !body.is_empty() {
            // A group can also own Geometry (an ER attribute is an ellipse
            // whose child is only its leader line). Visio paints the group's
            // geometry first and the children over it.
            let own_master_geometry = node
                .master_sections
                .iter()
                .any(|section| section.name == "Geometry");
            if !own_master_geometry || has_own_geometry {
                return (body, texts, 0);
            }
            body_children = body;
        } else if !has_own_geometry
            && !node.master_sections.iter().any(|section| section.name == "Geometry")
        {
            // A body that paints nothing (text-only children) leaves the group's own master
            // geometry as the drawing: org-chart frames, DVCALL data bars.
            return (body, texts, 0);
        }
        body_texts = texts;
    }
    let gradient = fill_gradient_value(
        &node.sections,
        node.cells
            .get("FillGradientAngle")
            .and_then(|cell| numeric_cell(Some(cell))),
        node.cells
            .get("FillGradientDir")
            .and_then(|cell| numeric_cell(Some(cell))),
        node.cells
            .get("FillGradientEnabled")
            .and_then(|cell| numeric_cell(Some(cell)))
            // Not set on the shape: the style says THEMEVAL(), so the theme's fill matrix decides
            // whether a gradient is drawn (a solid matrix leaves the shape's stored gradient
            // rows unused).
            .unwrap_or(if quick_fill_gradient.is_none() { 0.0 } else { 1.0 })
            != 0.0,
        theme,
        &styles.fill_gradient_sections(node.fill_style.as_deref()),
    )
    .or_else(|| {
        // The theme's quick-style gradient only applies while the shape has not
        // switched gradients off (FillGradientEnabled = 0).
        let enabled = node
            .cells
            .get("FillGradientEnabled")
            .and_then(|cell| numeric_cell(Some(cell)))
            .unwrap_or(1.0)
            != 0.0;
        quick_fill_gradient.filter(|_| enabled).cloned()
    });
    // A legacy fill-pattern gradient (patterns 25-40) blends along a smoothstep curve, not the
    // ease-out of the theme gradients.
    let gradient = gradient.map(|mut value| {
        let legacy = node
            .cells
            .get("FillPattern")
            .and_then(|cell| numeric_cell(Some(cell)))
            .is_some_and(|pattern| (25.0..=29.0).contains(&pattern));
        if legacy {
            if let Some(object) = value.as_object_mut() {
                object.insert("smooth".to_string(), Value::Bool(true));
            }
        }
        value
    });
    let stroke_gradient = gradient_value(
        &node.sections,
        "LineGradient",
        node.cells
            .get("LineGradientAngle")
            .and_then(|cell| numeric_cell(Some(cell))),
        node.cells
            .get("LineGradientDir")
            .and_then(|cell| numeric_cell(Some(cell))),
        node.cells
            .get("LineGradientEnabled")
            .and_then(|cell| numeric_cell(Some(cell)))
            .unwrap_or(0.0)
            != 0.0,
        theme,
        &styles.line_gradient_sections(node.line_style.as_deref()),
    );
    let mut parts = Vec::new();
    let mut geometry_sections: Vec<&Section> = node
        .sections
        .iter()
        .filter(|section| section.name == "Geometry")
        .collect();
    // Visio paints filled geometry first and line-only geometry on top. A
    // master can store a line-only detail (the SDL final-state X) before the
    // filled circle; keeping XML order would bury the detail under the fill.
    geometry_sections.sort_by_key(|section| {
        resolved_section_num(node, *section, "NoFill").unwrap_or(0.0) != 0.0
    });
    for section in geometry_sections {
        // A page shape that carries its own geometry is the authority on
        // visibility. Visio stores the section-level NoShow/NoFill/NoLine
        // cells on the master, though, and the page section simply inherits
        // them; the merged section therefore has to be consulted, evaluated
        // against this instance's User cells. Master-only geometry is
        // evaluated the same way — electrical stencils hide whole variant
        // arms with formulas such as `IF(User.Type=1,0,1)` or `NOT(User.Form)`.
        // If such a formula cannot be evaluated we leave the section visible
        // rather than trusting a cached value from a different master variant.
        let own = node
            .own_sections
            .iter()
            .find(|own| own.name == section.name && own.index == section.index);
        let show_value = if own.is_some() {
            resolved_section_num(node, section, "NoShow")
        } else {
            resolved_master_section_num(node, section, "NoShow")
        };
        let no_show = show_value.is_some_and(|value| value != 0.0);
        if no_show {
            continue;
        }
        // Geometry owned by a group that also has children is only drawn when
        // its visibility is known; a variant arm gated by a formula that cannot
        // be evaluated (BPMN event types) must stay hidden.
        if !body_children.is_empty()
            && show_value.is_none()
            && section
                .cells
                .get("NoShow")
                .is_some_and(|cell| usable_visibility_formula(cell))
        {
            continue;
        }
        let no_fill = resolved_section_num(node, section, "NoFill").unwrap_or(0.0) != 0.0;
        let no_line = resolved_section_num(node, section, "NoLine").unwrap_or(0.0) != 0.0;
        let close = !no_fill && fill.is_some_and(|fill| fill != "none");
        let path = crate::geometry::path_from_section(section, w_in, h_in, close, true);
        if path.is_empty() {
            continue;
        }
        // Visio fills an open path as if it were closed but strokes it open: split such a path
        // into a closed fill and an open line, or the line would gain a closing edge.
        let open_line = if close && !no_line {
            let open = crate::geometry::path_from_section(section, w_in, h_in, false, true);
            crate::geometry::has_open_subpath(&open).then_some(open)
        } else {
            None
        };
        let flips = (
            cell_num(node, "FlipX").unwrap_or(0.0) != 0.0,
            cell_num(node, "FlipY").unwrap_or(0.0) != 0.0,
        );
        if let Some(open) = open_line {
            parts.push(ShapePart {
                d: crate::geometry::flip_path(&path, w_px, h_px, flips.0, flips.1),
                fill: Some(fill.unwrap_or("none").to_string()),
                fill_opacity: Some(fill_opacity),
                stroke: Some("none".to_string()),
                stroke_opacity: None,
                stroke_width: Some(0.0),
                dash: None,
                fill_gradient: gradient.clone(),
                stroke_gradient: None,
                fill_pattern: quick_fill_pattern.cloned(),
                bottom_edge: None,
                begin_arrow: None,
                end_arrow: None,
                source: None,
                extra: Default::default(),
            });
            parts.push(ShapePart {
                d: crate::geometry::flip_path(&open, w_px, h_px, flips.0, flips.1),
                fill: Some("none".to_string()),
                fill_opacity: None,
                stroke: Some(stroke.unwrap_or("none").to_string()),
                stroke_opacity: if stroke_opacity >= 1.0 { None } else { Some(stroke_opacity) },
                stroke_width: Some(stroke_width.unwrap_or(0.0)),
                dash: dash.map(str::to_string),
                fill_gradient: None,
                stroke_gradient: stroke_gradient.clone(),
                fill_pattern: None,
                bottom_edge: None,
                begin_arrow: None,
                end_arrow: None,
                source: None,
                extra: Default::default(),
            });
            continue;
        }
        // The shape's own FlipX/FlipY mirror its geometry about the box (text is not mirrored).
        let path = crate::geometry::flip_path(
            &path,
            w_px,
            h_px,
            cell_num(node, "FlipX").unwrap_or(0.0) != 0.0,
            cell_num(node, "FlipY").unwrap_or(0.0) != 0.0,
        );
        parts.push(ShapePart {
            d: path,
            fill: Some(if no_fill {
                "none".to_string()
            } else {
                fill.unwrap_or("none").to_string()
            }),
            fill_opacity: if no_fill { None } else { Some(fill_opacity) },
            stroke: Some(if no_line {
                "none".to_string()
            } else {
                stroke.unwrap_or("none").to_string()
            }),
            stroke_opacity: if no_line || stroke_opacity >= 1.0 {
                None
            } else {
                Some(stroke_opacity)
            },
            stroke_width: Some(if no_line {
                0.0
            } else {
                stroke_width.unwrap_or(0.0)
            }),
            dash: if no_line { None } else { dash.map(str::to_string) },
            fill_gradient: if no_fill { None } else { gradient.clone() },
            stroke_gradient: if no_line {
                None
            } else {
                stroke_gradient.clone()
            },
            fill_pattern: if no_fill {
                None
            } else {
                quick_fill_pattern.cloned()
            },
            bottom_edge: None,
            begin_arrow: None,
            end_arrow: None,
            source: None,
            extra: Default::default(),
        });
    }
    merge_section_fills(&mut parts);
    let begin = arrow_value(node, &["LineBeginArrow", "BeginArrow"], styles, 0.0);
    let end = arrow_value(node, &["LineEndArrow", "EndArrow"], styles, 0.0);
    for part in parts.iter_mut() {
        apply_line_ends(part, begin, end, &node.cells);
    }
    // A group's own geometry against its members, by the group's `DisplayMode` cell (probed in Visio, step 219: a group with its own blue
    // rectangle over red members, make-displaymode.mjs): 0 = the group's own geometry is not drawn, 1 = it is drawn BEHIND the members,
    // 2 or no cell = it is drawn IN FRONT of them (the cell's default). DVCALL "Filled circles" has no cell and its pie (the group's own
    // geometry) lies over the grey circle of its member; DGICON, PEVALV and DVCALL Lights / Faces state DisplayMode 1.
    let mut front_parts = 0;
    match cell_num(node, "DisplayMode") {
        Some(mode) if mode == 0.0 => {
            parts.clear();
            parts.extend(body_children);
        }
        Some(mode) if mode == 1.0 => parts.extend(body_children),
        _ => {
            // the order of `parts` stays (own geometry first: the golden models); the renderer draws the first `front_parts` last
            if !body_children.is_empty() {
                front_parts = parts.len();
            }
            parts.extend(body_children);
        }
    }
    (parts, body_texts, front_parts)
}

/// Give an open, stroked path its arrowheads (`BeginArrow`/`EndArrow` with their size cells). Closed
/// paths and compound paths do not take line ends.
pub(crate) fn apply_line_ends(
    part: &mut ShapePart,
    begin: f64,
    end: f64,
    cells: &BTreeMap<String, Cell>,
) {
    if (begin < 1.0 && end < 1.0)
        || part.d.matches('M').count() != 1
        || part.d.contains(['Z', 'z'])
        || part.stroke.as_deref().is_none_or(|stroke| stroke == "none")
    {
        return;
    }
    let end_of = |kind: f64, size_cell: &str| {
        (kind >= 1.0).then(|| layoutcraft_model::LineEnd {
            kind: kind as u32,
            size: cells
                .get(size_cell)
                .and_then(|cell| numeric_cell(Some(cell)))
                .map_or(2, |value| value.max(0.0) as u32),
        })
    };
    part.begin_arrow = end_of(begin, "BeginArrowSize");
    part.end_arrow = end_of(end, "EndArrowSize");
}

/// Visio fills all Geometry sections of a shape as one compound path, so a section nested inside
/// another leaves a hole (BLOCK "Concentric layer": two circles are a ring). Parts that fill the
/// same plain colour are therefore filled together, even-odd, by the first of them; the others
/// keep only their outline.
/// Bounding box of an absolute `M L C Q A Z` path (an arc contributes its radii box around the
/// chord's midpoint, exact for the half circles ellipses become).
fn path_box(d: &str) -> Option<(f64, f64, f64, f64)> {
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut command = ' ';
    let mut numbers: Vec<f64> = Vec::new();
    let mut token = String::new();
    let take = |command: char, numbers: &mut Vec<f64>, points: &mut Vec<(f64, f64)>| {
        let width = match command {
            'M' | 'L' => 2,
            'C' => 6,
            'Q' => 4,
            'A' => 7,
            _ => 0,
        };
        if width > 0 {
            for chunk in numbers.chunks_exact(width) {
                if width == 7 {
                    if let Some(&(sx, sy)) = points.last() {
                        let (mx, my) = ((sx + chunk[5]) / 2.0, (sy + chunk[6]) / 2.0);
                        points.push((mx - chunk[0], my - chunk[1]));
                        points.push((mx + chunk[0], my + chunk[1]));
                    }
                }
                for pair in chunk[if width == 7 { 5 } else { 0 }..].chunks_exact(2) {
                    points.push((pair[0], pair[1]));
                }
            }
        }
        numbers.clear();
    };
    for c in d.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphabetic() || c == ' ' || c == ',' {
            if !token.is_empty() {
                if let Ok(value) = token.parse::<f64>() {
                    numbers.push(value);
                }
                token.clear();
            }
            if c.is_ascii_alphabetic() {
                take(command, &mut numbers, &mut points);
                command = c;
            }
        } else if c == '-' && !token.is_empty() && !token.ends_with('e') {
            if let Ok(value) = token.parse::<f64>() {
                numbers.push(value);
            }
            token.clear();
            token.push(c);
        } else {
            token.push(c);
        }
    }
    take(command, &mut numbers, &mut points);
    let first = *points.first()?;
    Some(points.iter().fold((first.0, first.1, first.0, first.1), |b, p| {
        (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1))
    }))
}

fn merge_section_fills(parts: &mut Vec<ShapePart>) {
    let plain = |part: &ShapePart| {
        part.fill.as_deref().is_some_and(|fill| fill != "none")
            && part.fill_gradient.is_none()
            && part.fill_pattern.is_none()
            && part.fill_opacity.unwrap_or(1.0) >= 1.0
    };
    let indices: Vec<usize> = (0..parts.len()).filter(|&index| plain(&parts[index])).collect();
    if indices.len() < 2 {
        return;
    }
    let colour = parts[indices[0]].fill.clone();
    if indices.iter().any(|&index| parts[index].fill != colour) {
        return;
    }
    // Only sections that lie inside another one change anything; leave the rest alone.
    let boxes: Vec<Option<(f64, f64, f64, f64)>> = indices.iter().map(|&index| path_box(&parts[index].d)).collect();
    let nested = |inner: &(f64, f64, f64, f64), outer: &(f64, f64, f64, f64)| {
        inner.0 >= outer.0 - 1e-6
            && inner.1 >= outer.1 - 1e-6
            && inner.2 <= outer.2 + 1e-6
            && inner.3 <= outer.3 + 1e-6
    };
    let any_nested = boxes.iter().enumerate().any(|(i, a)| {
        boxes.iter().enumerate().any(|(j, b)| match (a, b) {
            (Some(a), Some(b)) => i != j && nested(a, b),
            _ => false,
        })
    });
    if !any_nested {
        return;
    }
    let mut fill_part = parts[indices[0]].clone();
    fill_part.d = indices.iter().map(|&index| parts[index].d.as_str()).collect::<Vec<_>>().join(" ");
    fill_part.stroke = Some("none".to_string());
    fill_part.stroke_width = Some(0.0);
    fill_part.stroke_opacity = None;
    fill_part.dash = None;
    fill_part.stroke_gradient = None;
    for &index in &indices {
        parts[index].fill = Some("none".to_string());
        parts[index].fill_opacity = None;
    }
    parts.insert(indices[0], fill_part);
}

/// Read a `FillGradient` section into the model's part gradient form.
/// Positions/opacity are normalised; `angle` is Visio's radians.
pub(crate) fn fill_gradient_value(
    sections: &[Section],
    angle: Option<f64>,
    dir: Option<f64>,
    enabled: bool,
    theme: &Theme,
    style_sections: &[&[Section]],
) -> Option<Value> {
    gradient_value(
        sections,
        "FillGradient",
        angle,
        dir,
        enabled,
        theme,
        style_sections,
    )
}

/// Read a gradient section (`FillGradient` or `LineGradient`) into the
/// model's part gradient form. Positions/opacity are normalised; `angle` is
/// Visio's radians and `dir` its gradient-type enumeration.
///
/// A gradient row without `GradientStopColor` inherits the colour from the
/// matching row of the FillStyle/LineStyle gradient section — Visio's
/// network and 3-D stencils author only the stop positions on the shape and
/// keep the colours on the style. If no style row supplies a colour, Visio
/// falls back to white.
pub(crate) fn gradient_value(
    sections: &[Section],
    section_name: &str,
    angle: Option<f64>,
    dir: Option<f64>,
    enabled: bool,
    theme: &Theme,
    style_sections: &[&[Section]],
) -> Option<Value> {
    if !enabled {
        return None;
    }
    let style_section = style_sections.iter().find_map(|sections| {
        sections
            .iter()
            .find(|section| section.name == section_name)
    });
    let section = sections
        .iter()
        .find(|section| section.name == section_name)?;
    let style_rows: &[crate::shapes::SectionRow] = style_section
        .map(|style| style.rows.as_slice())
        .unwrap_or(&[]);
    let mut stops = Vec::new();
    for (index, row) in section.rows.iter().enumerate() {
        let position = row
            .cells
            .get("GradientStopPosition")
            .and_then(|cell| cell.value.as_deref())
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or_else(|| {
                if section.rows.len() > 1 {
                    index as f64 / (section.rows.len() - 1) as f64
                } else {
                    0.0
                }
            });
        let color_cell = row.cells.get("GradientStopColor").or_else(|| {
            // Prefer the style row at the same index; otherwise the nearest
            // authored stop position.
            style_rows
                .get(index)
                .and_then(|style| style.cells.get("GradientStopColor"))
                .or_else(|| {
                    style_rows
                        .iter()
                        .min_by(|a, b| {
                            let pos = |row: &crate::shapes::SectionRow| {
                                row.cells
                                    .get("GradientStopPosition")
                                    .and_then(|cell| cell.value.as_deref())
                                    .and_then(|value| value.parse::<f64>().ok())
                                    .unwrap_or(0.0)
                            };
                            (pos(a) - position)
                                .abs()
                                .partial_cmp(&(pos(b) - position).abs())
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .and_then(|style| style.cells.get("GradientStopColor"))
                })
        });
        let color = color_cell
            .and_then(|cell| theme.color_of(cell))
            .unwrap_or_else(|| "#ffffff".to_string());
        let trans_cell = row
            .cells
            .get("GradientStopColorTrans")
            .or_else(|| {
                style_rows
                    .get(index)
                    .and_then(|style| style.cells.get("GradientStopColorTrans"))
            });
        let trans = trans_cell
            .and_then(|cell| cell.value.as_deref())
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0);
        stops.push(json!({
            "color": color,
            "opacity": (1.0 - trans).clamp(0.0, 1.0),
            "position": position,
        }));
    }
    if stops.len() < 2 {
        return None;
    }
    stops.sort_by(|a, b| {
        a["position"]
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&b["position"].as_f64().unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let dir = dir.unwrap_or(0.0).round().clamp(0.0, 13.0) as u32;
    let mut value = json!({
        "angle": angle.unwrap_or(0.0),
        "stops": stops,
    });
    if dir != 0 {
        value["dir"] = json!(dir);
    }
    Some(value)
}

/// Resolve an outer drop shadow from `ShapeShdw*` cells. `ShdwPattern` 0 is
/// Visio's "no shadow paint" and is the common cached value on shapes with a
/// dormant shadow type.
fn shadow_value(node: &ResolvedShape, theme: &Theme) -> Option<layoutcraft_model::Shadow> {
    let kind = cell_num(node, "ShapeShdwType").unwrap_or(0.0);
    let pattern = cell_num(node, "ShdwPattern").unwrap_or(0.0);
    let show = cell_num(node, "ShapeShdwShow").unwrap_or(1.0);
    // A QuickStyle effect can supply the shadow without materialising
    // ShapeShdw cells on the shape. `ShdwPattern` 0 is the "no shadow paint"
    // state that leaves the quick-style effect in charge.
    if pattern == 0.0 {
        let matrix = cell_num(node, "QuickStyleEffectsMatrix");
        let shadow_color = cell_num(node, "QuickStyleShadowColor");
        if let Some(shadow) = theme
            .quick_effect_paint(matrix, shadow_color)
            .and_then(|effects| effects.shadow)
        {
            return Some(layoutcraft_model::Shadow {
                dx: shadow.dx * DPI,
                dy: shadow.dy * DPI,
                blur: shadow.blur * DPI,
                color: shadow.color,
                opacity: shadow.opacity,
                kind: 1,
                scale: 1.0,
                oblique: 0.0,
            });
        }
    }
    if kind == 0.0 && pattern != 0.0 && show != 0.0 {
        // A legacy shadow (`ShdwPattern` set, no `ShapeShdwType`): a flat copy of the shape in
        // the shadow colour, offset by the page's `ShdwOffsetX/Y`.
        let (dx, dy) = theme.legacy_shadow_offset?;
        let color = node
            .cells
            .get("ShdwForegnd")
            .and_then(|cell| theme.color_of(cell))
            .unwrap_or_else(|| "#000000".to_string());
        let transparency = cell_num(node, "ShdwForegndTrans").unwrap_or(0.0);
        return Some(layoutcraft_model::Shadow {
            dx: dx * DPI,
            dy: -dy * DPI,
            blur: 0.0,
            color,
            opacity: (1.0 - transparency).clamp(0.0, 1.0),
            kind: 1,
            scale: 1.0,
            oblique: 0.0,
        });
    }
    if kind == 0.0 || pattern == 0.0 || show == 0.0 {
        return None;
    }
    let color = node
        .cells
        .get("ShdwForegnd")
        .and_then(|cell| theme.color_of(cell))
        .unwrap_or_else(|| "#000000".to_string());
    let dx = cell_num(node, "ShapeShdwOffsetX").unwrap_or(0.0) * DPI;
    // Visio's Y axis points up; page coordinates are top-down.
    let dy = -cell_num(node, "ShapeShdwOffsetY").unwrap_or(0.0) * DPI;
    let blur = cell_num(node, "ShapeShdwBlur").unwrap_or(0.0) * DPI;
    let scale = cell_num(node, "ShapeShdwScaleFactor").unwrap_or(1.0);
    let oblique = cell_num(node, "ShapeShdwObliqueAngle").unwrap_or(0.0);
    // an oblique shadow with another scale or angle shows without any offset (Visio draws it standing on the shape's bottom edge)
    let projected = kind.round() as u8 == 2 && ((scale - 1.0).abs() > 1e-6 || oblique.abs() > 1e-6);
    if dx.abs() < 0.01 && dy.abs() < 0.01 && blur < 0.01 && !projected {
        return None;
    }
    let transparency = cell_num(node, "ShdwForegndTrans").unwrap_or(0.0);
    Some(layoutcraft_model::Shadow {
        dx,
        dy,
        blur: blur.max(0.0),
        color,
        opacity: (1.0 - transparency).clamp(0.0, 1.0),
        kind: match kind.round() as u8 {
            2 => 2,
            3 => 3,
            _ => 1,
        },
        scale: if scale > 0.0 { scale } else { 1.0 },
        oblique,
    })
}

fn bevel_value(node: &ResolvedShape) -> Option<layoutcraft_model::Bevel> {
    let top = cell_num(node, "BevelTopType").unwrap_or(0.0);
    let bottom = cell_num(node, "BevelBottomType").unwrap_or(0.0);
    let (kind, width_name, height_name, is_bottom) = if top > 0.0 {
        (top, "BevelTopWidth", "BevelTopHeight", false)
    } else if bottom > 0.0 {
        (bottom, "BevelBottomWidth", "BevelBottomHeight", true)
    } else {
        return None;
    };
    let width = cell_num(node, width_name).unwrap_or(0.0) * DPI;
    let height = cell_num(node, height_name).unwrap_or(0.0) * DPI;
    if width <= 0.01 && height <= 0.01 {
        return None;
    }
    Some(layoutcraft_model::Bevel {
        kind: kind.round() as u32,
        width: width.max(0.0),
        height: height.max(0.0),
        bottom: is_bottom,
    })
}

/// Explicit `GlowSize`/`GlowColor`/`GlowColorTrans` effects on a shape./// Explicit `GlowSize`/`GlowColor`/`GlowColorTrans` effects on a shape.
fn glow_value(node: &ResolvedShape, theme: &Theme) -> Option<layoutcraft_model::Glow> {
    let radius = cell_num(node, "GlowSize").unwrap_or(0.0) * DPI;
    if radius <= 0.01 {
        return None;
    }
    let color = node
        .cells
        .get("GlowColor")
        .and_then(|cell| theme.color_of(cell))
        .unwrap_or_else(|| "#000000".to_string());
    let transparency = cell_num(node, "GlowColorTrans").unwrap_or(0.0);
    Some(layoutcraft_model::Glow {
        radius,
        color,
        opacity: (1.0 - transparency).clamp(0.0, 1.0),
    })
}

fn soft_edge_value(node: &ResolvedShape) -> Option<layoutcraft_model::SoftEdge> {
    let radius = cell_num(node, "SoftEdgesSize").unwrap_or(0.0) * DPI;
    (radius > 0.01).then_some(layoutcraft_model::SoftEdge { radius })
}

fn reflection_value(node: &ResolvedShape) -> Option<layoutcraft_model::Reflection> {
    let size = cell_num(node, "ReflectionSize")
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    if size <= 0.001 {
        return None;
    }
    let transparency = cell_num(node, "ReflectionTrans").unwrap_or(0.0);
    Some(layoutcraft_model::Reflection {
        size,
        distance: cell_num(node, "ReflectionDist").unwrap_or(0.0) * DPI,
        blur: cell_num(node, "ReflectionBlur").unwrap_or(0.0) * DPI,
        opacity: (1.0 - transparency).clamp(0.0, 1.0),
    })
}

fn instance_user_value(node: &ResolvedShape, name: &str) -> Option<String> {
    if let Some(cell) = node.cells.get(&format!("User.{name}")) {
        return cell.value.as_deref().map(strip_quotes);
    }
    let section = node
        .sections
        .iter()
        .find(|section| section.name == "User")?;
    let row = section
        .rows
        .iter()
        .find(|row| row.name.as_deref() == Some(name))?;
    Some(
        row.cells
            .get("Value")
            .and_then(|cell| cell.value.as_deref())
            .map(strip_quotes)
            .unwrap_or_default(),
    )
}

fn strip_quotes(value: &str) -> String {
    value.trim_matches('"').to_string()
}

fn field_label(node: &ResolvedShape) -> Option<String> {
    fn collect(node: &ResolvedShape, lines: &mut Vec<String>) {
        if let Some(section) = node.sections.iter().find(|section| section.name == "Field") {
            for row in &section.rows {
                let value = row
                    .cells
                    .get("Value")
                    .and_then(|cell| cell.value.as_deref())
                    .unwrap_or("")
                    .trim()
                    .trim_matches('"');
                if value.is_empty() || value == "0" || lines.iter().any(|line| line == value) {
                    continue;
                }
                lines.push(value.to_string());
            }
        }
    }
    let mut lines = Vec::new();
    collect(node, &mut lines);
    if node
        .own_sections
        .iter()
        .any(|section| section.name == "Geometry")
    {
        for child in &node.children {
            // A child whose `HideText` is set (the optional/foreign-key markers of a database
            // attribute) shows nothing.
            if cell_num(child, "HideText").is_some_and(|value| value != 0.0) {
                continue;
            }
            collect(child, &mut lines);
        }
    }
    (!lines.is_empty()).then(|| {
        lines.join(
            "
",
        )
    })
}

fn section<'a>(node: &'a ResolvedShape, name: &str, index: Option<&str>) -> Option<&'a Section> {
    node.sections.iter().find(|section| {
        section.name == name
            && index
                .map(|index| section.index.as_deref() == Some(index))
                .unwrap_or(true)
    })
}

/// Visio list items paint their band using `User.BackFillColor`; the entity
/// body only covers the non-item area. Add that band behind the marker icon.
/// True when the shape has Geometry sections of its own and every one of them
/// is hidden by `NoShow` (evaluated like `geometry_parts` does).
fn own_geometry_hidden(node: &ResolvedShape) -> bool {
    let sections: Vec<&Section> = node
        .sections
        .iter()
        .filter(|section| section.name == "Geometry")
        .collect();
    !sections.is_empty()
        && sections.iter().all(|section| {
            let own = node
                .own_sections
                .iter()
                .any(|own| own.name == section.name && own.index == section.index);
            if own {
                resolved_section_num(node, section, "NoShow")
            } else {
                resolved_master_section_num(node, section, "NoShow")
            }
            .is_some_and(|value| value != 0.0)
        })
}

fn list_band_part(node: &ResolvedShape, w: f64, h: f64) -> Option<ShapePart> {
    let is_item = instance_user_value(node, "AttributeName").is_some()
        || instance_user_value(node, "PrimaryKey").is_some()
        || instance_user_value(node, "ForeignKey").is_some();
    if !is_item {
        return None;
    }
    let fill = instance_user_value(node, "BackFillColor")?;
    if !fill.starts_with('#') {
        return None;
    }
    Some(ShapePart {
        d: format!(
            "M0 {h:.2}L{w:.2} {h:.2}L{w:.2} 0L0 0L0 {h:.2}Z",
            h = h,
            w = w
        ),
        fill: Some(fill),
        fill_opacity: Some(1.0),
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
        source: None,
        extra: Default::default(),
    })
}

/// Numeric cells a ShapeSheet formula in this instance can reference:
/// direct cells plus `User.*` and `Scratch.*` rows. Values come from the
/// merged page/master sections, so a page override wins.
fn instance_values(node: &ResolvedShape) -> BTreeMap<String, f64> {
    let mut values: BTreeMap<String, f64> = node
        .cells
        .iter()
        .filter_map(|(name, cell)| {
            cell.value
                .as_deref()
                .and_then(|value| value.parse::<f64>().ok())
                .map(|value| (name.clone(), value))
        })
        .collect();
    for section in &node.sections {
        match section.name.as_str() {
            "User" => {
                for row in &section.rows {
                    let Some(name) = row.name.as_deref() else {
                        continue;
                    };
                    if let Some(value) = row
                        .cells
                        .get("Value")
                        .and_then(|cell| cell.value.as_deref())
                        .and_then(|value| value.parse::<f64>().ok())
                    {
                        values.insert(format!("User.{name}"), value);
                    }
                }
            }
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
                            values.insert(format!("Scratch.{name}{row_number}"), value);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for (name, value) in &node.master_top_user {
        values.entry(name.clone()).or_insert(*value);
    }
    values
}

/// Resolve a section-level flag such as `NoShow` or `NoFill`. The formula is
/// evaluated against the instance's cells when possible; otherwise the cached
/// value is used. Inherited section cells are cached by the master, so
/// evaluating is what keeps them correct for a resized/moved instance.
fn resolved_section_num(node: &ResolvedShape, section: &Section, name: &str) -> Option<f64> {
    let cell = section.cells.get(name)?;
    if let Some(text) = cell
        .formula
        .as_deref()
        .filter(|formula| *formula != "Inh" && *formula != "No Formula" && !formula.is_empty())
    {
        let values = instance_values(node);
        if let Some(value) = formula::evaluate(text, &mut |reference| {
            geometry_section_ref(node, reference, 0).or_else(|| lookup_instance_ref(node, &values, reference))
        }) {
            return Some(value);
        }
    }
    cell.value.as_deref().and_then(|value| value.parse().ok())
}

/// A formula may read another Geometry section's visibility cell: `Geometry1.NoShow` is the
/// first Geometry section's cell (PEVALV weld symbols hide variant arms that way).
fn geometry_section_ref(node: &ResolvedShape, reference: &str, depth: u32) -> Option<f64> {
    if depth > 8 {
        return None;
    }
    let reference = strip_sheet_ref(reference);
    let rest = reference.strip_prefix("Geometry")?;
    let (number, cell_name) = rest.split_once('.')?;
    let number: usize = number.parse().ok()?;
    if !matches!(cell_name, "NoShow" | "NoFill" | "NoLine") || number == 0 {
        return None;
    }
    let mut sections: Vec<&Section> = node.sections.iter().filter(|section| section.name == "Geometry").collect();
    sections.sort_by_key(|section| section.index.as_deref().and_then(|v| v.parse::<i64>().ok()).unwrap_or(0));
    let section = sections.get(number - 1)?;
    let cell = section.cells.get(cell_name)?;
    if let Some(text) = cell
        .formula
        .as_deref()
        .filter(|formula| *formula != "Inh" && *formula != "No Formula" && !formula.is_empty())
    {
        let values = instance_values(node);
        if let Some(value) = formula::evaluate(text, &mut |inner| {
            geometry_section_ref(node, inner, depth + 1).or_else(|| lookup_instance_ref(node, &values, inner))
        }) {
            return Some(if value != 0.0 { 1.0 } else { 0.0 });
        }
    }
    cell.value.as_deref().and_then(|value| value.parse().ok())
}

/// Master-section formulas reference cells as `Sheet.5!User.SubType`; the
/// instance lookup only stores `User.SubType`.
/// A reference such as `Sheet.5!Width` names the master's top shape when 5 is its id (and not this
/// shape): read that shape's size and control rows instead of this node's own cells.
fn lookup_instance_ref(node: &ResolvedShape, values: &BTreeMap<String, f64>, reference: &str) -> Option<f64> {
    if let Some((sheet, rest)) = reference.rsplit_once('!') {
        let id = sheet.strip_prefix("Sheet.");
        if id.is_some()
            && id == node.master_top_id.as_deref()
            && id != node.master_shape_id.as_deref()
        {
            if let Some(value) = values.get(&format!("top.{rest}")) {
                return Some(*value);
            }
        }
    }
    values.get(strip_sheet_ref(reference)).copied()
}

fn strip_sheet_ref(reference: &str) -> &str {
    reference.rsplit('!').next().unwrap_or(reference)
}

/// Visibility of a master-only Geometry section. Formulas are evaluated
/// against the page instance; when a formula cannot be evaluated, the section
/// stays visible instead of using a cached value that may belong to another
/// master variant. A static cached value is still authoritative when the cell
/// carries no formula.
fn resolved_master_section_num(node: &ResolvedShape, section: &Section, name: &str) -> Option<f64> {
    let cell = section.cells.get(name)?;
    if let Some(text) = cell
        .formula
        .as_deref()
        .filter(|formula| *formula != "Inh" && *formula != "No Formula" && !formula.is_empty())
    {
        let values = instance_values(node);
        return formula::evaluate(text, &mut |reference| {
            geometry_section_ref(node, reference, 0).or_else(|| lookup_instance_ref(node, &values, reference))
        })
        .map(|value| if value != 0.0 { 1.0 } else { 0.0 });
    }
    cell.value.as_deref().and_then(|value| value.parse().ok())
}

fn cell<'a>(node: &'a ResolvedShape, name: &str) -> Option<&'a Cell> {
    node.cells.get(name)
}

fn cell_num(node: &ResolvedShape, name: &str) -> Option<f64> {
    cell(node, name)
        .and_then(|c| c.value.as_deref())
        .and_then(|v| v.parse().ok())
}

/// Corner radius from the `Rounding` cell, converted to pixels. Visio stores
/// the radius in internal inches; zero means square corners.
fn rounding_value(node: &ResolvedShape) -> Option<f64> {
    cell_num(node, "Rounding")
        .filter(|value| *value > 1e-6)
        .map(|value| value * DPI)
}

/// Extra text-block rotation from `TxtAngle`, in screen coordinates. Visio
/// stores the text angle relative to the (possibly mirrored) shape; the
/// screen-space extra rotation is `-sign * TxtAngle`, where `sign` flips for a
/// mirrored shape. Electrical stencils set `TxtAngle = -Angle` so labels stay
/// upright when the symbol is rotated.
fn text_rotation(node: &ResolvedShape) -> Option<f64> {
    let cell = node.cells.get("TxtAngle")?;
    let angle = if let Some(text) = cell
        .formula
        .as_deref()
        .filter(|formula| *formula != "Inh" && *formula != "No Formula" && !formula.is_empty())
    {
        let values = instance_values(node);
        formula::evaluate(text, &mut |reference| {
            lookup_instance_ref(node, &values, reference)
        })?
    } else {
        cell.value.as_deref()?.parse::<f64>().ok()?
    };
    if angle.abs() < 1e-9 {
        return None;
    }
    let mirrored = (cell_num(node, "FlipX").unwrap_or(0.0) != 0.0)
        ^ (cell_num(node, "FlipY").unwrap_or(0.0) != 0.0);
    Some(if mirrored { angle } else { -angle })
}

fn row_cell_string(row: &crate::shapes::SectionRow, name: &str) -> String {
    row_cell_string_opt(row, name).unwrap_or_default()
}

fn row_cell_string_opt(row: &crate::shapes::SectionRow, name: &str) -> Option<String> {
    row.cells.get(name).and_then(|c| c.value.clone())
}

fn row_cell_num(row: &crate::shapes::SectionRow, name: &str) -> Option<f64> {
    row.cells
        .get(name)
        .and_then(|c| c.value.as_deref())
        .and_then(|v| v.parse().ok())
}

#[cfg(test)]
mod fit_line_tests {
    use super::formula_fits_text_line;

    #[test]
    fn only_a_block_that_cannot_be_narrower_than_its_text_stays_on_one_line() {
        assert!(formula_fits_text_line("TEXTWIDTH(TheText)"));
        assert!(formula_fits_text_line("MAX(TEXTWIDTH(TheText),Width)"));
        assert!(!formula_fits_text_line("GUARD(MIN(Width*2.5,TEXTWIDTH(TheText)))"), "a capped block wraps");
        assert!(!formula_fits_text_line("Width*1.5"));
    }
}
