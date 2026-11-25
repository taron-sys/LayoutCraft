//! The inputs the sweeps start from: the public fixtures, and small draw.io, Excalidraw, EMF and WMF files built here.
use crate::hostile::{deflate, honest, write_zip, Entry};
use layoutcraft_vsdx::Package;

/// Every public fixture package: (file name, bytes).
pub fn fixtures() -> Vec<(String, Vec<u8>)> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/public/sources");
    let mut out = Vec::new();
    let mut names: Vec<_> = std::fs::read_dir(dir).expect("the public fixtures are in the repository").filter_map(|e| e.ok()).map(|e| e.path()).collect();
    names.sort();
    for path in names {
        if path.extension().and_then(|e| e.to_str()) == Some("vsdx") {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("fixture").to_string();
            out.push((name, std::fs::read(&path).expect("fixture bytes")));
        }
    }
    assert!(out.len() >= 7, "expected the public fixtures, found {}", out.len());
    out
}

/// The parts of a package: (name, bytes), in name order.
pub fn parts(package_bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let package = Package::from_bytes(package_bytes).expect("a fixture opens");
    package.names().map(|n| (n.to_string(), package.entry(n).map(|b| b.to_vec()).unwrap_or_default())).collect()
}

/// The package rebuilt from `parts` with honest headers (deflated).
pub fn zip(parts: &[(String, Vec<u8>)]) -> Vec<u8> {
    let entries: Vec<Entry> = parts.iter().map(|(name, data)| honest(name, data, true)).collect();
    write_zip(&entries)
}

/// A stencil: the built-in "Basic shapes" one.
pub fn stencil() -> Vec<u8> {
    layoutcraft_vsdx::builtin::builtin_stencil().expect("the built-in stencil")
}

pub const DRAWIO: &str = r##"<mxfile host="x"><diagram id="d1" name="Page-1"><mxGraphModel dx="800" dy="600" grid="1" pageWidth="850" pageHeight="1100"><root>
<mxCell id="0"/><mxCell id="1" parent="0"/>
<mxCell id="g" value="Group" style="group" vertex="1" connectable="0" parent="1"><mxGeometry x="10" y="10" width="300" height="200" as="geometry"/></mxCell>
<mxCell id="a" value="A &amp; B" style="rounded=1;whiteSpace=wrap;html=1;fillColor=#dae8fc;strokeColor=#6c8ebf;fontSize=14;fontStyle=1" vertex="1" parent="g"><mxGeometry x="20" y="20" width="120" height="60" as="geometry"/></mxCell>
<mxCell id="b" value="&lt;b&gt;Bold&lt;/b&gt;&lt;br&gt;line" style="ellipse;whiteSpace=wrap;html=1;rotation=30;" vertex="1" parent="g"><mxGeometry x="180" y="100" width="100" height="80" as="geometry"/></mxCell>
<mxCell id="e" value="edge" style="edgeStyle=orthogonalEdgeStyle;rounded=0;endArrow=classic;html=1;" edge="1" parent="1" source="a" target="b"><mxGeometry relative="1" as="geometry"><Array as="points"><mxPoint x="50" y="300"/><mxPoint x="200" y="300"/></Array></mxGeometry></mxCell>
<mxCell id="i" value="" style="shape=image;image=data:image/png,iVBORw0KGgo=;" vertex="1" parent="1"><mxGeometry x="400" y="10" width="40" height="40" as="geometry"/></mxCell>
<mxCell id="t" value="free text" style="text;html=1;align=center;verticalAlign=middle;" vertex="1" parent="1"><mxGeometry x="400" y="100" width="80" height="20" as="geometry"/></mxCell>
<mxCell id="f" style="endArrow=none;" edge="1" parent="1"><mxGeometry relative="1" as="geometry"><mxPoint x="400" y="200" as="sourcePoint"/><mxPoint x="500" y="260" as="targetPoint"/></mxGeometry></mxCell>
</root></mxGraphModel></diagram></mxfile>"##;

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn url_encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A draw.io file whose diagram is stored the compressed way: base64(raw deflate(url-encoded XML)).
pub fn drawio_compressed() -> String {
    let start = DRAWIO.find("<mxGraphModel").unwrap_or(0);
    let end = DRAWIO.find("</mxGraphModel>").map(|n| n + "</mxGraphModel>".len()).unwrap_or(DRAWIO.len());
    let packed = base64(&deflate(url_encode(&DRAWIO[start..end]).as_bytes()));
    format!("<mxfile><diagram name=\"Packed\">{packed}</diagram></mxfile>")
}

pub const EXCALIDRAW: &str = r##"{"type":"excalidraw","version":2,"elements":[
{"id":"a","type":"rectangle","x":0,"y":0,"width":96,"height":48,"angle":0.3,"strokeColor":"#000000","backgroundColor":"#ff0000","strokeWidth":2,"roundness":{"type":3},"groupIds":["g1"],"boundElements":[{"id":"ar","type":"arrow"},{"id":"ta","type":"text"}]},
{"id":"ta","type":"text","x":5,"y":5,"width":50,"height":20,"text":"Hi\nthere","containerId":"a","fontSize":20,"fontFamily":1,"textAlign":"center"},
{"id":"b","type":"ellipse","x":300,"y":0,"width":96,"height":48,"groupIds":["g1"],"frameId":"fr"},
{"id":"d","type":"diamond","x":300,"y":100,"width":60,"height":60},
{"id":"fr","type":"frame","x":-10,"y":-10,"width":500,"height":300,"name":"Frame"},
{"id":"ar","type":"arrow","x":96,"y":24,"width":204,"height":0,"points":[[0,0],[100,30],[204,0]],"startBinding":{"elementId":"a","focus":0,"gap":1},"endBinding":{"elementId":"b","focus":0,"gap":1},"endArrowhead":"triangle","startArrowhead":"dot"},
{"id":"l","type":"line","x":10,"y":200,"width":100,"height":50,"points":[[0,0],[50,50],[100,0],[0,0]]},
{"id":"fd","type":"freedraw","x":10,"y":300,"width":50,"height":50,"points":[[0,0],[10,10],[20,5],[50,50]]},
{"id":"im","type":"image","x":400,"y":10,"width":40,"height":40,"fileId":"f1"},
{"id":"x","type":"embeddable","x":1,"y":1,"width":10,"height":10}
],"appState":{"viewBackgroundColor":"#ffffff"},"files":{"f1":{"mimeType":"image/png","id":"f1","dataURL":"data:image/png;base64,iVBORw0KGgo="}}}"##;

