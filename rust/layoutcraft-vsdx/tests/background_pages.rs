//! Background pages: making a page a background, showing one beneath another page, the shell's page indexes counting only the
//! pages that are not backgrounds, and the references being dropped when a background becomes an ordinary page again.
use layoutcraft_vsdx::draw::{draw_shape, DrawKind};
use layoutcraft_vsdx::pageops::{add_page, background_info, delete_page, make_background, make_foreground, move_page, rename_page, set_back_page};
use layoutcraft_vsdx::{import_document, write_blank_document, BlankOptions, Package};

fn package(bytes: Vec<u8>) -> Package {
    Package::from_bytes(&bytes).unwrap()
}

/// Pages "Main", "Second", "Third", the first with one rectangle on it.
fn three_pages() -> Package {
    let blank = write_blank_document(&BlankOptions { width_in: 8.5, height_in: 11.0, metric: false, name: "Main".to_string() }).unwrap();
    let p = package(blank);
    let (b, _) = draw_shape(&p, 0, DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0).unwrap();
    let p = package(b);
    let (b, i) = add_page(&p, 0, Some("Second")).unwrap();
    assert_eq!(i, 1);
    let p = package(b);
    let (b, _) = add_page(&p, 1, Some("Third")).unwrap();
    package(b)
}

fn names(p: &Package) -> Vec<String> {
    p.pages().unwrap().into_iter().map(|page| page.name).collect()
}

fn pages_xml(p: &Package) -> String {
    p.text("visio/pages/pages.xml").unwrap().to_string()
}

#[test]
fn a_background_page_leaves_the_tabs_and_goes_first() {
    let p = three_pages();
    let p = package(make_background(&p, 0).unwrap());
    assert_eq!(names(&p), vec!["Second", "Third"], "the shell lists only the pages that are not backgrounds");
    let xml = pages_xml(&p);
    let main = xml.find("NameU='Main'").unwrap();
    let second = xml.find("NameU='Second'").unwrap();
    assert!(main < second, "Visio writes background pages first: {xml}");
    assert!(xml[..xml[main..].find('>').unwrap() + main].contains("Background='1'"));
    let info = background_info(&p).unwrap();
    assert_eq!(info.backgrounds.len(), 1);
    assert_eq!(info.backgrounds[0].1, "Main");
    assert_eq!(info.assigned, vec![None, None]);
}

#[test]
fn page_operations_count_only_the_shown_pages() {
    let p = package(make_background(&three_pages(), 0).unwrap());
    // index 0 is now "Second", although "Main" is the first Page element
    let p = package(rename_page(&p, 0, "Renamed").unwrap());
    assert_eq!(names(&p), vec!["Renamed", "Third"]);
    let p = package(move_page(&p, 0, 1).unwrap());
    assert_eq!(names(&p), vec!["Third", "Renamed"]);
    let (b, shown) = add_page(&p, 0, Some("New")).unwrap();
    assert_eq!(shown, 1);
    let p = package(b);
    assert_eq!(names(&p), vec!["Third", "New", "Renamed"]);
    let p = package(delete_page(&p, 1).unwrap());
    assert_eq!(names(&p), vec!["Third", "Renamed"]);
    assert_eq!(background_info(&p).unwrap().backgrounds.len(), 1, "the background page stays");
    assert!(pages_xml(&p).contains("NameU='Main'"));
}

#[test]
fn a_page_shows_its_background_beneath_itself() {
    let p = three_pages();
    let before = import_document(&p).unwrap().pages[1].shapes.len();
    let p = package(make_background(&p, 0).unwrap());
    let id = background_info(&p).unwrap().backgrounds[0].0.clone();
    let p = package(set_back_page(&p, 1, Some(&id)).unwrap()); // "Third"
    let doc = import_document(&p).unwrap();
    assert_eq!(doc.pages.len(), 2);
    assert_eq!(doc.pages[0].shapes.len(), before, "Second has no background");
    assert!(doc.pages[1].shapes.len() > before, "Third shows the rectangle of the background page");
    assert!(pages_xml(&p).contains(&format!("BackPage='{id}'")));
    assert_eq!(background_info(&p).unwrap().assigned, vec![None, Some(id.clone())]);
    // clearing drops the attribute
    let p = package(set_back_page(&p, 1, None).unwrap());
    assert!(!pages_xml(&p).contains("BackPage"));
    assert_eq!(import_document(&p).unwrap().pages[1].shapes.len(), before);
}

#[test]
fn turning_a_background_back_drops_the_references() {
    let p = package(make_background(&three_pages(), 0).unwrap());
    let id = background_info(&p).unwrap().backgrounds[0].0.clone();
    let p = package(set_back_page(&p, 0, Some(&id)).unwrap());
    let (b, shown) = make_foreground(&p, &id).unwrap();
    let p = package(b);
    assert_eq!(names(&p), vec!["Main", "Second", "Third"]);
    assert_eq!(shown, 0);
    let xml = pages_xml(&p);
    assert!(!xml.contains("BackPage") && !xml.contains("Background"), "{xml}");
    assert!(background_info(&p).unwrap().backgrounds.is_empty());
}

#[test]
fn refusals() {
    let p = three_pages();
    // a page cannot be its own background, and only background pages can be used
    let bg = package(make_background(&p, 0).unwrap());
    let id = background_info(&bg).unwrap().backgrounds[0].0.clone();
    assert!(set_back_page(&p, 1, Some("0")).is_err(), "page 0 is not a background page yet");
    assert!(set_back_page(&bg, 0, Some("999")).is_err());
    // a page that has a background of its own cannot become one (Visio hangs on it)
    let with = package(set_back_page(&bg, 0, Some(&id)).unwrap());
    assert!(make_background(&with, 0).is_err());
    // the last page that is not a background stays
    let one = package(delete_page(&package(delete_page(&p, 2).unwrap()), 1).unwrap());
    assert!(make_background(&one, 0).is_err());
    assert!(make_foreground(&p, "0").is_err(), "not a background page");
}
