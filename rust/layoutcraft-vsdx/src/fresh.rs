//! Fresh VSDX package writer: one page per model page, with inline shapes,
//! connectors, rich text, Shape Data and rectangle/path geometry.
use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use layoutcraft_model::{Connector, Document, Endpoint, Shape};

use crate::writer::{new_connector_xml, patch_rich_text, NewConnector};
use crate::Result;

pub fn write_fresh_document(document: &Document) -> Result<Vec<u8>> {
    write_fresh_document_with_media(document, &BTreeMap::new())
}

pub fn write_fresh_document_with_media(
    document: &Document,
    media: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>> {
    let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut pages_xml = String::new();
    let mut pages_rels = String::new();
    let mut media_parts: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    let mut media_defaults: BTreeMap<String, String> = BTreeMap::new();
    let mut media_sequence = 1usize;
    for (page_index, page) in document.pages.iter().enumerate() {
        let (page_w, page_h) = page_size(page);
        let (page_xml, rels, page_media) = page_parts(
            page,
            page_index,
            page_h,
            media,
            &mut media_parts,
            &mut media_defaults,
            &mut media_sequence,
        )?;
        entries.extend(page_media);
        let page_part = format!("visio/pages/page{}.xml", page_index + 1);
        entries.insert(page_part.clone(), page_xml.into_bytes());
        entries.insert(
            format!("visio/pages/_rels/page{}.xml.rels", page_index + 1),
            rels.into_bytes(),
        );
        pages_rels.push_str(&format!(
            "<Relationship Id='rId{}' Type='http://schemas.microsoft.com/visio/2010/relationships/page' Target='page{}.xml'/>",
            page_index + 1,
            page_index + 1
        ));
        pages_xml.push_str(&format!(
            "<Page ID='{}' NameU='{}' IsCustomNameU='1' Name='{}' IsCustomName='1' ViewScale='-1' ViewCenterX='{}' ViewCenterY='{}'><PageSheet LineStyle='0' FillStyle='0' TextStyle='0'><Cell N='PageWidth' V='{}' U='IN'/><Cell N='PageHeight' V='{}' U='IN'/><Cell N='PageScale' V='1' U='IN'/><Cell N='DrawingScale' V='1' U='IN_F'/><Cell N='DrawingSizeType' V='0'/><Cell N='DrawingScaleType' V='0'/><Cell N='InhibitSnap' V='0'/><Cell N='DrawingResizeType' V='1'/><Cell N='PrintPageOrientation' V='2'/><Cell N='PaperKind' V='1'/></PageSheet><Rel r:id='rId{}'/></Page>",
            page_index,
            escape_attr(&page.name),
            escape_attr(&page.name),
            page_w / 2.0,
            page_h / 2.0,
            num(page_w),
            num(page_h),
            page_index + 1
        ));
    }
    entries.insert(
        "visio/pages/pages.xml".to_string(),
        format!(
            "<?xml version='1.0' encoding='utf-8' standalone='yes'?><Pages xmlns='http://schemas.microsoft.com/office/visio/2012/main' xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships' xml:space='preserve'>{pages_xml}</Pages>"
        )
        .into_bytes(),
    );
    entries.insert(
        "visio/pages/_rels/pages.xml.rels".to_string(),
        format!(
            "<?xml version='1.0' encoding='utf-8' standalone='yes'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'>{pages_rels}</Relationships>"
        )
        .into_bytes(),
    );
    entries.insert(
        "visio/document.xml".to_string(),
        document_xml().into_bytes(),
    );
    entries.insert(
        "visio/masters/masters.xml".to_string(),
        "<?xml version='1.0' encoding='utf-8' standalone='yes'?><Masters xmlns='http://schemas.microsoft.com/office/visio/2012/main' xml:space='preserve'></Masters>".as_bytes().to_vec(),
    );
    entries.insert(
        "visio/masters/_rels/masters.xml.rels".to_string(),
        "<?xml version='1.0' encoding='utf-8' standalone='yes'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'></Relationships>".as_bytes().to_vec(),
    );
    entries.insert(
        "visio/windows.xml".to_string(),
        "<?xml version='1.0' encoding='utf-8' standalone='yes'?><Windows ClientWidth='1920' ClientHeight='900' xmlns='http://schemas.microsoft.com/office/visio/2012/main' xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships' xml:space='preserve'><Window ID='0' WindowType='Drawing' WindowState='1073741824' WindowLeft='-8' WindowTop='-31' WindowWidth='1936' WindowHeight='939' ContainerType='Page' Page='0' ViewScale='-1' ViewCenterX='5.5' ViewCenterY='4.25'><ShowRulers>1</ShowRulers><ShowGrid>0</ShowGrid><ShowPageBreaks>1</ShowPageBreaks><ShowGuides>1</ShowGuides><ShowConnectionPoints>0</ShowConnectionPoints><GlueSettings>9</GlueSettings><SnapSettings>65847</SnapSettings><SnapExtensions>34</SnapExtensions><SnapAngles/><DynamicGridEnabled>1</DynamicGridEnabled><TabSplitterPos>0.5</TabSplitterPos></Window></Windows>".as_bytes().to_vec(),
    );
    entries.insert(
        "visio/theme/theme1.xml".to_string(),
        theme_xml().as_bytes().to_vec(),
    );
    entries.insert(
        "docProps/core.xml".to_string(),
        "<?xml version='1.0' encoding='utf-8' standalone='yes'?><cp:coreProperties xmlns:cp='http://schemas.openxmlformats.org/package/2006/metadata/core-properties' xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:dcterms='http://purl.org/dc/terms/' xmlns:dcmitype='http://purl.org/dc/dcmitype/' xmlns:xsi='http://www.w3.org/2001/XMLSchema-instance'><dc:title>LayoutCraft</dc:title><dc:creator>LayoutCraft</dc:creator><cp:lastModifiedBy>LayoutCraft</cp:lastModifiedBy><dcterms:created xsi:type='dcterms:W3CDTF'>2026-10-28T00:00:00Z</dcterms:created><dcterms:modified xsi:type='dcterms:W3CDTF'>2026-10-28T00:00:00Z</dcterms:modified></cp:coreProperties>".as_bytes().to_vec(),
    );
    entries.insert(
        "docProps/app.xml".to_string(),
        "<?xml version='1.0' encoding='utf-8' standalone='yes'?><Properties xmlns='http://schemas.openxmlformats.org/officeDocument/2006/extended-properties' xmlns:vt='http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes'><Application>LayoutCraft</Application><ScaleCrop>false</ScaleCrop><Company></Company><LinksUpToDate>false</LinksUpToDate><SharedDoc>false</SharedDoc><HyperlinksChanged>false</HyperlinksChanged><AppVersion>1.0</AppVersion></Properties>".as_bytes().to_vec(),
    );
    entries.insert(
        "visio/_rels/document.xml.rels".to_string(),
        "<?xml version='1.0' encoding='utf-8' standalone='yes'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='rId1' Type='http://schemas.microsoft.com/visio/2010/relationships/masters' Target='masters/masters.xml'/><Relationship Id='rId2' Type='http://schemas.microsoft.com/visio/2010/relationships/pages' Target='pages/pages.xml'/><Relationship Id='rId3' Type='http://schemas.microsoft.com/visio/2010/relationships/windows' Target='windows.xml'/><Relationship Id='rId4' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme' Target='theme/theme1.xml'/></Relationships>".as_bytes().to_vec(),
    );
    entries.insert(
        "_rels/.rels".to_string(),
        "<?xml version='1.0' encoding='utf-8' standalone='yes'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='rId1' Type='http://schemas.microsoft.com/visio/2010/relationships/document' Target='visio/document.xml'/><Relationship Id='rId2' Type='http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties' Target='docProps/core.xml'/><Relationship Id='rId3' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties' Target='docProps/app.xml'/></Relationships>".as_bytes().to_vec(),
    );
    let mut content_types = String::from(
        "<?xml version='1.0' encoding='utf-8' standalone='yes'?><Types xmlns='http://schemas.openxmlformats.org/package/2006/content-types'><Default Extension='rels' ContentType='application/vnd.openxmlformats-package.relationships+xml'/><Default Extension='xml' ContentType='application/xml'/><Override PartName='/visio/document.xml' ContentType='application/vnd.ms-visio.drawing.main+xml'/><Override PartName='/visio/masters/masters.xml' ContentType='application/vnd.ms-visio.masters+xml'/><Override PartName='/visio/pages/pages.xml' ContentType='application/vnd.ms-visio.pages+xml'/><Override PartName='/visio/windows.xml' ContentType='application/vnd.ms-visio.windows+xml'/><Override PartName='/visio/theme/theme1.xml' ContentType='application/vnd.openxmlformats-officedocument.theme+xml'/><Override PartName='/docProps/core.xml' ContentType='application/vnd.openxmlformats-package.core-properties+xml'/><Override PartName='/docProps/app.xml' ContentType='application/vnd.openxmlformats-officedocument.extended-properties+xml'/>",
    );
    for (extension, media_type) in &media_defaults {
        content_types.push_str(&format!(
            "<Default Extension='{}' ContentType='{}'/>",
            escape_attr(extension),
            escape_attr(media_type)
        ));
    }
    for index in 0..document.pages.len() {
        content_types.push_str(&format!(
            "<Override PartName='/visio/pages/page{}.xml' ContentType='application/vnd.ms-visio.page+xml'/>",
            index + 1
        ));
    }
    content_types.push_str("</Types>");
    entries.insert(
        "[Content_Types].xml".to_string(),
        content_types.into_bytes(),
    );

    write_entries(entries)
}

/// What a new, empty drawing looks like: the page size (always stored in inches, as Visio does), whether
/// the page is shown in millimetres (`metric`: the page and drawing scale are "1 mm", as in a Visio drawing
/// made from a metric template) and the page name.
#[derive(Debug, Clone)]
pub struct BlankOptions {
    pub width_in: f64,
    pub height_in: f64,
    pub metric: bool,
    pub name: String,
}

/// Visio's `PaperKind` for a page size (inches, either orientation): 1 Letter, 3 Tabloid, 8 A3, 9 A4, 0 other.
fn paper_kind(width_in: f64, height_in: f64) -> u32 {
    let (short, long) = if width_in <= height_in { (width_in, height_in) } else { (height_in, width_in) };
    let close = |a: f64, b: f64| (a - b).abs() < 0.02;
    if close(short, 8.5) && close(long, 11.0) {
        1
    } else if close(short, 11.0) && close(long, 17.0) {
        3
    } else if close(short, 11.693) && close(long, 16.535) {
        8
    } else if close(short, 8.268) && close(long, 11.693) {
        9
    } else {
        0
    }
}

/// An empty one-page drawing of the given size: the fresh-document package of `write_fresh_document` with
/// the page sheet set to the chosen size, scale, orientation and paper kind, and the window centred on the page.
pub fn write_blank_document(options: &BlankOptions) -> Result<Vec<u8>> {
    let (w, h) = (options.width_in, options.height_in);
    if !(w.is_finite() && h.is_finite()) || !(0.5..=200.0).contains(&w) || !(0.5..=200.0).contains(&h) {
        return Err(crate::Error("the page must be between 0.5 and 200 inches (about 13 mm and 5 m) on each side".to_string()));
    }
    let name: String = options.name.trim().chars().filter(|c| !c.is_control() && !"<>&'\"".contains(*c)).take(31).collect();
    let name = if name.is_empty() { "Page-1".to_string() } else { name };
    let document: Document = serde_json::from_value(serde_json::json!({ "pages": [{ "name": name, "shapes": [], "connectors": [] }] }))
        .map_err(|e| crate::Error(e.to_string()))?;
    let package = crate::Package::from_reader(Cursor::new(write_fresh_document(&document)?))?;
    let mut extras: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut pages = package.text("visio/pages/pages.xml")?.to_string();
    let scale = if options.metric { "<Cell N='PageScale' V='0.03937007874015748' U='MM'/><Cell N='DrawingScale' V='0.03937007874015748' U='MM'/>" } else { "<Cell N='PageScale' V='1' U='IN'/><Cell N='DrawingScale' V='1' U='IN_F'/>" };
    let sheet_start = pages.find("<Cell N='PageWidth'").ok_or_else(|| crate::Error("the fresh page has no page sheet".to_string()))?;
    let sheet_end = pages[sheet_start..].find("<Cell N='DrawingSizeType'").map(|n| sheet_start + n).ok_or_else(|| crate::Error("unexpected page sheet".to_string()))?;
    pages.replace_range(
        sheet_start..sheet_end,
        &format!("<Cell N='PageWidth' V='{}' U='IN'/><Cell N='PageHeight' V='{}' U='IN'/>{scale}", crate::writer::num_text(w), crate::writer::num_text(h)),
    );
    let orientation = if w > h { 2 } else { 1 };
    pages = pages.replace("<Cell N='PrintPageOrientation' V='2'/>", &format!("<Cell N='PrintPageOrientation' V='{orientation}'/>"));
    pages = pages.replace("<Cell N='PaperKind' V='1'/>", &format!("<Cell N='PaperKind' V='{}'/>", paper_kind(w, h)));
    // The view starts at the middle of the page.
    let centre = |text: &str| -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(at) = rest.find("ViewCenterX='") {
            let value_start = at + "ViewCenterX='".len();
            out.push_str(&rest[..value_start]);
            let value_end = rest[value_start..].find('\'').map_or(rest.len(), |n| value_start + n);
            out.push_str(&crate::writer::num_text(w / 2.0));
            rest = &rest[value_end..];
            if let Some(y) = rest.find("ViewCenterY='") {
                let ys = y + "ViewCenterY='".len();
                out.push_str(&rest[..ys]);
                let ye = rest[ys..].find('\'').map_or(rest.len(), |n| ys + n);
                out.push_str(&crate::writer::num_text(h / 2.0));
                rest = &rest[ye..];
            }
        }
        out.push_str(rest);
        out
    };
    pages = centre(&pages);
    extras.insert("visio/pages/pages.xml".to_string(), pages.into_bytes());
    let windows = centre(package.text("visio/windows.xml")?);
    extras.insert("visio/windows.xml".to_string(), windows.into_bytes());
    // The way Visio itself writes a NEW drawing (probe-theme-apply.ps1, step 257 / Q7): the document's styles are Visio's set (No Style, Text Only, None, Normal,
    // Guide, Theme; the "Theme" style holds THEMEVAL() formulas for every line / fill / text property), the default styles are 3 (Normal) and 4 (Guide), and there is NO
    // theme part until one is applied (Design > Themes). Shapes drawn afterwards say LineStyle = FillStyle = TextStyle = 3 and carry no colour cells of their own, so
    // they take their colours from the theme.
    let document = package.text("visio/document.xml")?.to_string();
    let styles = crate::themegen::style_sheets_xml();
    let from = document.find("<StyleSheets>").ok_or_else(|| crate::Error("the fresh document has no style sheets".to_string()))?;
    let to = document.find("</StyleSheets>").map(|n| n + "</StyleSheets>".len()).ok_or_else(|| crate::Error("the fresh document has no style sheets".to_string()))?;
    let mut document = format!("{}{}{}", &document[..from], styles, &document[to..]);
    document = document.replace("DefaultTextStyle='0' DefaultLineStyle='0' DefaultFillStyle='0'", "DefaultTextStyle='3' DefaultLineStyle='3' DefaultFillStyle='3' DefaultGuideStyle='4'");
    extras.insert("visio/document.xml".to_string(), document.into_bytes());
    let rels = package.text("visio/_rels/document.xml.rels")?.to_string();
    if let Some(at) = rels.find("theme/theme1.xml") {
        // drop the relationship that points at the theme part
        let start = rels[..at].rfind("<Relationship ").unwrap_or(0);
        let end = rels[at..].find("/>").map(|n| at + n + 2).unwrap_or(rels.len());
        extras.insert("visio/_rels/document.xml.rels".to_string(), format!("{}{}", &rels[..start], &rels[end..]).into_bytes());
    }
    let types = package.text("[Content_Types].xml")?.to_string();
    if let Some(at) = types.find("<Override PartName='/visio/theme/theme1.xml'") {
        let end = types[at..].find("/>").map(|n| at + n + 2).unwrap_or(types.len());
        extras.insert("[Content_Types].xml".to_string(), format!("{}{}", &types[..at], &types[end..]).into_bytes());
    }
    crate::writer::write_package_edit(&package, extras, &["visio/theme/theme1.xml".to_string()])
}

