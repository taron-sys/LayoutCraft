#![cfg(private_fixtures)] // reads the private test corpus (docs/38-private-fixtures.md)
use std::path::PathBuf;
use layoutcraft_render::{scene_from_document, to_svg};
use layoutcraft_vsdx::{import_document, Package};

/// `eesemi-masters.vsdx` (Visio, Semiconductors stencil dropped on pages).
fn svg_of(page: usize) -> String {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/eesemi-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    to_svg(&scene_from_document(&doc, page).unwrap())
}

/// Page 0, "Bipolar": the base bar and the emitter arrow are `PolylineTo` rows whose formulas still
/// contain `Width*..` terms (the cached value is the polyline), and a polyline may hold one point.
/// Both used to collapse to degenerate paths. The symbol's lines have `LineCap` 1 = flat ends.
#[test]
fn bipolar_transistor_has_bar_arrow_and_flat_ends() {
    let svg = svg_of(0);
    let corners: usize = svg
        .split("<path")
        .filter(|p| p.contains("fill=\"#000000\""))
        .map(|p| p.matches('L').count())
        .sum();
    assert!(corners >= 6, "bar and arrow head are missing ({corners} line segments in filled paths)");
    assert!(svg.contains("stroke-linecap=\"butt\""), "LineCap 1 must give flat ends");
}

/// Page 3, "Varactor": the envelope is an `Ellipse` row whose first axis point (A, B) is the
/// vertical one; the radii came out as 0.
#[test]
fn ellipse_with_vertical_first_axis_point_has_radii() {
    let svg = svg_of(3);
    assert!(!svg.contains("A0 0 "), "ellipse with zero radius");
}

/// `peinst-masters.vsdx` page 7 ("PID Hydraulic 2"): the group holds a half-inch line with `NoLine`
/// and `NoFill` next to two tiny visible strokes. Visio exports the ink, not the group box.
#[test]
fn hidden_child_does_not_widen_the_picture() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/peinst-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let scene = scene_from_document(&doc, 7).unwrap();
    assert!(scene.width < 25.0, "picture is {} px wide", scene.width);
}

/// `netsme-masters.vsdx` page 4 ("Gateway", Network Symbols stencil, document without a theme
/// part): Visio draws black outlines and white fills (COM-verified); the quick-style accent
/// (blue fill, grey line) must not appear.
#[test]
fn legacy_network_symbol_has_no_theme_accent() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/netsme-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 4).unwrap());
    assert!(!svg.contains("#a5a5a5"), "grey accent line");
    assert!(!svg.contains("#4472c4"), "blue accent fill");
}

/// `pevalv-masters.vsdx` page 1 ("Diaphragm valve"): the shape has no geometry of its own, only its
/// master's. Master geometry runs y-up like any other, so the actuator stem (y 0.125 -> 0.21875 in)
/// must point up the page; it was drawn mirrored (downwards) before.
#[test]
fn inherited_master_geometry_is_not_mirrored() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/pevalv-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 1).unwrap());
    assert!(svg.contains("M12 12L12 3"), "stem is not pointing up: {svg}");
}

/// `pevalv-masters.vsdx` page 5 ("Socket weld"): variant arms are hidden by formulas that read
/// other sections (`Geometry1.NoShow`, `NOT(Geometry1.NoShow)`); the circle and the diagonal of
/// the other variant must not be drawn.
#[test]
fn geometry_sections_can_read_other_sections_visibility() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/pevalv-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 5).unwrap());
    assert!(!svg.contains(" A"), "hidden arc variant drawn: {svg}");
    assert!(!svg.contains("C"), "hidden curve variant drawn");
}

/// `cubicl-masters.vsdx` page 2 ("Large L-shaped workstation"): child shapes read `Controls.Row_N.Y`
/// of their parent in their geometry formulas. Without those cells the L-shape collapsed; Visio's
/// export of the master is 391 x 400 px (aspect 0.978) and ours is about 124 x 128 user units.
#[test]
fn master_children_can_read_parent_controls() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/cubicl-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 2).unwrap());
    let view_box = svg.split("viewBox=\"").nth(1).and_then(|rest| rest.split('"').next()).unwrap();
    let parts: Vec<f64> = view_box.split_whitespace().map(|n| n.parse().unwrap()).collect();
    let aspect = parts[2] / parts[3];
    assert!((aspect - 0.9775).abs() < 0.03, "aspect {aspect} from viewBox {view_box}");
}

