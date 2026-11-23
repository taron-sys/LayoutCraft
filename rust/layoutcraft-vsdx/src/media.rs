//! Picture/media import and LayoutCraft-authored data-graphic metadata.
//!
//! Basis: pictures are `Foreign` / `ForeignData` shapes whose bytes sit in package parts reached through relationships ([MS-VSDX] and the Open Packaging
//! Conventions, ECMA-376 Part 2); the data-graphic metadata is LayoutCraft's own representation (docs/13, not Visio's native format).
use std::collections::BTreeMap;
use std::path::Path;

use sha2::{Digest, Sha256};
use layoutcraft_model::Image as Picture;
use layoutcraft_model::{DataColor, DataGraphic, Media, NativeDataGraphic, Shape};

use crate::shapes::{Cell, ForeignData, ResolvedShape};
use crate::{Package, Relationship};

const DPI: f64 = 96.0;

#[derive(Debug, Default, Clone)]
pub(crate) struct MediaCollector {
    pub records: BTreeMap<String, Media>,
    by_part: BTreeMap<String, String>,
    seq: usize,
}

impl MediaCollector {
    fn media_for(
        &mut self,
        package: &Package,
        page_rels: &BTreeMap<String, Relationship>,
        foreign: &ForeignData,
    ) -> Option<Media> {
        let rel_id = foreign.rel_id.as_deref()?;
        let part = ole_preview_part(package, page_rels.get(rel_id)?.target.clone());
        if let Some(id) = self.by_part.get(&part) {
            return self.records.get(id).cloned();
        }
        let bytes = package.entry(&part)?;
        let ext = Path::new(&part)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("bin")
            .to_ascii_lowercase();
        // EMF pictures are converted to SVG so every renderer can draw them. An EMF is only SIZED here, from its header (the
        // conversion happens when a renderer asks for the bytes, `media_bytes`); a WMF is converted because its conversion can
        // still fail after the header.
        let emf_size = (matches!(ext.as_str(), "emf" | "wmf") && crate::emf::is_enhanced_metafile(bytes))
            .then(|| crate::emf::emf_natural_size(bytes))
            .flatten();
        let converted = (matches!(ext.as_str(), "emf" | "wmf") && !crate::emf::is_enhanced_metafile(bytes))
            .then(|| crate::emf::metafile_to_svg_at(bytes, None))
            .flatten();
        let metafile_ok = emf_size.is_some() || converted.is_some();
        let media_type = match ext.as_str() {
            "png" => "image/png",
            "jpeg" | "jpg" => "image/jpeg",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            "emf" | "wmf" if metafile_ok => "image/svg+xml",
            _ => "application/octet-stream",
        };
        let unsupported = matches!(ext.as_str(), "emf" | "wmf" | "tif" | "tiff") && !metafile_ok;
        let (width, height) = if let Some((w, h)) = emf_size {
            (w.round() as u32, h.round() as u32)
        } else if let Some(svg) = &converted {
            (svg.width_px.round() as u32, svg.height_px.round() as u32)
        } else if unsupported {
            (0, 0)
        } else {
            // the size is in the header: decoding a 5 MB picture to read two numbers cost most of the open time
            image::ImageReader::new(std::io::Cursor::new(bytes))
                .with_guessed_format()
                .ok()
                .and_then(|reader| reader.into_dimensions().ok())
                .unwrap_or((0, 0))
        };
        let adjust = match ext.as_str() {
            "png" => !is_indexed_png(bytes),
            "jpeg" | "jpg" | "bmp" => true,
            _ => false,
        };
        let id = format!("m{}", self.seq + 1);
        self.seq += 1;
        let name = Path::new(&part)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let record = Media {
            id: Some(id.clone()),
            src: None,
            media_type: Some(media_type.to_string()),
            ext: Some(ext),
            name: Some(name),
            part: Some(part.clone()),
            width: Some(width),
            height: Some(height),
            adjust: Some(adjust),
            unsupported: Some(unsupported),
            sha256: Some(sha256_hex(bytes)),
            bytes: Some(bytes.len() as u64),
            extra: Default::default(),
        };
        self.records.insert(id.clone(), record.clone());
        self.by_part.insert(part, id);
        Some(record)
    }
}

