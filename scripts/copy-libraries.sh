#!/bin/sh
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Makes a macOS executable carry the libraries it needs from outside macOS:
#
#   ./scripts/copy-libraries.sh <executable> <library folder> <run path>
#
# Copies every library the executable needs from outside /usr/lib and
# /System, libkeyfinder and FFTW from Homebrew, into <library folder>, and the
# libraries those need in turn. Each copy gets the install name
# @rpath/<file>, every reference to it is changed to match, and the
# executable gets <run path>, such as @executable_path/../Frameworks, so it
# finds them there. Fails if any reference to /opt/homebrew or /usr/local is
# left. Signing is left to the caller, since changing a file breaks its
# signature.

set -eu

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <executable> <library folder> <run path>" >&2
  exit 1
fi
executable=$1
libraries=$2
run_path=$3
mkdir -p "$libraries"

is_system_library() {
  case "$1" in
    /usr/lib/* | /System/*) return 0 ;;
    *) return 1 ;;
  esac
}
# The libraries a Mach-O file refers to, without its own install name.
dependencies() {
  otool -L "$1" | tail -n +2 | sed 's/^[[:space:]]*\([^ ]*\) .*/\1/' | grep -v "^$(otool -D "$1" | tail -n +2)\$" || true
}

pending="$executable"
while [ -n "$pending" ]; do
  next=""
  for file in $pending; do
    for dependency in $(dependencies "$file"); do
      case "$dependency" in
        @rpath/* | @executable_path/* | @loader_path/*) continue ;;
      esac
      if is_system_library "$dependency"; then
        continue
      fi
      name=$(basename "$dependency")
      copy="$libraries/$name"
      if [ ! -e "$copy" ]; then
        cp -L "$dependency" "$copy"
        chmod u+w "$copy"
        install_name_tool -id "@rpath/$name" "$copy"
        next="$next $copy"
      fi
      install_name_tool -change "$dependency" "@rpath/$name" "$file"
    done
  done
  pending=$next
done
install_name_tool -add_rpath "$run_path" "$executable"
for file in "$libraries"/*.dylib; do
  [ -e "$file" ] || continue
  install_name_tool -add_rpath "@loader_path" "$file" 2> /dev/null || true
done

for file in "$executable" "$libraries"/*.dylib; do
  [ -e "$file" ] || continue
  if otool -L "$file" | tail -n +2 | grep -E '/(opt/homebrew|usr/local)/' > /dev/null; then
    echo "error: $file still refers to a library outside $libraries:" >&2
    otool -L "$file" >&2
    exit 1
  fi
done