/// `calndr-masters.vsdx` page 4 ("Special event" balloons): the picture is an EMF whose gradients
/// are masked bitmaps, an OR mask first and then a coloured AND bitmap. The AND pass was dropped, so
/// only the outlines were drawn and the balloons came out unfilled.
#[test]
fn masked_bitmap_picture_keeps_its_colours() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/calndr-masters.vsdx");
    let package = Package::open(source).unwrap();
    let doc = import_document(&package).unwrap();
    let mut media = std::collections::BTreeMap::new();
    for (id, record) in &doc.media {
        if let Some(bytes) = layoutcraft_vsdx::media_bytes(&package, record) {
            media.insert(id.clone(), bytes);
        }
    }
    let scene = layoutcraft_render::scene_from_document_with_options(&doc, 4, &media, layoutcraft_render::SceneOptions::default()).unwrap();
    let png = layoutcraft_render::render_png(&scene, 3.0).unwrap();
    let img = image::load_from_memory(&png).unwrap().to_rgba8();
    let yellow = img.pixels().filter(|p| p.0[0] > 220 && p.0[1] > 170 && p.0[2] < 90 && p.0[3] > 200).count();
    let green = img
        .pixels()
        .filter(|p| (p.0[1] as i32) > p.0[0] as i32 + 30 && (p.0[1] as i32) > p.0[2] as i32 + 30 && p.0[3] > 200)
        .count();
    assert!(yellow > 500, "yellow balloon is not filled ({yellow} yellow pixels)");
    assert!(green > 300, "green balloon is not filled ({green} green pixels)");
}

/// `gantt-masters.vsdx` page 2 ("Milestone"): the master is a group with `Width = 0` (a point on the
/// timeline) whose child is the diamond. A zero-width group was dropped, so the page came out empty.
#[test]
fn zero_width_group_keeps_its_children() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/gantt-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 2).unwrap());
    assert!(svg.contains("fill=\"#000000\""), "the milestone diamond is missing: {svg}");
}

/// `gantt-masters.vsdx` page 4 ("Text block 8pt"): a shape with no fill and no line is exported by
/// Visio as its text, not as its one-inch-wide empty box.
#[test]
fn text_only_shape_is_bounded_by_its_text() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/gantt-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 4).unwrap());
    let width: f64 = svg.split("width=\"").nth(1).and_then(|rest| rest.split('"').next()).unwrap().parse().unwrap();
    assert!(width < 45.0, "picture is {width} px wide, the empty box is 96");
}

/// `radial-gradients.vsdx` (Visio, seven 2 x 1.5 in rectangles, `FillGradientDir` 1-7, black to white)
/// against Visio's own export `radial-gradients.visio.png`. Visio's radial gradients are circles in
/// pixel space that reach the farthest corner, blended along an ease-out curve; direction 4 starts at
/// the top edge's centre. We used ellipses with a linear blend.
#[test]
fn radial_gradients_follow_visios_export() {
    let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus");
    let package = Package::open(corpus.join("radial-gradients.vsdx")).unwrap();
    let doc = import_document(&package).unwrap();
    let scene = scene_from_document(&doc, 0).unwrap();
    let ours = image::load_from_memory(&layoutcraft_render::render_png(&scene, 1.0).unwrap()).unwrap().to_rgb8();
    let visio = image::open(corpus.join("radial-gradients.visio.png")).unwrap().to_rgb8();
    assert_eq!((ours.width(), ours.height()), (visio.width(), visio.height()), "picture size");
    // (name, rectangle index, start and end of a sample line as fractions of the 192 x 144 px box)
    let lines: [(&str, u32, (f64, f64), (f64, f64)); 5] = [
        ("dir 1 along the bottom edge", 0, (0.99, 0.97), (0.0, 0.97)),
        ("dir 3 from the centre", 2, (0.5, 0.5), (0.99, 0.5)),
        ("dir 4 down the centre line", 3, (0.5, 0.03), (0.5, 0.97)),
        ("dir 5 up the centre line", 4, (0.5, 0.97), (0.5, 0.03)),
        ("dir 6 along the top edge", 5, (0.99, 0.03), (0.0, 0.03)),
    ];
    for (name, index, a, b) in lines {
        let (x0, y0) = ((index % 3) as f64 * 2.4 * 96.0, (index / 3) as f64 * 2.0 * 96.0);
        for step in [4, 8, 12, 16] {
            let t = step as f64 / 20.0;
            let x = x0 + (a.0 + (b.0 - a.0) * t) * 191.0;
            let y = y0 + (a.1 + (b.1 - a.1) * t) * 143.0;
            let (ours_v, visio_v) = (ours.get_pixel(x as u32, y as u32).0[0] as i32, visio.get_pixel(x as u32, y as u32).0[0] as i32);
            assert!((ours_v - visio_v).abs() <= 16, "{name} at {t}: ours {ours_v}, Visio {visio_v}");
        }
    }
}

/// `orgch-masters.vsdx` page 7 ("Staff"): the text style says `Themed` for its font and the document
/// has no theme part, so Visio uses the root style's font (Cambria). It was drawn in the default
/// sans-serif stack.
#[test]
fn themed_font_without_a_theme_falls_back_to_the_root_style() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/orgch-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 7).unwrap());
    assert!(svg.contains("font-family=\"Cambria\""), "{svg}");
    assert!(!svg.contains("Segoe UI"), "{svg}");
}

