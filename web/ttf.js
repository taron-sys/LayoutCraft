// A small TrueType reader and subsetter for the vector PDF (no library): glyph lookup through the cmap (formats 4 and 12), advance widths, and a subset
// of the glyf outlines (glyph ids stay as they are, unused glyphs become empty) written as a new TrueType file that a PDF embeds as /FontFile2.
// OpenType fonts with CFF outlines, font collections and fonts whose licence bits forbid embedding (OS/2 fsType 2: restricted licence) are refused.

const u16 = (dv, o) => dv.getUint16(o), i16 = (dv, o) => dv.getInt16(o), u32 = (dv, o) => dv.getUint32(o);

export function parseTtf(bytes) {
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const tag = u32(dv, 0);
  if (tag === 0x74746366) throw new Error('a font collection (.ttc) cannot be embedded');
  if (tag === 0x4f54544f) throw new Error('an OpenType font with CFF outlines cannot be embedded (TrueType outlines only)');
  if (tag !== 0x00010000 && tag !== 0x74727565) throw new Error('not a TrueType font');
  const count = u16(dv, 4);
  const tables = {};
  for (let i = 0; i < count; i++) {
    const o = 12 + 16 * i;
    tables[String.fromCharCode(bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3])] = { offset: u32(dv, o + 8), length: u32(dv, o + 12) };
  }
  for (const t of ['head', 'hhea', 'maxp', 'hmtx', 'loca', 'glyf', 'cmap']) if (!tables[t]) throw new Error(`the font has no ${t} table`);
  const head = tables.head.offset, hhea = tables.hhea.offset;
  const font = {
    bytes, dv, tables,
    unitsPerEm: u16(dv, head + 18),
    bbox: [i16(dv, head + 36), i16(dv, head + 38), i16(dv, head + 40), i16(dv, head + 42)],
    longLoca: i16(dv, head + 50) === 1,
    ascent: i16(dv, hhea + 4), descent: i16(dv, hhea + 6),
    numHMetrics: u16(dv, hhea + 34),
    numGlyphs: u16(dv, tables.maxp.offset + 4),
    italicAngle: 0, capHeight: 0, fsType: 0,
  };
  if (tables.post) font.italicAngle = dv.getInt32(tables.post.offset + 4) / 65536;
  if (tables['OS/2']) {
    const os2 = tables['OS/2'].offset;
    font.fsType = u16(dv, os2 + 8);
    if (tables['OS/2'].length >= 90 && u16(dv, os2) >= 2) font.capHeight = i16(dv, os2 + 88);
  }
  if (font.fsType & 0x0002) throw new Error('the font licence (OS/2 fsType) does not allow embedding');
  if (!font.capHeight) font.capHeight = Math.round(font.ascent * 0.7);
  // the cmap: the best Unicode subtable
  const cmap = tables.cmap.offset;
  const n = u16(dv, cmap + 2);
  let best = null;
  for (let i = 0; i < n; i++) {
    const platform = u16(dv, cmap + 4 + 8 * i), encoding = u16(dv, cmap + 6 + 8 * i), offset = cmap + u32(dv, cmap + 8 + 8 * i);
    const format = u16(dv, offset);
    const rank = platform === 3 && encoding === 10 && format === 12 ? 4 : platform === 0 && format === 12 ? 3 : platform === 3 && encoding === 1 && format === 4 ? 2 : platform === 0 && format === 4 ? 1 : 0;
    if (rank && (!best || rank > best.rank)) best = { rank, offset, format };
  }
  if (!best) throw new Error('the font has no Unicode cmap');
  font.cmap = best;
  return font;
}

/** Glyph id of a code point (0 = the font has no glyph for it). */
export function glyphOf(font, cp) {
  const { dv } = font, { offset, format } = font.cmap;
  if (format === 4) {
    if (cp > 0xffff) return 0;
    const segX2 = u16(dv, offset + 6), ends = offset + 14, starts = ends + segX2 + 2, deltas = starts + segX2, ranges = deltas + segX2;
    for (let i = 0; i < segX2; i += 2) {
      if (cp > u16(dv, ends + i)) continue;
      const start = u16(dv, starts + i);
      if (cp < start) return 0;
      const range = u16(dv, ranges + i), delta = i16(dv, deltas + i);
      if (range === 0) return (cp + delta) & 0xffff;
      const g = u16(dv, ranges + i + range + 2 * (cp - start));
      return g === 0 ? 0 : (g + delta) & 0xffff;
    }
    return 0;
  }
  const groups = u32(dv, offset + 12);
  for (let i = 0; i < groups; i++) {
    const o = offset + 16 + 12 * i, start = u32(dv, o), end = u32(dv, o + 4);
    if (cp >= start && cp <= end) return u32(dv, o + 8) + (cp - start);
    if (cp < start) return 0;
  }
  return 0;
}

