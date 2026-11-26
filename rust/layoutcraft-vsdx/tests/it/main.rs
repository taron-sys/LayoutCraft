//! Hostile input for the package reader and the converters of `layoutcraft-vsdx`: one directed test per crash or limit.
//! The sweeps over mutated files run through the engine (`layoutcraft-engine/tests/it`), where the whole path is.

// The module is shared with the engine test target by path, and each target uses a part of it.
#[allow(dead_code)]
mod hostile;
mod legacy;
mod regress;
mod stencil_styles;
