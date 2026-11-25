#!/usr/bin/env bash
# Notarisation of the signed disk image with Apple's notary service, then stapling the ticket (docs/36-packaging.md, "Adding signing").
#   tools/packaging/macos/notarize.sh <file.dmg>
# Does nothing, and succeeds, without the secrets (the release stays unnotarised). Secrets, given to this step only:
#   APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD (an app-specific password of that Apple ID)
# The image must have been signed with a Developer ID by sign.sh. Not exercised yet: no Apple Developer ID exists for the project.
set -euo pipefail
# shellcheck source=tools/packaging/lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"

dmg="${1:?usage: notarize.sh <file.dmg>}"
if [ -z "${APPLE_ID:-}" ] || [ -z "${APPLE_TEAM_ID:-}" ] || [ -z "${APPLE_APP_PASSWORD:-}" ]; then
  lc_log "no APPLE_ID / APPLE_TEAM_ID / APPLE_APP_PASSWORD secrets: not notarising"
  exit 0
fi
xcrun notarytool submit "$dmg" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" --wait --timeout 30m
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
