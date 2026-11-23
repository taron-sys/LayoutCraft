//! Bitmap records inside metafiles (EMF STRETCHDIBITS/BITBLT, WMF STRETCHDIB):
//! about 40% of the metafile pictures in the legacy vendor drawings carry them.
use layoutcraft_vsdx::emf::{emf_to_svg_at, wmf_to_svg_at};

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

fn emf_header(w: i32, h: i32) -> Vec<u8> {
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

/// BITMAPINFOHEADER for an uncompressed bottom-up bitmap.
fn bmi(width: i32, height: i32, bits: u16, colors_used: u32) -> Vec<u8> {
    let mut h = Vec::new();
    h.extend(40u32.to_le_bytes());
    h.extend(width.to_le_bytes());
    h.extend(height.to_le_bytes());
    h.extend(1u16.to_le_bytes());
    h.extend(bits.to_le_bytes());
    h.extend(0u32.to_le_bytes()); // BI_RGB
    h.extend(0u32.to_le_bytes()); // size image
    h.extend(0i32.to_le_bytes());
    h.extend(0i32.to_le_bytes());
    h.extend(colors_used.to_le_bytes());
    h.extend(0u32.to_le_bytes());
    h
}

/// 2x2, 24 bits per pixel, bottom-up. Top row: blue, white. Bottom row: red, green.
fn pixels_2x2() -> Vec<u8> {
    let mut bits = Vec::new();
    bits.extend([0, 0, 255, 0, 255, 0, 0, 0]); // bottom row (BGR): red, green, 2 pad
    bits.extend([255, 0, 0, 255, 255, 255, 0, 0]); // top row: blue, white, 2 pad
    bits
}

fn stretch_dibits(dest: (i32, i32, i32, i32), src: (i32, i32, i32, i32), info: &[u8], bits: &[u8], rop: u32) -> Vec<u8> {
    let mut p = vec![0u8; 16]; // bounds
    for v in [dest.0, dest.1, src.0, src.1, src.2, src.3] {
        p.extend(v.to_le_bytes());
    }
    let bmi_off = 80u32;
    let bits_off = bmi_off + info.len() as u32;
    for v in [bmi_off, info.len() as u32, bits_off, bits.len() as u32] {
        p.extend(v.to_le_bytes());
    }
    p.extend(0u32.to_le_bytes()); // usage
    p.extend(rop.to_le_bytes());
    p.extend(dest.2.to_le_bytes());
    p.extend(dest.3.to_le_bytes());
    p.extend(info);
    p.extend(bits);
    emf_rec(81, &p)
}

fn png_pixels(svg: &str) -> image::RgbaImage {
    let start = svg.find("data:image/png;base64,").expect("embedded png") + "data:image/png;base64,".len();
    let end = start + svg[start..].find('"').unwrap();
    let encoded = &svg[start..end];
    let mut bytes = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for ch in encoded.bytes().filter(|b| *b != b'=') {
        let v = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    image::load_from_memory(&bytes).expect("valid png").to_rgba8()
}

#[test]
fn emf_stretchdibits_draws_the_bitmap_with_correct_pixels() {
    let mut emf = emf_header(20, 20);
    emf.extend(stretch_dibits((4, 6, 10, 8), (0, 0, 2, 2), &bmi(2, 2, 24, 0), &pixels_2x2(), 0x00CC_0020));
    emf.extend(emf_rec(14, &[0u8; 12]));
    let svg = emf_to_svg_at(&emf, None).expect("converted").svg;
    assert!(svg.contains("<image x=\"4.00\" y=\"6.00\" width=\"10.00\" height=\"8.00\""), "{}", &svg[..svg.len().min(400)]);
    let img = png_pixels(&svg);
    assert_eq!((img.width(), img.height()), (2, 2));
    assert_eq!(img.get_pixel(0, 0).0, [0, 0, 255, 255], "top-left blue");
    assert_eq!(img.get_pixel(1, 0).0, [255, 255, 255, 255], "top-right white");
    assert_eq!(img.get_pixel(0, 1).0, [255, 0, 0, 255], "bottom-left red");
    assert_eq!(img.get_pixel(1, 1).0, [0, 255, 0, 255], "bottom-right green");
}

#[test]
fn source_rectangle_crops_the_bitmap() {
    let mut emf = emf_header(20, 20);
    // Only the right column (x = 1, width 1) of the source.
    emf.extend(stretch_dibits((0, 0, 5, 10), (1, 0, 1, 2), &bmi(2, 2, 24, 0), &pixels_2x2(), 0x00CC_0020));
    emf.extend(emf_rec(14, &[0u8; 12]));
    let svg = emf_to_svg_at(&emf, None).unwrap().svg;
    let img = png_pixels(&svg);
    assert_eq!((img.width(), img.height()), (1, 2));
    assert_eq!(img.get_pixel(0, 0).0, [255, 255, 255, 255]);
    assert_eq!(img.get_pixel(0, 1).0, [0, 255, 0, 255]);
}

#[test]
fn indexed_bitmaps_use_their_palette() {
    // 1 bit per pixel, 8x1: palette 0 = black, 1 = red; pattern 10101010.
    let mut info = bmi(8, 1, 1, 2);
    info.extend([0, 0, 0, 0, 0, 0, 255, 0]); // BGRA entries: black, red
    let bits = [0b1010_1010u8, 0, 0, 0]; // one row padded to 4 bytes
    let mut emf = emf_header(20, 20);
    emf.extend(stretch_dibits((0, 0, 8, 1), (0, 0, 8, 1), &info, &bits, 0x00CC_0020));
    emf.extend(emf_rec(14, &[0u8; 12]));
    let img = png_pixels(&emf_to_svg_at(&emf, None).unwrap().svg);
    assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(img.get_pixel(1, 0).0, [0, 0, 0, 255]);
    assert_eq!(img.get_pixel(6, 0).0, [255, 0, 0, 255]);
}

#[test]
fn raster_operations_without_a_bitmap_and_masks() {
    // BLACKNESS with no source bitmap fills the rectangle black.
    let mut emf = emf_header(20, 20);
    emf.extend(stretch_dibits((1, 1, 5, 5), (0, 0, 0, 0), &[], &[], 0x0000_0042));
    emf.extend(emf_rec(14, &[0u8; 12]));
    let svg = emf_to_svg_at(&emf, None).unwrap().svg;
    assert!(svg.contains("fill=\"#000000\""), "{svg}");
    assert!(!svg.contains("<image"), "{svg}");
    // Black-and-white SRCAND masks are skipped (they only punch holes for a following OR pass).
    let mask: Vec<u8> = vec![0, 0, 0, 255, 255, 255, 0, 0, /* row 2 */ 255, 255, 255, 0, 0, 0, 0, 0];
    let mut emf = emf_header(20, 20);
    emf.extend(stretch_dibits((0, 0, 2, 2), (0, 0, 2, 2), &bmi(2, 2, 24, 0), &mask, 0x0088_00C6));
    emf.extend(emf_rec(14, &[0u8; 12]));
    assert!(!emf_to_svg_at(&emf, None).unwrap().svg.contains("<image"));
}

#[test]
fn coloured_and_bitmap_is_drawn_with_white_transparent() {
    // An OR mask followed by an AND picture is the other order of a masked blit: the AND bitmap
    // carries the colours and white leaves the destination alone.
    let mut emf = emf_header(20, 20);
    emf.extend(stretch_dibits((0, 0, 2, 2), (0, 0, 2, 2), &bmi(2, 2, 24, 0), &pixels_2x2(), 0x0088_00C6));
    emf.extend(emf_rec(14, &[0u8; 12]));
    let svg = emf_to_svg_at(&emf, None).unwrap().svg;
    let img = png_pixels(&svg);
    assert_eq!(img.get_pixel(1, 0).0[3], 0, "the white pixel must be transparent");
    assert_eq!(img.get_pixel(0, 1).0, [255, 0, 0, 255], "red stays");
}

#[test]
fn wmf_stretchdib_places_the_bitmap() {
    // WMF header + SETWINDOWORG/EXT + STRETCHDIB.
    fn rec(func: u16, params: &[u8]) -> Vec<u8> {
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
    let i16s = |v: &[i16]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    let mut wmf = Vec::new();
    wmf.extend(1u16.to_le_bytes());
    wmf.extend(9u16.to_le_bytes());
    wmf.extend(0x0300u16.to_le_bytes());
    wmf.extend([0u8; 12]);
    wmf.extend(rec(0x020B, &i16s(&[0, 0]))); // window org
    wmf.extend(rec(0x020C, &i16s(&[20, 20]))); // window ext
    // STRETCHDIB: rop(4) usage(2) srcH srcW srcY srcX destH destW destY destX, then the DIB.
    let mut p = Vec::new();
    p.extend(0x00CC_0020u32.to_le_bytes());
    p.extend(0u16.to_le_bytes());
    p.extend(i16s(&[2, 2, 0, 0, 8, 10, 6, 4]));
    p.extend(bmi(2, 2, 24, 0));
    p.extend(pixels_2x2());
    wmf.extend(rec(0x0F43, &p));
    wmf.extend(rec(0, &[]));
    let svg = wmf_to_svg_at(&wmf, None).expect("converted").svg;
    assert!(svg.contains("<image x=\"4.00\" y=\"6.00\" width=\"10.00\" height=\"8.00\""), "{}", &svg[..svg.len().min(400)]);
    let img = png_pixels(&svg);
    assert_eq!(img.get_pixel(0, 0).0, [0, 0, 255, 255]);
}

#[test]
fn truncated_or_hostile_bitmaps_do_not_panic() {
    let mut emf = emf_header(20, 20);
    // Claims 4000x4000 pixels but carries 8 bytes of data.
    emf.extend(stretch_dibits((0, 0, 4, 4), (0, 0, 4000, 4000), &bmi(4000, 4000, 24, 0), &[0u8; 8], 0x00CC_0020));
    // Offsets that point outside the record.
    let mut bad = stretch_dibits((0, 0, 2, 2), (0, 0, 2, 2), &bmi(2, 2, 24, 0), &pixels_2x2(), 0x00CC_0020);
    bad[8 + 40..8 + 44].copy_from_slice(&0xFFFF_0000u32.to_le_bytes());
    emf.extend(bad);
    emf.extend(emf_rec(14, &[0u8; 12]));
    let _ = emf_to_svg_at(&emf, None);
    for cut in 0..emf.len().min(200) {
        let _ = emf_to_svg_at(&emf[..cut], None);
    }
}

/// A path built from `MoveTo` and several `PolyLineTo` / `PolyBezierTo` records is one figure: each
/// record continues from the current point. Starting a subpath per record made a fill or clip close
/// every piece on its own and cut wedges out of the shape.
#[test]
fn path_records_continue_the_open_figure() {
    fn points16(kind: u32, pts: &[(i16, i16)]) -> Vec<u8> {
        let mut p = vec![0u8; 16]; // bounds
        p.extend((pts.len() as u32).to_le_bytes());
        for (x, y) in pts {
            p.extend(x.to_le_bytes());
            p.extend(y.to_le_bytes());
        }
        emf_rec(kind, &p)
    }
    let mut emf = emf_header(40, 40);
    emf.extend(emf_rec(59, &[])); // BEGINPATH
    let mut mv = Vec::new();
    mv.extend(2i32.to_le_bytes());
    mv.extend(2i32.to_le_bytes());
    emf.extend(emf_rec(27, &mv)); // MOVETOEX
    emf.extend(points16(89, &[(10, 2), (10, 10)])); // POLYLINETO16
    emf.extend(points16(88, &[(8, 12), (4, 12), (2, 10)])); // POLYBEZIERTO16
    emf.extend(points16(89, &[(2, 2)])); // POLYLINETO16 back to the start
    emf.extend(emf_rec(61, &[])); // CLOSEFIGURE
    emf.extend(emf_rec(60, &[])); // ENDPATH
    emf.extend(emf_rec(62, &[0u8; 16])); // FILLPATH
    emf.extend(emf_rec(14, &[0u8; 12]));
    let svg = emf_to_svg_at(&emf, None).unwrap().svg;
    let d = svg.split("<path d=\"").nth(1).and_then(|rest| rest.split('"').next()).expect("a path");
    assert_eq!(d.matches('M').count(), 1, "one subpath expected: {d}");
    assert!(d.ends_with('Z'), "{d}");
}
