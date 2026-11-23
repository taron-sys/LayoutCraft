//! Evidence: make-builtin-drawing.mjs and check-builtin-in-visio.ps1 (drawings that use these masters are opened in Visio over COM).
//!
//! A small built-in stencil ("Basic shapes") that needs no file: the shapes of the draw tools and a few flowchart
//! blocks, as real masters. `builtin_stencil` builds it as a stencil package in memory; the shell opens it like any
//! stencil, so a master is copied into the drawing on its first drop (`stencil::import_master`: master part, content
//! types, the page's relationship to it) and Visio shows the instances as instances of masters of the drawing.
use std::io::{Cursor, Write};

use crate::draw::{closed_shape_xml, connection_rows, line_shape_xml, polygon, SIDES};
use crate::writer::{escape_attr, num_text};
use crate::Result;

/// The ID of the top shape of every master. Visio drops a master whose top shape has ID 1 (found by bisecting
/// a drawing in Visio: instances vanish and the master reads back empty); Visio itself writes 5.
const MASTER_SHAPE_ID: u64 = 5;

#[derive(Clone, Copy)]
enum Form {
    Poly(&'static [(f64, f64)]),
    Ellipse,
    /// A rectangle with rounded corners: the corner radius as a fraction of the height.
    Rounded(f64),
    Line { arrow: bool },
}

struct Block {
    name: &'static str,
    w: f64,
    h: f64,
    form: Form,
}

const RECT: &[(f64, f64)] = &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
const DIAMOND: &[(f64, f64)] = &[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)];
const TRIANGLE: &[(f64, f64)] = &[(0.0, 0.0), (1.0, 0.0), (0.5, 1.0)];
const HEXAGON: &[(f64, f64)] = &[(0.25, 0.0), (0.75, 0.0), (1.0, 0.5), (0.75, 1.0), (0.25, 1.0), (0.0, 0.5)];
const PENTAGON: &[(f64, f64)] = &[(0.5, 1.0), (1.0, 0.62), (0.81, 0.0), (0.19, 0.0), (0.0, 0.62)];
const CROSS: &[(f64, f64)] = &[(0.33, 0.0), (0.67, 0.0), (0.67, 0.33), (1.0, 0.33), (1.0, 0.67), (0.67, 0.67), (0.67, 1.0), (0.33, 1.0), (0.33, 0.67), (0.0, 0.67), (0.0, 0.33), (0.33, 0.33)];
const RIGHT_ARROW: &[(f64, f64)] = &[(0.0, 0.3), (0.6, 0.3), (0.6, 0.0), (1.0, 0.5), (0.6, 1.0), (0.6, 0.7), (0.0, 0.7)];
const PARALLELOGRAM: &[(f64, f64)] = &[(0.2, 0.0), (1.0, 0.0), (0.8, 1.0), (0.0, 1.0)];
const TRAPEZOID: &[(f64, f64)] = &[(0.2, 0.0), (0.8, 0.0), (1.0, 1.0), (0.0, 1.0)];
/// The flowchart Document: a rectangle whose bottom edge is one wave (sampled in eighths, so it needs no curve rows).
const DOCUMENT: &[(f64, f64)] = &[(0.0, 1.0), (1.0, 1.0), (1.0, 0.12), (0.875, 0.19), (0.75, 0.22), (0.625, 0.19), (0.5, 0.12), (0.375, 0.05), (0.25, 0.02), (0.125, 0.05), (0.0, 0.12)];

