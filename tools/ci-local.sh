#!/usr/bin/env bash
# The checks of .github/workflows/ci.yml, run on this machine (Linux and macOS; on Windows use Git Bash or WSL).
#   tools/ci-local.sh                 all checks on the committed state of HEAD
#   tools/ci-local.sh --quick         only the fast checks: provenance, notices, wasm build and load, cargo-deny (no tests)
#   tools/ci-local.sh --release       also the release build of the two programs (CI does this on main only)
#   tools/ci-local.sh --ref <rev>     check another commit or branch instead of HEAD
#   tools/ci-local.sh --keep          keep the temporary checkout (its path is printed)
#
# The checks run in a clean checkout of the commit made with `git worktree`, so the folders this repository ignores (test/, examples/,
# private/) are not there: the result is what CI, and a stranger's clone, sees. Uncommitted changes are NOT checked; commit them first.
# Build output is kept between runs in $LC_CI_TARGET (default ~/.cache/layoutcraft-ci-target) so that a second run is quick.
#
# Not covered: the Windows and macOS legs of CI. A step whose tool is missing is reported as SKIPPED, never as passed.
set -uo pipefail

quick=0 release=0 keep=0 ref=HEAD
while [ $# -gt 0 ]; do
  case "$1" in
    --quick) quick=1 ;;
    --release) release=1 ;;
    --keep) keep=1 ;;
    --ref) shift; ref="${1:?--ref needs a commit or branch}" ;;
    -h | --help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

root="$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)" || exit 2
commit="$(git -C "$root" rev-parse --verify --quiet "$ref^{commit}")" || { echo "not a commit: $ref" >&2; exit 2; }
if [ "$ref" = HEAD ] && [ -n "$(git -C "$root" status --porcelain --untracked-files=no)" ]; then
  echo "note: there are uncommitted changes; they are not checked (only commit ${commit:0:9})." >&2
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/layoutcraft-ci.XXXXXX")" || exit 2
cleanup() {
  if [ "$keep" = 1 ]; then echo "kept: $work"; return; fi
  git -C "$root" worktree remove --force "$work" > /dev/null 2>&1 || rm -rf "$work"
  git -C "$root" worktree prune
}
trap cleanup EXIT
git -C "$root" worktree add --quiet --detach "$work" "$commit" || exit 2

# the same settings as the env of ci.yml
export LAYOUTCRAFT_PUBLIC_BUILD=1
export CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
export CARGO_TOOLCHAIN='' # the scripts pin a toolchain for the maintainers' Windows machine; here the default one is used
export CARGO_TARGET_DIR="${LC_CI_TARGET:-$HOME/.cache/layoutcraft-ci-target}"
mkdir -p "$CARGO_TARGET_DIR"

names=() results=() times=()
# step <name> <command...>: runs in the checkout, never stops the others
step() {
  local name="$1"; shift
  echo; echo "=== $name"
  local start=$SECONDS
  if (cd "$work" && "$@"); then results+=(PASSED); else results+=(FAILED); fi
  names+=("$name"); times+=($((SECONDS - start)))
}
skip() { echo; echo "=== $1"; echo "skipped: $2"; names+=("$1"); results+=(SKIPPED); times+=(0); }

load_check() {
  node --input-type=module -e "
    import { loadEngine } from './web/engine.js'
    import { readFileSync } from 'node:fs'
    const engine = await loadEngine(readFileSync('web/layoutcraft.wasm'))
    if (typeof engine.import !== 'function') throw new Error('the engine has no import()')"
}
# build-web-wasm.sh copies the result from rust/target, so this one step builds there and not in the shared target directory
wasm_build_and_load() { env -u CARGO_TARGET_DIR sh tools/build-web-wasm.sh public && load_check; }
cargo_rust() { (cd rust && cargo "$@"); }

step "provenance guard" cargo_rust xtask provenance
step "third-party notices are current" cargo_rust xtask notices --check

if ! command -v node > /dev/null 2>&1; then
  skip "wasm build and load" "node is not installed"
elif ! rustup target list --installed 2> /dev/null | grep -qx wasm32-unknown-unknown; then
  skip "wasm build and load" "the Rust target is missing (rustup target add wasm32-unknown-unknown)"
else
  step "wasm build and load" wasm_build_and_load
fi

if cargo deny --version > /dev/null 2>&1; then
  step "licences and advisories (cargo-deny)" cargo_rust deny --manifest-path Cargo.toml check --all-features
else
  skip "licences and advisories (cargo-deny)" "cargo-deny is not installed (cargo install cargo-deny)"
fi

if [ "$quick" = 0 ]; then
  step "tests (cargo test --workspace)" cargo_rust test --workspace --no-fail-fast
  step "public fixture conformance" cargo_rust run --quiet -p layoutcraft-conformance -- --root .. --manifest ../fixtures/public/manifest.json
fi
if [ "$release" = 1 ]; then
  step "release build" cargo_rust build --release -p layoutcraft-app -p layoutcraft-mcp
fi

echo; echo "=== summary for ${commit:0:9}$([ "$ref" != HEAD ] && echo " ($ref)")"
bad=0
for i in "${!names[@]}"; do
  printf '  %-8s %-42s %4ss\n' "${results[$i]}" "${names[$i]}" "${times[$i]}"
  [ "${results[$i]}" = FAILED ] && bad=1
done
[ "$quick" = 1 ] && echo "  (--quick: tests and conformance were not run)"
echo "  Windows and macOS are not covered; only CI runs them."
exit "$bad"
