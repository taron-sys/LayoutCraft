// The fonts the vector PDF embeds. `fonts/manifest.json` (next to this file) lists open-licence TrueType files and the font families they stand in for:
//   { "fonts": [ { "name": "LiberationSans", "aliases": ["arial", "helvetica", "liberation sans"],
//                  "files": { "regular": "LiberationSans-Regular.ttf", "bold": "LiberationSans-Bold.ttf", "italic": "...", "boldItalic": "..." } } ] }
// A drawing that uses a listed family gets a subset of that file embedded; every other family (and every drawing when no font file is there) keeps the 14
// standard PDF fonts. A file that is missing, not TrueType, or whose licence bits forbid embedding is skipped without an error.
import { EmbeddedFont } from './ttf.js';

const familiesOf = (css) => String(css || '').split(',').map((f) => f.trim().replace(/^["']|["']$/g, '').toLowerCase()).filter(Boolean);

/** The font families named in some SVG texts (lower case, each CSS list split at the commas). */
export function familiesInSvg(texts) {
  const out = new Set();
  for (const text of texts) for (const m of text.matchAll(/font-family="([^"]*)"/g)) for (const f of familiesOf(m[1].replace(/&quot;/g, '"'))) out.add(f);
  return out;
}

/**
 * Fetch the manifest and the files of the listed fonts that `families` asks for. Returns `{ pick(cssFamilyList, bold, italic) -> EmbeddedFont | null, count }`
 * or null when there is nothing to embed. `fetchBytes(url)` and `fetchJson(url)` can be given for tests (default: fetch next to this module).
 */
export async function loadPdfFonts(families, { base = new URL('./fonts/', import.meta.url).href, fetchJson = null, fetchBytes = null } = {}) {
  const getJson = fetchJson || (async (url) => { const r = await fetch(url); if (!r.ok) throw new Error(String(r.status)); return r.json(); });
  const getBytes = fetchBytes || (async (url) => { const r = await fetch(url); if (!r.ok) throw new Error(String(r.status)); return new Uint8Array(await r.arrayBuffer()); });
  let manifest;
  try { manifest = await getJson(base + 'manifest.json'); } catch { return null; }
  const entries = [];
  for (const entry of manifest?.fonts || []) {
    const aliases = (entry.aliases || []).map((a) => String(a).toLowerCase());
    if (!aliases.some((a) => families.has(a))) continue;
    const variants = {};
    for (const [style, file] of Object.entries(entry.files || {})) {
      try { variants[style] = new EmbeddedFont(await getBytes(base + file), `${entry.name}-${style}`); } catch { /* skipped: missing, not TrueType, or not allowed to embed */ }
    }
    if (Object.keys(variants).length) entries.push({ aliases, variants });
  }
  if (!entries.length) return null;
  return {
    count: entries.length,
    fonts: () => entries.flatMap((e) => Object.values(e.variants)),
    pick(css, bold, italic) {
      for (const family of familiesOf(css)) {
        const entry = entries.find((e) => e.aliases.includes(family));
        if (!entry) continue;
        const v = entry.variants;
        return (bold && italic ? v.boldItalic : null) || (bold ? v.bold : null) || (italic ? v.italic : null) || v.regular || Object.values(v)[0];
      }
      return null;
    },
  };
}
