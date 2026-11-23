#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::f64::consts::PI;
use std::path::PathBuf;
use layoutcraft_vsdx::{import_document, Package};

fn example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples")
        .join(name)
}

#[test]
fn floor_plan_uses_drawing_scale_and_space_fields() {
    let package = Package::open(example("Simple.Rectangular.Room.Visio.Floor.Plan.vsdx")).unwrap();
    let document = import_document(&package).unwrap();
    let page = &document.pages[0];
    assert!((page.scale - 1.0 / 48.0).abs() < 1e-6);
    let room = page
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("1"))
        .expect("room");
    assert!(!room.parts.is_empty(), "room walls");
    let space = page
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("157"))
        .expect("space");
    assert!(space.label.contains("Master Bedroom"));
    assert!(space.label.contains("551 sq. ft."));
    let part = &space.parts[0];
    let pattern = part.fill_pattern.as_ref().expect("hatch pattern");
    assert_eq!(pattern.kind, 2);
    assert_eq!(pattern.fore, "#008000");
    assert!(part.fill_opacity.is_some_and(|opacity| opacity < 0.5));
}

#[test]
fn org_chart_pictures_use_master_relative_media() {
    let package = Package::open(example("Visio.Org.Chart.Diagram.Sample.vsdx")).unwrap();
    let document = import_document(&package).unwrap();
    assert_eq!(document.media.len(), 6);
    let images = document.pages[0]
        .shapes
        .iter()
        .filter(|shape| shape.image.is_some())
        .count();
    assert_eq!(images, 10);
    assert!(document.pages[0]
        .shapes
        .iter()
        .filter_map(|shape| shape.image.as_ref())
        .any(|image| image.name.as_deref() == Some("image1.jpeg")));
}

#[test]
fn network_stencils_hide_background_boxes_and_keep_style_gradient_fills() {
    // ImageNode/network stencils draw a full-box child from an
    // ImageNodeBackground style (LineColorTrans/FillForegndTrans = 100%).
    // It must stay invisible instead of outlining every device.
    let package = Package::open(example("computer-network.vsdx")).unwrap();
    let document = import_document(&package).unwrap();
    let icon = document.pages[0]
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("2"))
        .expect("image node");
    assert!(
        icon.parts.iter().any(|part| part
            .fill_opacity
            .is_some_and(|opacity| opacity <= 0.001)
            && part.stroke_width.unwrap_or(0.0) == 0.0),
        "ImageNode background box should be transparent"
    );
    assert!(
        !icon.parts.iter().any(|part| part.stroke.as_deref()
            == Some("#000000")
            && part.stroke_width.unwrap_or(0.0) > 0.0),
        "stencil box outline is still visible"
    );

    // The Ethernet bus takes its gradient colours from the FillStyle's
    // FillGradient section; the shape rows only carry stop positions.
    let package = Package::open(example("Network Diagram_start.vsdx")).unwrap();
    let document = import_document(&package).unwrap();
    let bus = document.pages[0]
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("5"))
        .expect("Ethernet bus");
    let gradient = bus
        .parts
        .iter()
        .find_map(|part| part.fill_gradient.as_ref())
        .expect("Ethernet bus gradient");
    let stops = gradient
        .get("stops")
        .and_then(|value| value.as_array())
        .expect("gradient stops");
    assert!(stops.len() >= 2);
    assert!(stops
        .iter()
        .any(|stop| stop.get("color").and_then(|color| color.as_str()) == Some("#4e8ec2")));
}

#[test]
fn electrical_stencils_keep_labels_upright_and_hide_variant_arms() {
    let package = Package::open(example("Base Electrical.vstx")).unwrap();
    let document = import_document(&package).unwrap();
    let page = &document.pages[0];

    // Resistor instances are rotated 90 degrees but set TxtAngle = -Angle, so
    // the label must stay upright. Hidden SubType/Form arms must not draw.
    let resistor = page
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("20"))
        .expect("resistor");
    assert_eq!(resistor.parts.len(), 2, "hidden resistor arms drew");
    let text_rot = resistor.text_rot.expect("resistor TxtAngle");
    assert!(
        (resistor.rot * PI / 180.0 + text_rot).abs() < 1e-6,
        "resistor label is not upright"
    );

    // The fixed capacitor must not draw the variable-capacitor arrow arm.
    let capacitor = page
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("35"))
        .expect("capacitor");
    assert_eq!(capacitor.parts.len(), 4, "variable-capacitor arm drew");

    // The AC source triangle appears only when User.AT is set (it is 0 here).
    let ac = page
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("36"))
        .expect("AC source");
    assert_eq!(ac.parts.len(), 5, "AC variant triangle drew");

    // A connector glued to the rotated resistor must attach at its pin, not
    // at a doubly-rotated point.
    let connector = page
        .connectors
        .iter()
        .find(|connector| connector.visio_id.as_deref() == Some("140"))
        .expect("connector");
    let local = connector.from.local.as_ref().expect("glue local");
    assert!((local.get("y").and_then(|value| value.as_f64()).unwrap() - 0.4975).abs() < 1e-3);
}

