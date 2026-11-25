#!/usr/bin/env bash
# Creates the GitHub release of a tag from the assembled folder: a DRAFT with every file attached, checked against SHA256SUMS.txt, and only then published.
#   GH_TOKEN=... GITHUB_REPOSITORY=owner/name tools/packaging/publish-release.sh <tag> <folder>
# The token needs contents: write; this is the only script of the pipeline that is run with it. A release that already exists and is published is never
# touched; a draft left over from an earlier failed run is completed (files replaced).
set -euo pipefail
# shellcheck source=tools/packaging/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

tag="${1:?usage: publish-release.sh <tag> <folder>}"
dir="${2:?usage: publish-release.sh <tag> <folder>}"
: "${GH_TOKEN:?GH_TOKEN is not set}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is not set}"
version="$(lc_version)"
[ "$tag" = "v$version" ] || lc_die "tag $tag is not v$version"

cd "$dir"
sha256sum --check --strict SHA256SUMS.txt # the files are the ones the build jobs made (nothing changed between the jobs)
files=(LayoutCraft-*)
files+=(SHA256SUMS.txt)
flags=()
case "$version" in *-*) flags+=(--prerelease) ;; esac

if existing="$(gh release view "$tag" --repo "$GITHUB_REPOSITORY" --json isDraft --jq .isDraft 2> /dev/null)"; then
  [ "$existing" = true ] || lc_die "the release $tag is already published; refusing to change it"
  lc_log "completing the existing draft $tag"
  gh release upload "$tag" --repo "$GITHUB_REPOSITORY" --clobber "${files[@]}"
else
  lc_log "creating the draft $tag"
  gh release create "$tag" --repo "$GITHUB_REPOSITORY" --draft --verify-tag --title "LayoutCraft $tag" --generate-notes ${flags[@]+"${flags[@]}"} "${files[@]}"
fi

# the draft has exactly the files we meant to attach, with the same content (GitHub reports the SHA-256 of every uploaded file)
remote="$(gh release view "$tag" --repo "$GITHUB_REPOSITORY" --json assets --jq '.assets[] | "\(.digest // "-")  \(.name)"' | sort -k2)"
expected="$(for f in "${files[@]}"; do printf '%s\n' "$f"; done | sort)"
[ "$(printf '%s\n' "$remote" | awk '{ print $2 }' | sort)" = "$expected" ] || lc_die "the files of the draft are not the expected ones: $remote"
while read -r digest name; do
  [ "$digest" = "-" ] && continue # a gh without digest support: the names were checked above
  local_sum="sha256:$(sha256sum -- "$name" | cut -d' ' -f1)"
  [ "$digest" = "$local_sum" ] || lc_die "$name was uploaded as $digest, but is $local_sum here"
done <<< "$remote"

lc_log "publishing $tag"
gh release edit "$tag" --repo "$GITHUB_REPOSITORY" --draft=false
