//! A master of several shapes that lists a ruler guide first. Drawings made here, by our writer; the guide and the group are
//! added to its parts.
use layoutcraft_engine::Engine;
use layoutcraft_vsdx::Package;
use std::collections::BTreeMap;

/// A page with one group made from the built-in rectangle master, after a guide was put in front of the rectangle there.
fn drawing() -> Engine {
    let mut engine = Engine::new();
    engine.new_blank(4.0, 3.0, false, "Page-1").unwrap();
    let stencil: serde_json::Value = serde_json::from_str(&engine.stencil_builtin().unwrap()).unwrap();
    engine.stencil_drop(0, stencil["index"].as_u64().unwrap() as usize, "1", 2.0, 1.5).unwrap();
    let package = engine.package.as_ref().unwrap();
    let page = package.pages().unwrap()[0].part.clone();
    let master = "visio/masters/master1.xml".to_string();
    let guide = "<Shape ID='9' Type='Guide'><Cell N='PinX' V='0.2'/><Cell N='PinY' V='0.5'/><Cell N='Width' V='8'/><Cell N='Height' V='0'/><Cell N='LocPinX' V='4' F='Width*0.5'/><Cell N='LocPinY' V='0'/><Section N='Geometry' IX='0'><Cell N='NoFill' V='1'/><Cell N='NoLine' V='0'/><Row T='InfiniteLine' IX='1'><Cell N='X' V='0'/><Cell N='Y' V='0'/><Cell N='A' V='8'/><Cell N='B' V='0'/></Row></Section></Shape>";
    let master_xml = package.text(&master).unwrap().replacen("<Shape ID='5'", &format!("{guide}<Shape ID='5'"), 1);
    let group = "<Shapes><Shape ID='1' Type='Group' Master='1'><Cell N='PinX' V='2'/><Cell N='PinY' V='1.5'/><Cell N='Width' V='0'/><Cell N='Height' V='1'/><Cell N='LocPinX' V='0'/><Cell N='LocPinY' V='0.5'/><Shapes><Shape ID='2' Type='Guide' MasterShape='9'><Cell N='PinX' V='0'/><Cell N='PinY' V='0.5'/></Shape><Shape Del='1' MasterShape='5' ID='4294967295'/><Shape ID='3' Type='Shape' MasterShape='5'><Cell N='PinX' V='0'/><Cell N='PinY' V='0.5'/></Shape></Shapes></Shape></Shapes>";
    let xml = package.text(&page).unwrap().to_string();
    let (from, to) = (xml.find("<Shapes>").unwrap(), xml.rfind("</Shapes>").unwrap() + "</Shapes>".len());
    let page_xml = format!("{}{group}{}", &xml[..from], &xml[to..]);
    assert!(master_xml.contains("InfiniteLine"));
    let parts = BTreeMap::from([(page, page_xml.into_bytes()), (master, master_xml.into_bytes())]);
    let bytes = layoutcraft_vsdx::writer::write_package_edit(package, parts, &[]).unwrap();
    assert!(Package::from_bytes(&bytes).is_ok());
    let mut read = Engine::new();
    read.open(bytes).unwrap();
    read
}

#[test]
fn a_group_made_from_such_a_master_draws_its_members_and_not_the_guide() {
    let engine = drawing();
    let scene = layoutcraft_render::scene_from_document(engine.document().unwrap(), 0).unwrap();
    // nothing reaches far beyond the 4 x 3 inch page (96 pixels to the inch)
    assert!(scene.width < 2.0 * 4.0 * 96.0 && scene.height < 2.0 * 3.0 * 96.0, "{} x {}", scene.width, scene.height);
    let drawn = &engine.document().unwrap().pages[0].shapes;
    assert!(!drawn.iter().any(|shape| shape.visio_id.as_deref() == Some("4294967295")), "a member taken out is not drawn");
    assert!(drawn.iter().any(|shape| shape.w > 100.0), "the rectangle of the master is drawn: {:?}", drawn.iter().map(|s| (s.w, s.h)).collect::<Vec<_>>());
}
