// Third-party notices of the native programs (docs/36-packaging.md): the Rust crates that end up in layoutcraft-app.exe and layoutcraft-mcp.exe
// (normal dependencies only, for the Windows target), their licences, and the licence texts found in the crate sources.
//   node tools/gen-third-party.mjs          writes THIRD-PARTY-NOTICES.txt and docs/36-licences.json
//   node tools/gen-third-party.mjs --check  fails when the files are out of date
import { execFileSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

const ROOT = path.resolve(import.meta.dirname, '..');
const RUST = path.join(ROOT, 'rust');
const TARGET = process.env.LC_TARGET || 'x86_64-pc-windows-gnu';
// the maintainers' Windows machine pins a toolchain; elsewhere set CARGO_TOOLCHAIN= (empty) to use the default one
const TC = process.env.CARGO_TOOLCHAIN ?? '+stable-x86_64-pc-windows-gnu';
const meta = JSON.parse(execFileSync('cargo', [...(TC ? [TC] : []), 'metadata', '--format-version', '1', '--offline', '--filter-platform', TARGET], { cwd: RUST, maxBuffer: 1 << 28 }).toString());
const byId = new Map(meta.packages.map((p) => [p.id, p]));
const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
const roots = ['layoutcraft-app', 'layoutcraft-mcp'].map((n) => meta.packages.find((p) => p.name === n).id);

// closure over normal dependencies (kind null) that apply to the target
const seen = new Set();
const todo = [...roots];
while (todo.length) {
  const id = todo.pop();
  if (seen.has(id)) continue;
  seen.add(id);
  for (const d of nodes.get(id).deps) if (d.dep_kinds.some((k) => k.kind === null)) todo.push(d.pkg);
}
const own = new Set(meta.packages.filter((p) => p.name.startsWith('layoutcraft-')).map((p) => p.id));
const crates = [...seen].filter((id) => !own.has(id)).map((id) => byId.get(id)).sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));

const PERMISSIVE = /^(MIT|Apache-2\.0|BSD-[23]-Clause|ISC|Zlib|Unlicense|0BSD|CC0-1\.0|Unicode-3\.0|Unicode-DFS-2016|BSL-1\.0|MIT-0|OFL-1\.1|LicenseRef-UFL-1\.0|Ubuntu-font-1\.0|NCSA)$/;
const words = (expr) => (expr || '').replace(/[()]/g, ' ').split(/\s+(?:OR|AND|WITH|\/)\s+|\s+/).filter((w) => w && w !== 'OR' && w !== 'AND' && w !== 'WITH');
const rows = crates.map((c) => {
  const lic = c.license || (c.license_file ? `see ${c.license_file}` : 'UNKNOWN');
  // an OR expression is fine when one alternative is permissive
  const alternatives = (c.license || '').split(/\s+OR\s+|\//).map((a) => a.replace(/[()]/g, '').trim());
  const ok = alternatives.some((alt) => words(alt).every((w) => PERMISSIVE.test(w)));
  return { name: c.name, version: c.version, license: lic, repository: c.repository || '', ok, dir: path.dirname(c.manifest_path) };
});
const flagged = rows.filter((r) => !r.ok);

const texts = new Map(); // hash -> {text, users}
for (const r of rows) {
  let files = [];
  try { files = fs.readdirSync(r.dir).filter((f) => /^(licen[cs]e|copying|unlicense|notice)/i.test(f) && fs.statSync(path.join(r.dir, f)).isFile()); } catch { /* source not unpacked */ }
  // the font crate keeps its licence texts next to the fonts
  const fontDir = path.join(r.dir, 'fonts');
  if (fs.existsSync(fontDir)) files.push(...fs.readdirSync(fontDir).filter((f) => /\.txt$/i.test(f)).map((f) => `fonts/${f}`));
  r.texts = files;
  r.entries = files.map((f) => ({ dir: r.dir, f, borrowed: false }));
}
// crates whose licence files are not in the packaged source (the egui family keeps them at the workspace root):
// the standard text of the same licence, taken from another crate that uses the same licence expression
for (const r of rows) {
  if (r.entries.length) continue;
  const donor = rows.find((d) => d.license === r.license && d.entries.some((e) => !e.f.startsWith('fonts/')));
  if (donor) r.entries = donor.entries.filter((e) => !e.f.startsWith('fonts/')).map((e) => ({ ...e, borrowed: true }));
}
for (const r of rows) {
  for (const { dir, f, borrowed } of r.entries) {
    const text = fs.readFileSync(path.join(dir, f), 'utf8').replace(/\r\n/g, '\n').trim();
    const h = crypto.createHash('sha256').update(text).digest('hex').slice(0, 12);
    if (!texts.has(h)) texts.set(h, { text, users: [] });
    texts.get(h).users.push(`${r.name} ${r.version} (${borrowed ? 'standard text of the licence' : f})`);
  }
}

let out = 'LayoutCraft - third-party notices\n';
out += `The programs layoutcraft-app and layoutcraft-mcp contain the following Rust crates (target ${TARGET}, normal dependencies).\n`;
out += 'Each crate is used under the licence shown (for "A OR B" one of the two). The licence texts follow the list.\n\n';
out += `${rows.length} crates\n\n`;
for (const r of rows) out += `${r.name} ${r.version}  -  ${r.license}${r.repository ? `  -  ${r.repository}` : ''}\n`;
out += '\n\n==== Licence texts ====\n';
for (const [h, t] of texts) out += `\n---- ${h}: used by ${t.users.slice(0, 6).join(', ')}${t.users.length > 6 ? ` and ${t.users.length - 6} more` : ''}\n\n${t.text}\n`;
out += '\n';
const summary = { generated: 'by visio-test-harness/gen-third-party.mjs', target: TARGET, crates: rows.map(({ name, version, license, ok }) => ({ name, version, license, ok })), flagged: flagged.map((r) => `${r.name} ${r.version} ${r.license}`), withoutText: rows.filter((r) => !r.entries.length).map((r) => `${r.name} ${r.version}`) };

const noticesPath = path.join(ROOT, 'THIRD-PARTY-NOTICES.txt');
const jsonPath = path.join(ROOT, 'docs/36-licences.json');
const jsonText = `${JSON.stringify(summary, null, 1)}\n`;
if (process.argv.includes('--check')) {
  const same = fs.existsSync(noticesPath) && fs.readFileSync(noticesPath, 'utf8') === out && fs.readFileSync(jsonPath, 'utf8') === jsonText;
  console.log(same ? 'third-party notices are current' : 'third-party notices are STALE: run node tools/gen-third-party.mjs');
  process.exit(same ? 0 : 1);
}
fs.writeFileSync(noticesPath, out);
fs.writeFileSync(jsonPath, jsonText);
console.log(`${rows.length} crates, ${texts.size} distinct licence texts, ${flagged.length} flagged, ${summary.withoutText.length} without a licence file`);
for (const f of summary.flagged) console.log('FLAGGED', f);
const byLicense = {};
for (const r of rows) byLicense[r.license] = (byLicense[r.license] || 0) + 1;
console.log(Object.entries(byLicense).sort((a, b) => b[1] - a[1]).map(([k, v]) => `${v} x ${k}`).join('\n'));
