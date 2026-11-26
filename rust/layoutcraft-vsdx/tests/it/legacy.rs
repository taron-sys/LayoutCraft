//! Drawings in the old formats (`layoutcraft_vsdx::legacy`). The inputs are built here by hand from the layout recorded in
//! `docs/47-legacy-formats.md`; they are our own work, so no third-party drawing is needed to run these tests.
use crate::hostile::{mutate_bytes, scale, seed, Rng, Sweep};
use layoutcraft_vsdx::{import_document, legacy, Package};

/// A record of a version 11 drawing: header, data and the zero bytes that separate it from the next one.
fn record(kind: u32, id: u32, data: &[u8], trailer: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for word in [kind, id, 0, data.len() as u32] {
        out.extend(word.to_le_bytes());
    }
    out.extend([0, 0, 0]);
    out.extend(data);
    out.extend(vec![0; trailer]);
    out
}

fn cells(values: &[f64]) -> Vec<u8> {
    values.iter().flat_map(|value| std::iter::once(0x41).chain(value.to_le_bytes())).collect()
}

/// A shape record: no parent, master and master shape as given, no styles.
fn shape(id: u32, master: Option<u32>, master_shape: Option<u32>) -> Vec<u8> {
    let mut data = vec![0xffu8; 54];
    data[10..14].copy_from_slice(&0u32.to_le_bytes());
    data[18..22].copy_from_slice(&master.unwrap_or(u32::MAX).to_le_bytes());
    data[26..30].copy_from_slice(&master_shape.unwrap_or(u32::MAX).to_le_bytes());
    let mut out = record(0x48, id, &data, 0);
    // PinX, PinY, Width, Height, LocPinX, LocPinY, Angle, then the two flips
    let mut transform = cells(&[4.0, 5.0, 2.0, 1.0, 1.0, 0.5, 0.0]);
    transform.extend([0, 0, 0]);
    out.extend(record(0x9b, 1, &transform, 0));
    out
}

fn geometry(points: &[(f64, f64)]) -> Vec<u8> {
    // the list record: header length 0, the ids of the section's flags and rows
    let mut list = 0u32.to_le_bytes().to_vec();
    list.extend((4 * (points.len() as u32 + 1)).to_le_bytes());
    for id in 0..=points.len() as u32 {
        list.extend(id.to_le_bytes());
    }
    let mut out = record(0x6c, 2, &list, 0);
    out.extend(record(0x89, 0, &[0, 0, 0], 0));
    for (at, (x, y)) in points.iter().enumerate() {
        out.extend(record(if at == 0 { 0x8a } else { 0x8b }, at as u32 + 1, &cells(&[*x, *y]), 0));
    }
    out
}

struct Document {
    bytes: Vec<u8>,
}

impl Document {
    /// Append a stream and return the 18-byte pointer to it. A compressed stream here consists of literal bytes only.
    fn stream(&mut self, kind: u32, mut format: u16, data: &[u8], compress: bool) -> Vec<u8> {
        let stored: Vec<u8> = if compress {
            format |= 2;
            data.chunks(8).flat_map(|chunk| std::iter::once(0xff).chain(chunk.iter().copied())).collect()
        } else {
            data.to_vec()
        };
        let mut pointer = Vec::new();
        for word in [kind, 0, self.bytes.len() as u32, stored.len() as u32] {
            pointer.extend(word.to_le_bytes());
        }
        pointer.extend(format.to_le_bytes());
        self.bytes.extend(stored);
        pointer
    }

    fn list(&mut self, kind: u32, children: &[Vec<u8>]) -> Vec<u8> {
        let mut data = Vec::new();
        for word in [8u32, 0, children.len() as u32, 0] {
            data.extend(word.to_le_bytes());
        }
        data.extend(children.concat());
        self.stream(kind, 0x50, &data, false)
    }
}