/// The bytes a renderer should embed for a media record: EMF pictures are
/// converted to SVG (matching the record's `image/svg+xml` type); everything
/// else is the package part unchanged.
pub fn media_bytes(package: &Package, media: &Media) -> Option<Vec<u8>> {
    let part = media.part.as_deref()?;
    let bytes = package.entry(part)?;
    // BMP pictures are served as PNG: the SVG/PNG renderers cannot draw an `image/bmp` data
    // URI, so every BMP shape used to come out blank (the scene sniffs the PNG signature).
    if media.ext.as_deref() == Some("bmp") {
        let image = image::load_from_memory(bytes).ok()?;
        let mut png = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .ok()?;
        return Some(png);
    }
    if matches!(media.ext.as_deref(), Some("emf" | "wmf")) && media.media_type.as_deref() == Some("image/svg+xml") {
        let display = media.extra.get("displayWidth").and_then(|value| value.as_f64());
        return crate::emf::metafile_to_svg_at(bytes, display).map(|svg| svg.svg.into_bytes());
    }
    Some(bytes.to_vec())
}

/// An OLE-embedded object (`ForeignType='Object'`, for example an embedded Visio
/// drawing) is shown as a preview picture. The shape's relationship points at the
/// embedded part; that part's own relationships name the preview image. Anything
/// that is already an image, or has no image relationship, is returned unchanged.
fn ole_preview_part(package: &Package, part: String) -> String {
    if is_image_part(&part) {
        return part;
    }
    if let Ok(rels) = package.relationships(&part) {
        if let Some(image) = rels
            .values()
            .find(|rel| rel.kind.ends_with("/image") && is_image_part(&rel.target))
        {
            return image.target.clone();
        }
    }
    part
}

fn is_indexed_png(bytes: &[u8]) -> bool {
    bytes.len() > 25
        && bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a])
        && bytes[25] == 3
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub(crate) struct ImageSource {
    foreign: ForeignData,
    cells: BTreeMap<String, Cell>,
    master_part: Option<String>,
    outline: Option<crate::shapes::Section>,
}

/// The Geometry section of a Foreign shape the picture is cut to. A master can carry several
/// variants (a diamond and a circle, all hidden by NoShow); the first one is the one Visio uses.
fn picture_outline(node: &ResolvedShape) -> Option<crate::shapes::Section> {
    node.sections.iter().find(|section| section.name == "Geometry").cloned()
}

/// True when the path is just the box itself (four corners), which needs no clip.
fn is_plain_box(d: &str, w: f64, h: f64) -> bool {
    let numbers: Vec<f64> = d
        .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect();
    if d.contains(['A', 'C', 'Q']) || numbers.len() > 10 {
        return false;
    }
    let tol = 0.5;
    numbers.chunks(2).all(|c| {
        c.len() == 2
            && (c[0].abs() < tol || (c[0] - w).abs() < tol)
            && (c[1].abs() < tol || (c[1] - h).abs() < tol)
    })
}

pub(crate) fn collect_foreign(nodes: &[ResolvedShape]) -> BTreeMap<String, ImageSource> {
    let mut out = BTreeMap::new();
    fn walk(nodes: &[ResolvedShape], out: &mut BTreeMap<String, ImageSource>) {
        for node in nodes {
            if let Some(foreign) = &node.foreign {
                out.insert(
                    node.id.clone(),
                    ImageSource {
                        foreign: foreign.clone(),
                        cells: node.cells.clone(),
                        master_part: node.master_part.clone(),
                        outline: picture_outline(node),
                    },
                );
            }
            walk(&node.children, out);
        }
    }
    walk(nodes, &mut out);
    out
}

