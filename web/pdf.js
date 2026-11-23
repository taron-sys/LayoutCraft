// A small PDF writer for the shell's "Export PDF": one PDF page per drawing page, each page one JPEG picture of the drawing at the page's size.
// No library: the file is a header, numbered objects, a cross-reference table and a trailer, written by hand.
const encoder = new TextEncoder();

/** Escape a string for a PDF literal string (Latin-1 only; anything else becomes '?'). */
function literal(text) {
  let out = '';
  for (const ch of String(text)) {
    const c = ch.codePointAt(0);
    if (ch === '(' || ch === ')' || ch === '\\') out += '\\' + ch;
    else if (c >= 32 && c < 127) out += ch;
    else if (c >= 160 && c < 256) out += '\\' + c.toString(8).padStart(3, '0');
    else out += '?';
  }
  return `(${out})`;
}
const num = (n) => (Math.round(n * 1000) / 1000).toString();

/**
 * pages: [{jpeg: Uint8Array (a baseline or progressive RGB JPEG), pixelWidth, pixelHeight, widthPt, heightPt}] or, for a vector page,
 *        [{vector: {content, fonts: Map(base font -> resource name), gstates: Map(key -> {name, dict}), patterns: [{name, dict}]}, widthPt, heightPt}] (svgpdf.js)
 * options: {title}
 * Returns the PDF as a Uint8Array. Object numbers: 1 catalog, 2 page tree, 3 info, then three per page (page, content stream, picture).
 */
