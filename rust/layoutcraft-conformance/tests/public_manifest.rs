//! The redistributable self-authored fixture set must pass the same runner as the private oracle.
use std::process::Command;

#[test]
fn the_public_self_authored_manifest_passes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO_BIN_EXE_layoutcraft-conformance"))
        .arg("--root")
        .arg(&root)
        .arg("--manifest")
        .arg(root.join("fixtures/public/manifest.json"))
        .output()
        .expect("run layoutcraft-conformance");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "public fixtures failed the conformance runner\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("checks passed"),
        "no conformance summary in the output:\n{stdout}"
    );
    assert!(
        !stdout.contains("failed"),
        "the conformance runner reported a failure:\n{stdout}"
    );
}