const BLOCKS: &[Block] = &[
    Block { name: "Rectangle", w: 1.5, h: 1.0, form: Form::Poly(RECT) },
    Block { name: "Rounded rectangle", w: 1.5, h: 1.0, form: Form::Rounded(0.15) },
    Block { name: "Ellipse", w: 1.5, h: 1.0, form: Form::Ellipse },
    Block { name: "Triangle", w: 1.25, h: 1.0, form: Form::Poly(TRIANGLE) },
    Block { name: "Hexagon", w: 1.5, h: 1.0, form: Form::Poly(HEXAGON) },
    Block { name: "Pentagon", w: 1.25, h: 1.25, form: Form::Poly(PENTAGON) },
    Block { name: "Cross", w: 1.0, h: 1.0, form: Form::Poly(CROSS) },
    Block { name: "Right arrow", w: 1.5, h: 1.0, form: Form::Poly(RIGHT_ARROW) },
    Block { name: "Line", w: 1.5, h: 0.0, form: Form::Line { arrow: false } },
    Block { name: "Arrow line", w: 1.5, h: 0.0, form: Form::Line { arrow: true } },
    Block { name: "Process", w: 1.5, h: 1.0, form: Form::Poly(RECT) },
    Block { name: "Decision", w: 1.5, h: 1.0, form: Form::Poly(DIAMOND) },
    Block { name: "Terminator", w: 1.5, h: 0.75, form: Form::Rounded(0.5) },
    Block { name: "Data", w: 1.5, h: 1.0, form: Form::Poly(PARALLELOGRAM) },
    Block { name: "Manual operation", w: 1.5, h: 1.0, form: Form::Poly(TRAPEZOID) },
    Block { name: "Document", w: 1.5, h: 1.0, form: Form::Poly(DOCUMENT) },
];

/// The ID the stencil gives master `index` (1-based) and its stable unique id.
fn unique_id(index: usize) -> String {
    format!("{{56495350-4C4F-4000-8000-{index:012X}}}")
}

fn master_shape(block: &Block) -> String {
    let (w, h) = (block.w, block.h);
    match block.form {
        Form::Line { arrow } => {
            let xml = line_shape_xml(MASTER_SHAPE_ID, block.name, 0.0, 0.0, w, 0.0, Some(3));
            if arrow {
                xml.replacen("<Section N='Geometry'", "<Cell N='EndArrow' V='4'/><Cell N='EndArrowSize' V='2'/><Section N='Geometry'", 1)
            } else {
                xml
            }
        }
        Form::Ellipse => {
            let geometry = format!(
                "<Row T='Ellipse' IX='1'><Cell N='X' V='{}' F='Width*0.5'/><Cell N='Y' V='{}' F='Height*0.5'/><Cell N='A' V='{}' U='DL' F='Width*1'/><Cell N='B' V='{}' U='DL' F='Height*0.5'/><Cell N='C' V='{}' U='DL' F='Width*0.5'/><Cell N='D' V='{}' U='DL' F='Height*1'/></Row>",
                num_text(w / 2.0), num_text(h / 2.0), num_text(w), num_text(h / 2.0), num_text(w / 2.0), num_text(h)
            );
            closed_shape_xml(MASTER_SHAPE_ID, block.name, 0.0, 0.0, w, h, false, None, &geometry, &connection_rows(&SIDES, w, h), Some(3))
        }
        Form::Rounded(fraction) => closed_shape_xml(MASTER_SHAPE_ID, block.name, 0.0, 0.0, w, h, false, Some(h * fraction), &polygon(RECT, w, h), &connection_rows(&SIDES, w, h), Some(3)),
        Form::Poly(points) => {
            // Connection points: the middles of the bounding box sides (the shapes are symmetric enough for that).
            closed_shape_xml(MASTER_SHAPE_ID, block.name, 0.0, 0.0, w, h, false, None, &polygon(points, w, h), &connection_rows(&SIDES, w, h), Some(3))
        }
    }
}

/// The master list `[(name, w, h)]` in master-id order (id = position + 1).
pub fn builtin_names() -> Vec<(&'static str, f64, f64)> {
    BLOCKS.iter().map(|b| (b.name, b.w, b.h)).collect()
}