export function buildPdf(pages, { title = 'Drawing' } = {}) {
  if (!pages.length) throw new Error('A PDF needs at least one page');
  const chunks = [];
  const offsets = [];
  let length = 0;
  const push = (data) => { const bytes = typeof data === 'string' ? encoder.encode(data) : data; chunks.push(bytes); length += bytes.length; };
  const begin = (n) => { offsets[n] = length; push(`${n} 0 obj\n`); };
  push('%PDF-1.4\n');
  push(new Uint8Array([0x25, 0xe2, 0xe3, 0xcf, 0xd3, 0x0a])); // a comment of bytes above 127: tells transfer tools the file is binary
  // Object numbers: 1 catalog, 2 page tree, 3 info, then per page: the page, its content stream and either its picture or its vector extras
  // (one object per font and per shading pattern; the transparency states are written inline).
  let next = 4;
  // an embedded font (ttf.js EmbeddedFont) is written once for the whole file: Type0 font, CID font, descriptor, the subset file and its ToUnicode map
  const embeddedObjs = new Map();
  const embeddedNumbers = (ef) => {
    if (!embeddedObjs.has(ef)) embeddedObjs.set(ef, { type0: next++, cid: next++, desc: next++, file: next++, uni: next++ });
    return embeddedObjs.get(ef);
  };
  const numbered = pages.map((p) => {
    const page = next++, content = next++;
    if (p.vector) {
      const fontObjs = [...p.vector.fonts.entries()].map(([base, res]) => {
        const ef = p.vector.embedded?.get(base);
        return ef ? { base, res, no: embeddedNumbers(ef).type0, embedded: true } : { base, res, no: next++ };
      });
      const patternObjs = p.vector.patterns.map((pat) => ({ ...pat, no: next++ }));
      return { p, page, content, fontObjs, patternObjs };
    }
    return { p, page, content, image: next++ };
  });
  const kids = numbered.map((n) => `${n.page} 0 R`).join(' ');
  begin(1); push('<< /Type /Catalog /Pages 2 0 R >>\nendobj\n');
  begin(2); push(`<< /Type /Pages /Kids [${kids}] /Count ${pages.length} >>\nendobj\n`);
  begin(3); push(`<< /Title ${literal(title)} /Producer (${(globalThis.layoutcraftProduct || { name: 'LayoutCraft' }).name}) >>\nendobj\n`);
  for (const n of numbered) {
    const { p, page, content } = n;
    const w = num(p.widthPt), h = num(p.heightPt);
    if (p.vector) {
      const v = p.vector;
      const fonts = n.fontObjs.map((f) => `/${f.res} ${f.no} 0 R`).join(' ');
      const gs = [...v.gstates.values()].map((g) => `/${g.name} ${g.dict}`).join(' ');
      const pats = n.patternObjs.map((q) => `/${q.name} ${q.no} 0 R`).join(' ');
      begin(page);
      push(`<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${w} ${h}] /Resources << /ProcSet [/PDF /Text] ${fonts ? `/Font << ${fonts} >> ` : ''}${gs ? `/ExtGState << ${gs} >> ` : ''}${pats ? `/Pattern << ${pats} >> ` : ''}>> /Contents ${content} 0 R >>\nendobj\n`);
      const stream = encoder.encode(v.content);
      begin(content);
      push(`<< /Length ${stream.length} >>\nstream\n`);
      push(stream);
      push('\nendstream\nendobj\n');
      for (const f of n.fontObjs) { if (f.embedded) continue; begin(f.no); push(`<< /Type /Font /Subtype /Type1 /BaseFont /${f.base} /Encoding /WinAnsiEncoding >>\nendobj\n`); }
      for (const q of n.patternObjs) { begin(q.no); push(`${q.dict}\nendobj\n`); }
    } else {
      begin(page);
      push(`<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${w} ${h}] /Resources << /XObject << /Im0 ${n.image} 0 R >> /ProcSet [/PDF /ImageC] >> /Contents ${content} 0 R >>\nendobj\n`);
      const stream = `q ${w} 0 0 ${h} 0 0 cm /Im0 Do Q\n`;
      begin(content);
      push(`<< /Length ${stream.length} >>\nstream\n${stream}endstream\nendobj\n`);
      begin(n.image);
      push(`<< /Type /XObject /Subtype /Image /Width ${p.pixelWidth} /Height ${p.pixelHeight} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length ${p.jpeg.length} >>\nstream\n`);
      push(p.jpeg);
      push('\nendstream\nendobj\n');
    }
  }
  for (const [ef, o] of embeddedObjs) {
    const font = ef.font, gids = [...ef.used.keys()].sort((a, b) => a - b);
    const baseName = `${ef.tag}+${ef.name}`;
    const file = ef.subset();
    const scale = 1000 / font.unitsPerEm;
    begin(o.type0);
    push(`<< /Type /Font /Subtype /Type0 /BaseFont /${baseName} /Encoding /Identity-H /DescendantFonts [${o.cid} 0 R] /ToUnicode ${o.uni} 0 R >>\nendobj\n`);
    begin(o.cid);
    push(`<< /Type /Font /Subtype /CIDFontType2 /BaseFont /${baseName} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor ${o.desc} 0 R /DW 1000 /W [${gids.map((g) => `${g} [${num(ef.width(g))}]`).join(' ')}] /CIDToGIDMap /Identity >>\nendobj\n`);
    begin(o.desc);
    push(`<< /Type /FontDescriptor /FontName /${baseName} /Flags 4 /FontBBox [${font.bbox.map((v) => Math.round(v * scale)).join(' ')}] /ItalicAngle ${num(font.italicAngle)} /Ascent ${Math.round(font.ascent * scale)} /Descent ${Math.round(font.descent * scale)} /CapHeight ${Math.round(font.capHeight * scale)} /StemV 80 /FontFile2 ${o.file} 0 R >>\nendobj\n`);
    begin(o.file);
    push(`<< /Length ${file.length} /Length1 ${file.length} >>\nstream\n`);
    push(file);
    push('\nendstream\nendobj\n');
    // ToUnicode: every used glyph id -> its code point (UTF-16BE)
    const utf16 = (cp) => (cp > 0xffff ? [0xd800 + ((cp - 0x10000) >> 10), 0xdc00 + ((cp - 0x10000) & 0x3ff)] : [cp]).map((u) => u.toString(16).padStart(4, '0')).join('');
    const map = gids.map((g) => `<${g.toString(16).padStart(4, '0')}> <${utf16(ef.used.get(g))}>`);
    let body = '';
    for (let i = 0; i < map.length; i += 100) body += `${Math.min(100, map.length - i)} beginbfchar\n${map.slice(i, i + 100).join('\n')}\nendbfchar\n`;
    const cmap = `/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <ffff>\nendcodespacerange\n${body}endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n`;
    begin(o.uni);
    push(`<< /Length ${cmap.length} >>\nstream\n${cmap}endstream\nendobj\n`);
  }
  const total = next; // number of objects + 1
  const xref = length;
  let table = `xref\n0 ${total}\n0000000000 65535 f \n`;
  for (let n = 1; n < total; n++) table += `${String(offsets[n]).padStart(10, '0')} 00000 n \n`;
  push(table);
  push(`trailer\n<< /Size ${total} /Root 1 0 R /Info 3 0 R >>\nstartxref\n${xref}\n%%EOF\n`);
  const out = new Uint8Array(length);
  let at = 0;
  for (const c of chunks) { out.set(c, at); at += c.length; }
  return out;
}
