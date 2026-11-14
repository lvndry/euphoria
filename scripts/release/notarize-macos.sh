#!/bin/sh
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Submits a signed file to Apple's notary service, waits for the verdict and
# staples the ticket to it:
#
#   ./scripts/release/notarize-macos.sh <Euphoria.app | Euphoria.dmg>
#   ./scripts/release/notarize-macos.sh --no-staple <file.zip>
#
# An application bundle is zipped first, as notarytool needs. A zip of
# command line files is submitted as it is with --no-staple, because a plain
# executable cannot hold a stapled ticket: Gatekeeper then checks Apple's
# service online the first time it runs the file.
#
# The App Store Connect API key comes from the environment:
# EUPHORIA_NOTARY_KEY_PATH (the .p8 file), EUPHORIA_NOTARY_KEY_ID and
# EUPHORIA_NOTARY_ISSUER_ID. The script fails unless Apple accepts the file.

set -eu

staple=yes
if [ "${1:-}" = "--no-staple" ]; then
  staple=no
  shift
fi
if [ "$#" -ne 1 ] || [ ! -e "$1" ]; then
  echo "usage: $0 [--no-staple] <app, dmg or zip>" >&2
  exit 1
fi
target=$1
for variable in EUPHORIA_NOTARY_KEY_PATH EUPHORIA_NOTARY_KEY_ID EUPHORIA_NOTARY_ISSUER_ID; do
  eval "value=\${$variable:-}"
  if [ -z "$value" ]; then
    echo "error: $variable is not set" >&2
    exit 1
  fi
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
submission=$target
if [ -d "$target" ]; then
  submission="$work/$(basename "$target").zip"
  ditto -c -k --keepParent "$target" "$submission"
fi

credentials="--key $EUPHORIA_NOTARY_KEY_PATH --key-id $EUPHORIA_NOTARY_KEY_ID --issuer $EUPHORIA_NOTARY_ISSUER_ID"
echo "Submitting $(basename "$target") to the notary service"
# shellcheck disable=SC2086
xcrun notarytool submit "$submission" $credentials --wait --output-format json > "$work/result.json" || true
cat "$work/result.json"
status=$(plutil -extract status raw -o - "$work/result.json" 2> /dev/null || true)
identifier=$(plutil -extract id raw -o - "$work/result.json" 2> /dev/null || true)
if [ "$status" != "Accepted" ]; then
  echo "error: the notary service returned '${status:-no verdict}' for $(basename "$target")" >&2
  if [ -n "$identifier" ]; then
    # shellcheck disable=SC2086
    xcrun notarytool log "$identifier" $credentials >&2 || true
  fi
  exit 1
fi

if [ "$staple" = yes ]; then
  xcrun stapler staple "$target"
  xcrun stapler validate "$target"
fi
echo "Notarized $(basename "$target")"
