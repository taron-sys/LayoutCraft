//! Print setup of a page (Page Setup > Print Setup): the cells of the page sheet Visio uses, probed with visio-test-harness/probe-print-setup.ps1.
use std::io::Cursor;

use layoutcraft_vsdx::pageops::{print_setup, set_print_setup, PrintSetup};
use layoutcraft_vsdx::{write_blank_document, BlankOptions, Package};

fn blank() -> Package {
    Package::from_reader(Cursor::new(write_blank_document(&BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "Page-1".into() }).unwrap())).unwrap()
}

fn page_sheet(package: &Package) -> String {
    let xml = package.text("visio/pages/pages.xml").unwrap().to_string();
    let a = xml.find("<PageSheet").unwrap();
    xml[a..a + xml[a..].find("</PageSheet>").unwrap()].to_string()
}

#[test]
fn a_new_drawing_has_the_default_print_setup_and_landscape_orientation() {
    let setup = print_setup(&blank(), 0).unwrap();
    assert_eq!(setup.orientation, 2, "a landscape page");
    assert_eq!(setup.margins, [0.25; 4]);
    assert_eq!((setup.fit, setup.pages, setup.scale, setup.center, setup.grid), (false, (1, 1), (1.0, 1.0), (false, false), false));
}

#[test]
fn print_settings_round_trip_and_only_changed_cells_are_written_like_visio() {
    let wanted = PrintSetup { orientation: 1, margins: [0.5, 0.25, 0.75, 0.25], grid: true, scale: (0.5, 0.5), fit: true, pages: (2, 1), center: (true, true) };
    let bytes = set_print_setup(&blank(), 0, &wanted).unwrap();
    let package = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(print_setup(&package, 0).unwrap(), wanted);
    let sheet = page_sheet(&package);
    for cell in ["N='PageLeftMargin' V='0.5'", "N='PageTopMargin' V='0.75'", "N='ScaleX' V='0.5'", "N='ScaleY' V='0.5'", "N='PagesX' V='2'", "N='CenterX' V='1'", "N='CenterY' V='1'", "N='OnPage' V='1'", "N='PrintGrid' V='1'", "N='PrintPageOrientation' V='1'"] {
        assert!(sheet.contains(cell), "{cell} in {sheet}");
    }
    // the cells left at their default are not written (Visio omits them)
    for cell in ["PageRightMargin", "PageBottomMargin", "PagesY"] {
        assert!(!sheet.contains(&format!("N='{cell}'")), "{cell} is a default: {sheet}");
    }
    // back to the defaults: the cells go again
    let bytes = set_print_setup(&package, 0, &PrintSetup { orientation: 2, ..PrintSetup::default() }).unwrap();
    let back = Package::from_reader(Cursor::new(bytes)).unwrap();
    assert_eq!(print_setup(&back, 0).unwrap(), PrintSetup { orientation: 2, ..PrintSetup::default() });
    let sheet = page_sheet(&back);
    for cell in ["PageLeftMargin", "ScaleX", "PagesX", "CenterX", "OnPage", "PrintGrid"] {
        assert!(!sheet.contains(&format!("N='{cell}'")), "{cell} removed again: {sheet}");
    }
}

#[test]
fn nonsense_print_settings_are_refused() {
    let package = blank();
    assert!(set_print_setup(&package, 0, &PrintSetup { margins: [-1.0, 0.25, 0.25, 0.25], ..PrintSetup::default() }).is_err());
    assert!(set_print_setup(&package, 0, &PrintSetup { scale: (0.0, 1.0), ..PrintSetup::default() }).is_err());
    assert!(set_print_setup(&package, 0, &PrintSetup { pages: (0, 1), ..PrintSetup::default() }).is_err());
    assert!(set_print_setup(&package, 0, &PrintSetup { margins: [f64::NAN, 0.25, 0.25, 0.25], ..PrintSetup::default() }).is_err());
}

