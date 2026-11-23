//! 16-bit WMF pictures. Visio stores legacy metafile pictures as plain WMF (no
//! placeable header) under an `.emf` name; the `Adtran *` vendor drawings in the
//! visioStencils collection were blank until these were converted.
use layoutcraft_vsdx::emf::{metafile_to_svg_at, wmf_to_svg_at};

/// One WMF record: size in words (u32), function (u16), then the parameters.
fn rec(func: u16, params: &[i16]) -> Vec<u8> {
    let words = 3 + params.len();
    let mut out = Vec::new();
    out.extend((words as u32).to_le_bytes());
    out.extend(func.to_le_bytes());
    for p in params {
        out.extend(p.to_le_bytes());
    }
    out
}

fn rec_bytes(func: u16, params: &[u8]) -> Vec<u8> {
    let words = 3 + params.len().div_ceil(2);
    let mut out = Vec::new();
    out.extend((words as u32).to_le_bytes());
    out.extend(func.to_le_bytes());
    out.extend(params);
    if params.len() % 2 == 1 {
        out.push(0);
    }
    out
}

fn header() -> Vec<u8> {
    let mut h = Vec::new();
    h.extend(1u16.to_le_bytes()); // memory metafile
    h.extend(9u16.to_le_bytes()); // header size in words
    h.extend(0x0300u16.to_le_bytes()); // version
    h.extend(0u32.to_le_bytes()); // size (unused by the converter)
    h.extend(0u16.to_le_bytes()); // objects
    h.extend(0u32.to_le_bytes()); // max record
    h.extend(0u16.to_le_bytes()); // members
    h
}

fn placeable(left: i16, top: i16, right: i16, bottom: i16, inch: u16) -> Vec<u8> {
    let mut h = Vec::new();
    h.extend(0x9AC6_CDD7u32.to_le_bytes());
    h.extend(0u16.to_le_bytes());
    for v in [left, top, right, bottom] {
        h.extend(v.to_le_bytes());
    }
    h.extend(inch.to_le_bytes());
    h.extend(0u32.to_le_bytes());
    h.extend(0u16.to_le_bytes()); // checksum
    h
}

fn eof() -> Vec<u8> {
    rec(0, &[])
}

/// SETWINDOWORG/EXT store y before x.
fn window(org: (i16, i16), ext: (i16, i16)) -> Vec<u8> {
    let mut out = rec(0x020B, &[org.1, org.0]);
    out.extend(rec(0x020C, &[ext.1, ext.0]));
    out
}

fn red_brush_selected() -> Vec<u8> {
    // CREATEBRUSHINDIRECT: style 0 (solid), COLORREF 0x000000ff (red), hatch 0.
    let mut out = rec(0x02FC, &[0, 0x00ff, 0x0000, 0]);
    out.extend(rec(0x012D, &[0])); // SELECTOBJECT 0
    out
}

#[test]
fn rectangle_parameters_are_left_top_right_bottom_after_the_window_mapping() {
    let mut wmf = header();
    wmf.extend(window((-100, -50), (200, 100)));
    wmf.extend(red_brush_selected());
    // RECTANGLE is stored bottom, right, top, left.
    wmf.extend(rec(0x041B, &[10, 40, -30, -60]));
    wmf.extend(eof());
    let svg = wmf_to_svg_at(&wmf, None).expect("converted");
    assert!(svg.svg.contains("viewBox=\"0 0 200.00 100.00\""), "{}", svg.svg);
    assert!(svg.svg.contains("fill=\"#ff0000\""), "{}", svg.svg);
    // left -60, top -30, right 40, bottom 10 -> shifted by the window origin
    // (-100, -50): x 40..140, y 20..60.
    assert!(svg.svg.contains("M40.00 20.00L140.00 20.00L140.00 60.00L40.00 60.00Z"), "{}", svg.svg);
}

