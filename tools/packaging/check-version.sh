#!/usr/bin/env bash
# Checks that both programs carry the same, well-formed version and, for a release tag, that the tag is "v<version>".
#   tools/packaging/check-version.sh            local or dry run: prints the version
#   tools/packaging/check-version.sh v0.1.0     tag build: fails unless the tag is v<version>
# In GitHub Actions it also writes version= and prerelease= to $GITHUB_OUTPUT.
set -euo pipefail
# shellcheck source=tools/packaging/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

tag="${1:-}"
version="$(lc_version)"
if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+(\.[0-9A-Za-z]+)*)?$ ]]; then
  lc_die "version \"$version\" in the Cargo.toml files is not MAJOR.MINOR.PATCH[-prerelease]"
fi
if [ -n "$tag" ] && [ "$tag" != "v$version" ]; then
  lc_die "the tag is \"$tag\" but layoutcraft-app and layoutcraft-mcp are at version $version (expected the tag v$version). Fix the version in rust/layoutcraft-app/Cargo.toml and rust/layoutcraft-mcp/Cargo.toml (and rust/Cargo.lock), or tag the right commit."
fi
prerelease=false
case "$version" in *-*) prerelease=true ;; esac
lc_log "version $version (prerelease: $prerelease)${tag:+, tag $tag matches}"
if [ -n "${GITHUB_OUTPUT:-}" ]; then
  {
    echo "version=$version"
    echo "prerelease=$prerelease"
  } >> "$GITHUB_OUTPUT"
fi
