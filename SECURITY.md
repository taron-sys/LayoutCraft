# Security policy

## Reporting a vulnerability

Please report a vulnerability **privately**: use GitHub's "Report a vulnerability" button on the Security tab of this repository (private vulnerability reporting).
Do not open a public issue for it. Include the affected version or commit, what you did, and a sample file if one is needed (a file that triggers the problem is the most useful report).

You can expect an acknowledgement within a few working days. This is a volunteer project: there is no bounty and no guaranteed fix time, but confirmed problems are fixed in the
latest release and credited if you wish.

## Supported versions

Only the latest release and the current `main` branch.

## What the program reads

LayoutCraft opens files you give it (`.vsdx`, `.vstx`, `.vssx` packages, draw.io and Excalidraw files, its own JSON). Treat a document from an untrusted source like any other
downloaded file. The reader is written to refuse hostile packages rather than trust them:

- ZIP entries are limited to 256 MiB each and 512 MiB in total (`MAX_ENTRY_BYTES`, `MAX_TOTAL_BYTES` in `rust/layoutcraft-vsdx/src/lib.rs`); shape nesting is limited to a depth of 200 (`MAX_SHAPE_DEPTH`).
- The browser build runs the Rust core as WebAssembly; a test harness (25 hostile-file cases: bad ZIPs, size bombs, deep nesting, fuzzed input) runs before releases.
- The MCP servers (`mcp/`, `rust/layoutcraft-mcp`) refuse file access outside the folders they were started with.

These limits are a first line of defence, not a promise that every malformed file is harmless. Reports that show a crash, a hang, unbounded memory use, or access outside the allowed folders are in scope.

## Dependencies

Dependency licences, sources and security advisories are checked with `cargo deny` (`rust/deny.toml`). One advisory is currently accepted with a written reason (an unmaintained font-parsing crate, RUSTSEC-2026-0192).