/// `orgch-masters.vsdx` page 0 ("Executive"): `ShdwPattern` 1 with no `ShapeShdwType` is a legacy
/// shadow, a flat grey copy of the shape moved by the page's `ShdwOffsetX/Y` (0.118 in right and
/// down) and drawn under it. Master children had no shadow support at all.
#[test]
fn legacy_shadow_of_a_master_child_is_drawn() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/orgch-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let scene = scene_from_document(&doc, 0).unwrap();
    let png = layoutcraft_render::render_png(&scene, 2.0).unwrap();
    let img = image::load_from_memory(&png).unwrap().to_rgba8();
    let grey = img.pixels().filter(|p| p.0[0] == p.0[1] && p.0[1] == p.0[2] && (190..215).contains(&p.0[0]) && p.0[3] > 250).count();
    assert!(grey > 1500, "no grey shadow ({grey} shadow-coloured pixels)");
}

/// `gantt-masters.vsdx` page 6 ("Right-angle horizontal"): its text takes the 0.0556 in margins of
/// the "Normal" text style, leaving 85 px of the 96 px box, so Visio wraps it onto four lines.
#[test]
fn text_margins_come_from_the_text_style() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/gantt-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 6).unwrap());
    assert_eq!(svg.matches("<text ").count(), 4, "expected four lines: {svg}");
}

/// `eepath-masters.vsdx` page 5 ("Elbow bus 1"): the master shape has `FlipY = 1`, so its up-pointing
/// geometry is drawn pointing down (the arrow tip is the lowest point, y = 50 px in a 50 px box) and
/// its label moves with the flip. Page shapes' own flips were ignored.
#[test]
fn own_flip_mirrors_the_geometry() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/eepath-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 5).unwrap());
    // The tip of the arrow: x = 50, y = 50 (bottom of the box); unflipped it was at y = 0.
    assert!(svg.contains("L50 50L"), "arrow tip is not at the bottom: {svg}");
}

/// Same page: Visio fills an open path as if it were closed but strokes it open. The outline must not
/// gain a closing edge (a vertical line on the left), so the stroked path has no `Z`.
#[test]
fn open_path_is_filled_but_stroked_open() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/eepath-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 5).unwrap());
    let stroked: Vec<&str> = svg.split("<path ").filter(|p| p.contains("stroke=\"#000000\"")).collect();
    assert!(!stroked.is_empty(), "no stroked path: {svg}");
    assert!(stroked.iter().all(|p| !p.split('"').nth(1).unwrap_or("").ends_with('Z')), "stroke is closed: {svg}");
}

/// `eepath-masters.vsdx` page 6 ("3-line bus"): the label's frame is far wider than the three lines
/// (131 px against 50 px); Visio exports the ink of the text, so the picture is about as wide as the
/// lines, and the lines keep their square caps (`LineCap` 1 = flat).
#[test]
fn wide_text_frame_does_not_widen_the_picture() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/eepath-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 6).unwrap());
    let width: f64 = svg.split("width=\"").nth(1).and_then(|rest| rest.split('"').next()).unwrap().parse().unwrap();
    assert!(width < 70.0, "picture is {width} px wide, the lines are 50");
    assert!(svg.contains("stroke-linecap=\"butt\""), "the lines lost their flat caps");
}

/// `peinst-masters.vsdx` page 6 ("PID Electric Binary"): a 0.5 x 0.03 in group whose only painted
/// parts are three small hatch ticks (12 x 6 px, half the box area); Visio exports those ticks, not
/// the 48 px wide box. The picture was 48 px wide because the ink had to cover less than half the box.
#[test]
fn group_with_a_small_ink_does_not_keep_its_box() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/peinst-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 6).unwrap());
    let width: f64 = svg.split("width=\"").nth(1).and_then(|rest| rest.split('"').next()).unwrap().parse().unwrap();
    assert!(width < 20.0, "picture is {width} px wide, the ticks span 12");
}

/// PERT 2: the cells are shapes without a Character row; their 8 pt comes from the text style.
/// They used to fall back to a 13 px default and overflowed their cells.
#[test]
fn master_children_take_their_text_size_from_the_text_style() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/pert-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 1).unwrap());
    assert!(!svg.contains("font-size=\"13\""), "a cell label still uses the 13 px default");
}

/// NETLME Data Center: a bare `Themed` line weight in a document without a theme part is the
/// root style's 0.75 pt (1 px), not the Office theme's 1 pt (1.33 px).
#[test]
fn themed_line_weight_without_a_theme_falls_back_to_the_root_style() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/netlme-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 0).unwrap());
    assert!(svg.contains("stroke-width=\"1\"") && !svg.contains("stroke-width=\"1.33\""), "{svg}");
}