#[test]
fn polygon_points_and_fill_mode_are_read() {
    let mut wmf = header();
    wmf.extend(window((0, 0), (100, 100)));
    wmf.extend(red_brush_selected());
    wmf.extend(rec(0x0106, &[2])); // SETPOLYFILLMODE winding
    wmf.extend(rec(0x0324, &[3, 0, 0, 50, 0, 50, 50])); // POLYGON
    wmf.extend(eof());
    let svg = wmf_to_svg_at(&wmf, None).unwrap();
    assert!(svg.svg.contains("M0.00 0.00L50.00 0.00L50.00 50.00Z"), "{}", svg.svg);
    assert!(svg.svg.contains("fill-rule=\"nonzero\""), "{}", svg.svg);
}

#[test]
fn placeable_header_gives_the_natural_size() {
    let mut wmf = placeable(0, 0, 1440, 720, 1440); // 1 in x 0.5 in
    wmf.extend(header());
    wmf.extend(window((0, 0), (1440, 720)));
    wmf.extend(eof());
    let svg = wmf_to_svg_at(&wmf, None).unwrap();
    assert!((svg.width_px - 96.0).abs() < 0.01 && (svg.height_px - 48.0).abs() < 0.01);
}

#[test]
fn negative_extent_flips_the_axis() {
    let mut wmf = header();
    // y grows upward: the window spans y 0..100 with the origin (y = 100) at the top.
    wmf.extend(window((0, 100), (100, -100)));
    wmf.extend(red_brush_selected());
    wmf.extend(rec(0x0324, &[3, 0, 0, 50, 0, 50, 50]));
    wmf.extend(eof());
    let svg = wmf_to_svg_at(&wmf, None).unwrap();
    // y = 0 maps to the bottom edge (100), y = 50 to 50.
    assert!(svg.svg.contains("M0.00 100.00L50.00 100.00L50.00 50.00Z"), "{}", svg.svg);
}

#[test]
fn clip_rectangle_text_and_object_slots() {
    let mut wmf = header();
    wmf.extend(window((0, 0), (100, 100)));
    wmf.extend(rec(0x0416, &[80, 90, 10, 20])); // INTERSECTCLIPRECT: bottom,right,top,left
    // Create object 0 then delete it; the next object reuses slot 0.
    wmf.extend(rec(0x02FC, &[0, 0, 0, 0]));
    wmf.extend(rec(0x01F0, &[0]));
    wmf.extend(red_brush_selected());
    wmf.extend(rec(0x041B, &[50, 50, 0, 0]));
    // EXTTEXTOUT: y, x, count, options, string
    let mut text = Vec::new();
    for v in [30i16, 20, 2, 0] {
        text.extend(v.to_le_bytes());
    }
    text.extend(b"Hi");
    wmf.extend(rec_bytes(0x0A32, &text));
    wmf.extend(eof());
    let svg = wmf_to_svg_at(&wmf, None).unwrap();
    assert!(svg.svg.contains("<clipPath id=\"c1\">"), "{}", svg.svg);
    assert!(svg.svg.contains("clip-path=\"url(#c1)\""), "{}", svg.svg);
    assert!(svg.svg.contains("fill=\"#ff0000\""), "{}", svg.svg);
    assert!(svg.svg.contains(">Hi</text>"), "{}", svg.svg);
}

#[test]
fn metafile_dispatch_picks_wmf_and_rejects_junk() {
    let mut wmf = header();
    wmf.extend(window((0, 0), (10, 10)));
    wmf.extend(eof());
    assert!(metafile_to_svg_at(&wmf, None).is_some());
    assert!(metafile_to_svg_at(b"definitely not a metafile", None).is_none());
    assert!(metafile_to_svg_at(&[], None).is_none());
}

#[test]
fn truncated_and_hostile_records_do_not_panic() {
    let mut wmf = header();
    wmf.extend(window((0, 0), (100, 100)));
    // A polygon claiming 60,000 points but carrying 2.
    let mut bad = rec(0x0324, &[0, 0, 0, 0, 0]);
    bad[6..8].copy_from_slice(&60000u16.to_le_bytes());
    wmf.extend(bad);
    // A record whose declared size overruns the data.
    wmf.extend([0xff, 0xff, 0x00, 0x00, 0x24, 0x03]);
    let _ = wmf_to_svg_at(&wmf, None);
    for cut in 0..wmf.len().min(80) {
        let _ = wmf_to_svg_at(&wmf[..cut], None);
    }
}
