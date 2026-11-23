// Dumps the web ribbon's tab / group / command table (LayoutCraft/web/ribbon.js: the TABS and ICONS constants) to
// LayoutCraft/rust/layoutcraft-app/src/ribbon-web.json, the data the native app's ribbon is built from (docs/34, step N3.1).
// Run it again after the web ribbon changes; `node dump-ribbon.mjs --check` fails when the file is out of date.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const source = fs.readFileSync(path.join(here, '..', 'web', 'ribbon.js'), 'utf8');
const grab = (name, open, close) => {
  const m = new RegExp(`const ${name} = (\\${open}[\\s\\S]*?\\n\\${close});`).exec(source);
  if (!m) throw new Error(`const ${name} not found in ribbon.js`);
  return new Function(`return (${m[1]});`)();
};
const TABS = grab('TABS', '[', ']');
const ICONS = grab('ICONS', '{', '}');
const out = {
  // an entry of a group is a command id, or [label, [ids]] (a drop-down menu holding those commands); a group of drop-downs only has such entries
  tabs: TABS.map(([name, groups, note]) => ({
    name,
    note: note ?? null,
    groups: groups.map(([gname, items]) => ({
      name: gname,
      items: items.map((item) => (Array.isArray(item) ? { menu: item[0], ids: item[1] } : { id: item })),
    })),
  })),
  icons: ICONS,
};
const target = path.join(here, '..', 'rust', 'layoutcraft-app', 'src', 'ribbon-web.json');
const text = JSON.stringify(out, null, 1) + '\n';
if (process.argv.includes('--check')) {
  const have = fs.existsSync(target) ? fs.readFileSync(target, 'utf8') : '';
  if (have !== text) {
    console.error('ribbon-web.json is out of date: run node tools/dump-ribbon.mjs');
    process.exit(1);
  }
  console.log('ribbon-web.json is up to date');
} else {
  fs.writeFileSync(target, text);
  console.log(`wrote ${target}: ${out.tabs.length} tabs, ${out.tabs.reduce((n, t) => n + t.groups.length, 0)} groups, ${Object.keys(ICONS).length} icons`);
}
