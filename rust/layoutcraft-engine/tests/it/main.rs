//! Hostile input: a file the engine opens may be anything. These tests feed it damaged, truncated, mutated and
//! hand-built hostile files and require that it never panics, never hangs and answers with an error or a partial result.
//! `LAYOUTCRAFT_HOSTILE_SCALE=20 cargo test -p layoutcraft-engine --test it` runs a larger sweep (`hostile.rs` lists the knobs).

// The module is shared with the vsdx test target by path, and each target uses a part of it.
#[allow(dead_code)]
#[path = "../../../layoutcraft-vsdx/tests/it/hostile.rs"]
mod hostile;
mod limits;
mod pipeline;
mod seeds;
mod sweep;