/// The package bytes of the built-in stencil.
pub fn builtin_stencil() -> Result<Vec<u8>> {
    const NS: &str = "xmlns='http://schemas.microsoft.com/office/visio/2012/main' xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships'";
    let mut masters = format!("<?xml version='1.0' encoding='utf-8' ?><Masters {NS} xml:space='preserve'>");
    let mut rels = String::from("<?xml version='1.0' encoding='UTF-8' standalone='yes'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'>");
    let mut types = String::from("<?xml version='1.0' encoding='UTF-8' standalone='yes'?><Types xmlns='http://schemas.openxmlformats.org/package/2006/content-types'><Default Extension='rels' ContentType='application/vnd.openxmlformats-package.relationships+xml'/><Default Extension='xml' ContentType='application/xml'/><Override PartName='/visio/document.xml' ContentType='application/vnd.ms-visio.stencil.main+xml'/><Override PartName='/visio/masters/masters.xml' ContentType='application/vnd.ms-visio.masters+xml'/>");
    let mut parts: Vec<(String, String)> = Vec::new();
    for (i, block) in BLOCKS.iter().enumerate() {
        let n = i + 1;
        let name = escape_attr(block.name);
        masters.push_str(&format!(
            "<Master ID='{n}' NameU='{name}' IsCustomNameU='1' Name='{name}' IsCustomName='1' Prompt='' IconSize='1' AlignName='2' MatchByName='0' IconUpdate='0' UniqueID='{u}' BaseID='{u}' PatternFlags='0' Hidden='0' MasterType='1'><PageSheet LineStyle='0' FillStyle='0' TextStyle='0'><Cell N='PageWidth' V='8.5'/><Cell N='PageHeight' V='11'/><Cell N='PageScale' V='1' U='IN'/><Cell N='DrawingScale' V='1' U='IN_F'/></PageSheet><Rel r:id='rId{n}'/></Master>",
            u = unique_id(n)
        ));
        rels.push_str(&format!("<Relationship Id='rId{n}' Type='http://schemas.microsoft.com/visio/2010/relationships/master' Target='master{n}.xml'/>"));
        types.push_str(&format!("<Override PartName='/visio/masters/master{n}.xml' ContentType='application/vnd.ms-visio.master+xml'/>"));
        parts.push((
            format!("visio/masters/master{n}.xml"),
            format!("<?xml version='1.0' encoding='utf-8' ?><MasterContents {NS} xml:space='preserve'><Shapes>{}</Shapes></MasterContents>", master_shape(block)),
        ));
    }
    masters.push_str("</Masters>");
    rels.push_str("</Relationships>");
    types.push_str("</Types>");
    let document = format!("<?xml version='1.0' encoding='utf-8' ?><VisioDocument {NS} xml:space='preserve'><DocumentSettings TopPage='0' DefaultTextStyle='3' DefaultLineStyle='3' DefaultFillStyle='3' DefaultGuideStyle='4'/>{}</VisioDocument>", crate::themegen::style_sheets_xml());
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut put = |name: &str, text: &str| -> Result<()> {
        writer.start_file(name, options)?;
        writer.write_all(text.as_bytes())?;
        Ok(())
    };
    put("[Content_Types].xml", &types)?;
    put("_rels/.rels", "<?xml version='1.0' encoding='UTF-8' standalone='yes'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='rId1' Type='http://schemas.microsoft.com/visio/2010/relationships/document' Target='visio/document.xml'/></Relationships>")?;
    put("visio/document.xml", &document)?;
    put("visio/_rels/document.xml.rels", "<?xml version='1.0' encoding='UTF-8' standalone='yes'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='rId1' Type='http://schemas.microsoft.com/visio/2010/relationships/masters' Target='masters/masters.xml'/></Relationships>")?;
    put("visio/masters/masters.xml", &masters)?;
    put("visio/masters/_rels/masters.xml.rels", &rels)?;
    for (name, xml) in &parts {
        put(name, xml)?;
    }
    Ok(writer.finish()?.into_inner())
}
