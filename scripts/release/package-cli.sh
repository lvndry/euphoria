#!/bin/sh
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Packages the euphoria command line tool as a tarball that runs on a Mac
# without Homebrew:
#
#   ./scripts/release/package-cli.sh <output folder>
#
# Builds the CLI in release mode and writes
# <output folder>/euphoria-<version>-<target>.tar.gz, holding one folder of
# the same name with bin/euphoria, lib/ with libkeyfinder and FFTW, the
# licence and the README. The executable finds its libraries through the run
# path @executable_path/../lib, so the folder can be unpacked anywhere. A
# Homebrew formula can install bin/ and lib/ as they are. Pass --skip-build
# to package the binary already in target/release.

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
if [ "$(uname -s)" != "Darwin" ]; then
  echo "error: this script packages the macOS build" >&2
  exit 1
fi

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repository/crates/euphoria-cli/Cargo.toml" | head -n 1)
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
python3 "$repository/scripts/macos_libraries.py" bundle \
  --root "$folder" \
  --executable "$folder/bin/euphoria" \
  --libraries "$folder/lib" \
  --run-path "@executable_path/../lib"
for file in "$folder/lib"/*.dylib "$folder/bin/euphoria"; do
  [ -e "$file" ] || continue
  codesign --force --sign - --timestamp=none "$file"
done
cp "$repository/LICENSE" "$repository/README.md" "$folder/"
mkdir -p "$folder/licenses"
cp "$repository/licenses/webpki-root-certs-CDLA-Permissive-2.0.txt" "$repository/licenses/demucs-NOTICE.txt" "$folder/licenses/"

python3 "$repository/scripts/macos_libraries.py" verify --root "$folder" --executable "$folder/bin/euphoria"
"$folder/bin/euphoria" --version > /dev/null
tar -C "$staging" -czf "$output/$name.tar.gz" "$name"
rm -rf "$staging"
echo "Built $output/$name.tar.gz"
