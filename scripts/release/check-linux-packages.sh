#!/bin/sh
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Proves that the Linux release files run on a clean system, one that never
# had libkeyfinder, FFTW or a Rust toolchain:
#
#   ./scripts/release/check-linux-packages.sh <folder with the release files>
#
# The command line tarball is unpacked into bare Ubuntu 22.04 and Fedora
# containers, and analyzes a test file there. The AppImage is unpacked, its
# libraries are checked with scripts/linux_libraries.py, and it is started in
# an Ubuntu 22.04 container that has only the libraries every desktop has,
# under a virtual X server with software Vulkan, where its window must appear.
# Needs Docker.

set -eu

repository=$(cd "$(dirname "$0")/../.." && pwd)
if [ $# -ne 1 ]; then
  echo "usage: $0 <folder with the release files>" >&2
  exit 1
fi
files=$(cd "$1" && pwd)
architecture=$(uname -m)
cli_tarball=$(find "$files" -maxdepth 1 -name "euphoria-*-$architecture-unknown-linux-gnu.tar.gz" | head -n 1)
appimage=$(find "$files" -maxdepth 1 -name "Euphoria-*-$architecture.AppImage" | head -n 1)
if [ -z "$cli_tarball" ] || [ -z "$appimage" ]; then
  echo "error: $files needs the command line tarball and the AppImage for $architecture" >&2
  exit 1
fi

# Docker Hub and the package mirrors are sometimes unreachable for a minute,
# so the steps that download are retried with a growing pause.
retry() {
  attempt=1
  until "$@"; do
    if [ "$attempt" -eq 5 ]; then
      echo "giving up after 5 attempts: $*" >&2
      return 1
    fi
    echo "attempt $attempt failed, trying again in $((attempt * 15))s: $*" >&2
    sleep $((attempt * 15))
    attempt=$((attempt + 1))
  done
}

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
cp "$repository/crates/euphoria-core/tests/fixtures/sine-440hz-mono.wav" "$scratch/"
tar -xzf "$cli_tarball" -C "$scratch"
cli_folder=$(find "$scratch" -maxdepth 1 -type d -name 'euphoria-*' | head -n 1)
python3 "$repository/scripts/linux_libraries.py" verify --root "$cli_folder" --executable "$cli_folder/bin/euphoria"

for image in ubuntu:22.04 fedora:latest; do
  retry docker pull --quiet "$image" > /dev/null
  docker run --rm --network none \
    -v "$scratch:/check:ro" -e EUPHORIA_DATA_DIR=/tmp/euphoria \
    "$image" sh -c '
      set -eu
      cli=$(ls -d /check/euphoria-*/)
      "$cli/bin/euphoria" --version
      "$cli/bin/euphoria" analyze --json /check/sine-440hz-mono.wav > /tmp/result.json
      grep -q "\"key\"" /tmp/result.json
    '
  echo "The command line tool runs on a clean $image."
done

cp "$appimage" "$scratch/Euphoria.AppImage"
chmod +x "$scratch/Euphoria.AppImage"
(cd "$scratch" && ./Euphoria.AppImage --appimage-extract > /dev/null)
python3 "$repository/scripts/linux_libraries.py" verify \
  --root "$scratch/squashfs-root" \
  --executable "$scratch/squashfs-root/usr/bin/euphoria-desktop"

retry docker pull --quiet ubuntu:22.04 > /dev/null
retry docker build --quiet --tag euphoria-clean-desktop - > /dev/null << 'DOCKERFILE'
FROM ubuntu:22.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      libasound2 libfontconfig1 libfreetype6 libxkbcommon0 libxkbcommon-x11-0 \
      libxcb1 libx11-xcb1 libwayland-client0 libvulkan1 mesa-vulkan-drivers \
      fonts-dejavu-core xvfb x11-utils \
 && rm -rf /var/lib/apt/lists/*
DOCKERFILE
docker run --rm --network none -v "$scratch:/check:ro" -e EUPHORIA_DATA_DIR=/tmp/euphoria \
  euphoria-clean-desktop sh -c '
    set -eu
    Xvfb :99 -screen 0 1600x1000x24 > /tmp/xvfb.log 2>&1 &
    export DISPLAY=:99
    for attempt in $(seq 50); do
      xdpyinfo > /dev/null 2>&1 && break
      sleep 0.2
    done
    /check/squashfs-root/AppRun > /tmp/app.log 2>&1 &
    app=$!
    for attempt in $(seq 60); do
      if xwininfo -root -tree | grep -q "\"Euphoria\""; then
        kill "$app"
        echo "The window appeared."
        exit 0
      fi
      if ! kill -0 "$app" 2> /dev/null; then
        echo "error: the app exited before showing its window:" >&2
        cat /tmp/app.log >&2
        exit 1
      fi
      sleep 0.5
    done
    echo "error: no window appeared within 30 seconds:" >&2
    cat /tmp/app.log >&2
    exit 1
  '
echo "The AppImage starts on a clean Ubuntu 22.04 desktop."