// ---- metafiles ---------------------------------------------------------------------------------------------------------------------

fn emf_rec(kind: u32, body: &[u8]) -> Vec<u8> {
    let mut padded = body.to_vec();
    while padded.len() % 4 != 0 {
        padded.push(0);
    }
    let mut out = Vec::new();
    out.extend(kind.to_le_bytes());
    out.extend(((padded.len() + 8) as u32).to_le_bytes());
    out.extend(padded);
    out
}

fn le32(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// A small valid EMF: header, pen, brush, rectangle, polygon, polyline, text, EOF.
pub fn emf() -> Vec<u8> {
    let mut records = Vec::new();
    records.extend(emf_rec(38, &le32(&[1, 0, 2, 0, 0, 0x00_00_FF]))); // CREATEPEN
    records.extend(emf_rec(39, &le32(&[2, 0, 0x00_FF_00, 0]))); // CREATEBRUSHINDIRECT
    records.extend(emf_rec(37, &le32(&[1]))); // SELECTOBJECT
    records.extend(emf_rec(37, &le32(&[2])));
    records.extend(emf_rec(43, &le32(&[5, 5, 90, 60]))); // RECTANGLE
    let mut poly = le32(&[0, 0, 100, 100, 4]);
    poly.extend(le32(&[10, 10, 90, 10, 50, 80, 10, 10]));
    records.extend(emf_rec(3, &poly)); // POLYGON
    let mut line = le32(&[0, 0, 100, 100, 3]);
    line.extend(le32(&[0, 0, 50, 50, 100, 0]));
    records.extend(emf_rec(4, &line)); // POLYLINE
    let mut text = le32(&[0, 0, 100, 100, 1, 10, 20]);
    text.extend(le32(&[2, 0, 0, 0, 0, 0]));
    let wide: Vec<u8> = "Hi".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let mut body = text;
    body.extend(wide);
    records.extend(emf_rec(84, &body)); // EXTTEXTOUTW
    records.extend(emf_rec(14, &le32(&[0, 0, 0]))); // EOF
    let mut header = le32(&[0, 0, 99, 99, 0, 0, 2646, 2646]); // bounds, frame
    header.extend(0x464D_4520u32.to_le_bytes());
    header.extend(0x0001_0000u32.to_le_bytes());
    header.extend(((88 + records.len()) as u32).to_le_bytes());
    header.extend(le32(&[8]));
    header.extend(4u16.to_le_bytes()); // handles
    header.extend(0u16.to_le_bytes());
    header.extend(le32(&[0, 0, 0]));
    header.extend(le32(&[1000, 1000, 265, 265])); // device, millimetres
    let mut out = emf_rec(1, &header);
    out.extend(records);
    out
}

fn wmf_rec(func: u16, params: &[i16]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(((3 + params.len()) as u32).to_le_bytes());
    out.extend(func.to_le_bytes());
    for p in params {
        out.extend(p.to_le_bytes());
    }
    out
}

/// A small valid placeable WMF: window, polygon, polyline, rectangle, text, EOF.
pub fn wmf() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(0x9AC6_CDD7u32.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    for v in [0i16, 0, 100, 100] {
        out.extend(v.to_le_bytes());
    }
    out.extend(96u16.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    for word in [1u16, 9, 0x0300] {
        out.extend(word.to_le_bytes());
    }
    out.extend(0u32.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(wmf_rec(0x020C, &[100, 100])); // SETWINDOWEXT (y, x)
    out.extend(wmf_rec(0x020B, &[0, 0])); // SETWINDOWORG
    out.extend(wmf_rec(0x0324, &[4, 10, 10, 90, 10, 50, 80, 10, 10])); // POLYGON
    out.extend(wmf_rec(0x0325, &[3, 0, 0, 50, 50, 100, 0])); // POLYLINE
    out.extend(wmf_rec(0x041B, &[60, 90, 5, 5])); // RECTANGLE
    out.extend(wmf_rec(0x0521, &[2, 0x6948, 20, 10])); // TEXTOUT "Hi"
    out.extend(wmf_rec(0, &[]));
    out
}

/// Every seed of the metafile sweep.
pub fn metafiles() -> Vec<(String, Vec<u8>)> {
    vec![("seed.emf".to_string(), emf()), ("seed.wmf".to_string(), wmf())]
}

/// One of the fixtures with a media part added (for a metafile inside a package): the part `visio/media/m.emf` with `data`.
pub fn with_media(fixture: &[u8], name: &str, data: &[u8]) -> Vec<u8> {
    let mut p = parts(fixture);
    p.push((name.to_string(), data.to_vec()));
    zip(&p)
}
