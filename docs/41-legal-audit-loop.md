# 41 — Legal-risk audit loop

- **Status:** Working note
- **Date:** 2026-11-22
- **Purpose:** run a repeatable audit over the public tree that removes legal-risk findings in code or documentation, so the
  questions left for a lawyer are few and clearly identified.

This is not legal advice. The loop does not decide whether something is lawful; it removes unverifiable claims and third-party
material from what the project publishes, and marks the genuinely open questions for the owner/counsel.

## How to run

The guard is deterministic and should stay green:

```sh
npm run check:provenance        # node tools/check-provenance.mjs
```

The audit itself is an agent loop. Run `/loop` with the prompt below (a 20–30 minute interval is reasonable), or run one pass by
hand. The prompt is self-contained; it assumes the agent has read `AGENTS.md`, `PROVENANCE.md`, `docs/14` and `docs/39`.

The loop covers the **public** tree, because that is what gets published. The private working tree is deliberately out of scope:
its unverified historical notes are annotated and kept for counsel, and the private audit note covers them. Do not pull private
material into a public report to "complete" the audit.

## Loop prompt

```text
You are doing one pass of a legal-risk audit of the LayoutCraft repository. The goal is to keep lawyer/counsel questions as close
to zero as possible by fixing what can be fixed in code or documentation.

SCOPE AND SAFETY
- Work only in the public tree. Never read, quote, copy or modify docs-private/, private/, test/, examples/,
  rust/layoutcraft-engine/tests/oracle/, or any build artifact that is not in the repository. Those places may contain
  third-party or unverified material.
- Read AGENTS.md, PROVENANCE.md, docs/14-v1-scope-provenance.md and docs/39-fixture-replacement.md first.
- Do not commit, push, publish, release or send anything externally. Do not claim legal clearance and do not give legal advice.
- Do not delete evidence. If a claim is unverified, annotate it (in a private file, leave it alone in this pass). If a finding
  needs private context, say only "owner/counsel decision" in the report, without private details.
- Make the smallest change that removes the risk. Add a regression guard (a check script, test or CI step) when the risk can
  come back.

EACH PASS
1. Run `node tools/check-provenance.mjs` and record the result.
2. Scan the publishable files (`git ls-files -co --exclude-standard`) and pick the highest-risk finding:
   false provenance claim > third-party asset or code > trademark/branding > missing provenance/licence > personal data >
   stale legal TODO.
3. Classify: A = fix in code/docs now; B = owner decision; C = genuine counsel question. Only implement A.
4. Implement A, then run the relevant checks and report exactly what ran:
   node tools/check-provenance.mjs; cargo test -p <crate>; node tools/gen-third-party.mjs --check; cargo deny check (if set up).
5. For B/C, write one short paragraph with the code-side alternatives you considered. Do not implement a legal conclusion.

SEARCH CATEGORIES
- Provenance claims: "clean room", "only/solely from public docs", "written from scratch", "no Microsoft code" as a claim,
  "legally cleared/approved/safe", "lawyer-approved", "copyright-safe", or any statement that sources were only one kind.
- Behaviour sources: binary analysis, Ghidra, IDA, pseudocode, disassembly, decompilation, VISLIB/VISXDATA/VISBRGR, function
  addresses, RTTI dumps, evidence IDs, "unpacked Visio", private build paths.
- Third-party material: .vsdx/.vssx/.vstx/.vsd/.vdx, vendor stencil artwork, vendor icons/previews/templates, proprietary
  fonts, Visio-exported PNGs. Check they are ignored/private and not tracked; fixtures/public is the only distributable set and
  must stay self-authored.
- Trademarks/branding: "Visio" used other than descriptively, "clone", Microsoft logos/marks, names suggesting affiliation.
- Provenance coverage: public code comments naming vendor stencils, measured tables without a "measured, not copied" note,
  source files with no evidence note, assets without an origin/licence line.
- Personal data: user names, home paths, Windows user paths, emails, tokens, keys, local absolute paths in source, docs,
  fixtures, build scripts and generated wasm.
- Licences: new dependencies not in THIRD-PARTY-NOTICES.txt, copyleft, notices drift, missing font/asset licence files.
- Release artifacts: anything embedding private fixtures/themes, user paths or the private font table. Check
  tools/build-web-wasm.sh, tools/package-portable.ps1 and .github/workflows.
- Stale legal TODOs: "lawyer", "legal review", "licence decision" that the project already answered by not distributing the
  material or by generating self-authored fixtures.

OUTPUT (in chat; never write private details into a tracked file)
| # | file:line | risk | evidence | A/B/C | code-side fix (applied or proposed) | checks |
Then: "Applied this pass: ..." and "Owner/counsel questions left: N", one line each.
Stop after one A fix, or when no A finding remains. If nothing is found, say so and stop the loop.
```