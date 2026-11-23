//! Drawing scale: PageScale / DrawingScale (values in inches, display unit in U) and DrawingScaleType in the page sheet; the
//! shapes' own cells and the paper size stay; the shell's page index counts only pages that are not backgrounds.
use layoutcraft_vsdx::draw::{draw_shape, DrawKind};
use layoutcraft_vsdx::pageops::{add_page, is_metric_unit, make_background, page_scale, set_drawing_scale};
use layoutcraft_vsdx::{write_blank_document, BlankOptions, Package};

fn blank(metric: bool) -> Package {
    let bytes = write_blank_document(&BlankOptions { width_in: 8.5, height_in: 11.0, metric, name: "Main".to_string() }).unwrap();
    let p = Package::from_bytes(&bytes).unwrap();
    let (b, _) = draw_shape(&p, 0, DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0).unwrap();
    Package::from_bytes(&b).unwrap()
}

fn sheet_cell(p: &Package, name: &str) -> String {
    let xml = p.text("visio/pages/pages.xml").unwrap();
    let at = xml.find(&format!("<Cell N='{name}'")).unwrap_or_else(|| panic!("no {name} in {xml}"));
    xml[at..at + xml[at..].find('>').unwrap() + 1].to_string()
}

#[test]
fn a_new_drawing_is_one_to_one() {
    let inch = page_scale(&blank(false), 0).unwrap();
    assert_eq!((inch.paper, inch.drawing, inch.scale_type), (1.0, 1.0, 0));
    assert!(!is_metric_unit(&inch.paper_unit));
    let mm = page_scale(&blank(true), 0).unwrap();
    assert!(is_metric_unit(&mm.paper_unit) && (mm.paper - 1.0).abs() < 1e-9, "{mm:?}");
}

#[test]
fn architectural_scale_is_written_like_visio() {
    let p = blank(false);
    let p = Package::from_bytes(&set_drawing_scale(&p, 0, 0.25, "in", 1.0, "ft").unwrap()).unwrap();
    assert!(sheet_cell(&p, "PageScale").contains("V='0.25'") && sheet_cell(&p, "PageScale").contains("U='IN'"), "{}", sheet_cell(&p, "PageScale"));
    assert!(sheet_cell(&p, "DrawingScale").contains("V='12'") && sheet_cell(&p, "DrawingScale").contains("U='FT'"));
    assert!(sheet_cell(&p, "DrawingScaleType").contains("V='1'"));
    let s = page_scale(&p, 0).unwrap();
    assert_eq!((s.paper, s.paper_unit.as_str(), s.drawing, s.drawing_unit.as_str(), s.scale_type), (0.25, "IN", 1.0, "FT", 1));
}

#[test]
fn metric_custom_and_none() {
    let p = blank(true);
    let p = Package::from_bytes(&set_drawing_scale(&p, 0, 1.0, "mm", 50.0, "mm").unwrap()).unwrap();
    assert!(sheet_cell(&p, "PageScale").contains("V='0.0393700787") && sheet_cell(&p, "DrawingScale").contains("V='1.968503937") && sheet_cell(&p, "DrawingScaleType").contains("V='4'"));
    let s = page_scale(&p, 0).unwrap();
    assert!((s.paper - 1.0).abs() < 1e-9 && (s.drawing - 50.0).abs() < 1e-9 && s.drawing_unit == "MM" && s.scale_type == 4, "{s:?}");
    let p = Package::from_bytes(&set_drawing_scale(&p, 0, 1.0, "in", 3.0, "m").unwrap()).unwrap();
    assert_eq!(page_scale(&p, 0).unwrap().scale_type, 5, "mixed units are a custom scale");
    let p = Package::from_bytes(&set_drawing_scale(&p, 0, 2.0, "cm", 20.0, "mm").unwrap()).unwrap();
    assert_eq!(page_scale(&p, 0).unwrap().scale_type, 0, "equal lengths in different units are 1:1");
}

#[test]
fn shapes_and_paper_size_stay() {
    let p = blank(false);
    let page_part = p.pages().unwrap()[0].part.clone();
    let before = p.entry(&page_part).unwrap().to_vec();
    let size = (p.pages().unwrap()[0].width, p.pages().unwrap()[0].height);
    let q = Package::from_bytes(&set_drawing_scale(&p, 0, 1.0, "in", 4.0, "ft").unwrap()).unwrap();
    assert_eq!(q.entry(&page_part).unwrap(), &before[..], "the page contents are untouched");
    assert_eq!((q.pages().unwrap()[0].width, q.pages().unwrap()[0].height), size);
}

#[test]
fn bad_scales_are_refused() {
    let p = blank(false);
    assert!(set_drawing_scale(&p, 0, 0.0, "in", 1.0, "ft").is_err());
    assert!(set_drawing_scale(&p, 0, 1.0, "in", f64::NAN, "ft").is_err());
    assert!(set_drawing_scale(&p, 0, 1.0, "furlong", 1.0, "ft").is_err());
    assert!(set_drawing_scale(&p, 0, 1.0, "mm", 1e9, "m").is_err(), "beyond 1:10000");
    assert!(set_drawing_scale(&p, 5, 1.0, "in", 1.0, "ft").is_err(), "no such page");
}

#[test]
fn the_page_index_skips_background_pages() {
    let p = blank(false);
    let (b, _) = add_page(&p, 0, Some("Second")).unwrap();
    let p = Package::from_bytes(&b).unwrap();
    let p = Package::from_bytes(&make_background(&p, 0).unwrap()).unwrap(); // "Main" is a background now, "Second" is shown page 0
    let p = Package::from_bytes(&set_drawing_scale(&p, 0, 1.0, "in", 2.0, "ft").unwrap()).unwrap();
    assert_eq!(page_scale(&p, 0).unwrap().drawing_unit, "FT");
    // the background page (first in the file) kept its 1:1
    let xml = p.text("visio/pages/pages.xml").unwrap();
    let second = xml.find("NameU='Second'").unwrap();
    assert!(!xml[..second].contains("U='FT'"), "the background page was changed: {xml}");
}
