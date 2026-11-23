//! Shape hyperlinks in Visio's form (probe-hyperlinks.ps1): a `Hyperlink` section of `Row_n` rows with the cells in Visio's order, an
//! empty address or sub-address written `F='No Formula'`, a link to a page of the drawing as `Pages[name]!ThePage!PAGENAME()`.
use layoutcraft_vsdx::draw::{draw_shape, DrawKind};
use layoutcraft_vsdx::hyperlinks::{read_hyperlinks, set_hyperlinks, Hyperlink};
use layoutcraft_vsdx::pageops::add_page;
use layoutcraft_vsdx::{write_blank_document, BlankOptions, Package};

fn pkg(bytes: Vec<u8>) -> Package {
    Package::from_bytes(&bytes).unwrap()
}

/// A blank two-page drawing ("Main", "Second page") with one rectangle on page 0.
fn drawing() -> (Package, String) {
    let bytes = write_blank_document(&BlankOptions { width_in: 8.5, height_in: 11.0, metric: false, name: "Main".to_string() }).unwrap();
    let p = pkg(bytes);
    let (b, _) = add_page(&p, 0, Some("Second page")).unwrap();
    let p = pkg(b);
    let (b, id) = draw_shape(&p, 0, DrawKind::Rectangle, 1.0, 1.0, 3.0, 2.0).unwrap();
    (pkg(b), id)
}

fn web() -> Hyperlink {
    Hyperlink { description: "Example & co".into(), address: "https://example.com/a?b=1&c=2".into(), new_window: true, ..Default::default() }
}

fn page_xml(p: &Package) -> String {
    let part = p.pages().unwrap()[0].part.clone();
    p.text(&part).unwrap().to_string()
}