#[test]
fn the_paper_size_follows_the_paper_kind_and_orientation() {
    let package = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(
        layoutcraft_vsdx::fresh::write_blank_document(&layoutcraft_vsdx::fresh::BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "P".to_string() }).unwrap(),
    ))
    .unwrap();
    // a blank 11 x 8.5 page is Letter landscape
    let (w, h) = layoutcraft_vsdx::pageops::paper_size(&package, 0).unwrap().unwrap();
    assert!((w - 11.0).abs() < 1e-9 && (h - 8.5).abs() < 1e-9, "{w} x {h}");
    let a4 = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(
        layoutcraft_vsdx::fresh::write_blank_document(&layoutcraft_vsdx::fresh::BlankOptions { width_in: 8.268, height_in: 11.693, metric: true, name: "P".to_string() }).unwrap(),
    ))
    .unwrap();
    let (w, h) = layoutcraft_vsdx::pageops::paper_size(&a4, 0).unwrap().unwrap();
    assert!((w - 8.268).abs() < 1e-9 && (h - 11.693).abs() < 1e-9, "A4 portrait: {w} x {h}");
}

// needs the private extra themes (private module, see private/README.md)
#[cfg(private_themes)]
#[test]
fn applying_a_theme_replaces_the_theme_part_and_writes_the_scheme_cells() {
    let package = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(
        layoutcraft_vsdx::fresh::write_blank_document(&layoutcraft_vsdx::fresh::BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "T".to_string() }).unwrap(),
    ))
    .unwrap();
    let bytes = layoutcraft_vsdx::pageops::apply_theme(&package, "Integral").unwrap();
    let back = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(bytes)).unwrap();
    let theme = back.text("visio/theme/theme1.xml").unwrap();
    assert!(theme.contains("name=\"Essential\"") && theme.contains("1CADE4"));
    let pages = back.text("visio/pages/pages.xml").unwrap();
    for cell in ["ColorSchemeIndex", "EffectSchemeIndex", "ConnectorSchemeIndex", "FontSchemeIndex", "ThemeIndex"] {
        assert!(pages.contains(&format!("<Cell N='{cell}' V='36'/>")), "{cell}");
    }
    // a second theme replaces the first one, the cells follow
    let again = layoutcraft_vsdx::pageops::apply_theme(&back, "gemstone").unwrap();
    let back2 = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(again)).unwrap();
    assert!(back2.text("visio/theme/theme1.xml").unwrap().contains("9F2936"));
    assert!(back2.text("visio/pages/pages.xml").unwrap().contains("<Cell N='ThemeIndex' V='48'/>"));
    assert!(layoutcraft_vsdx::pageops::apply_theme(&back2, "nope").is_err());
}