/** Advance width of a glyph in font units. */
export function advanceOf(font, gid) {
  const index = gid < font.numHMetrics ? gid : font.numHMetrics - 1;
  return u16(font.dv, font.tables.hmtx.offset + 4 * index);
}
function lsbOf(font, gid) {
  const { dv, tables } = font;
  return gid < font.numHMetrics ? i16(dv, tables.hmtx.offset + 4 * gid + 2) : i16(dv, tables.hmtx.offset + 4 * font.numHMetrics + 2 * (gid - font.numHMetrics));
}
function glyphRange(font, gid) {
  const { dv, tables } = font, loca = tables.loca.offset;
  const a = font.longLoca ? u32(dv, loca + 4 * gid) : 2 * u16(dv, loca + 2 * gid);
  const b = font.longLoca ? u32(dv, loca + 4 * gid + 4) : 2 * u16(dv, loca + 2 * gid + 2);
  return [tables.glyf.offset + a, tables.glyf.offset + b];
}
/** The component glyph ids of a composite glyph. */
function components(font, gid) {
  const [from, to] = glyphRange(font, gid);
  if (to - from < 10 || i16(font.dv, from) >= 0) return [];
  const out = [];
  let o = from + 10, flags;
  do {
    flags = u16(font.dv, o);
    out.push(u16(font.dv, o + 2));
    o += 4 + (flags & 1 ? 4 : 2) + (flags & 8 ? 2 : flags & 0x40 ? 4 : flags & 0x80 ? 8 : 0);
  } while (flags & 0x20 && o < to);
  return out;
}

/** A font as the PDF uses it: the parsed file plus the glyphs the document used (gid -> code point). */
export class EmbeddedFont {
  constructor(bytes, name) {
    this.font = parseTtf(bytes);
    this.name = String(name || 'Embedded').replace(/[^A-Za-z0-9-]/g, '');
    this.used = new Map();
  }
  gidOf(cp) { return glyphOf(this.font, cp); }
  /** Width of the glyph in 1/1000 em. */
  width(gid) { return (advanceOf(this.font, gid) * 1000) / this.font.unitsPerEm; }
  use(gid, cp) { if (!this.used.has(gid)) this.used.set(gid, cp); }
  /** Six capital letters from the name and the glyph set (the subset tag the PDF font name starts with). */
  get tag() {
    let h = 2166136261;
    for (const ch of this.name + [...this.used.keys()].join(',')) h = Math.imul(h ^ ch.charCodeAt(0), 16777619) >>> 0;
    let out = '';
    for (let i = 0; i < 6; i++) { out += String.fromCharCode(65 + (h % 26)); h = Math.floor(h / 26) + (i + 1) * 7919; }
    return out;
  }
  /** The subset TrueType file (glyph ids unchanged; only the used glyphs and their components keep outlines). */
  subset() {
    const font = this.font;
    const keep = new Set([0, ...this.used.keys()]);
    const queue = [...keep];
    while (queue.length) for (const c of components(font, queue.pop())) if (c < font.numGlyphs && !keep.has(c)) { keep.add(c); queue.push(c); }
    const glyf = [], loca = new DataView(new ArrayBuffer(4 * (font.numGlyphs + 1)));
    let at = 0;
    for (let g = 0; g < font.numGlyphs; g++) {
      loca.setUint32(4 * g, at);
      if (!keep.has(g)) continue;
      const [a, b] = glyphRange(font, g);
      const data = font.bytes.subarray(a, b);
      glyf.push(data);
      const padded = (data.length + 3) & ~3;
      if (padded > data.length) glyf.push(new Uint8Array(padded - data.length));
      at += padded;
    }
    loca.setUint32(4 * font.numGlyphs, at);
    const hmtx = new DataView(new ArrayBuffer(4 * font.numGlyphs));
    for (let g = 0; g < font.numGlyphs; g++) { hmtx.setUint16(4 * g, advanceOf(font, g)); hmtx.setInt16(4 * g + 2, lsbOf(font, g)); }
    const copy = (t) => font.bytes.slice(font.tables[t].offset, font.tables[t].offset + font.tables[t].length);
    const head = copy('head'); new DataView(head.buffer).setUint32(8, 0); new DataView(head.buffer).setInt16(50, 1);
    const hhea = copy('hhea'); new DataView(hhea.buffer).setUint16(34, font.numGlyphs);
    const glyfBytes = new Uint8Array(at); { let o = 0; for (const d of glyf) { glyfBytes.set(d, o); o += d.length; } }
    const parts = { glyf: glyfBytes, head, hhea, hmtx: new Uint8Array(hmtx.buffer), loca: new Uint8Array(loca.buffer), maxp: copy('maxp') };
    const tags = Object.keys(parts).sort();
    const dirSize = 12 + 16 * tags.length;
    let total = dirSize;
    for (const t of tags) total += (parts[t].length + 3) & ~3;
    const out = new Uint8Array(total), dv = new DataView(out.buffer);
    dv.setUint32(0, 0x00010000); dv.setUint16(4, tags.length);
    const maxPow = 2 ** Math.floor(Math.log2(tags.length));
    dv.setUint16(6, maxPow * 16); dv.setUint16(8, Math.log2(maxPow)); dv.setUint16(10, tags.length * 16 - maxPow * 16);
    let offset = dirSize, headAt = 0;
    tags.forEach((t, i) => {
      const o = 12 + 16 * i, data = parts[t];
      for (let k = 0; k < 4; k++) out[o + k] = t.charCodeAt(k);
      let sum = 0;
      for (let k = 0; k < data.length; k += 4) sum = (sum + (((data[k] << 24) | ((data[k + 1] || 0) << 16) | ((data[k + 2] || 0) << 8) | (data[k + 3] || 0)) >>> 0)) >>> 0;
      dv.setUint32(o + 4, sum); dv.setUint32(o + 8, offset); dv.setUint32(o + 12, data.length);
      out.set(data, offset);
      if (t === 'head') headAt = offset;
      offset += (data.length + 3) & ~3;
    });
    let all = 0;
    for (let k = 0; k < out.length; k += 4) all = (all + (dv.getUint32(k))) >>> 0;
    dv.setUint32(headAt + 8, (0xb1b0afba - all) >>> 0);
    return out;
  }
}