/// Rectangular gradient presets (8-11) are nested rectangles around a corner or the centre; they
/// are drawn as a pattern of bands (SVG has no such gradient), not a linear fade.
#[test]
fn rectangular_gradient_presets_are_drawn_as_nested_bands() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/rect-gradients.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 0).unwrap());
    assert!(svg.matches("<pattern").count() >= 4, "expected four rectangular presets");
}

/// OFFFRN 2: a group has no line of its own, but its parts do; the picture must leave room for
/// half of the parts' line width instead of clipping the arms of the chair.
#[test]
fn group_parts_pad_the_picture_by_their_own_line_width() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/offfrn-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 2).unwrap());
    let viewbox: Vec<f64> = svg
        .split("viewBox=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .split(' ')
        .map(|v| v.parse().unwrap())
        .collect();
    assert!(viewbox[2] > 27.0, "viewBox {viewbox:?} is as narrow as the bare geometry");
}

/// PEVESS 3: the reaction band is a hatch (FillPattern 2); master children used to lose their
/// pattern and paint the foreground colour as a solid.
#[test]
fn master_children_keep_their_hatch_pattern() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/pevess-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 3).unwrap());
    assert!(svg.contains("<pattern"), "the hatched band is not a pattern");
}

/// BPMN Expanded Sub-Process: two grey placeholder shapes are hidden by
/// `NoShow = GUARD(Sheet.5!User.IsInstance)`, a User row of the master's top shape.
#[test]
fn nested_shapes_read_the_user_cells_of_the_masters_top_shape() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/bpmn-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 3).unwrap());
    assert!(!svg.contains("#646464"), "the placeholder shapes are still drawn");
}

/// UCMNME Diagram Overview: a master container paints its own outline under its sub-shapes (it
/// was dropped), the sub-shapes are not drawn twice, and the stencil's built-in theme (ThemeIndex 37)
/// means black outlines, not this document's accent colour.
#[test]
fn master_container_draws_its_outline_once_in_the_stencils_colours() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/ucmnme-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let svg = to_svg(&scene_from_document(&doc, 4).unwrap());
    assert!(svg.contains("M0 384L480 384"), "the 5 x 4 in outline is missing");
    assert_eq!(svg.matches("Communication").count(), 1, "title drawn more than once");
    assert!(!svg.contains("#c05046"), "the document's accent colour is used");
}

/// CALNDR Month: the grid's rows are sized by `CEILING(text height, 1/16 in)`; with the
/// significance dropped every caption was a full inch tall and the week grid collapsed.
#[test]
fn calendar_month_rows_use_ceiling_with_significance() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/calndr-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let scene = scene_from_document(&doc, 0).unwrap();
    // 10.4 x 7.3 in at 96 dpi; the picture used to be far taller than the master.
    assert!(scene.height < 7.3 * 96.0 + 20.0, "picture is {} px tall", scene.height);
}

/// EEPATH 3-line bus: the "W1" label hangs below the lines, and Visio's picture leaves room under
/// its ink (116 px at 3.06 scale, about 38 user px, against 30 without the room).
#[test]
fn label_below_the_shape_gets_room_under_its_ink() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/eepath-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let scene = scene_from_document(&doc, 6).unwrap();
    assert!(scene.height > 36.0, "picture is {} px tall", scene.height);
}

fn corpus_scene(name: &str, page: usize) -> layoutcraft_render::Scene {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus").join(name);
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    scene_from_document(&doc, page).unwrap()
}

fn corpus_svg(name: &str, page: usize) -> String {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus").join(name);
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    to_svg(&scene_from_document(&doc, page).unwrap())
}

/// CHART bar graph: each bar's NoShow/HideText formulas read `Sheet.5!Width`, the group's width.
/// Read as the bar's own width they hid every bar but the first, and the dotted NonPrinting unit
/// bar (Visio leaves it out of its export) widened the picture.
#[test]
fn bar_graph_draws_all_bars_and_skips_the_nonprinting_guide() {
    let svg = corpus_svg("chart-masters.vsdx", 0);
    assert!(svg.matches("<path").count() >= 10, "bars missing");
    assert!(svg.contains(">90<") && svg.contains(">30<"), "bar labels missing");
    let scene = corpus_scene("chart-masters.vsdx", 0);
    assert!(scene.width < 3.75 * 96.0 + 20.0, "picture is {} px wide", scene.width);
}

/// BLOCK concentric layer: two nested circles in separate Geometry sections fill as one compound
/// path, so the inner circle is a hole.
#[test]
fn nested_geometry_sections_leave_a_hole() {
    let svg = corpus_svg("block-masters.vsdx", 6);
    assert!(
        svg.split("<path").any(|path| path.contains("evenodd") && path.matches('M').count() >= 2),
        "ring fill is not one compound path"
    );
}