fn page_size(page: &layoutcraft_model::Page) -> (f64, f64) {
    let mut max_x = 0.0f64;
    let mut max_y = 0.0f64;
    for shape in &page.shapes {
        max_x = max_x.max(shape.x + shape.w);
        max_y = max_y.max(shape.y + shape.h);
    }
    for connector in &page.connectors {
        for endpoint in [&connector.from, &connector.to] {
            if let Some(point) = &endpoint.point {
                if let (Some(x), Some(y)) = (
                    point.get("x").and_then(serde_json::Value::as_f64),
                    point.get("y").and_then(serde_json::Value::as_f64),
                ) {
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
    }
    (
        (max_x / 96.0 + 1.0).max(11.0).ceil(),
        (max_y / 96.0 + 1.0).max(8.5).ceil(),
    )
}

fn page_parts(
    page: &layoutcraft_model::Page,
    page_index: usize,
    page_h: f64,
    media: &BTreeMap<String, Vec<u8>>,
    media_parts: &mut BTreeMap<String, (String, String, String)>,
    media_defaults: &mut BTreeMap<String, String>,
    media_sequence: &mut usize,
) -> Result<(String, String, BTreeMap<String, Vec<u8>>)> {
    let shape_by_visio: BTreeMap<&str, &Shape> = page
        .shapes
        .iter()
        .filter_map(|shape| shape.visio_id.as_deref().map(|id| (id, shape)))
        .collect();
    let mut xml = "<?xml version='1.0' encoding='utf-8' standalone='yes'?><PageContents xmlns='http://schemas.microsoft.com/office/visio/2012/main' xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships' xml:space='preserve'><Shapes>".to_string();
    let mut page_media = BTreeMap::new();
    let mut page_rels = String::new();
    for (index, shape) in page.shapes.iter().enumerate() {
        let id = shape
            .visio_id
            .clone()
            .unwrap_or_else(|| format!("shape{}", index + 1));
        let mut picture = None;
        if let Some(image) = &shape.image {
            if let Some(bytes) = image
                .media_id
                .as_deref()
                .and_then(|media_id| media.get(media_id))
            {
                let media_id = image.media_id.clone().unwrap_or_default();
                let (part, extension, media_type) = if let Some(existing) =
                    media_parts.get(&media_id)
                {
                    existing.clone()
                } else {
                    let extension = image
                        .name
                        .as_deref()
                        .and_then(|name| std::path::Path::new(name).extension())
                        .and_then(|extension| extension.to_str())
                        .unwrap_or("png")
                        .to_ascii_lowercase();
                    let media_type =
                        image
                            .media_type
                            .clone()
                            .unwrap_or_else(|| match extension.as_str() {
                                "jpg" | "jpeg" => "image/jpeg".to_string(),
                                "gif" => "image/gif".to_string(),
                                "bmp" => "image/bmp".to_string(),
                                _ => "image/png".to_string(),
                            });
                    let part = format!("visio/media/image{}_fresh.{extension}", *media_sequence);
                    *media_sequence += 1;
                    media_parts.insert(
                        media_id.clone(),
                        (part.clone(), extension.clone(), media_type.clone()),
                    );
                    media_defaults.insert(extension.clone(), media_type.clone());
                    page_media.insert(part.clone(), bytes.clone());
                    (part, extension, media_type)
                };
                if !page_media.contains_key(&part) {
                    page_media.insert(part.clone(), bytes.clone());
                }
                let rel_id = format!("rIdImg{}_{}", page_index + 1, index + 1);
                page_rels.push_str(&format!(
                    "<Relationship Id='{}' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/image' Target='../media/{}'/>",
                    escape_attr(&rel_id),
                    escape_attr(part.rsplit('/').next().unwrap_or("image.png"))
                ));
                picture = Some((part, extension, media_type, rel_id));
            }
        }
        xml.push_str(&shape_xml(shape, &id, page_h, picture.as_ref()));
    }
    let mut connects = String::new();
    for (index, connector) in page.connectors.iter().enumerate() {
        let id = connector
            .visio_id
            .clone()
            .unwrap_or_else(|| format!("connector{}", index + 1));
        let (from, to) = connector_points(connector, &shape_by_visio, page_h);
        let new = NewConnector::new(&id, from, to)
            .stroke(
                connector
                    .stroke
                    .clone()
                    .unwrap_or_else(|| "#000000".to_string()),
            )
            .stroke_width(connector.stroke_width.unwrap_or(1.0))
            .label(&connector.label)
            .arrow_end(connector.arrow_end.as_deref() != Some("none"));
        xml.push_str(&new_connector_xml(&new, page_h));
        let glue_from = connector.from.shape_id.as_deref().and_then(strip_shape_id);
        let glue_to = connector.to.shape_id.as_deref().and_then(strip_shape_id);
        if let Some(from) = glue_from {
            connects.push_str(&format!(
                "<Connect FromSheet='{}' FromCell='BeginX' FromPart='9' ToSheet='{}' ToCell='PinX' ToPart='3'/>",
                escape_attr(&id),
                escape_attr(from)
            ));
        }
        if let Some(to) = glue_to {
            connects.push_str(&format!(
                "<Connect FromSheet='{}' FromCell='EndX' FromPart='12' ToSheet='{}' ToCell='PinX' ToPart='3'/>",
                escape_attr(&id),
                escape_attr(to)
            ));
        }
    }
    xml.push_str("</Shapes>");
    if !connects.is_empty() {
        xml.push_str(&format!("<Connects>{connects}</Connects>"));
    }
    xml.push_str("</PageContents>");
    let rels = format!(
        "<?xml version='1.0' encoding='utf-8' standalone='yes'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'>{page_rels}</Relationships>"
    );
    Ok((xml, rels, page_media))
}

fn shape_xml(
    shape: &Shape,
    id: &str,
    page_h: f64,
    picture: Option<&(String, String, String, String)>,
) -> String {
    if let Some((_, extension, _, rel_id)) = picture {
        return picture_shape_xml(shape, id, page_h, extension, rel_id);
    }
    let w = shape.w.max(2.0) / 96.0;
    let h = shape.h.max(2.0) / 96.0;
    let pin_x = (shape.x + shape.w / 2.0) / 96.0;
    let pin_y = page_h - (shape.y + shape.h / 2.0) / 96.0;
    let mut out = format!("<Shape ID='{}' Type='Shape'>", escape_attr(id));
    for (name, value) in [
        ("PinX", pin_x),
        ("PinY", pin_y),
        ("Width", w),
        ("Height", h),
        ("LocPinX", w / 2.0),
        ("LocPinY", h / 2.0),
        ("Angle", -shape.rot * std::f64::consts::PI / 180.0),
        ("FlipX", if shape.flip_x { 1.0 } else { 0.0 }),
        ("FlipY", if shape.flip_y { 1.0 } else { 0.0 }),
        (
            "FillPattern",
            if shape.fill.as_deref() == Some("none") {
                0.0
            } else {
                1.0
            },
        ),
        (
            "LinePattern",
            if shape.stroke.as_deref() == Some("none") || shape.stroke_width == Some(0.0) {
                0.0
            } else {
                1.0
            },
        ),
    ] {
        out.push_str(&format!("<Cell N='{name}' V='{}'/>", num(value)));
    }
    if let Some(fill) = &shape.fill {
        if fill != "none" {
            out.push_str(&format!(
                "<Cell N='FillForegnd' V='{}'/>",
                escape_attr(fill)
            ));
        }
    }
    if let Some(stroke) = &shape.stroke {
        if stroke != "none" && shape.stroke_width != Some(0.0) {
            out.push_str(&format!(
                "<Cell N='LineColor' V='{}'/><Cell N='LineWeight' V='{}' U='IN'/>",
                escape_attr(stroke),
                num(shape.stroke_width.unwrap_or(1.0) / 96.0)
            ));
        }
    }
    let parts = &shape.parts;
    if parts.is_empty() {
        out.push_str("<Section N='Geometry' IX='0'><Cell N='NoFill' V='0'/><Cell N='NoLine' V='0'/><Cell N='NoShow' V='0'/>");
        for (index, (x, y)) in [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h), (0.0, 0.0)]
            .iter()
            .enumerate()
        {
            let kind = if index == 0 { "MoveTo" } else { "LineTo" };
            out.push_str(&format!(
                "<Row T='{kind}' IX='{}'><Cell N='X' V='{}'/><Cell N='Y' V='{}'/></Row>",
                index + 1,
                num(*x),
                num(*y)
            ));
        }
        out.push_str("</Section>");
    } else {
        for (index, part) in parts.iter().enumerate() {
            let no_fill = part.fill.as_deref() == Some("none");
            let no_line = part.stroke.as_deref() == Some("none") || part.stroke_width == Some(0.0);
            out.push_str(&format!(
                "<Section N='Geometry' IX='{}'><Cell N='NoFill' V='{}'/><Cell N='NoLine' V='{}'/><Cell N='NoShow' V='0'/>",
                index,
                if no_fill { 1 } else { 0 },
                if no_line { 1 } else { 0 }
            ));
            for row in path_rows(&part.d, w, h) {
                out.push_str(&format!(
                    "<Row T='{}' IX='{}'><Cell N='X' V='{}'/><Cell N='Y' V='{}'/></Row>",
                    escape_attr(&row.0),
                    row.1,
                    num(row.2),
                    num(row.3)
                ));
            }
            out.push_str("</Section>");
        }
    }
    let font_size = shape.font_size.unwrap_or(12.0).max(1.0) / 96.0;
    let color = shape.text_color.as_deref().unwrap_or("#1b2733");
    let align = match shape.align.as_deref().unwrap_or("center") {
        "left" => 0,
        "right" => 2,
        _ => 1,
    };
    out.push_str(&format!(
        "<Section N='Character'><Row IX='0'><Cell N='Font' V='{}'/><Cell N='Color' V='{}'/><Cell N='Size' V='{}' U='PT'/><Cell N='Style' V='{}'/><Cell N='LangID' V='en-GB'/></Row></Section><Section N='Paragraph'><Row IX='0'><Cell N='HorzAlign' V='{align}'/><Cell N='Bullet' V='0'/><Cell N='BulletStr' V=''/></Row></Section>",
        escape_attr(shape.font_family.as_deref().unwrap_or("Calibri")),
        escape_attr(color),
        num(font_size),
        (if shape.bold.unwrap_or(false) { 1 } else { 0 })
            | (if shape.italic.unwrap_or(false) { 2 } else { 0 })
            | (if shape.underline.unwrap_or(false) { 4 } else { 0 })
    ));
    if !shape.data.is_empty() {
        out.push_str("<Section N='Property'>");
        for row in &shape.data {
            out.push_str(&format!(
                "<Row N='{}'><Cell N='Value' V='{}' U='STR'/><Cell N='Label' V='{}'/><Cell N='Prompt' V='{}'/><Cell N='Format' V='{}'/><Cell N='Type' V='{}'/><Cell N='Invisible' V='{}'/></Row>",
                escape_attr(&row.name),
                escape_attr(&row.value),
                escape_attr(&row.label),
                escape_attr(&row.prompt),
                escape_attr(&row.format),
                row.r#type,
                row.invisible
            ));
        }
        out.push_str("</Section>");
    }
    let label = if shape.label.is_empty() {
        String::new()
    } else {
        format!("<Text>{}</Text>", escape_text(&shape.label))
    };
    out.push_str(&label);
    out.push_str("</Shape>");
    if let Some(rich) = &shape.rich {
        out = patch_rich_text(&out, rich);
    }
    out
}

fn picture_shape_xml(
    shape: &Shape,
    id: &str,
    page_h: f64,
    extension: &str,
    rel_id: &str,
) -> String {
    let w = shape.w.max(2.0) / 96.0;
    let h = shape.h.max(2.0) / 96.0;
    let pin_x = (shape.x + shape.w / 2.0) / 96.0;
    let pin_y = page_h - (shape.y + shape.h / 2.0) / 96.0;
    let mut out = format!("<Shape ID='{}' Type='Foreign'>", escape_attr(id));
    for (name, value) in [
        ("PinX", pin_x),
        ("PinY", pin_y),
        ("Width", w),
        ("Height", h),
        ("LocPinX", w / 2.0),
        ("LocPinY", h / 2.0),
        ("Angle", -shape.rot * std::f64::consts::PI / 180.0),
        ("FlipX", if shape.flip_x { 1.0 } else { 0.0 }),
        ("FlipY", if shape.flip_y { 1.0 } else { 0.0 }),
        ("ImgOffsetX", 0.0),
        ("ImgOffsetY", 0.0),
        ("ImgWidth", w),
        ("ImgHeight", h),
        ("FillPattern", 0.0),
        ("LinePattern", 0.0),
    ] {
        out.push_str(&format!("<Cell N='{name}' V='{}'/>", num(value)));
    }
    out.push_str("<Section N='Geometry' IX='0'><Cell N='NoFill' V='0'/><Cell N='NoLine' V='0'/><Cell N='NoShow' V='0'/>");
    for (index, (x, y)) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]
        .iter()
        .enumerate()
    {
        let kind = if index == 0 { "RelMoveTo" } else { "RelLineTo" };
        out.push_str(&format!(
            "<Row T='{kind}' IX='{}'><Cell N='X' V='{}'/><Cell N='Y' V='{}'/></Row>",
            index + 1,
            num(*x),
            num(*y)
        ));
    }
    out.push_str(&format!(
        "</Section><ForeignData ForeignType='Bitmap' CompressionType='{}'><Rel r:id='{}'/></ForeignData>",
        escape_attr(&extension.to_ascii_uppercase()),
        escape_attr(rel_id)
    ));
    let label = if shape.label.is_empty() {
        shape
            .image
            .as_ref()
            .and_then(|image| image.name.clone())
            .unwrap_or_default()
    } else {
        shape.label.clone()
    };
    if !label.is_empty() {
        out.push_str(&format!("<Text>{}</Text>", escape_text(&label)));
    }
    out.push_str("</Shape>");
    out
}

fn path_rows(path: &str, w: f64, h: f64) -> Vec<(String, usize, f64, f64)> {
    let mut rows = Vec::new();
    let mut numbers = Vec::new();
    let mut current = String::new();
    let mut commands = Vec::new();
    for c in path.chars() {
        if c.is_ascii_alphabetic() {
            if !current.is_empty() {
                if let Ok(value) = current.parse::<f64>() {
                    numbers.push(value);
                }
                current.clear();
            }
            commands.push(c.to_string());
        } else if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E' {
            current.push(c);
        } else if !current.is_empty() {
            if let Ok(value) = current.parse::<f64>() {
                numbers.push(value);
            }
            current.clear();
        }
    }
    if !current.is_empty() {
        if let Ok(value) = current.parse::<f64>() {
            numbers.push(value);
        }
    }
    let mut index = 0usize;
    let mut numbers_at = 0usize;
    let mut x: f64 = 0.0;
    let mut y: f64 = 0.0;
    let mut start = (0.0f64, 0.0f64);
    for command in commands {
        let count = match command.to_ascii_uppercase().as_str() {
            "M" | "L" | "T" => 2,
            "H" | "V" => 1,
            "C" => 6,
            "S" | "Q" => 4,
            "A" => 7,
            "Z" => 0,
            _ => continue,
        };
        if command.eq_ignore_ascii_case("z") {
            if (x - start.0).abs() > 1e-9 || (y - start.1).abs() > 1e-9 {
                index += 1;
                rows.push((
                    "LineTo".to_string(),
                    index,
                    start.0 / 96.0,
                    h - start.1 / 96.0,
                ));
            }
            continue;
        }
        while numbers_at + count <= numbers.len() {
            let values = &numbers[numbers_at..numbers_at + count];
            numbers_at += count;
            let (cmd, px, py) = match command.to_ascii_uppercase().as_str() {
                "M" => {
                    x = values[0];
                    y = values[1];
                    start = (x, y);
                    ("MoveTo", x, y)
                }
                "L" => {
                    x = values[0];
                    y = values[1];
                    ("LineTo", x, y)
                }
                "H" => {
                    x = values[0];
                    ("LineTo", x, y)
                }
                "V" => {
                    y = values[0];
                    ("LineTo", x, y)
                }
                "C" => {
                    x = values[4];
                    y = values[5];
                    ("LineTo", x, y)
                }
                "S" | "Q" => {
                    x = values[2];
                    y = values[3];
                    ("LineTo", x, y)
                }
                "A" => {
                    x = values[5];
                    y = values[6];
                    ("LineTo", x, y)
                }
                "T" => {
                    x = values[0];
                    y = values[1];
                    ("LineTo", x, y)
                }
                _ => continue,
            };
            index += 1;
            rows.push((cmd.to_string(), index, px / 96.0, h - py / 96.0));
        }
    }
    if rows.is_empty() {
        index += 1;
        rows.push(("MoveTo".to_string(), index, 0.0, h));
        index += 1;
        rows.push(("LineTo".to_string(), index, w, h));
    }
    rows
}