pub(crate) fn attach_images(
    shapes: &mut [Shape],
    foreign: &BTreeMap<String, ImageSource>,
    page_rels: &BTreeMap<String, Relationship>,
    package: &Package,
    media: &mut MediaCollector,
) {
    let mut master_rels_cache: BTreeMap<String, BTreeMap<String, Relationship>> = BTreeMap::new();
    for shape in shapes {
        let Some(id) = shape.visio_id.as_deref() else {
            continue;
        };
        let Some(fd) = foreign.get(id) else {
            continue;
        };
        // A picture inherited from a master points at the master package's
        // relationship table (the grey avatar in org-chart boxes), not the
        // page's. Resolve that table when the page target is not an image.
        let mut rels = page_rels;
        let page_target = fd
            .foreign
            .rel_id
            .as_deref()
            .and_then(|rel| page_rels.get(rel))
            .map(|rel| rel.target.as_str());
        let page_is_image = page_target.is_some_and(is_image_part);
        if !page_is_image {
            if let Some(master_part) = fd.master_part.as_deref() {
                if !master_rels_cache.contains_key(master_part) {
                    if let Ok(rels) = package.relationships(master_part) {
                        master_rels_cache.insert(master_part.to_string(), rels);
                    }
                }
                if let Some(cached) = master_rels_cache.get(master_part) {
                    rels = cached;
                }
            }
        }
        let Some(record) = media.media_for(package, rels, &fd.foreign) else {
            continue;
        };
        // An EMF's hairlines depend on the size it is shown at: remember the
        // first shape's width so the SVG conversion keeps them one pixel wide.
        if matches!(record.ext.as_deref(), Some("emf" | "wmf")) && record.media_type.as_deref() == Some("image/svg+xml") {
            if let Some(stored) = record.id.as_ref().and_then(|id| media.records.get_mut(id)) {
                stored
                    .extra
                    .entry("displayWidth".to_string())
                    .or_insert_with(|| serde_json::json!(shape.w.max(1.0)));
            }
        }
        let w = shape.w.max(1.0);
        let h = shape.h.max(1.0);
        let offset_x = cell_num_from_cells(&fd.cells, "ImgOffsetX", 0.0) * DPI / w;
        let offset_y = cell_num_from_cells(&fd.cells, "ImgOffsetY", 0.0) * DPI / h;
        let scale_x = cell_num_from_cells(&fd.cells, "ImgWidth", w / DPI) * DPI / w;
        let scale_y = cell_num_from_cells(&fd.cells, "ImgHeight", h / DPI) * DPI / h;
        let adjustable = record.adjust.unwrap_or(false);
        let brightness = if adjustable {
            (cell_num_from_cells(&fd.cells, "Brightness", 0.5) - 0.5) * 2.0
        } else {
            0.0
        };
        let contrast = if adjustable {
            (cell_num_from_cells(&fd.cells, "Contrast", 0.5) - 0.5) * 2.0
        } else {
            0.0
        };
        let gamma = if adjustable {
            cell_num_from_cells(&fd.cells, "Gamma", 1.0)
        } else {
            1.0
        };
        // A picture whose Foreign shape has its own outline (a circle, a stone, a rounded square) is
        // shown inside it; the section's NoShow only hides the outline itself.
        let clip = fd.outline.as_ref().and_then(|section| {
            let d = crate::geometry::path_from_section(section, shape.w / DPI, shape.h / DPI, true, true);
            (!d.is_empty() && !is_plain_box(&d, shape.w, shape.h)).then_some(d)
        });
        shape.image = Some(Picture {
            media_id: record.id.clone(),
            name: record.name.clone(),
            media_type: record.media_type.clone(),
            foreign_type: fd.foreign.foreign_type.clone(),
            offset_x,
            offset_y,
            scale_x,
            scale_y,
            brightness,
            contrast,
            gamma,
            transparency: cell_num_from_cells(&fd.cells, "Transparency", 0.0),
            unsupported: record.unsupported.unwrap_or(false),
            extra: {
                let mut extra: layoutcraft_model::Extra = Default::default();
                if let Some(clip) = clip {
                    extra.insert("clipPath".to_string(), serde_json::json!(clip));
                }
                extra
            },
        });
        shape.shape_type = "image".to_string();
        // An OLE object (an embedded Excel sheet) keeps its own fill and outline behind the
        // preview picture; plain bitmaps and metafiles draw only the picture.
        if fd.foreign.foreign_type.as_deref() != Some("Object") || shape.parts.is_empty() {
            shape.fill = Some("none".to_string());
            shape.fill_opacity = Some(1.0);
            shape.stroke = Some("#000000".to_string());
            shape.stroke_width = Some(0.0);
            shape.parts.clear();
        }
    }
}

fn is_image_part(part: &str) -> bool {
    matches!(
        Path::new(part)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some(
            "png"
                | "jpeg"
                | "jpg"
                | "gif"
                | "bmp"
                | "webp"
                | "svg"
                | "emf"
                | "wmf"
                | "tif"
                | "tiff"
        )
    )
}

fn cell_num_from_cells(cells: &BTreeMap<String, Cell>, name: &str, fallback: f64) -> f64 {
    cells
        .get(name)
        .and_then(|c| c.value.as_deref())
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}

/// Cell-name prefixes of files written before the first release (two earlier names of the data-graphic cells); only read, never written.
const OLD_DATA_GRAPHIC_PREFIXES: [&str; 2] = ["stencilcraftDG", "vislopioDG"];

