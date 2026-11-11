#!/bin/sh
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Builds the desktop app in release mode and packages it for Linux as an
# AppImage, one file that runs on a desktop without libkeyfinder or FFTW:
#
#   ./scripts/bundle-linux.sh
#   ./target/release/bundle/Euphoria-<version>-<architecture>.AppImage
#
# Writes target/release/bundle/Euphoria.AppDir and
# target/release/bundle/Euphoria-<version>-<architecture>.AppImage. Pass
# --skip-build to package the binary already in target/release.
#
# libkeyfinder and FFTW are copied into usr/lib by scripts/linux_libraries.py,
# which gives the app the run path $ORIGIN/../lib, and the script then checks
# that every library of every binary in the AppDir is part of the base system
# or of the AppDir. The AppImage is made with appimagetool and the AppImage
# runtime, both downloaded from their pinned releases and checked against
# their checksums. Needs patchelf and rsvg-convert (librsvg2-bin).

set -eu

repository=$(cd "$(dirname "$0")/.." && pwd)
output="$repository/target/release/bundle"
appdir="$output/Euphoria.AppDir"
binary="$repository/target/release/euphoria-desktop"
tools="$repository/target/appimage-tools"
icon_artwork="$repository/assets/logo/app-icon.svg"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repository/crates/euphoria-desktop/Cargo.toml" | head -n 1)
architecture=$(uname -m)
appimagetool_release="1.9.1"
runtime_release="20251108"
build=yes

for argument in "$@"; do
  case "$argument" in
    --skip-build) build=no ;;
    *)
      echo "error: unknown option $argument (expected --skip-build)" >&2
      exit 1
      ;;
  esac
done

if [ "$(uname -s)" != "Linux" ]; then
  echo "error: AppImages can only be built on Linux" >&2
  exit 1
fi
if [ -z "$version" ]; then
  echo "error: no version found in crates/euphoria-desktop/Cargo.toml" >&2
  exit 1
fi
case "$architecture" in
  x86_64)
    appimagetool_checksum="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"
    runtime_checksum="2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d"
    ;;
  aarch64)
    appimagetool_checksum="f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158"
    runtime_checksum="00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444"
    ;;
  *)
    echo "error: no AppImage tools are pinned for $architecture" >&2
    exit 1
    ;;
esac
for tool in patchelf rsvg-convert curl sha256sum; do
  if ! command -v "$tool" > /dev/null; then
    echo "error: $tool is needed to build the AppImage" >&2
    exit 1
  fi
done

# Downloads a pinned file once: fetch <url> <file> <sha256>. A file that does
# not match its checksum is removed, never used.
fetch() {
  if [ ! -f "$2" ] || ! echo "$3  $2" | sha256sum --check --quiet > /dev/null 2>&1; then
    rm -f "$2"
    curl --silent --show-error --fail --location --retry 5 --retry-all-errors \
      --output "$2.part" "$1"
    if ! echo "$3  $2.part" | sha256sum --check --quiet; then
      rm -f "$2.part"
      echo "error: $1 does not match its checksum" >&2
      exit 1
    fi
    mv "$2.part" "$2"
  fi
  chmod +x "$2"
}
mkdir -p "$tools"
appimagetool="$tools/appimagetool-$appimagetool_release-$architecture.AppImage"
runtime="$tools/runtime-$runtime_release-$architecture"
fetch "https://github.com/AppImage/appimagetool/releases/download/$appimagetool_release/appimagetool-$architecture.AppImage" \
  "$appimagetool" "$appimagetool_checksum"
fetch "https://github.com/AppImage/type2-runtime/releases/download/$runtime_release/runtime-$architecture" \
  "$runtime" "$runtime_checksum"

if [ "$build" = yes ]; then
  cargo build --release --locked -p euphoria --manifest-path "$repository/Cargo.toml"
fi

rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/lib" \
  "$appdir/usr/share/applications" \
  "$appdir/usr/share/icons/hicolor/256x256/apps" \
  "$appdir/usr/share/icons/hicolor/scalable/apps" \
  "$appdir/usr/share/licenses/euphoria"
executable="$appdir/usr/bin/euphoria-desktop"
cp "$binary" "$executable"
chmod u+w "$executable"
cp "$repository/LICENSE" "$appdir/usr/share/licenses/euphoria/LICENSE"

# Copies every library the executable needs from outside the base system
# into usr/lib, following needed libraries the way the dynamic loader does,
# and fails if any of them cannot be found.
python3 "$repository/scripts/linux_libraries.py" bundle \
  --root "$appdir" \
  --executable "$executable" \
  --libraries "$appdir/usr/lib" \
  --run-path '$ORIGIN/../lib'

cp "$icon_artwork" "$appdir/usr/share/icons/hicolor/scalable/apps/euphoria.svg"
rsvg-convert --width 256 --height 256 --output "$appdir/usr/share/icons/hicolor/256x256/apps/euphoria.png" "$icon_artwork"
cp "$appdir/usr/share/icons/hicolor/256x256/apps/euphoria.png" "$appdir/euphoria.png"
ln -s euphoria.png "$appdir/.DirIcon"

# The file name and StartupWMClass match the app_id the app gives its
# window, so desktops show the window with this entry's name and icon.
cat > "$appdir/usr/share/applications/euphoria.desktop" << DESKTOP
[Desktop Entry]
Type=Application
Name=Euphoria
GenericName=Key and BPM Analyzer
Comment=Find the key and BPM of every track in your music collection
Exec=euphoria-desktop
Icon=euphoria
Terminal=false
Categories=AudioVideo;Audio;Music;
Keywords=DJ;key;BPM;Camelot;harmonic;mixing;
StartupWMClass=euphoria
X-AppImage-Version=$version
DESKTOP
cp "$appdir/usr/share/applications/euphoria.desktop" "$appdir/euphoria.desktop"
ln -s usr/bin/euphoria-desktop "$appdir/AppRun"

# Proves the result rather than trusting how it was made: every library of
# every binary in the AppDir is a base system library or a file in the
# AppDir.
python3 "$repository/scripts/linux_libraries.py" verify --root "$appdir" --executable "$executable"
minimum_glibc=$(python3 "$repository/scripts/linux_libraries.py" minimum-glibc --root "$appdir")

appimage="$output/Euphoria-$version-$architecture.AppImage"
rm -f "$appimage"
# appimagetool is itself an AppImage. Unpacking it on every run makes it work
# where FUSE is missing, as in containers.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$architecture" VERSION="$version" "$appimagetool" \
  --no-appstream --runtime-file "$runtime" "$appdir" "$appimage"
echo "Built $appimage (glibc $minimum_glibc or later)"