fn connector_points(
    connector: &Connector,
    shapes: &BTreeMap<&str, &Shape>,
    page_h: f64,
) -> ((f64, f64), (f64, f64)) {
    (
        endpoint_point(&connector.from, shapes, page_h),
        endpoint_point(&connector.to, shapes, page_h),
    )
}

fn endpoint_point(endpoint: &Endpoint, shapes: &BTreeMap<&str, &Shape>, page_h: f64) -> (f64, f64) {
    let _ = page_h;
    if let Some(point) = &endpoint.point {
        if let (Some(x), Some(y)) = (
            point.get("x").and_then(serde_json::Value::as_f64),
            point.get("y").and_then(serde_json::Value::as_f64),
        ) {
            return (x, y);
        }
    }
    let Some(shape_id) = endpoint.shape_id.as_deref().and_then(strip_shape_id) else {
        return (0.0, 0.0);
    };
    let Some(shape) = shapes.get(shape_id) else {
        return (0.0, 0.0);
    };
    let lx = endpoint
        .local
        .as_ref()
        .and_then(|local| local.get("x").and_then(serde_json::Value::as_f64))
        .unwrap_or(0.5);
    let ly = endpoint
        .local
        .as_ref()
        .and_then(|local| local.get("y").and_then(serde_json::Value::as_f64))
        .unwrap_or(0.5);
    (shape.x + lx * shape.w, shape.y + ly * shape.h)
}