/// ENTITY Category: `HideText = GUARD(User.DBHideDiscriminator)` hides the label of a plain
/// top-level master shape.
#[test]
fn hide_text_applies_to_top_level_master_shapes() {
    let svg = corpus_svg("entity-masters.vsdx", 4);
    assert!(!svg.contains("Discriminator"), "hidden label drew");
}

/// HVACD straight duct: the master's text is a field; the instance stores its own result.
#[test]
fn nested_field_text_uses_the_instances_value() {
    let svg = corpus_svg("hvacd-masters.vsdx", 0);
    assert!(svg.contains("6,35mm"), "instance field text missing");
    assert!(!svg.contains("24in."), "master's cached field text drew");
}

/// CALOUT Annotation: the text child is turned -90 inside a group turned 90, so the text ends up
/// upright, and the dotted leader that runs outside the group box counts towards the picture.
#[test]
fn annotation_text_is_upright_and_the_leader_widens_the_picture() {
    let svg = corpus_svg("calout-masters.vsdx", 4);
    // net rotation of the text group is zero: the two rotations cancel
    assert!(svg.contains("rotate(-90") && svg.contains("rotate(90"), "{svg}");
    let scene = corpus_scene("calout-masters.vsdx", 4);
    assert!(scene.width > 120.0, "leader missing from the picture: {} px wide", scene.width);
}

/// VENNDIAG captions: `Size = Themed` is 12 pt (not the 8 pt of the style) and the captions' text,
/// turned -90 inside a shape turned 90, sits on the leader line.
#[test]
fn venn_captions_use_the_themed_size_and_turn_with_their_shape() {
    let svg = corpus_svg("venndiag-masters.vsdx", 4);
    assert!(svg.contains("font-size=\"16\""), "caption is not 12 pt: {svg}");
}

/// BPRES report masters are embedded Excel sheets (`ForeignType='Object'`); Visio paints the
/// shape's own fill and outline behind the preview picture.
#[test]
fn ole_object_keeps_its_fill_behind_the_preview() {
    let svg = corpus_svg("bpres-masters.vsdx", 6);
    assert!(svg.contains("#ffffc1"), "OLE fill missing: {svg}");
}

/// BA_GRP Venn shape: the stored FillGradient rows are unused when the theme's fill matrix has no
/// gradient (`FillGradientEnabled = THEMEVAL()` is FALSE); the shape is a solid 50 % fill.
#[test]
fn stored_gradient_rows_are_unused_when_the_theme_has_none() {
    let svg = corpus_svg("bagrp-masters.vsdx", 0);
    assert!(!svg.contains("Gradient"), "gradient drawn: {svg}");
    assert!(svg.contains("#9dbb61"));
}

/// DGICON status icon: the circle's NoShow is `IF(INDEX(n,"1;0;1;1;1;0;0")=1,FALSE,TRUE)`.
#[test]
fn index_over_a_literal_list_shows_the_status_circle() {
    let svg = corpus_svg("dgicon-masters.vsdx", 3);
    assert!(svg.contains("#7ac943"), "circle hidden: {svg}");
}

/// ADS site link bridge: master children with BeginArrow/EndArrow 4 get filled arrowheads.
#[test]
fn line_shaped_master_children_draw_their_arrowheads() {
    let svg = corpus_svg("ads-masters.vsdx", 2);
    assert!(svg.matches("Z\" fill=\"#000000\" stroke=\"none\"").count() >= 4, "arrowheads missing");
}

/// ADO Container uses fill pattern 9, a one-device-pixel checkerboard.
#[test]
fn pattern_9_is_a_pixel_checkerboard() {
    let svg = corpus_svg("ado-masters.vsdx", 2);
    assert!(svg.contains("width=\"2\" height=\"2\""), "checkerboard tile missing");
}

/// ALARM keypad: the "C" label beside the icon sits in a text style with a 4 pt left indent (its
/// centred text moves 2 pt right) and Visio pads a label that sticks out of the shape's box, so
/// its picture is 48 px wide (ours was 38).
#[test]
fn indented_label_beside_an_icon_widens_the_picture() {
    let scene = corpus_scene("alarm-masters.vsdx", 2);
    assert!(scene.width > 47.0, "picture too narrow: {}", scene.width);
    let svg = corpus_svg("alarm-masters.vsdx", 2);
    assert!(svg.contains("<text x=\"100.9"), "label not indented: {svg}");
}

/// BLOCK3 "Vertical bar": the master group is turned -90 degrees with FlipY set; its body must be
/// mirrored about the box before the turn (the extrusion's dark face goes to the right in
/// Visio's export, it came out on the left).
#[test]
fn flipped_master_group_mirrors_its_body() {
    let svg = corpus_svg("block3-masters.vsdx", 3);
    assert!(svg.contains("L108 -12"), "extrusion not mirrored: {svg}");
}

