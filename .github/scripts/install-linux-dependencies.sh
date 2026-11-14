#!/usr/bin/env bash
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Installs the packages the Linux build and packaging need, then libkeyfinder
# and the ONNX Runtime the stem separation links. The package mirrors are
# sometimes unreachable for a minute, so each network step is retried with a
# growing pause before the job is failed.
set -euo pipefail

attempts=5

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

if [[ "${1:-}" == "--list-packages" ]]; then
  printf '%s\n' "${packages[@]}"
  exit 0
fi

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

# The CI workflows restore these packages from a cache and set
# EUPHORIA_APT_PACKAGES_INSTALLED, which skips the mirror round trips.
if [[ "${EUPHORIA_APT_PACKAGES_INSTALLED:-}" != "1" ]]; then
  retry update_package_lists
  retry sudo apt-get install -y -o Acquire::Retries=3 "${packages[@]}"
fi
retry "$(dirname "$0")/install-libkeyfinder.sh"

# The stem separation links the ONNX Runtime that the ort crate downloads,
# pyke's prebuilt libonnxruntime.a, which was built with GCC 13 and asks
# Ubuntu 22.04's libstdc++ for GLIBCXX_3.4.30 symbols that only GCC 13 and
# later provide. The shared library Microsoft releases for the same ONNX
# Runtime 1.31.0 needs only GLIBCXX_3.4.9, and libstdc++ is a base system
# library the packages never bundle, so linking that one lets the binaries
# run on 22.04 without a newer C++ runtime. It is installed into
# /usr/local/lib and found through it: the build passes
# ORT_LIB_PATH=/usr/local/lib and ORT_PREFER_DYNAMIC_LINK=1, which make
# ort-sys link libonnxruntime instead of downloading the static one.
onnx_runtime_version="1.31.0"
# The release names use x64 and aarch64, whatever uname -m reports.
case "$(uname -m)" in
  x86_64) onnx_runtime_architecture="x64" ;;
  arm64|aarch64) onnx_runtime_architecture="aarch64" ;;
  *)
    echo "error: no pinned ONNX Runtime is available for $(uname -m)" >&2
    exit 1
    ;;
esac
case "$onnx_runtime_architecture" in
  x64) onnx_runtime_checksum="cc5c72baf5ae5c8238a6841f0897227be2d02826b9cf98eaf02fdefaeeb03a57" ;;
  aarch64) onnx_runtime_checksum="c5b8b3cca31f3d643a3b313b8f42e2d0f16a4fcefd02cda343d97bd5306afaab" ;;
esac
install_onnx_runtime() {
  local archive
  local folder
  archive="$(mktemp)"
  folder="$(mktemp -d)"
  curl --silent --show-error --fail --location --output "$archive" \
    "https://github.com/microsoft/onnxruntime/releases/download/v${onnx_runtime_version}/onnxruntime-linux-${onnx_runtime_architecture}-${onnx_runtime_version}.tgz"
  echo "$onnx_runtime_checksum  $archive" | sha256sum --check --quiet
  tar -xzf "$archive" -C "$folder"
  sudo cp "$folder"/onnxruntime-linux-${onnx_runtime_architecture}-${onnx_runtime_version}/lib/libonnxruntime.so* /usr/local/lib/
  sudo ldconfig
  rm -rf "$archive" "$folder"
}
retry install_onnx_runtime