/// Reads a User cell. Files written before the first release used two older prefixes for the same cells (`layoutcraftDG` -> `<old>DG`); they are read as well.
fn user_value(node: &ResolvedShape, name: &str) -> Option<String> {
    user_value_exact(node, name).or_else(|| {
        let rest = name.strip_prefix("layoutcraftDG")?;
        OLD_DATA_GRAPHIC_PREFIXES.iter().find_map(|prefix| user_value_exact(node, &format!("{prefix}{rest}")))
    })
}

fn user_value_exact(node: &ResolvedShape, name: &str) -> Option<String> {
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

fn user_number(node: &ResolvedShape, name: &str, fallback: f64) -> f64 {
    user_value(node, name)
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(fallback)
}

fn property_number(node: &ResolvedShape, field: &str) -> Option<f64> {
    let section = node
        .sections
        .iter()
        .find(|section| section.name == "Property")?;
    let row = section
        .rows
        .iter()
        .find(|row| row.name.as_deref() == Some(field))?;
    row.cells
        .get("Value")
        .and_then(|cell| cell.value.as_deref())
        .and_then(|value| value.trim().parse::<f64>().ok())
}

/// Restore LayoutCraft-authored colour rules and callout/bar/icon metadata from
/// the transport `User` rows written by the patch writer.
pub(crate) fn restore_data_graphic(node: &ResolvedShape, shape: &mut Shape) {
    if user_value(node, "layoutcraftDGColor")
        .filter(|value| !value.is_empty())
        .is_some()
    {
        let field = user_value(node, "layoutcraftDGColorField").unwrap_or_default();
        let low = user_value(node, "layoutcraftDGColorLow").unwrap_or_else(|| "#f8696b".to_string());
        let mid = user_value(node, "layoutcraftDGColorMid").unwrap_or_else(|| "#ffeb84".to_string());
        let high = user_value(node, "layoutcraftDGColorHigh").unwrap_or_else(|| "#63be7b".to_string());
        let min = user_number(node, "layoutcraftDGColorMin", 0.0);
        let max = user_number(node, "layoutcraftDGColorMax", 100.0);
        let base_fill = user_value(node, "layoutcraftDGColorBase").filter(|value| !value.is_empty());
        let value = property_number(node, &field).unwrap_or(min);
        let ratio = if (max - min).abs() > 1e-9 {
            ((value - min) / (max - min)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        shape.fill = Some(if ratio < 1.0 / 3.0 {
            low.clone()
        } else if ratio < 2.0 / 3.0 {
            mid.clone()
        } else {
            high.clone()
        });
        shape.data_color = Some(DataColor {
            field,
            min,
            max,
            low,
            mid,
            high,
            base_fill,
            extra: Default::default(),
        });
    }

    let Some(target) = user_value(node, "layoutcraftDGTarget").filter(|value| !value.is_empty())
    else {
        return;
    };
    let raw_kind = user_value(node, "layoutcraftDGKind").unwrap_or_default();
    let icon = raw_kind == "icon"
        || user_value(node, "layoutcraftDGIcon").is_some_and(|value| !value.is_empty());
    let bar = !icon
        && (raw_kind == "bar"
            || user_value(node, "layoutcraftDGBar").is_some_and(|value| !value.is_empty()));
    let heading = !icon && !bar && raw_kind == "heading";
    if bar || icon {
        if bar {
            if let Some(body) = shape.parts.first() {
                if body.fill.as_deref().is_some_and(|fill| fill != "none") {
                    shape.fill = body.fill.clone();
                }
            }
            if let Some(track) = shape.parts.last() {
                if track
                    .stroke
                    .as_deref()
                    .is_some_and(|stroke| stroke != "none")
                {
                    shape.stroke = track.stroke.clone();
                    shape.stroke_width = track.stroke_width.or(shape.stroke_width);
                }
            }
        } else {
            if let Some(body) = shape
                .parts
                .iter()
                .find(|part| part.fill.as_deref().is_some_and(|fill| fill != "none"))
            {
                shape.fill = body.fill.clone();
            }
            if let Some(outline) = shape.parts.iter().find(|part| {
                part.stroke
                    .as_deref()
                    .is_some_and(|stroke| stroke != "none")
            }) {
                shape.stroke = outline.stroke.clone();
                shape.stroke_width = outline.stroke_width.or(shape.stroke_width);
            }
        }
    }
    shape.parts.clear();
    shape.shape_type = if icon {
        "dataIcon"
    } else if bar {
        "dataBar"
    } else {
        "dataCallout"
    }
    .to_string();
    let field = user_value(node, "layoutcraftDGField").unwrap_or_default();
    let label = user_value(node, "layoutcraftDGLabel").unwrap_or_default();
    let position = user_value(node, "layoutcraftDGPosition")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| if icon { "left" } else { "top" }.to_string());
    let min = user_number(node, "layoutcraftDGMin", 0.0);
    let max = user_number(node, "layoutcraftDGMax", 100.0);
    shape.callout = Some(DataGraphic {
        kind: if icon {
            "icon"
        } else if bar {
            "bar"
        } else if heading {
            "heading"
        } else {
            "text"
        }
        .to_string(),
        icon_set: icon.then(|| {
            user_value(node, "layoutcraftDGIconSet")
                .filter(|value| matches!(value.as_str(), "boxes" | "diamonds" | "circles"))
                .unwrap_or_else(|| "boxes".to_string())
        }),
        field,
        label,
        position,
        gap: None,
        auto_size: user_value(node, "layoutcraftDGAuto").as_deref() != Some("0"),
        min: Some(min),
        max: Some(max),
        show_value: Some(if icon {
            false
        } else {
            user_value(node, "layoutcraftDGShowValue").as_deref() != Some("0")
        }),
        target_visio_id: Some(target.clone()),
        extra: Default::default(),
    });
    shape.attached_to = Some(format!("shape:visio:{target}"));
    shape.data.retain(|row| row.name != "msvCalloutField");
}

/// Split `Sheet.22!Prop.IPAddress` into the target shape ID and property name.
pub(crate) fn parse_property_reference(formula: &str) -> Option<(String, String)> {
    let formula = formula.trim();
    let rest = formula.strip_prefix("Sheet.")?;
    let (id, property) = rest.split_once("!Prop.")?;
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) || property.is_empty() {
        return None;
    }
    let name: String = property
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then(|| (id.to_string(), name))
}

/// Recognise a Visio-native data graphic instance (a master whose `User` section
/// carries `msvCalloutType`) and record where it reads from. The shape keeps its
/// cached geometry; nothing is regenerated from this metadata.
pub(crate) fn restore_native_data_graphic(node: &ResolvedShape, shape: &mut Shape) {
    if shape.callout.is_some() {
        return;
    }
    let Some(kind) = user_value(node, "msvCalloutType").and_then(|value| {
        match value.trim().to_ascii_lowercase().as_str() {
            "text callout" => Some("text"),
            "heading" => Some("heading"),
            "data bar" => Some("bar"),
            "icon set" => Some("icon"),
            _ => None,
        }
    }) else {
        return;
    };
    let row = node
        .sections
        .iter()
        .find(|section| section.name == "Property")
        .and_then(|section| {
            section
                .rows
                .iter()
                .find(|row| row.name.as_deref() == Some("msvCalloutField"))
        });
    let value_cell = row.and_then(|row| row.cells.get("Value"));
    let (target, field) = value_cell
        .and_then(|cell| cell.formula.as_deref())
        .and_then(parse_property_reference)
        .map_or((None, String::new()), |(id, name)| (Some(id), name));
    shape.native_data_graphic = Some(NativeDataGraphic {
        kind: kind.to_string(),
        master: node.master_name.clone().unwrap_or_default(),
        target_visio_id: target,
        field,
        label: user_value(node, "LabelText").unwrap_or_default(),
        value: value_cell
            .and_then(|cell| cell.value.as_deref())
            .map(strip_quotes)
            .unwrap_or_default(),
        extra: Default::default(),
    });
}

pub(crate) fn resolve_callout_links(_shapes: &mut [Shape]) {}

#[cfg(test)]
mod native_tests {
    use super::parse_property_reference;

    #[test]
    fn parses_property_reference() {
        assert_eq!(
            parse_property_reference("Sheet.22!Prop.IPAddress"),
            Some(("22".to_string(), "IPAddress".to_string()))
        );
        assert_eq!(
            parse_property_reference("Sheet.7!Prop._VisDM_Operating_System"),
            Some(("7".to_string(), "_VisDM_Operating_System".to_string()))
        );
        assert_eq!(parse_property_reference("Prop.Row_1"), None);
        assert_eq!(parse_property_reference("Sheet.x!Prop.A"), None);
    }
}