fn strip_shape_id(value: &str) -> Option<&str> {
    Some(value.strip_prefix("shape:visio:").unwrap_or(value))
}

fn document_xml() -> String {
    "<?xml version='1.0' encoding='utf-8' standalone='yes'?><VisioDocument xmlns='http://schemas.microsoft.com/office/visio/2012/main' xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships' xml:space='preserve'><DocumentSettings TopPage='0' DefaultTextStyle='0' DefaultLineStyle='0' DefaultFillStyle='0'><GlueSettings>9</GlueSettings><SnapSettings>65847</SnapSettings><SnapExtensions>34</SnapExtensions><SnapAngles/><DynamicGridEnabled>1</DynamicGridEnabled><ProtectStyles>0</ProtectStyles><ProtectShapes>0</ProtectShapes><ProtectMasters>0</ProtectMasters><ProtectBkgnds>0</ProtectBkgnds></DocumentSettings><Colors><ColorEntry IX='24' RGB='#7F7F7F'/></Colors><StyleSheets><StyleSheet ID='0' NameU='No Style' IsCustomNameU='1' Name='No Style' IsCustomName='1'><Cell N='EnableLineProps' V='1'/><Cell N='EnableFillProps' V='1'/><Cell N='EnableTextProps' V='1'/><Cell N='LineWeight' V='0.01041666666666667'/><Cell N='LineColor' V='0'/><Cell N='LinePattern' V='1'/><Cell N='FillForegnd' V='1'/><Cell N='FillPattern' V='1'/><Cell N='VerticalAlign' V='1'/></StyleSheet></StyleSheets><DocumentSheet NameU='TheDoc' IsCustomNameU='1' Name='TheDoc' IsCustomName='1' LineStyle='0' FillStyle='0' TextStyle='0'><Cell N='OutputFormat' V='0'/><Cell N='LockPreview' V='0'/><Cell N='AddMarkup' V='0'/><Cell N='ViewMarkup' V='0'/><Cell N='DocLockReplace' V='0' U='BOOL'/><Cell N='NoCoauth' V='0' U='BOOL'/><Cell N='DocLockDuplicatePage' V='0' U='BOOL'/><Cell N='PreviewQuality' V='0'/><Cell N='PreviewScope' V='0'/><Cell N='DocLangID' V='en-US'/></DocumentSheet></VisioDocument>".to_string()
}

