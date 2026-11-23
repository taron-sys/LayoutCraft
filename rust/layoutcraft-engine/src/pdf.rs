//! File > Export > PDF, second half: a Rust port of `web/pdf.js` (`buildPdf`, docs/34, step N4.9b) and the export of the whole drawing. One PDF page per drawing page: a vector page
//! (`svgpdf`: real paths, text and gradients) when the converter can draw everything on it, otherwise a picture of the page (the web keeps a JPEG; the native app has no JPEG encoder
//! and embeds the rendered page as a Flate-compressed RGB image). Embedded TrueType subsets are not part of the native export (it has no font files): standard fonts only.
//! `tests/pdf_oracle.rs` compares the bytes of the PDFs of the drawings whose pages are all vector pages with what the web writes.
use std::io::Write;

use crate::svgpdf::PdfPage;
use crate::svgutil::{js_round, js_string};
use crate::Engine;

/// One page of the file.
pub enum PdfPageOut {
    Vector(PdfPage),
    /// A picture of the page: its pixel size, the Flate (zlib) compressed 8 bit RGB rows and the page size in points.
    Raster { pixel_width: u32, pixel_height: u32, rgb_flate: Vec<u8>, width_pt: f64, height_pt: f64 },
}

fn num(n: f64) -> String {
    js_string(js_round(n * 1000.0) / 1000.0)
}

/// Escape a string for a PDF literal string (Latin-1 only; anything else becomes '?').
fn literal(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        let c = ch as u32;
        if matches!(ch, '(' | ')' | '\\') {
            out.push('\\');
            out.push(ch);
        } else if (32..127).contains(&c) {
            out.push(ch);
        } else if (160..256).contains(&c) {
            out.push_str(&format!("\\{c:03o}"));
        } else {
            out.push('?');
        }
    }
    format!("({out})")
}