/// DVCALL "Data bar 3": the master group's own geometry is the green bar; its only child is a
/// text-only shape, so the body paints nothing. The group geometry must still be drawn.
#[test]
fn group_geometry_is_drawn_when_its_children_paint_nothing() {
    let svg = corpus_svg("dvcall-masters.vsdx", 7);
    assert!(svg.contains("fill=\"#9dbb61\""), "bar missing: {svg}");
}

/// DVCALL "Phone callout": the left-aligned label sits one point (1.33 px) inside its box, because
/// of its 1 pt LeftMargin.
#[test]
fn left_aligned_child_text_starts_after_its_left_margin() {
    let svg = corpus_svg("dvcall-masters.vsdx", 1);
    assert!(svg.contains("<text x=\"57.9"), "label not inset by its margin: {svg}");
}

/// BTHKT "Corner bathtub": the tub is an Ellipse row whose two axis points lie on the diagonals, so
/// it is a tilted ellipse (drawn with Beziers), not an axis-aligned circle (an `A` arc).
#[test]
fn ellipse_row_with_diagonal_axes_is_tilted() {
    let svg = corpus_svg("bthkt-masters.vsdx", 3);
    assert!(!svg.contains("A22.4"), "tub drawn as a circle: {svg}");
    assert!(svg.contains("C"), "no curves: {svg}");
}

/// ADO "Site link": the checker-patterned building sides have no FillBkgnd of their own and the
/// document has no theme part, so the pattern's background is black (the root style), not a theme tint.
#[test]
fn pattern_background_without_theme_is_black() {
    let svg = corpus_svg("ado-masters.vsdx", 6);
    assert!(svg.contains("<pattern") && svg.contains("fill=\"#000000\"/><rect x=\"1\""), "checker back not black: {svg}");
}

/// DBIDEF1X "Entity": the optional-attribute marker child has `HideText` set, so the group's
/// fallback label (built from its children's fields) must not show "(O)" next to each attribute.
#[test]
fn hidden_marker_fields_are_not_part_of_the_group_label() {
    let svg = corpus_svg("dbidef1x-masters.vsdx", 0);
    assert!(!svg.contains("(O)") && !svg.contains(">O)"), "hidden marker shown: {svg}");
}

/// CYCLEDIAG "Partial layer 4": the shape's `LineWeight` is a bare `THEMEVAL()` over the "Block
/// Normal" style whose weight is `THEMEVAL("LineWeight",0.24PT)`; Visio draws the theme's 0.75 pt
/// (1 px), not the 0.24 pt default.
#[test]
fn themed_style_line_weight_uses_the_theme_weight() {
    let svg = corpus_svg("cyclediag-masters.vsdx", 3);
    assert!(svg.contains("stroke-width=\"1\""), "arc outline not 0.75 pt: {svg}");
}

/// BCKGRN "Vertical Gradient" (FillPattern 28): Visio blends the legacy pattern gradients 25-29
/// along a smoothstep curve (white stays near white at the top), not the ease-out of theme
/// gradients; the stop at one quarter of the height is lighter than a straight mix.
#[test]
fn legacy_pattern_gradient_is_smooth() {
    let svg = corpus_svg("bckgrn-masters.vsdx", 2);
    assert!(svg.contains("stop-color=\"#fafafa\"") || svg.contains("stop-color=\"#f9f9f9\"") || svg.contains("stop-color=\"#fbfbfb\""), "no near-white second stop: {svg}");
}

/// AWS "Client": the 9 pt Cambria label sits below the icon in a one-line block. Visio puts its
/// baseline 0.73 of the 14.4 px line box below the block top (83.5), not 0.8 (84.5).
#[test]
fn cambria_icon_label_baseline_sits_higher() {
    let svg = corpus_svg("awsgen-masters.vsdx", 1);
    assert!(svg.contains("y=\"83.514\""), "label baseline not at 0.73 of the line: {svg}");
}

/// EEPATH Junction: a 0 x 0 inch shape whose circle (0.05 in across) is drawn in absolute inches
/// around the pin. It used to be dropped (zero width) and left an empty picture.
#[test]
fn zero_size_shape_with_geometry_is_drawn() {
    let svg = corpus_svg("eepath2013-masters.vsdx", 4);
    assert!(svg.contains("fill=\"#000000\""), "junction dot missing: {svg}");
}

/// AZURE "Cut and Paste": the master is dropped turned 90 degrees and its label hangs below the
/// icon in local space, so on screen it lies beside it. The picture must cover that label (it was
/// cut off, leaving just the scissors).
#[test]
fn rotated_shape_label_outside_the_box_widens_the_picture() {
    let svg = corpus_svg("azuregeneralsymbols-masters.vsdx", 5);
    let width: f64 = svg
        .split("viewBox=\"").nth(1).and_then(|v| v.split('"').next())
        .and_then(|v| v.split_whitespace().nth(2)).and_then(|w| w.parse().ok()).unwrap();
    assert!(width > 80.0, "label beside the rotated icon missing: width {width}");
}