#[test]
fn relative_elliptical_arcs_keep_stencil_glyphs() {
    let package = Package::open(example("Dev-Test-Deployment for PaaS.vsdx")).unwrap();
    let document = import_document(&package).unwrap();
    let database = document.pages[0]
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("1723"))
        .expect("SQL database");
    // The "SQL" glyphs are RelEllipticalArcTo-heavy; the largest part is the
    // letter geometry. Before relative-arc support it collapsed to ~22 points.
    let glyph = database
        .parts
        .iter()
        .max_by_key(|part| part.d.len())
        .expect("SQL glyph part");
    assert!(
        glyph.d.matches('L').count() > 500,
        "relative elliptical arcs were dropped: {}",
        glyph.d.len()
    );
}

#[test]
fn uml_state_stencil_uses_theme_colours_rounded_frame_and_arrows() {
    let package = Package::open(example("UML State - ATM.vsdx")).unwrap();
    let document = import_document(&package).unwrap();
    let page = &document.pages[0];

    let state = page
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("3"))
        .expect("state");
    // QuickStyleLineColor 102 resolves through the Visio variation palette.
    assert_eq!(state.stroke.as_deref(), Some("#73ae42"));
    assert!(
        state.rounding.is_some_and(|radius| radius > 5.0),
        "state frame should carry its Rounding radius"
    );
    let title = state
        .sub_texts
        .iter()
        .find(|text| text.text == "Idle")
        .expect("state title");
    assert_eq!(title.color, "#feffff");

    let detailed = page
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("49"))
        .expect("state with behaviour");
    let body = detailed
        .sub_texts
        .iter()
        .find(|text| text.text.contains("cardSubmitted"))
        .expect("behaviour text");
    assert_eq!(body.color, "#73ae42");

    // The Connector StyleSheet's themed EndArrow resolves to a filled arrow.
    let connector = page
        .connectors
        .iter()
        .find(|connector| connector.visio_id.as_deref() == Some("29"))
        .expect("connector");
    assert_eq!(connector.arrow_end.as_deref(), Some("filled"));
}

#[test]
fn sdl_final_state_draws_line_details_above_the_fill() {
    let package = Package::open(example("SDL Game Process.vsdx")).unwrap();
    let document = import_document(&package).unwrap();
    let final_state = document.pages[0]
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("12"))
        .expect("final state");
    // Visio paints the filled circle first and the X detail on top.
    assert!(final_state
        .parts
        .iter()
        .any(|part| part.fill.as_deref() == Some("#d8cbb3")));
    let last = final_state.parts.last().expect("detail part");
    assert_eq!(last.fill.as_deref(), Some("none"));
    assert_eq!(last.stroke.as_deref(), Some("#316176"));
}

#[test]
fn empty_template_pages_expose_page_dimensions() {
    let package = Package::open(example("Schedule.vstx")).unwrap();
    let pages = package.pages().unwrap();
    assert_eq!(pages.len(), 1);
    assert!(pages[0].width.is_some_and(|width| width > 10.0));
    assert!(pages[0].height.is_some_and(|height| height > 7.0));
    let document = import_document(&package).unwrap();
    assert!(document.pages[0].shapes.is_empty());
    assert!(document.pages[0].connectors.is_empty());
}

#[test]
fn background_pages_are_not_shown_as_drawing_pages() {
    let package = Package::open(example("Network Diagram_start.vsdx")).unwrap();
    let document = import_document(&package).unwrap();
    assert_eq!(document.pages.len(), 3);
    assert_eq!(document.pages[0].name, "Topology");
    assert!(document
        .pages
        .iter()
        .all(|page| !page.name.starts_with("VBackground")));
}

#[test]
fn network_image_nodes_center_their_geometry_and_labels() {
    let package = Package::open(example("computer-network.vsdx")).unwrap();
    let document = import_document(&package).unwrap();
    let page = &document.pages[0];
    let icon = page
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("2"))
        .expect("image node");
    assert!(!icon.parts.is_empty());
    let path = &icon.parts[0].d;
    assert!(
        !path.contains("-32.5"),
        "centre-based geometry was not offset: {path}"
    );
    let label = page
        .shapes
        .iter()
        .find(|shape| shape.visio_id.as_deref() == Some("21"))
        .expect("default label");
    assert_eq!(label.label, "oschi");
    assert_eq!(label.fill.as_deref(), Some("none"));
    assert!(label.parts.is_empty());
}