/// The PDF as bytes. Object numbers: 1 catalog, 2 page tree, 3 info, then per page the page, its content stream and either its picture or its fonts and patterns.
pub fn build_pdf(pages: &[PdfPageOut], title: &str) -> Result<Vec<u8>, String> {
    if pages.is_empty() {
        return Err("A PDF needs at least one page".to_string());
    }
    let mut out: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = vec![0];
    let mut begin = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, n: usize| {
        if offsets.len() <= n {
            offsets.resize(n + 1, 0);
        }
        offsets[n] = out.len();
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
    };
    out.extend_from_slice(b"%PDF-1.4\n");
    out.extend_from_slice(&[0x25, 0xe2, 0xe3, 0xcf, 0xd3, 0x0a]);
    let mut next = 4usize;
    struct Numbered {
        page: usize,
        content: usize,
        font_nos: Vec<usize>,
        pattern_nos: Vec<usize>,
        image: usize,
    }
    let numbered: Vec<Numbered> = pages
        .iter()
        .map(|p| {
            let page = next;
            next += 1;
            let content = next;
            next += 1;
            match p {
                PdfPageOut::Vector(v) => {
                    let font_nos = v.fonts.iter().map(|_| { next += 1; next - 1 }).collect();
                    let pattern_nos = v.patterns.iter().map(|_| { next += 1; next - 1 }).collect();
                    Numbered { page, content, font_nos, pattern_nos, image: 0 }
                }
                PdfPageOut::Raster { .. } => {
                    let image = next;
                    next += 1;
                    Numbered { page, content, font_nos: vec![], pattern_nos: vec![], image }
                }
            }
        })
        .collect();
    let kids: Vec<String> = numbered.iter().map(|n| format!("{} 0 R", n.page)).collect();
    begin(&mut out, &mut offsets, 1);
    out.extend_from_slice(b"<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    begin(&mut out, &mut offsets, 2);
    out.extend_from_slice(format!("<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n", kids.join(" "), pages.len()).as_bytes());
    begin(&mut out, &mut offsets, 3);
    out.extend_from_slice(format!("<< /Title {} /Producer (LayoutCraft) >>\nendobj\n", literal(title)).as_bytes());
    for (p, n) in pages.iter().zip(&numbered) {
        match p {
            PdfPageOut::Vector(v) => {
                let (w, h) = (num(v.width_pt), num(v.height_pt));
                let fonts: Vec<String> = v.fonts.iter().zip(&n.font_nos).map(|((_, res), no)| format!("/{res} {no} 0 R")).collect();
                let gs: Vec<String> = v.gstates.iter().map(|(_, name, dict)| format!("/{name} {dict}")).collect();
                let pats: Vec<String> = v.patterns.iter().zip(&n.pattern_nos).map(|((name, _), no)| format!("/{name} {no} 0 R")).collect();
                begin(&mut out, &mut offsets, n.page);
                out.extend_from_slice(
                    format!(
                        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Resources << /ProcSet [/PDF /Text] {}{}{}>> /Contents {} 0 R >>\nendobj\n",
                        if fonts.is_empty() { String::new() } else { format!("/Font << {} >> ", fonts.join(" ")) },
                        if gs.is_empty() { String::new() } else { format!("/ExtGState << {} >> ", gs.join(" ")) },
                        if pats.is_empty() { String::new() } else { format!("/Pattern << {} >> ", pats.join(" ")) },
                        n.content
                    )
                    .as_bytes(),
                );
                begin(&mut out, &mut offsets, n.content);
                out.extend_from_slice(format!("<< /Length {} >>\nstream\n", v.content.len()).as_bytes());
                out.extend_from_slice(v.content.as_bytes());
                out.extend_from_slice(b"\nendstream\nendobj\n");
                for ((base, _), no) in v.fonts.iter().zip(&n.font_nos) {
                    begin(&mut out, &mut offsets, *no);
                    out.extend_from_slice(format!("<< /Type /Font /Subtype /Type1 /BaseFont /{base} /Encoding /WinAnsiEncoding >>\nendobj\n").as_bytes());
                }
                for ((_, dict), no) in v.patterns.iter().zip(&n.pattern_nos) {
                    begin(&mut out, &mut offsets, *no);
                    out.extend_from_slice(format!("{dict}\nendobj\n").as_bytes());
                }
            }
            PdfPageOut::Raster { pixel_width, pixel_height, rgb_flate, width_pt, height_pt } => {
                let (w, h) = (num(*width_pt), num(*height_pt));
                begin(&mut out, &mut offsets, n.page);
                out.extend_from_slice(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Resources << /XObject << /Im0 {} 0 R >> /ProcSet [/PDF /ImageC] >> /Contents {} 0 R >>\nendobj\n", n.image, n.content).as_bytes());
                let stream = format!("q {w} 0 0 {h} 0 0 cm /Im0 Do Q\n");
                begin(&mut out, &mut offsets, n.content);
                out.extend_from_slice(format!("<< /Length {} >>\nstream\n{stream}endstream\nendobj\n", stream.len()).as_bytes());
                begin(&mut out, &mut offsets, n.image);
                out.extend_from_slice(format!("<< /Type /XObject /Subtype /Image /Width {pixel_width} /Height {pixel_height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n", rgb_flate.len()).as_bytes());
                out.extend_from_slice(rgb_flate);
                out.extend_from_slice(b"\nendstream\nendobj\n");
            }
        }
    }
    let total = next;
    let xref = out.len();
    let mut table = format!("xref\n0 {total}\n0000000000 65535 f \n");
    for n in 1..total {
        table.push_str(&format!("{:010} 00000 n \n", offsets.get(n).copied().unwrap_or(0)));
    }
    out.extend_from_slice(table.as_bytes());
    out.extend_from_slice(format!("trailer\n<< /Size {total} /Root 1 0 R /Info 3 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    Ok(out)
}

/// The result of exporting the drawing.
pub struct PdfExport {
    pub bytes: Vec<u8>,
    pub pages: usize,
    pub vector_pages: usize,
    /// why a page became a picture: "page 3: <clipPath>"
    pub fallbacks: Vec<String>,
}

impl Engine {
    /// File > Export > PDF: every page of the drawing as a PDF page (vector when the converter can draw it, else a picture of the page at `scale` times 96 dpi).
    pub fn export_pdf(&self, scale: f64) -> Result<PdfExport, String> {
        let count = self.page_count().ok_or("no document")?;
        let names: serde_json::Value = serde_json::from_str(&self.page_names_json()?).map_err(|e| e.to_string())?;
        let scale = scale.clamp(1.0, 4.0);
        let mut pages = Vec::new();
        let (mut vector_pages, mut fallbacks) = (0, Vec::new());
        for p in 0..count {
            match self.pdf_page(p)? {
                Ok(v) => {
                    vector_pages += 1;
                    pages.push(PdfPageOut::Vector(v));
                }
                Err(why) => {
                    fallbacks.push(format!("page {}: {}", p + 1, why.0));
                    let (w, h, rgb) = self.render_page_rgb(p, scale)?;
                    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                    z.write_all(&rgb).map_err(|e| e.to_string())?;
                    let rgb_flate = z.finish().map_err(|e| e.to_string())?;
                    pages.push(PdfPageOut::Raster { pixel_width: w, pixel_height: h, rgb_flate, width_pt: (w as f64 / scale) * 0.75, height_pt: (h as f64 / scale) * 0.75 });
                }
            }
        }
        let title = names.get(0).and_then(serde_json::Value::as_str).filter(|n| !n.is_empty()).unwrap_or("Drawing").to_string();
        Ok(PdfExport { bytes: build_pdf(&pages, &title)?, pages: count, vector_pages, fallbacks })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> PdfPage {
        PdfPage { content: "q\nQ\n".into(), fonts: vec![("Helvetica".into(), "F1".into())], gstates: vec![("0.5/1".into(), "GS1".into(), "<< /Type /ExtGState /ca 0.5 /CA 1 >>".into())], patterns: vec![], width_pt: 144.0, height_pt: 72.0 }
    }

    #[test]
    fn a_vector_page_file_has_the_objects_the_xref_points_at() {
        let pdf = build_pdf(&[PdfPageOut::Vector(page())], "T (x)").unwrap();
        let text = String::from_utf8_lossy(&pdf).to_string();
        assert!(text.starts_with("%PDF-1.4\n"));
        assert!(text.contains("/Title (T \\(x\\)) /Producer (LayoutCraft)"));
        assert!(text.contains("/MediaBox [0 0 144 72] /Resources << /ProcSet [/PDF /Text] /Font << /F1 6 0 R >> /ExtGState << /GS1 << /Type /ExtGState /ca 0.5 /CA 1 >> >> >> /Contents 5 0 R"));
        // the cross reference table: every offset starts the object it names (offsets are byte offsets: the file starts with bytes above 127)
        let latin: String = pdf.iter().map(|b| *b as char).collect();
        let start: usize = latin.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        let tail = &pdf[start..];
        assert!(tail.starts_with(b"xref\n0 7\n0000000000 65535 f \n"));
        let table = String::from_utf8_lossy(tail).to_string();
        for line in table.lines().skip(3).take(6) {
            let off: usize = line[..10].parse().unwrap();
            assert!(pdf[off..].starts_with(&format!("{} 0 obj", 1 + table.lines().skip(3).position(|l| l == line).unwrap()).into_bytes()), "{line}");
        }
        assert!(text.ends_with("%%EOF\n"));
    }

    #[test]
    fn a_picture_page_names_its_image_and_titles_are_latin_1() {
        let raster = PdfPageOut::Raster { pixel_width: 2, pixel_height: 1, rgb_flate: vec![1, 2, 3], width_pt: 10.5, height_pt: 5.0 };
        let pdf = build_pdf(&[raster], "\u{e9}\u{4e2d}").unwrap();
        let text = String::from_utf8_lossy(&pdf).to_string();
        assert!(text.contains("/Title (\\351?)"));
        assert!(text.contains("/XObject << /Im0 6 0 R >>"));
        assert!(text.contains("q 10.5 0 0 5 0 0 cm /Im0 Do Q\n"));
        assert!(text.contains("/Filter /FlateDecode /Length 3 >>"));
        assert!(build_pdf(&[], "x").is_err());
    }
}