/// ENTAPP "Laptop": the screen is FillPattern 31 with an explicit FillGradientDir 12, a rectangular
/// gradient from the top-left corner, not the 45 degree linear one the pattern number alone gives.
#[test]
fn legacy_pattern_with_explicit_rectangular_type_is_rectangular() {
    let svg = corpus_svg("entapp-masters.vsdx", 8);
    assert!(svg.contains("<pattern id=\"grad0p"), "no rectangular gradient pattern: {svg}");
}

/// EXCOBJ "Message delivery": the master holds a ruler guide (an infinite horizontal line). It is
/// page furniture and must not be drawn; it used to stretch the picture to 46000 units.
#[test]
fn master_guide_child_is_not_drawn() {
    let svg = corpus_svg("excobj-masters.vsdx", 8);
    let width: f64 = svg
        .split("viewBox=\"").nth(1).and_then(|v| v.split('"').next())
        .and_then(|v| v.split_whitespace().nth(2)).and_then(|w| w.parse().ok()).unwrap();
    assert!(width < 500.0, "guide stretched the picture: width {width}");
}

/// AZURE "MachinesAzureArc": one word wider than the 1 in label block breaks between characters
/// ("MachinesAzure" / "Arc"), as in Visio.
#[test]
fn word_wider_than_the_block_breaks_between_characters() {
    let svg = corpus_svg("azuremanagement-masters.vsdx", 5);
    assert!(svg.matches("<text ").count() >= 2, "long word not wrapped: {svg}");
}

/// MRKTDG "3-D matrix": the axis labels hide through `HideText = Geometry1.NoShow` (their own
/// section cell, cached 1), so no "text" label may be drawn around the faces; four "text" remain.
#[test]
fn matrix_axis_labels_stay_hidden() {
    let svg = corpus_svg("mrktdg-masters.vsdx", 2);
    assert_eq!(svg.matches("<text ").count(), 4, "axis labels shown: {svg}");
}

/// FREEQP "Monitor": the screen is a centre-out rectangular gradient (type 10) whose light centre
/// stays light for a wide plateau; it is drawn as bands, not a single point of colour.
#[test]
fn centre_out_rectangular_gradient_is_drawn_as_bands() {
    let svg = corpus_svg("freeqp-masters.vsdx", 1);
    assert!(svg.contains("<pattern id=\"grad"), "no banded gradient pattern: {svg}");
}

/// Line patterns 2-23 (Visio 16 gallery, 0.02 in lines): every pattern draws its measured run/gap
/// sequence in line-weight units with flat dashes. Pattern 2 is 8 on, 4 off: 15.36 / 7.68 px at 96 dpi.
#[test]
fn numbered_line_patterns_use_the_measured_dash_tables() {
    let svg = corpus_svg("linepattern-gallery.vsdx", 0);
    assert_eq!(svg.matches("stroke-dasharray=").count(), 22, "every pattern must be dashed: {svg}");
    assert!(svg.contains("stroke-dasharray=\"15.36 7.68\""), "pattern 2 (8,4): {svg}");
    assert!(svg.contains("stroke-dasharray=\"3.84 1.92\""), "pattern 23 (2,1): {svg}");
    assert!(svg.contains("stroke-linecap=\"butt\""), "dashes must not take a round cap");
}

/// Line ends 1-35 (Visio 16 gallery): every kind but the measured triangle (4) is drawn from the
/// arrow library, not skipped.
#[test]
fn numbered_arrowheads_are_drawn_from_the_gallery() {
    let svg = corpus_svg("arrowhead-gallery.vsdx", 0);
    assert!(svg.matches("<g transform=\"translate(").count() >= 33, "arrowheads missing: {svg}");
}

/// SDCONT "Waves": FillGradientEnabled is not set and the drawing has no theme, so the fill is flat.
#[test]
fn unset_gradient_flag_without_theme_gives_flat_fill() {
    let svg = corpus_svg("sdcont-masters.vsdx", 2);
    assert!(!svg.contains("<linearGradient") && !svg.contains("<pattern id=\"grad"), "unexpected gradient: {svg}");
}

/// PTSINT_VISIO2013 "Currency exchange": a rounded square whose shape-level line is unset; the frame is
/// the stroke of the part that traces the box. The clip alone cut that stroke in half, so the rounded
/// outline must be drawn from the part's line.
#[test]
fn rounded_master_frame_is_stroked_from_its_box_part() {
    let svg = corpus_svg("ptsint-visio2013-masters.vsdx", 3);
    assert!(svg.contains("<rect x=\"0\" y=\"0\" width=\"48\" height=\"48\" rx=\"6\" fill=\"none\" stroke=\"#000000\""), "no rounded outline: {svg}");
}

