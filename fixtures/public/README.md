# Public golden fixtures (self-authored)

This directory is the redistributable fixture set. Every `.vsdx` here was written by LayoutCraft's own engine from the drawing
operations in `rust/layoutcraft-fixtures`; the golden models are what our importer reads back, and the `untouched` hashes are a
no-op patch save. No Visio-authored file, stencil artwork, Microsoft asset or Visio-rendered image is part of this set.

- `manifest.json` — source hashes, canonical model hashes, counts and untouched-patch entry hashes (`schema/golden-manifest-v1.schema.json`).
- `sources/*.vsdx` — the generated packages.
- `models/*.json` — the canonical model our importer produces for each source, written with sorted keys so the committed
  text is the same whether or not another crate in the workspace enables `serde_json`'s `preserve_order` feature.
- `provenance.json` — per-fixture origin, status and redistribution (`schema/fixture-provenance-v1.schema.json`); all entries are
  `self-authored`, `cleared` and redistributable under MIT OR Apache-2.0. See `docs/39-fixture-replacement.md`.

Regenerate (deterministic — the test fails if the committed files and the generator disagree):

```sh
cd rust
cargo run -p layoutcraft-fixtures --bin make-public-fixtures
cargo run -p layoutcraft-fixtures --bin make-public-fixtures -- --check
```

Check with the browser-free conformance runner:

```sh
cd rust
cargo run -p layoutcraft-conformance -- --root .. --manifest ../fixtures/public/manifest.json
```

The Visio-authored corpus and its Visio-exported PNGs remain in the private validation location (`docs/38-private-fixtures.md`)
and are never distributed.
