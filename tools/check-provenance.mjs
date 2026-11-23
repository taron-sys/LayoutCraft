#!/usr/bin/env node
// Provenance guard for the public tree (docs/41-legal-audit-loop.md, PROVENANCE.md).
//
// Fails when a file that may be published contains an unverifiable provenance claim, a private path or a reference to private
// notes. It is deliberately a checklist of phrases, not a legal analysis: its job is to keep the public wording honest and to
// keep private material out of the public tree. Run: node tools/check-provenance.mjs
import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(fileURLToPath(new URL('..', import.meta.url)))

const skipDirs = new Set([
  '.git', 'node_modules', 'target', 'dist', 'docs-private', 'private', 'test', 'examples',
])
const skipFiles = new Set([
  '.gitignore',
  'THIRD-PARTY-NOTICES.txt',
  'rust/Cargo.lock',
  'package-lock.json',
  'tools/check-provenance.mjs',
  'docs/41-legal-audit-loop.md',
])
const binaryExtensions = new Set([
  '.7z', '.avif', '.bin', '.bmp', '.emf', '.gif', '.gz', '.ico', '.jpg', '.jpeg', '.pdf', '.png',
  '.tar', '.ttf', '.wasm', '.webp', '.woff', '.woff2', '.zip', '.vsd', '.vsdx', '.vssx', '.vstx', '.vdx',
])

const rules = [
  {
    name: 'unverified clean-room claim',
    pattern: /\bclean[-\s]?room\b/i,
    // a rule that forbids the claim is fine
    allow: (line) => /\b(not|no|never|do not|don't|must not)\b/i.test(line),
  },
  {
    name: 'binary-analysis terminology in a public file',
    pattern: /\b(Ghidra|IDA Pro|pseudocode|disassembl\w*|decompil\w*)\b/i,
    allow: (line) => /\b(not|no|never|do not|don't|must not)\b/i.test(line),
  },
  {
    name: 'binary-analysis terminology in a public file',
    pattern: /\bREA\b/,
    allow: (line) => /\b(not|no|never|do not|don't|must not)\b/i.test(line),
  },
  {
    name: 'Microsoft binary or class name in a public file',
    pattern: /\b(VISLIB|VISXDATA|VISBRGR|CDataExplorer|IVDataRecordset)\b/,
  },
  {
    name: 'claim of legal clearance',
    pattern: /\b((is|are|has been|have been|fully)\s+legally?\s+(cleared|approved|safe)|no\s+legal\s+risk|lawyer[-\s]?approved|copyright[-\s]?safe)\b/i,
  },
  {
    name: 'exclusive public-source claim',
    pattern: /\b(only|solely)\b[^.\n]{0,50}\bpublic\s+(documentation|docs|sources)\b/i,
    allow: (line) => /\b(not|no|never|do not|don't|must not)\b/i.test(line),
  },
  {
    name: 'exclusive-source claim',
    pattern: /\b(written|developed)\s+(only|solely)\s+from\b/i,
    allow: (line) => /\b(not|no|never|do not|don't|must not)\b/i.test(line),
  },
  {
    name: 'private working path',
    pattern: /(docs-private\b|C:\\Users\\|\/home\/[A-Za-z0-9._-]+\/|\barchlinuxuser\b)/i,
    // examples such as /home/me/... are placeholders in documentation and tests, not leaked paths
    allow: (line) => /\/(home|users)\/(me|user|username|example|you)\b/i.test(line),
  },
  {
    name: 'reference to a private note',
    pattern: /docs\/(08|10|11|16|23|24|25|26|27|28|29|30|31|33|37|40)-/,
  },
]

function fail(message) {
  console.error(`FAIL ${message}`)
  process.exitCode = 1
}

function publishableFiles() {
  try {
    const output = execFileSync('git', ['ls-files', '-co', '--exclude-standard', '-z'], {
      cwd: root,
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    })
    return output.split('\0').filter(Boolean)
  } catch {
    // no git (a source archive): walk the tree, honouring the same directory skip list
    const out = []
    const walk = (dir) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const rel = path.relative(root, path.join(dir, entry.name)).replaceAll('\\', '/')
        if (entry.isDirectory()) {
          if (!skipDirs.has(entry.name)) walk(path.join(dir, entry.name))
        } else {
          out.push(rel)
        }
      }
    }
    walk(root)
    return out
  }
}

function isTextFile(relative, absolute) {
  if (skipFiles.has(relative)) return false
  if (binaryExtensions.has(path.extname(relative).toLowerCase())) return false
  const stat = statSync(absolute)
  if (!stat.isFile() || stat.size > 1024 * 1024) return false
  const head = readFileSync(absolute).subarray(0, 8192)
  return !head.includes(0)
}

const files = publishableFiles()
  .filter((relative) => !relative.split('/').some((part) => skipDirs.has(part)))
  .filter((relative) => isTextFile(relative, path.join(root, relative)))

for (const relative of files) {
  const text = readFileSync(path.join(root, relative), 'utf8')
  const lines = text.split(/\r?\n/)
  lines.forEach((line, index) => {
    for (const rule of rules) {
      if (!rule.pattern.test(line)) continue
      if (rule.allow?.(line)) continue
      const excerpt = line.trim().replace(/\s+/g, ' ').slice(0, 180)
      fail(`${relative}:${index + 1}: ${rule.name}: ${excerpt}`)
    }
  })
}

// The public tree must not carry third-party drawings or stencils. Self-authored fixtures are the only .vsdx allowed.
const forbiddenFixture = (relative) =>
  /^(test|examples)\//.test(relative) ||
  (/\.(vsdx|vssx|vstx|vsd|vdx)$/i.test(relative) && !relative.startsWith('fixtures/public/sources/'))
for (const relative of files) {
  if (forbiddenFixture(relative)) fail(`${relative}: test/stencil package in the publishable tree`)
}
const privatePrefixes = ['docs-private/', 'private/', 'test/', 'examples/', 'rust/layoutcraft-engine/tests/oracle/']
try {
  const tracked = execFileSync('git', ['ls-files', '-z'], { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 })
  for (const relative of tracked.split('\0').filter(Boolean)) {
    if (forbiddenFixture(relative)) fail(`${relative}: test/stencil package is tracked`)
    if (privatePrefixes.some((prefix) => relative.startsWith(prefix))) fail(`${relative}: private path is tracked`)
  }
} catch {
  // no git: the working-tree check above is all we can do
}

// Required provenance material.
for (const required of ['PROVENANCE.md', 'web/fonts/LICENSE-Liberation.txt', 'web/fonts/LICENSE-Carlito-OFL.txt', 'web/fonts/LICENSE-Caladea-OFL.txt']) {
  if (!existsSync(path.join(root, required))) fail(`${required}: required provenance file is missing`)
}
const iconReadme = path.join(root, 'web/assets/icons/README.md')
if (!existsSync(iconReadme) || !/Provenance:/i.test(readFileSync(iconReadme, 'utf8'))) {
  fail('web/assets/icons/README.md: missing the icon provenance line')
}

if (process.exitCode) {
  console.error(`\nProvenance guard failed. See PROVENANCE.md and docs/41-legal-audit-loop.md.`)
} else {
  console.log(`Provenance guard: ${files.length} public files checked, no findings.`)
}
