//! A file whose only pages are background pages (several IT-vendor templates,
//! e.g. CommScope) used to fail with "page ... has no PageHeight": the
//! background pages were filtered out and a bare fallback list was used.
use std::io::{Cursor, Write};

use layoutcraft_vsdx::{import_document, Package};
use zip::write::SimpleFileOptions;

fn package(background: bool) -> Package {
    let mut buffer = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
        let options = SimpleFileOptions::default();
        let mut put = |name: &str, body: &str| {
            zip.start_file(name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        };
        put("[Content_Types].xml", "<?xml version='1.0'?><Types xmlns='http://schemas.openxmlformats.org/package/2006/content-types'/>");
        put("_rels/.rels", "<?xml version='1.0'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'/>");
        put(
            "visio/document.xml",
            "<?xml version='1.0'?><VisioDocument xmlns='http://schemas.microsoft.com/office/visio/2012/main'/>",
        );
        put(
            "visio/_rels/document.xml.rels",
            "<?xml version='1.0'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='rId1' Type='http://schemas.microsoft.com/visio/2010/relationships/pages' Target='pages/pages.xml'/></Relationships>",
        );
        let flag = if background { " Background='1'" } else { "" };
        put(
            "visio/pages/pages.xml",
            &format!("<?xml version='1.0'?><Pages xmlns='http://schemas.microsoft.com/office/visio/2012/main' xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships'><Page ID='0' NameU='Base Sheet'{flag}><PageSheet><Cell N='PageWidth' V='17'/><Cell N='PageHeight' V='11'/></PageSheet><Rel r:id='rId1'/></Page></Pages>"),
        );
        put(
            "visio/pages/_rels/pages.xml.rels",
            "<?xml version='1.0'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='rId1' Type='http://schemas.microsoft.com/visio/2010/relationships/page' Target='page1.xml'/></Relationships>",
        );
        put(
            "visio/pages/page1.xml",
            "<?xml version='1.0'?><PageContents xmlns='http://schemas.microsoft.com/office/visio/2012/main'><Shapes><Shape ID='1' Type='Shape'><Cell N='PinX' V='3'/><Cell N='PinY' V='4'/><Cell N='Width' V='2'/><Cell N='Height' V='1'/><Section N='Geometry' IX='0'><Row T='MoveTo' IX='1'><Cell N='X' V='0'/><Cell N='Y' V='0'/></Row><Row T='LineTo' IX='2'><Cell N='X' V='2'/><Cell N='Y' V='0'/></Row><Row T='LineTo' IX='3'><Cell N='X' V='2'/><Cell N='Y' V='1'/></Row><Row T='LineTo' IX='4'><Cell N='X' V='0'/><Cell N='Y' V='1'/></Row></Section></Shape></Shapes></PageContents>",
        );
        zip.finish().unwrap();
    }
    Package::from_reader(Cursor::new(buffer)).unwrap()
}

#[test]
fn background_only_file_imports_its_pages() {
    let document = import_document(&package(true)).expect("imports");
    assert_eq!(document.pages.len(), 1);
    assert_eq!(document.pages[0].name, "Base Sheet");
    assert_eq!(document.pages[0].shapes.len(), 1);
}

#[test]
fn ordinary_foreground_page_still_imports() {
    let document = import_document(&package(false)).expect("imports");
    assert_eq!(document.pages.len(), 1);
}