/// A compound file holding one stream, `VisioDocument` ([MS-CFB]: header, one FAT sector, one directory sector, the stream).
fn compound(mut stream: Vec<u8>) -> Vec<u8> {
    stream.resize(stream.len().max(4096).next_multiple_of(512), 0);
    let sectors = stream.len() / 512;
    assert!(sectors + 2 <= 128);
    let mut header = vec![0u8; 512];
    header[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    let mut put = |at: usize, value: u32| header[at..at + 4].copy_from_slice(&value.to_le_bytes());
    put(30, 9 | (6 << 16));
    put(44, 1);
    put(48, 1);
    put(56, 4096);
    put(60, 0xffff_fffe);
    put(68, 0xffff_fffe);
    for slot in 1..109 {
        put(76 + slot * 4, u32::MAX);
    }
    let mut fat = vec![u32::MAX; 128];
    fat[0] = 0xffff_fffd;
    fat[1] = 0xffff_fffe;
    for n in 0..sectors {
        fat[2 + n] = if n + 1 == sectors { 0xffff_fffe } else { 3 + n as u32 };
    }
    let mut directory = vec![0u8; 512];
    for (slot, (name, kind, start, size)) in [("Root Entry", 5u8, 0xffff_fffeu32, 0u32), ("VisioDocument", 2, 2, stream.len() as u32)].iter().enumerate() {
        let entry = &mut directory[slot * 128..(slot + 1) * 128];
        let wide: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        entry[..wide.len()].copy_from_slice(&wide);
        entry[64..66].copy_from_slice(&(wide.len() as u16 + 2).to_le_bytes());
        entry[66] = *kind;
        entry[116..120].copy_from_slice(&start.to_le_bytes());
        entry[120..124].copy_from_slice(&size.to_le_bytes());
    }
    let fat: Vec<u8> = fat.iter().flat_map(|value| value.to_le_bytes()).collect();
    [header, fat, directory, stream].concat()
}

/// A drawing with one master (a closed outline of three rows) and one page whose shape keeps two of those rows and has a text.
fn binary(version: u8) -> Vec<u8> {
    let mut document = Document { bytes: vec![0; 0x40] };
    document.bytes[..20].copy_from_slice(b"Visio (TM) Drawing\r\n");
    document.bytes[0x1a] = version;
    let master_shape = [shape(5, None, None), geometry(&[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0)])].concat();
    let master_shape = document.stream(0x48, 0xd0, &master_shape, false);
    let master = document.list(0x1e, &[master_shape]);
    // The master's stream index (0) is what page shapes refer to; its shape's id is in its record.
    let stencils = document.list(0x1d, &[master]);
    let mut sheet = record(0x46, 0, &[0xff; 54], 0);
    sheet.extend(record(0x92, 0, &cells(&[8.5, 11.0, 0.125, -0.125, 1.0, 1.0]), 4));
    let text: Vec<u8> = [0u8; 8].into_iter().chain("Hello\0".encode_utf16().flat_map(u16::to_le_bytes)).collect();
    let page = [sheet, shape(1, Some(0), Some(5)), geometry(&[(0.0, 0.0), (2.0, 1.0)]), record(0x0e, 3, &text, 0)].concat();
    let page = document.stream(0x15, 0xd0, &page, true);
    let pages = document.list(0x27, &[page]);
    let trailer = document.list(0x14, &[stencils, pages]);
    document.bytes[0x24..0x24 + 18].copy_from_slice(&trailer);
    compound(document.bytes)
}

const XML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<VisioDocument xmlns="http://schemas.microsoft.com/visio/2003/core">
 <Colors><ColorEntry IX="2" RGB="#FF0000"/></Colors>
 <FaceNames><FaceName ID="4" Name="Example Sans"/></FaceNames>
 <Pages><Page ID="0" NameU="Plan"><PageSheet><PageProps><PageWidth>8.5</PageWidth><PageHeight>11</PageHeight></PageProps></PageSheet>
  <Shapes><Shape ID="1" Type="Shape" MasterShape="9">
   <XForm><PinX>4</PinX><PinY>5</PinY><Width>2</Width><Height>1</Height><LocPinX F="Width*0.5">1</LocPinX><LocPinY>0.5</LocPinY></XForm>
   <Fill><FillForegnd>2</FillForegnd><FillPattern>1</FillPattern></Fill>
   <Char IX="0"><Font>4</Font></Char>
   <Geom IX="0"><MoveTo IX="1"><X>0</X><Y>0</Y></MoveTo><LineTo IX="2"><X>2</X><Y>0</Y></LineTo><LineTo IX="3"><X>2</X><Y>1</Y></LineTo></Geom>
   <Text><cp IX="0"/>Hello&#x1E; world</Text>
  </Shape></Shapes>
 </Page></Pages>
