//! Pages are paired with their parts through each page's own `<Rel r:id>` (the way Visio
//! writes it), not by sorting part names: with 10 or more pages a string sort puts
//! `page10.xml` before `page2.xml`, which showed the wrong drawing under "Page-2".
use std::io::{Cursor, Write};

use layoutcraft_vsdx::Package;
use zip::write::SimpleFileOptions;

fn package(pages: usize, with_rel: bool) -> Package {
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
        put("visio/document.xml", "<?xml version='1.0'?><VisioDocument xmlns='http://schemas.microsoft.com/office/visio/2012/main'/>");
        put("visio/_rels/document.xml.rels", "<?xml version='1.0'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='rId1' Type='http://schemas.microsoft.com/visio/2010/relationships/pages' Target='pages/pages.xml'/></Relationships>");
        // Part N is listed under the page named after N, but the rels are written in reverse
        // so that neither string order nor rel order accidentally matches.
        let mut list = String::new();
        let mut rels = String::new();
        for n in 1..=pages {
            let rel = if with_rel { format!("<Rel r:id='rId{n}'/>") } else { String::new() };
            list.push_str(&format!("<Page ID='{n}' NameU='Page-{n}'>{rel}</Page>"));
        }
        for n in (1..=pages).rev() {
            rels.push_str(&format!("<Relationship Id='rId{n}' Type='http://schemas.microsoft.com/visio/2010/relationships/page' Target='page{n}.xml'/>"));
            put(&format!("visio/pages/page{n}.xml"), "<?xml version='1.0'?><PageContents xmlns='http://schemas.microsoft.com/office/visio/2012/main'><Shapes/></PageContents>");
        }
        put("visio/pages/pages.xml", &format!("<?xml version='1.0'?><Pages xmlns='http://schemas.microsoft.com/office/visio/2012/main' xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships'>{list}</Pages>"));
        put("visio/pages/_rels/pages.xml.rels", &format!("<?xml version='1.0'?><Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'>{rels}</Relationships>"));
        zip.finish().unwrap();
    }
    Package::from_reader(Cursor::new(buffer)).unwrap()
}

#[test]
fn pages_follow_their_rel_ids_beyond_nine_pages() {
    let pages = package(12, true).pages().unwrap();
    assert_eq!(pages.len(), 12);
    for (index, page) in pages.iter().enumerate() {
        assert_eq!(page.name, format!("Page-{}", index + 1));
        assert_eq!(page.part, format!("visio/pages/page{}.xml", index + 1), "{}", page.name);
    }
}

#[test]
fn without_rel_ids_the_fallback_is_numeric_not_alphabetical() {
    let pages = package(12, false).pages().unwrap();
    assert_eq!(pages[1].part, "visio/pages/page2.xml");
    assert_eq!(pages[9].part, "visio/pages/page10.xml");
}
