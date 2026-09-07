#!/bin/sh
# Explicit maintainer operation. NOT called by npm, setup, tests or candidate CI.
# Uses identities already configured by the operator; never imports certificates.
set -eu
set +x
if [ "$#" -ne 1 ] || [ "$(uname -s)" != Darwin ] || [ "$(uname -m)" != arm64 ]; then
  echo 'Usage: sign-release.sh ABSOLUTE_RELEASE_BINARY_DIRECTORY (macOS arm64)' >&2
  exit 1
fi
case "$1" in /*) ;; *) echo 'Require an absolute binary directory.' >&2; exit 1 ;; esac
: "${MAGICVAULT_SIGNING_IDENTITY:?Set an explicitly authorized Developer ID Application identity}"
: "${MAGICVAULT_NOTARY_PROFILE:?Set a previously configured notarytool keychain profile}"
release_bin=$1
for name in magicvault magicvault-mcp magicvault-native-host; do
  if [ ! -f "$release_bin/$name" ] || [ -L "$release_bin/$name" ]; then
    echo 'Missing regular release executable.' >&2
    exit 1
  fi
done
# The binaries are modified in place. Sign BEFORE package-npm hashes/copies them.
for name in magicvault magicvault-mcp magicvault-native-host; do
  /usr/bin/codesign --force --options runtime --timestamp --sign "$MAGICVAULT_SIGNING_IDENTITY" "$release_bin/$name"
  /usr/bin/codesign --verify --strict "$release_bin/$name"
  signature=$(/usr/bin/codesign --display --verbose=4 "$release_bin/$name" 2>&1)
  case "$signature" in
    *"Authority=Developer ID Application:"*) ;;
    *) echo 'Release signing requires Developer ID Application authority.' >&2; exit 1 ;;
  esac
done
release_staging=$(mktemp -d "${TMPDIR:-/tmp}/magicvault-notarize.XXXXXXXX")
mkdir "$release_staging/binaries"
for name in magicvault magicvault-mcp magicvault-native-host; do
  cp "$release_bin/$name" "$release_staging/binaries/$name"
done
/usr/bin/ditto -c -k --keepParent "$release_staging/binaries" "$release_staging/MagicVault.zip"
# Bounded wait. Retain the private result for explicit diagnosis on failure.
/usr/bin/xcrun notarytool submit "$release_staging/MagicVault.zip" \
  --keychain-profile "$MAGICVAULT_NOTARY_PROFILE" --wait --timeout 20m --output-format json > "$release_staging/result.json"
node -e 'const fs = require("node:fs"); const r = JSON.parse(fs.readFileSync(process.argv[1])); if (r.status !== "Accepted") process.exit(1);' "$release_staging/result.json"
# ZIPs and bare executables cannot be stapled. Gatekeeper retrieves online tickets.
echo 'Notarization accepted. Package these exact signed bytes; no npm publication was performed.'
echo "Private notarization record retained at: $release_staging"
