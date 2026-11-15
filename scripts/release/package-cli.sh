#!/bin/sh
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Packages the euphoria command line tool as a tarball that runs on a Mac
# without Homebrew, or on Linux without the ONNX Runtime installed:
#
#   ./scripts/release/package-cli.sh <output folder>
#
# Builds the CLI in release mode and writes
# <output folder>/euphoria-<version>-<target>.tar.gz, holding one folder of
# the same name with bin/euphoria, lib/ with the libraries it links that are
# not part of the system (the ONNX Runtime on Linux, none on macOS), the
# licence and the README. The executable finds its libraries through the run
# path @executable_path/../lib on macOS and $ORIGIN/../lib on Linux, so the
# folder can be unpacked anywhere. A Homebrew formula can install bin/ and
# lib/ as they are. Pass --skip-build to package the binary already in
# target/release.
#
# On macOS the files are signed with an ad hoc signature, or with the
# Developer ID Application identity in EUPHORIA_SIGN_IDENTITY, the libraries
# first, with the hardened runtime. With that identity and the notary key in
# EUPHORIA_NOTARY_KEY_PATH, EUPHORIA_NOTARY_KEY_ID and
# EUPHORIA_NOTARY_ISSUER_ID, Apple notarizes a zip of bin/ and lib/ first.
# Command line files cannot be stapled, so the ticket stays with Apple and
# Gatekeeper looks it up online the first time a downloaded file runs.

set -eu

repository=$(cd "$(dirname "$0")/../.." && pwd)
output=""
build=yes
for argument in "$@"; do
  case "$argument" in
    --skip-build) build=no ;;
    -*)
      echo "error: unknown option $argument" >&2
      exit 1
      ;;
    *) output=$argument ;;
  esac
done
if [ -z "$output" ]; then
  echo "usage: $0 [--skip-build] <output folder>" >&2
  exit 1
fi
system=$(uname -s)
if [ "$system" != "Darwin" ] && [ "$system" != "Linux" ]; then
  echo "error: this script packages the macOS and Linux builds" >&2
  exit 1
fi

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repository/apps/euphoria-cli/Cargo.toml" | head -n 1)
target=$(rustc -vV | sed -n 's/^host: //p')
name="euphoria-$version-$target"

if [ "$build" = yes ]; then
  cargo build --release --locked -p euphoria-cli --manifest-path "$repository/Cargo.toml"
fi

mkdir -p "$output"
output=$(cd "$output" && pwd)
staging=$(mktemp -d)
folder="$staging/$name"
mkdir -p "$folder/bin" "$folder/lib"
cp "$repository/target/release/euphoria" "$folder/bin/euphoria"
chmod u+w "$folder/bin/euphoria"
mkdir -p "$folder/licenses"
cp "$repository/licenses/webpki-root-certs-CDLA-Permissive-2.0.txt" "$repository/licenses/demucs-NOTICE.txt" \
  "$repository/licenses/lame-LGPL-2.0.txt" "$repository/licenses/mp3lame-LGPL-3.0.txt" "$folder/licenses/"
# The licences of the C++ libraries compiled into the executable.
cp "$repository/crates/euphoria-core/vendor/signalsmith-stretch/LICENSE.txt" "$folder/licenses/signalsmith-stretch.txt"
cp "$repository/crates/euphoria-core/vendor/signalsmith-linear/LICENSE.txt" "$folder/licenses/signalsmith-linear.txt"
if [ "$system" = Linux ]; then
  python3 "$repository/scripts/linux_libraries.py" bundle \
    --root "$folder" \
    --executable "$folder/bin/euphoria" \
    --libraries "$folder/lib" \
    --run-path '$ORIGIN/../lib'
  cp "$repository/LICENSE" "$repository/README.md" "$folder/"
  python3 "$repository/scripts/linux_libraries.py" verify --root "$folder" --executable "$folder/bin/euphoria"
else
  python3 "$repository/scripts/macos_libraries.py" bundle \
    --root "$folder" \
    --executable "$folder/bin/euphoria" \
    --libraries "$folder/lib" \
    --run-path "@executable_path/../lib"
  sign_identity=${EUPHORIA_SIGN_IDENTITY:--}
  set -- "$folder/lib"/*.dylib
  [ -e "$1" ] || shift
  if [ "$#" -gt 0 ]; then
    python3 "$repository/scripts/macos_sign.py" files --identity "$sign_identity" "$@"
  fi
  # Homebrew rewrites the libraries' load paths on install and signs them
  # again ad hoc, so their team no longer matches the tool's; without this
  # entitlement the hardened runtime refuses to load them and the tool exits
  # at once.
  python3 "$repository/scripts/macos_sign.py" files --identity "$sign_identity" \
    --entitlements "$repository/scripts/release/cli-entitlements.plist" "$folder/bin/euphoria"
  for file in "$@" "$folder/bin/euphoria"; do
    codesign --verify --strict --verbose=1 "$file"
  done
  if [ "$sign_identity" != "-" ] && [ -n "${EUPHORIA_NOTARY_KEY_PATH:-}" ]; then
    notarization=$(mktemp -d)
    (cd "$folder" && zip -qr "$notarization/files.zip" bin lib)
    "$repository/scripts/release/notarize-macos.sh" --no-staple "$notarization/files.zip"
    rm -rf "$notarization"
  fi
  cp "$repository/LICENSE" "$repository/README.md" "$folder/"
  python3 "$repository/scripts/macos_libraries.py" verify --root "$folder" --executable "$folder/bin/euphoria"
fi
"$folder/bin/euphoria" --version > /dev/null
tar -C "$staging" -czf "$output/$name.tar.gz" "$name"
rm -rf "$staging"
echo "Built $output/$name.tar.gz"
