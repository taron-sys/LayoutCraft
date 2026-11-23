// Save as web page with live text and links (FILE-18, step 306). The pages' SVG are put INLINE into one self-contained HTML file (so the text can be selected, searched and read by
// a screen reader, not a picture), and the hyperlinks of the shapes become real <a> elements around the shapes. Pure functions: the shell collects the pages, this builds the file.
// Safety: only http(s), mailto and tel links and relative addresses are written as links; anything else (javascript:, data:, file:, ...) is dropped. A link to another page of the drawing
// (a sub-address that is a page name) switches to that page's tab.

const esc = (t) => String(t).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

/** The href to write for an address, or null when it is not safe to link. */
export function safeHref(address) {
  const a = String(address || '').trim();
  if (!a) return null;
  if (/^(https?:|mailto:|tel:)/i.test(a)) return a;
  if (/^[a-z][a-z0-9+.-]*:/i.test(a)) return null; // any other scheme
  if (/^\s*(\/\/|\\\\)/.test(a)) return null; // protocol-relative / UNC
  return a;
}

/** Make the ids of one inline SVG unique to its page (several SVGs in one HTML document share one id space). */
export function prefixIds(svg, prefix) {
  const ids = [...svg.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1]);
  let out = svg;
  for (const id of new Set(ids)) {
    const re = id.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    out = out.replace(new RegExp(`(\\sid=")${re}(")`, 'g'), `$1${prefix}${id}$2`)
      .replace(new RegExp(`url\\(#${re}\\)`, 'g'), `url(#${prefix}${id})`)
      .replace(new RegExp(`(href=")#${re}(")`, 'g'), `$1#${prefix}${id}$2`);
  }
  return out;
}

/** Wrap the `<g data-vid="vid" ...>` element of an SVG text in `open` (an `<a ...>` tag), an optional `<title>` and `</a>`; the text unchanged when the shape is not found. */
export function wrapShape(svg, vid, open, titleXml = '') {
  const start = svg.indexOf(`<g data-vid="${vid}"`);
  if (start < 0) return svg;
  const re = /<g(?=[\s/>])[^>]*?(\/?)>|<\/g>/g;
  re.lastIndex = start;
  let depth = 0, m;
  while ((m = re.exec(svg))) {
    if (m[0] === '</g>') depth--;
    else if (m[1] !== '/') depth++; // a self-closing group changes nothing
    if (depth === 0) {
      const end = m.index + m[0].length;
      return `${svg.slice(0, start)}${open}${titleXml}${svg.slice(start, end)}</a>${svg.slice(end)}`;
    }
  }
  return svg;
}

/**
 * The HTML file. `pages`: [{name, svg, width, height, links: {vid: [{address, subAddress, description, newWindow}]}}]. Returns {html, links, dropped}.
 */
export function buildWebPage(pages, { title = 'Drawing' } = {}) {
  let links = 0, dropped = 0;
  const nameToIndex = new Map(pages.map((p, i) => [String(p.name).toLowerCase(), i]));
  const sections = pages.map((p, i) => {
    let svg = prefixIds(String(p.svg).replace(/^<\?xml[^>]*\?>\s*/, ''), `p${i}-`);
    for (const [vid, list] of Object.entries(p.links || {})) {
      const link = (list || []).find((l) => !l.invisible) || list?.[0];
      if (!link) continue;
      let open = null;
      const target = link.subAddress && nameToIndex.has(String(link.subAddress).toLowerCase()) && !link.address ? nameToIndex.get(String(link.subAddress).toLowerCase()) : null;
      if (target !== null) open = `<a href="#page:${target}" data-page="${target}">`;
      else {
        const href = safeHref(link.address);
        if (href) open = `<a href="${esc(href)}"${link.newWindow ? ' target="_blank"' : ''} rel="noopener noreferrer">`;
      }
      if (!open) { dropped++; continue; }
      const wrapped = wrapShape(svg, vid, open, link.description ? `<title>${esc(link.description)}</title>` : '');
      if (wrapped === svg) continue;
      svg = wrapped;
      links++;
    }
    return `<section data-i="${i}"${i ? ' hidden' : ''} aria-label="${esc(p.name)}">${svg}</section>`;
  });
  const html = `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${esc(title)}</title>
<style>body{margin:0;font:14px system-ui,sans-serif;background:#f3f3f3}nav{display:flex;gap:2px;padding:6px 8px 0;background:#e6e6e6;flex-wrap:wrap}nav button{border:1px solid #bbb;border-bottom:0;background:#ddd;padding:5px 12px;cursor:pointer}nav button[aria-selected=true]{background:#fff;font-weight:600}main{padding:12px;overflow:auto}section svg{max-width:100%;height:auto;background:#fff;box-shadow:0 1px 4px #0003}svg a{cursor:pointer}svg a:hover{opacity:.85}svg text{user-select:text}</style></head><body>
<nav role="tablist">${pages.map((p, i) => `<button role="tab" aria-selected="${i === 0}" data-i="${i}">${esc(p.name)}</button>`).join('')}</nav>
<main>${sections.join('')}</main>
<script>(function(){function show(i){document.querySelectorAll('nav button').forEach(function(x){x.setAttribute('aria-selected',String(x.getAttribute('data-i')===String(i)))});document.querySelectorAll('section').forEach(function(s){s.hidden=s.getAttribute('data-i')!==String(i)})}document.querySelector('nav').addEventListener('click',function(e){var b=e.target.closest('button');if(b)show(b.getAttribute('data-i'))});document.querySelector('main').addEventListener('click',function(e){var a=e.target.closest('a[data-page]');if(a){e.preventDefault();show(a.getAttribute('data-page'))}})})();</script>
</body></html>
`;
  return { html, links, dropped };
}