#[test]
fn a_blank_drawing_has_visios_styles_no_theme_part_and_themed_new_shapes() {
    // Q7 (round 12): a new drawing is written the way Visio writes one; a shape drawn in it takes its look from the theme
    use layoutcraft_vsdx::draw::{draw_shape, DrawKind};
    let package = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(
        layoutcraft_vsdx::fresh::write_blank_document(&layoutcraft_vsdx::fresh::BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "Q".to_string() }).unwrap(),
    ))
    .unwrap();
    assert!(!package.contains("visio/theme/theme1.xml"), "no theme part until one is applied");
    assert!(!package.text("[Content_Types].xml").unwrap().contains("theme"), "no theme content type");
    assert!(!package.text("visio/_rels/document.xml.rels").unwrap().contains("theme"), "no theme relationship");
    let document = package.text("visio/document.xml").unwrap();
    assert!(document.contains("DefaultFillStyle='3'") && document.contains("DefaultGuideStyle='4'"));
    for name in ["No Style", "Text Only", "None", "Normal", "Guide", "Theme"] {
        assert!(document.contains(&format!("NameU='{name}'")), "{name}");
    }
    assert!(document.contains("THEMEVAL()"));
    let (bytes, id) = draw_shape(&package, 0, DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0).unwrap();
    let drawn = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(bytes)).unwrap();
    let shape = layoutcraft_vsdx::writer::shape_xml(&drawn, 0, &id).unwrap();
    assert!(shape.contains("LineStyle='3' FillStyle='3' TextStyle='3'"), "{shape}");
    for own in ["FillForegnd", "LineColor", "LineWeight", "N='Color'"] {
        assert!(!shape.contains(own), "a themed shape carries no own {own}: {shape}");
    }
    // a text box keeps its explicit no-fill / no-line
    let (bytes, id) = draw_shape(&drawn, 0, DrawKind::TextBox, 4.0, 1.0, 6.0, 2.0).unwrap();
    let both = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(bytes)).unwrap();
    let text_box = layoutcraft_vsdx::writer::shape_xml(&both, 0, &id).unwrap();
    assert!(text_box.contains("N='FillPattern' V='0'") && text_box.contains("N='LinePattern' V='0'"));
    // a document from another writer (no THEMEVAL styles) keeps the explicit look
    let old = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(
        layoutcraft_vsdx::fresh::write_fresh_document(&serde_json::from_value(serde_json::json!({ "pages": [{ "name": "P", "shapes": [], "connectors": [] }] })).unwrap()).unwrap(),
    ))
    .unwrap();
    let (bytes, id) = draw_shape(&old, 0, DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0).unwrap();
    let plain = layoutcraft_vsdx::writer::shape_xml(&layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(bytes)).unwrap(), 0, &id).unwrap();
    assert!(plain.contains("FillStyle='0'") && plain.contains("FillForegnd"), "{plain}");
}

// needs the private extra themes (private module, see private/README.md)
#[cfg(private_themes)]
#[test]
fn a_theme_variant_is_two_page_sheet_cells_and_a_new_theme_resets_it() {
    use layoutcraft_vsdx::pageops::{apply_theme, set_theme_variant, theme_variant};
    let package = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(
        layoutcraft_vsdx::fresh::write_blank_document(&layoutcraft_vsdx::fresh::BlankOptions { width_in: 11.0, height_in: 8.5, metric: false, name: "V".to_string() }).unwrap(),
    ))
    .unwrap();
    assert!(set_theme_variant(&package, 1).is_err(), "no theme yet");
    let themed = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(apply_theme(&package, "Integral").unwrap())).unwrap();
    assert_eq!(theme_variant(&themed), 0);
    let v2 = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(set_theme_variant(&themed, 2).unwrap())).unwrap();
    assert_eq!(theme_variant(&v2), 2);
    let pages = v2.text("visio/pages/pages.xml").unwrap();
    assert!(pages.contains("<Cell N='VariationColorIndex' V='2'/>") && pages.contains("<Cell N='VariationStyleIndex' V='2'/>"));
    assert!(set_theme_variant(&v2, 4).is_err());
    let again = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(apply_theme(&v2, "Organic").unwrap())).unwrap();
    assert_eq!(theme_variant(&again), 0, "a new theme starts at its first variant");
}

#[test]
fn a_public_theme_can_be_applied_and_gives_its_scheme_cells() {
    let package = blank();
    let bytes = layoutcraft_vsdx::pageops::apply_theme(&package, "Daylight").unwrap();
    let themed = layoutcraft_vsdx::Package::from_reader(std::io::Cursor::new(bytes)).unwrap();
    let part = themed.names().find(|n| n.starts_with("visio/theme/theme")).expect("a theme part").to_string();
    let theme = themed.text(&part).unwrap();
    assert!(theme.contains("name=\"Daylight\"") && theme.contains("3A7BD5"));
    assert!(page_sheet(&themed).contains("ThemeIndex"));
    assert!(layoutcraft_vsdx::pageops::apply_theme(&themed, "nope").is_err());
}