fn theme_xml() -> String {
    "<?xml version='1.0' encoding='utf-8' standalone='yes'?><a:theme xmlns:a='http://schemas.openxmlformats.org/drawingml/2006/main' name='LayoutCraft'><a:themeElements><a:clrScheme name='LayoutCraft'><a:dk1><a:srgbClr val='000000'/></a:dk1><a:lt1><a:srgbClr val='FFFFFF'/></a:lt1><a:dk2><a:srgbClr val='44546A'/></a:dk2><a:lt2><a:srgbClr val='E7E6E6'/></a:lt2><a:accent1><a:srgbClr val='4472C4'/></a:accent1><a:accent2><a:srgbClr val='ED7D31'/></a:accent2><a:accent3><a:srgbClr val='A5A5A5'/></a:accent3><a:accent4><a:srgbClr val='FFC000'/></a:accent4><a:accent5><a:srgbClr val='5B9BD5'/></a:accent5><a:accent6><a:srgbClr val='70AD47'/></a:accent6><a:hlink><a:srgbClr val='0563C1'/></a:hlink><a:folHlink><a:srgbClr val='954F72'/></a:folHlink></a:clrScheme><a:fontScheme name='LayoutCraft'><a:majorFont><a:latin typeface='Calibri'/><a:ea typeface=''/><a:cs typeface=''/></a:majorFont><a:minorFont><a:latin typeface='Calibri'/><a:ea typeface=''/><a:cs typeface=''/></a:minorFont></a:fontScheme><a:fmtScheme name='LayoutCraft'><a:fillStyleLst><a:solidFill><a:schemeClr val='phClr'/></a:solidFill><a:solidFill><a:schemeClr val='phClr'/></a:solidFill><a:solidFill><a:schemeClr val='phClr'/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w='6350' cap='flat' cmpd='sng' algn='ctr'><a:solidFill><a:schemeClr val='phClr'/></a:solidFill><a:prstDash val='solid'/><a:miter lim='800000'/></a:ln><a:ln w='6350' cap='flat' cmpd='sng' algn='ctr'><a:solidFill><a:schemeClr val='phClr'/></a:solidFill><a:prstDash val='solid'/><a:miter lim='800000'/></a:ln><a:ln w='6350' cap='flat' cmpd='sng' algn='ctr'><a:solidFill><a:schemeClr val='phClr'/></a:solidFill><a:prstDash val='solid'/><a:miter lim='800000'/></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val='phClr'/></a:solidFill><a:solidFill><a:schemeClr val='phClr'/></a:solidFill><a:solidFill><a:schemeClr val='phClr'/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>".to_string()
}

fn write_entries(entries: BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let order = [
        "[Content_Types].xml",
        "_rels/.rels",
        "visio/document.xml",
        "visio/_rels/document.xml.rels",
        "visio/pages/pages.xml",
        "visio/pages/_rels/pages.xml.rels",
    ];
    for name in order {
        if let Some(bytes) = entries.get(name) {
            writer.start_file(name, options)?;
            writer.write_all(bytes)?;
        }
    }
    for (name, bytes) in &entries {
        if order.contains(&name.as_str()) {
            continue;
        }
        writer.start_file(name, options)?;
        writer.write_all(bytes)?;
    }
    let cursor = writer.finish()?;
    Ok(cursor.into_inner())
}

fn num(value: f64) -> String {
    let rounded = (value * 1e9).round() / 1e9;
    if (rounded - rounded.round()).abs() < 1e-12 {
        format!("{}", rounded.round() as i64)
    } else {
        let mut text = format!("{rounded:.9}");
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
        text
    }
}

fn escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('\'', "&apos;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
