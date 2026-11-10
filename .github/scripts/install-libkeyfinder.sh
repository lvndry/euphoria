#!/usr/bin/env bash
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Builds and installs libkeyfinder from a checksummed release archive, for
# Linux distributions that do not package it.
set -euo pipefail

version="2.2.8"
expected_checksum="a54fc6c5ff435bb4b447f175bc97f9081fb5abf0edd5d125e6f5215c8fff4d11"
archive_url="https://github.com/mixxxdj/libkeyfinder/archive/refs/tags/${version}.tar.gz"
install_prefix="${INSTALL_PREFIX:-/usr/local}"

build_directory="$(mktemp -d)"
trap 'rm -rf "$build_directory"' EXIT

curl --silent --show-error --fail --location --output "$build_directory/source.tar.gz" "$archive_url"
echo "$expected_checksum  $build_directory/source.tar.gz" | sha256sum --check --quiet

tar -xzf "$build_directory/source.tar.gz" -C "$build_directory"
cmake -S "$build_directory/libkeyfinder-$version" -B "$build_directory/build" \
  -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_TESTING=OFF \
  -DCMAKE_INSTALL_PREFIX="$install_prefix"
cmake --build "$build_directory/build" --parallel
sudo cmake --install "$build_directory/build"
sudo ldconfig