#[test]
fn links_are_written_in_visios_row_order_and_read_back() {
    let (p, id) = drawing();
    let links = vec![
        web(),
        Hyperlink { description: "Go to the second page".into(), sub_address: "Second page".into(), ..Default::default() },
        Hyperlink { description: "Other file".into(), address: "C:\\temp\\other.vsdx".into(), sub_address: "Page-1".into(), extra_info: "x=1".into(), frame: "frm".into(), default: true, ..Default::default() },
    ];
    let p = pkg(set_hyperlinks(&p, 0, &id, &links).unwrap());
    let back = read_hyperlinks(&p, 0, &id).unwrap();
    assert_eq!(back.len(), 3);
    assert_eq!(back.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Row_1", "Row_2", "Row_3"]);
    assert_eq!(back[0].address, "https://example.com/a?b=1&c=2");
    assert_eq!(back[0].description, "Example & co", "the ampersand survives the XML escaping");
    assert!(back[0].new_window && !back[0].default);
    assert_eq!(back[1].sub_address, "Second page");
    assert_eq!(back[2].extra_info, "x=1");
    assert_eq!(back[2].frame, "frm");
    assert!(back[2].default);
    let xml = page_xml(&p);
    assert!(xml.contains("<Section N='Hyperlink'>"), "{xml}");
    let order = ["Description", "Address", "SubAddress", "ExtraInfo", "Frame", "NewWindow", "Default", "Invisible", "SortKey"];
    let row = &xml[xml.find("<Row N='Row_1'>").unwrap()..];
    let row = &row[..row.find("</Row>").unwrap()];
    let mut at = 0;
    for name in order {
        let found = row[at..].find(&format!("<Cell N='{name}'")).unwrap_or_else(|| panic!("{name} missing or out of order in {row}"));
        at += found + 1;
    }
    assert!(xml.find("<Section N='Hyperlink'>").unwrap() < xml.find("<Section N='Geometry'").unwrap(), "the section sits before the geometry, as in Visio's file");
}

#[test]
fn empty_address_and_page_links_carry_visios_formulas() {
    let (p, id) = drawing();
    let links = vec![web(), Hyperlink { sub_address: "Second page".into(), ..Default::default() }, Hyperlink { sub_address: "No such page".into(), ..Default::default() }];
    let p = pkg(set_hyperlinks(&p, 0, &id, &links).unwrap());
    let xml = page_xml(&p);
    assert!(xml.contains("<Cell N='SubAddress' V='' F='No Formula'/>"), "an empty sub-address: {xml}");
    assert!(xml.contains("<Cell N='Address' V='' F='No Formula'/>"), "an empty address");
    assert!(xml.contains("F='Pages[Second page]!ThePage!PAGENAME()'"), "a link to a page of the drawing follows the page name");
    assert!(!xml.contains("Pages[No such page]"), "a sub-address that is no page gets no formula");
}

#[test]
fn replacing_and_removing_links_keeps_names_and_drops_the_section() {
    let (p, id) = drawing();
    let p = pkg(set_hyperlinks(&p, 0, &id, &[web(), Hyperlink { address: "mailto:a@b.c".into(), invisible: true, ..Default::default() }]).unwrap());
    let mut links = read_hyperlinks(&p, 0, &id).unwrap();
    assert!(links[1].invisible);
    links.remove(0);
    links.push(Hyperlink { address: "https://third.example".into(), ..Default::default() });
    let p = pkg(set_hyperlinks(&p, 0, &id, &links).unwrap());
    let names: Vec<String> = read_hyperlinks(&p, 0, &id).unwrap().into_iter().map(|l| l.name).collect();
    assert_eq!(names, ["Row_2", "Row_1"], "the kept row keeps its name, the new link takes the free one");
    let p = pkg(set_hyperlinks(&p, 0, &id, &[]).unwrap());
    assert!(read_hyperlinks(&p, 0, &id).unwrap().is_empty());
    assert!(!page_xml(&p).contains("N='Hyperlink'"), "an empty list removes the section");
}

#[test]
fn only_the_first_default_stays_default_and_bad_input_is_refused() {
    let (p, id) = drawing();
    let two_defaults = vec![Hyperlink { address: "https://a.example".into(), default: true, ..Default::default() }, Hyperlink { address: "https://b.example".into(), default: true, ..Default::default() }];
    let p = pkg(set_hyperlinks(&p, 0, &id, &two_defaults).unwrap());
    let back = read_hyperlinks(&p, 0, &id).unwrap();
    assert!(back[0].default && !back[1].default);
    assert!(set_hyperlinks(&p, 0, &id, &[Hyperlink::default()]).is_err(), "a link needs an address or a sub-address");
    assert!(set_hyperlinks(&p, 0, &id, &[Hyperlink { address: "x\ny".into(), ..Default::default() }]).is_err(), "no control characters");
    assert!(set_hyperlinks(&p, 0, &id, &[Hyperlink { address: "a".repeat(3000), ..Default::default() }]).is_err(), "no endless text");
    assert!(set_hyperlinks(&p, 0, &id, &vec![web(); 65]).is_err(), "at most 64 links");
    assert!(set_hyperlinks(&p, 0, "999", &[web()]).is_err(), "an unknown shape");
}

#[test]
fn a_group_keeps_its_members_links_apart_from_its_own() {
    let (p, a) = drawing();
    let (b, c) = draw_shape(&p, 0, DrawKind::Rectangle, 4.0, 1.0, 6.0, 2.0).unwrap();
    let p = pkg(b);
    let (b, group) = layoutcraft_vsdx::groupedit::group_shapes(&p, 0, &[a.clone(), c.clone()]).unwrap();
    let p = pkg(b);
    let p = pkg(set_hyperlinks(&p, 0, &a, &[web()]).unwrap());
    assert_eq!(read_hyperlinks(&p, 0, &a).unwrap().len(), 1);
    assert!(read_hyperlinks(&p, 0, &c).unwrap().is_empty(), "the neighbour member has none");
    assert!(read_hyperlinks(&p, 0, &group).unwrap().is_empty(), "the group itself has none: a member's section is not the group's");
    let p = pkg(set_hyperlinks(&p, 0, &group, &[Hyperlink { address: "https://group.example".into(), ..Default::default() }]).unwrap());
    assert_eq!(read_hyperlinks(&p, 0, &group).unwrap()[0].address, "https://group.example");
    assert_eq!(read_hyperlinks(&p, 0, &a).unwrap()[0].address, "https://example.com/a?b=1&c=2", "the member keeps its own link");
}
