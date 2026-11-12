#!/usr/bin/env bash
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Installs the packages the Linux build and packaging need, then libkeyfinder. The package
# mirrors are sometimes unreachable for a minute, so each network step is
# retried with a growing pause before the job is failed.
set -euo pipefail

attempts=5

retry() {
  local attempt=1
  until "$@"; do
    if ((attempt == attempts)); then
      echo "giving up after $attempts attempts: $*" >&2
      return 1
    fi
    local pause=$((attempt * 15))
    echo "attempt $attempt of $attempts failed, trying again in ${pause}s: $*" >&2
    sleep "$pause"
    attempt=$((attempt + 1))
  done
}

update_package_lists() {
  # apt-get update only warns when a mirror cannot be reached, so the
  # warnings are turned into a failure that the retry can see.
  local output
  local status=0
  output="$(sudo apt-get update 2>&1)" || status=$?
  echo "$output"
  if ((status != 0)); then
    return "$status"
  fi
  ! grep -q '^W: Failed to fetch\|^E: ' <<<"$output"
}

retry update_package_lists
# cmake, FFTW and pkg-config build libkeyfinder and the core. The desktop app
# adds the libraries GPUI's X11 and Wayland back ends and cpal's ALSA output
# link against, and libclang for the bindings of the time stretcher. patchelf
# and rsvg-convert package the AppImage.
packages=(
  cmake libfftw3-dev pkg-config
  libasound2-dev libfontconfig-dev libfreetype-dev libwayland-dev
  libx11-xcb-dev libxkbcommon-x11-dev libclang-dev
  patchelf librsvg2-bin
)
retry sudo apt-get install -y -o Acquire::Retries=3 "${packages[@]}"
retry "$(dirname "$0")/install-libkeyfinder.sh"
