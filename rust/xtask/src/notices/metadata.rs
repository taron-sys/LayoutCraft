//! `cargo metadata` of the workspace and the closure of crates that end up in the two programs.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::js_compat::locale_compare;

/// The programs whose crates are listed.
pub const PROGRAMS: [&str; 2] = ["layoutcraft-app", "layoutcraft-mcp"];
/// Crates of this workspace are not third-party.
const OWN_PREFIX: &str = "layoutcraft-";

#[derive(Deserialize)]
pub struct Metadata {
    pub packages: Vec<Package>,
    resolve: Option<Resolve>,
}

#[derive(Deserialize, Clone)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub version: String,
    pub license: Option<String>,
    pub license_file: Option<String>,
    pub repository: Option<String>,
    pub manifest_path: String,
}

#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    id: String,
    #[serde(default)]
    deps: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    pkg: String,
    #[serde(default)]
    dep_kinds: Vec<DependencyKind>,
}

/// `kind` is null for a normal dependency, "dev" or "build" otherwise.
#[derive(Deserialize)]
struct DependencyKind {
    kind: Option<String>,
}

/// Runs `cargo [+toolchain] metadata` in `rust_dir`, offline, for one target. An empty `toolchain` passes no `+toolchain` argument.
pub fn load(rust_dir: &Path, toolchain: &str, target: &str) -> Result<Metadata, String> {
    let mut command = Command::new("cargo");
    if !toolchain.is_empty() {
        command.arg(toolchain);
    }
    command.args(["metadata", "--format-version", "1", "--offline", "--filter-platform", target]).current_dir(rust_dir);
    let output = command.output().map_err(|e| format!("cannot run cargo: {e}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed ({}):\n{}", output.status, String::from_utf8_lossy(&output.stderr)));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata: unexpected output: {e}"))
}

