//! EMF+ (GDI+) records: played when every drawing record is supported, otherwise the GDI
//! records are used as before. Found on the GOV.UK wireframe stencils, whose GDI fallback
//! (pattern-XOR raster operations) cannot show the header bar or the buttons.
use layoutcraft_vsdx::emf::emf_to_svg_at;

const SIGNATURE: u32 = 0x464D_4520;

fn emf_rec(kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut size = 8 + payload.len();
    let pad = (4 - size % 4) % 4;
    size += pad;
    let mut out = Vec::new();
    out.extend(kind.to_le_bytes());
    out.extend((size as u32).to_le_bytes());
    out.extend(payload);
    out.extend(std::iter::repeat(0u8).take(pad));
    out
}

fn header(w: i32, h: i32) -> Vec<u8> {
    let mut p = Vec::new();
    for v in [0, 0, w - 1, h - 1] {
        p.extend(v.to_le_bytes());
    }
    for v in [0, 0, w * 10, h * 10] {
        p.extend(v.to_le_bytes());
    }
    p.extend(SIGNATURE.to_le_bytes());
    p.extend(0x0001_0000u32.to_le_bytes());
    p.extend(0u32.to_le_bytes());
    p.extend(0u32.to_le_bytes());
    p.extend(8u16.to_le_bytes());
    p.extend(0u16.to_le_bytes());
    p.extend([0u8; 12]);
    for v in [w, h, w / 10, h / 10] {
        p.extend(v.to_le_bytes());
    }
    emf_rec(1, &p)
}

/// One EMF+ record: type, flags, size, data size, data.
fn plus(kind: u16, flags: u16, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(kind.to_le_bytes());
    out.extend(flags.to_le_bytes());
    out.extend(((12 + data.len()) as u32).to_le_bytes());
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    out
}

/// An EMR_COMMENT holding EMF+ records.
fn comment(records: &[Vec<u8>]) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    body.extend(b"EMF+");
    for r in records {
        body.extend(r);
    }
    let mut payload: Vec<u8> = Vec::new();
    payload.extend((body.len() as u32).to_le_bytes());
    payload.extend(body);
    emf_rec(70, &payload)
}

fn f32s(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn plus_header() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend(0xDBC0_1002u32.to_le_bytes()); // version
    data.extend(0u32.to_le_bytes()); // flags
    data.extend(96u32.to_le_bytes());
    data.extend(96u32.to_le_bytes());
    plus(0x4001, 1, &data)
}

fn fill_rects(argb: u32, rect: [f32; 4]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend(argb.to_le_bytes());
    data.extend(1u32.to_le_bytes());
    data.extend(f32s(&rect));
    plus(0x400A, 0x8000, &data)
}

fn gdi_polygon() -> Vec<u8> {
    // POLYGON16 with a 3-point triangle: the dual file's GDI fallback.
    let mut p = vec![0u8; 16];
    p.extend(3u32.to_le_bytes());
    for (x, y) in [(0i16, 0i16), (50, 0), (50, 50)] {
        p.extend(x.to_le_bytes());
        p.extend(y.to_le_bytes());
    }
    emf_rec(86, &p)
}

fn picture(records: Vec<Vec<u8>>) -> Vec<u8> {
    let mut emf = header(100, 100);
    emf.extend(comment(&[plus_header()]));
    emf.extend(comment(&records));
    emf.extend(gdi_polygon());
    emf.extend(emf_rec(14, &[0u8; 12]));
    emf
}

#[test]
fn fill_rects_draws_the_solid_colour_and_gdi_fallback_records_are_skipped() {
    let svg = emf_to_svg_at(&picture(vec![fill_rects(0xFF1D_70B8, [10.0, 20.0, 30.0, 40.0])]), None).unwrap().svg;
    assert!(svg.contains("M10.00 20.00L40.00 20.00L40.00 60.00L10.00 60.00Z"), "{svg}");
    assert!(svg.contains("fill=\"#1d70b8\""), "{svg}");
    // The GDI triangle is the fallback of the same drawing and must not be drawn on top.
    assert!(!svg.contains("L50.00 50.00"), "GDI fallback drawn: {svg}");
}