</VisioDocument>"##;

fn converted(bytes: &[u8]) -> (Package, Vec<String>) {
    let converted = legacy::convert(bytes).expect("converts");
    (Package::from_bytes(&converted.vsdx).expect("a package"), converted.notes)
}

#[test]
fn a_binary_drawing_becomes_a_package_with_its_page_shape_and_text() {
    let bytes = binary(11);
    assert_eq!(legacy::detect(&bytes), Some(legacy::LegacyKind::Binary));
    let (package, notes) = converted(&bytes);
    let document = import_document(&package).unwrap();
    let page = &document.pages[0];
    assert_eq!(page.shapes.len(), 1);
    assert_eq!(page.shapes[0].label, "Hello");
    assert!(page.shapes[0].w > page.shapes[0].h);
    assert!(notes.iter().any(|note| note.contains("glued")), "{notes:?}");
    // The shape has two of the master section's three rows: the third must not be inherited.
    let page_xml = package.text("visio/pages/page1.xml").unwrap();
    assert!(page_xml.contains("IX='3' Del='1'"), "{page_xml}");
}

#[test]
fn an_unreadable_binary_version_is_refused_by_name() {
    let error = legacy::convert(&binary(5)).unwrap_err().to_string();
    assert!(error.contains("version 5"), "{error}");
}

#[test]
fn an_xml_drawing_becomes_a_package_with_colours_fonts_formulas_and_text() {
    assert_eq!(legacy::detect(XML.as_bytes()), Some(legacy::LegacyKind::Xml));
    let (package, notes) = converted(XML.as_bytes());
    let page_xml = package.text("visio/pages/page1.xml").unwrap();
    // table numbers become what .vsdx writes, formulas stay, and the reference to a master that is not there goes
    assert!(page_xml.contains("N='FillForegnd' V='#ff0000'"), "{page_xml}");
    assert!(page_xml.contains("N='Font' V='Example Sans'"), "{page_xml}");
    assert!(page_xml.contains("F='Width*0.5'"), "{page_xml}");
    assert!(!page_xml.contains("MasterShape"), "{page_xml}");
    assert!(!notes.is_empty());
    let document = import_document(&package).unwrap();
    assert_eq!(document.pages[0].shapes[0].label, "Hello world");
}

#[test]
fn other_files_are_not_taken_for_old_drawings() {
    for bytes in [&b""[..], b"PK\x03\x04", b"<mxfile></mxfile>", b"<VisioDocument/>"] {
        assert_eq!(legacy::detect(bytes), None, "{bytes:?}");
    }
}

#[test]
fn damaged_old_drawings_never_panic() {
    let seeds = [("binary", binary(11)), ("xml", XML.as_bytes().to_vec())];
    let mut rng = Rng::new(seed() ^ 0x1e6);
    let mut sweep = Sweep::new("legacy");
    for n in 0..1500 * scale() {
        let (name, original) = rng.pick(&seeds).clone();
        let mut bytes = original;
        for _ in 0..1 + rng.below(3) {
            mutate_bytes(&mut rng, &mut bytes);
        }
        sweep.run(&format!("#{n} {name}"), bytes, |bytes| {
            if let Ok(converted) = legacy::convert(bytes) {
                if let Ok(package) = Package::from_bytes(&converted.vsdx) {
                    let _ = import_document(&package);
                }
            }
        });
    }
    sweep.finish();
}