/// ORGPAN "Manager Panel": the translucent name plate comes after the person picture in the master's
/// own order, so it must be drawn over the picture, not under it.
#[test]
fn master_part_after_the_picture_is_drawn_over_it() {
    let svg = corpus_svg("orgpan-masters.vsdx", 1);
    let picture = svg.find("<g data-vid=\"4\"").expect("picture shape missing");
    let plate = svg.rfind("fill-opacity=\"0.6\"").expect("name plate missing");
    assert!(plate > picture, "name plate is under the picture: {svg}");
}

fn corpus_picture(name: &str, page: usize) -> layoutcraft_model::Shape {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus").join(name);
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    doc.pages[page].shapes.iter().find(|shape| shape.image.is_some()).cloned().expect("no picture shape")
}

/// ORGPET "Staff Petals": the picture's Foreign shape is an Ellipse, so the photo is shown as a circle.
#[test]
fn picture_is_clipped_to_its_foreign_ellipse() {
    let shape = corpus_picture("orgpet-masters.vsdx", 6);
    let clip = shape.image.unwrap().extra.get("clipPath").and_then(|v| v.as_str().map(str::to_string));
    assert!(clip.is_some_and(|d| d.contains('A')), "no elliptical clip");
}

/// ORGSTO "Staff Stone": the picture takes the Foreign shape's Rounding (rounded corners).
#[test]
fn picture_keeps_the_rounding_of_its_shape() {
    let shape = corpus_picture("orgsto-masters.vsdx", 6);
    assert!(shape.rounding.unwrap_or(0.0) > 0.1, "rounding lost: {:?}", shape.rounding);
}

/// ORGTAC "Executive Shapetacular": the picture's Foreign shape has a diamond and an ellipse variant,
/// both hidden; the first (the diamond, straight segments only) cuts the picture.
#[test]
fn picture_uses_the_first_geometry_variant_as_its_outline() {
    let shape = corpus_picture("orgtac-masters.vsdx", 0);
    let clip = shape.image.unwrap().extra.get("clipPath").and_then(|v| v.as_str().map(str::to_string)).expect("no clip");
    assert!(clip.contains('L') && !clip.contains('A'), "not the diamond: {clip}");
}

/// PYRAMID "Inverted Pyramid with caption": the group is FlipX, so the caption blocks (which sit left
/// of the pyramid in the master) move to its right with the rest of the body.
#[test]
fn flipped_group_moves_its_text_blocks_with_the_body() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/pyramid-shapes-masters.vsdx");
    let doc = import_document(&Package::open(source).unwrap()).unwrap();
    let group = doc.pages[3].shapes.iter().find(|shape| shape.sub_texts.len() >= 5).expect("group with captions");
    assert!(group.sub_texts.iter().all(|text| text.x >= group.w), "captions still left of the group: w {} xs {:?}", group.w, group.sub_texts.iter().map(|t| t.x).collect::<Vec<_>>());
}

/// BSTORM "Multiple topic": the text style ("Org Normal") sets Calibri through `THEMEVAL("LatinFont",4)`
/// with the face cached; the document has no theme, but the style's own face is what Visio draws,
/// not the root style's Cambria.
#[test]
fn text_style_font_with_cached_face_beats_the_root_font() {
    let svg = corpus_svg("bstorm-masters.vsdx", 2);
    assert!(svg.contains("font-family=\"Calibri\"") && !svg.contains("Cambria"), "wrong face: {svg}");
}

/// VIDSUR "2 wire": a 10 in wide one-dimensional shape whose only ink is two short ticks in the middle;
/// Visio exports the ticks, not the 10 in box, so the picture must not be 960 units wide.
#[test]
fn box_much_wider_than_its_ink_does_not_stretch_the_picture() {
    let svg = corpus_svg("vidsur-masters.vsdx", 8);
    let width: f64 = svg
        .split("viewBox=\"").nth(1).and_then(|v| v.split('"').next())
        .and_then(|v| v.split_whitespace().nth(2)).and_then(|w| w.parse().ok()).unwrap();
    assert!(width < 120.0, "box stretched the picture: width {width}");
}

/// WEBSIT "Page group": three stacked rectangles of a rounded shape each get their own rounded corners;
/// clipping the lot to the shape's rounded box cut the two offset pages away.
#[test]
fn stacked_polygons_of_a_rounded_shape_are_rounded_one_by_one() {
    let svg = corpus_svg("websit-masters.vsdx", 3);
    assert!(!svg.contains("clip-path=\"url(#round"), "still clipped to the box: {svg}");
    assert!(svg.matches("<path d=\"M").filter(|_| true).count() >= 3 && svg.contains("C"), "pages not rounded: {svg}");
}