#[test]
fn world_transform_and_alpha_apply() {
    let transform = plus(0x402A, 0, &f32s(&[2.0, 0.0, 0.0, 2.0, 5.0, 5.0]));
    let svg = emf_to_svg_at(&picture(vec![transform, fill_rects(0x80FF_0000, [10.0, 10.0, 10.0, 10.0])]), None).unwrap().svg;
    assert!(svg.contains("M25.00 25.00L45.00 25.00L45.00 45.00L25.00 45.00Z"), "{svg}");
    assert!(svg.contains("fill=\"#ff0000\"") && svg.contains("fill-opacity=\"0.50"), "{svg}");
}

#[test]
fn save_and_restore_bring_the_transform_back() {
    let records = vec![
        plus(0x4025, 0, &1u32.to_le_bytes()),
        plus(0x402A, 0, &f32s(&[3.0, 0.0, 0.0, 3.0, 0.0, 0.0])),
        plus(0x4026, 0, &1u32.to_le_bytes()),
        fill_rects(0xFF00_FF00, [1.0, 1.0, 2.0, 2.0]),
    ];
    let svg = emf_to_svg_at(&picture(records), None).unwrap().svg;
    assert!(svg.contains("M1.00 1.00L3.00 1.00L3.00 3.00L1.00 3.00Z"), "{svg}");
}

#[test]
fn an_unsupported_drawing_record_falls_back_to_the_gdi_records() {
    // DrawString (0x401C) is not supported: the whole picture is drawn from the GDI records.
    let svg = emf_to_svg_at(&picture(vec![fill_rects(0xFF1D_70B8, [10.0, 20.0, 30.0, 40.0]), plus(0x401C, 0, &[0u8; 8])]), None)
        .unwrap()
        .svg;
    assert!(svg.contains("L50.00 50.00"), "GDI fallback expected: {svg}");
    assert!(!svg.contains("#1d70b8"), "{svg}");
}

/// A linear gradient brush object (type 4) with the given colours over the rectangle.
fn linear_gradient_brush(id: u8, start: u32, end: u32, rect: [f32; 4]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend(0xDBC0_1002u32.to_le_bytes()); // brush version
    data.extend(4u32.to_le_bytes()); // BrushTypeLinearGradient
    data.extend(0u32.to_le_bytes()); // flags
    data.extend(0u32.to_le_bytes()); // wrap mode
    data.extend(f32s(&rect));
    data.extend(start.to_le_bytes());
    data.extend(end.to_le_bytes());
    data.extend([0u8; 8]);
    plus(0x4008, (1 << 8) | id as u16, &data)
}

/// FillRects with the brush object `id` (no inline colour flag).
fn fill_rects_with_object(id: u32, rect: [f32; 4]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend(id.to_le_bytes());
    data.extend(1u32.to_le_bytes());
    data.extend(f32s(&rect));
    plus(0x400A, 0, &data)
}

#[test]
fn linear_gradient_brush_is_drawn_as_an_svg_gradient() {
    let records = vec![
        linear_gradient_brush(0, 0xFF00_00FF, 0xFFFF_0000, [0.0, 0.0, 100.0, 10.0]),
        fill_rects_with_object(0, [0.0, 0.0, 100.0, 50.0]),
    ];
    let svg = emf_to_svg_at(&picture(records), None).unwrap().svg;
    assert!(svg.contains("<linearGradient") && svg.contains("stop-color=\"#0000ff\"") && svg.contains("stop-color=\"#ff0000\""), "{svg}");
    assert!(svg.contains("fill=\"url(#eg"), "{svg}");
    assert!(!svg.contains("L50.00 50.00"), "GDI fallback drawn: {svg}");
}