impl Metadata {
    /// The third-party crates that the `programs` contain: the closure over normal dependencies (those that apply to the target the
    /// metadata was loaded for), without the crates of this workspace, in name and version order.
    pub fn third_party_crates(&self, programs: &[&str]) -> Result<Vec<Package>, String> {
        let by_id: HashMap<&str, &Package> = self.packages.iter().map(|p| (p.id.as_str(), p)).collect();
        let resolve = self.resolve.as_ref().ok_or("cargo metadata has no dependency graph")?;
        let nodes: HashMap<&str, &Node> = resolve.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let mut todo = Vec::new();
        for program in programs {
            let package = self.packages.iter().find(|p| p.name == *program).ok_or_else(|| format!("package {program} not found"))?;
            todo.push(package.id.as_str());
        }
        // a depth-first walk that remembers the order of discovery, which decides between two crates of the same name and version
        let mut seen_order = Vec::new();
        let mut seen = HashSet::new();
        while let Some(id) = todo.pop() {
            if !seen.insert(id) {
                continue;
            }
            seen_order.push(id);
            let node = nodes.get(id).ok_or_else(|| format!("{id}: not in the dependency graph"))?;
            todo.extend(node.deps.iter().filter(|d| d.dep_kinds.iter().any(|k| k.kind.is_none())).map(|d| d.pkg.as_str()));
        }
        let mut crates = Vec::new();
        for id in seen_order {
            let package = by_id.get(id).ok_or_else(|| format!("{id}: not in the package list"))?;
            if !package.name.starts_with(OWN_PREFIX) {
                crates.push((*package).clone());
            }
        }
        crates.sort_by(|a, b| locale_compare(&a.name, &b.name).then_with(|| locale_compare(&a.version, &b.version)));
        Ok(crates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A graph: app -> a (normal), app -> dev-only (dev), a -> b (build and normal), a -> windows-sys, mcp -> a, own crate in between.
    const GRAPH: &str = r#"{
        "packages": [
            {"id": "app", "name": "layoutcraft-app", "version": "0.1.0", "license": "MIT", "license_file": null, "repository": null, "manifest_path": "/x/app/Cargo.toml"},
            {"id": "mcp", "name": "layoutcraft-mcp", "version": "0.1.0", "license": "MIT", "license_file": null, "repository": null, "manifest_path": "/x/mcp/Cargo.toml"},
            {"id": "eng", "name": "layoutcraft-engine", "version": "0.1.0", "license": "MIT", "license_file": null, "repository": null, "manifest_path": "/x/eng/Cargo.toml"},
            {"id": "a", "name": "a", "version": "1.0.0", "license": "MIT", "license_file": null, "repository": "https://example.invalid/a", "manifest_path": "/c/a/Cargo.toml"},
            {"id": "b", "name": "b", "version": "2.0.0", "license": null, "license_file": "LICENSE.txt", "repository": null, "manifest_path": "/c/b/Cargo.toml"},
            {"id": "dev", "name": "dev-only", "version": "1.0.0", "license": "MIT", "license_file": null, "repository": null, "manifest_path": "/c/dev/Cargo.toml"},
            {"id": "build", "name": "build-only", "version": "1.0.0", "license": "MIT", "license_file": null, "repository": null, "manifest_path": "/c/build/Cargo.toml"},
            {"id": "ws", "name": "windows-sys", "version": "0.1.0", "license": "MIT", "license_file": null, "repository": null, "manifest_path": "/c/ws/Cargo.toml"},
            {"id": "w6", "name": "windows_x86_64_gnu", "version": "0.1.0", "license": "MIT", "license_file": null, "repository": null, "manifest_path": "/c/w6/Cargo.toml"}
        ],
        "resolve": {"nodes": [
            {"id": "app", "deps": [
                {"pkg": "eng", "dep_kinds": [{"kind": null, "target": null}]},
                {"pkg": "dev", "dep_kinds": [{"kind": "dev", "target": null}]},
                {"pkg": "build", "dep_kinds": [{"kind": "build", "target": null}]}]},
            {"id": "mcp", "deps": [{"pkg": "a", "dep_kinds": [{"kind": null, "target": null}]}]},
            {"id": "eng", "deps": [{"pkg": "a", "dep_kinds": [{"kind": null, "target": null}]}]},
            {"id": "a", "deps": [
                {"pkg": "b", "dep_kinds": [{"kind": "build", "target": null}, {"kind": null, "target": null}]},
                {"pkg": "ws", "dep_kinds": [{"kind": null, "target": null}]},
                {"pkg": "w6", "dep_kinds": [{"kind": null, "target": null}]}]},
            {"id": "b", "deps": []}, {"id": "dev", "deps": []}, {"id": "build", "deps": []}, {"id": "ws", "deps": []}, {"id": "w6", "deps": []}
        ]}
    }"#;

    fn names(metadata: &Metadata, programs: &[&str]) -> Vec<String> {
        metadata.third_party_crates(programs).unwrap().iter().map(|p| format!("{} {}", p.name, p.version)).collect()
    }

    #[test]
    fn the_closure_follows_normal_dependencies_only_skips_own_crates_and_sorts_by_collation() {
        let metadata: Metadata = serde_json::from_str(GRAPH).unwrap();
        // `_` sorts before `-`, as `localeCompare` did: windows_x86_64_gnu comes first
        assert_eq!(names(&metadata, &PROGRAMS), ["a 1.0.0", "b 2.0.0", "windows_x86_64_gnu 0.1.0", "windows-sys 0.1.0"]);
    }

    #[test]
    fn a_dependency_with_a_build_and_a_normal_kind_counts_and_one_with_only_dev_or_build_does_not() {
        let metadata: Metadata = serde_json::from_str(GRAPH).unwrap();
        let all = names(&metadata, &PROGRAMS);
        assert!(all.contains(&"b 2.0.0".to_string()));
        assert!(!all.iter().any(|n| n.starts_with("dev-only") || n.starts_with("build-only")));
    }

    #[test]
    fn an_unknown_program_or_a_missing_graph_is_an_error() {
        let metadata: Metadata = serde_json::from_str(GRAPH).unwrap();
        assert!(metadata.third_party_crates(&["nope"]).is_err());
        let without: Metadata = serde_json::from_str(r#"{"packages": []}"#).unwrap();
        assert!(without.third_party_crates(&[]).is_err());
    }
}
