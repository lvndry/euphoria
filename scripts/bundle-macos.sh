#!/bin/sh
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Builds the desktop app in release mode and packages it for macOS: an
# application bundle that runs on a Mac without Homebrew, and a disk image
# to install it from.
#
#   ./scripts/bundle-macos.sh
#   open target/release/bundle/Euphoria.app
#
# Writes target/release/bundle/Euphoria.app and
# target/release/bundle/Euphoria-<version>.dmg. Pass --no-dmg to stop after
# the app, and --skip-build to package the binary already in target/release.
#
# libkeyfinder and FFTW are copied into Contents/Frameworks by
# scripts/macos_libraries.py, with their install names changed so the app
# finds them there, and the script then checks that every library of every
# binary in the bundle is part of macOS or of the bundle. The bundle is signed
# with an ad hoc signature, which lets it run on the Mac that built it. It is
# not notarized: that needs a paid Apple Developer account. On another Mac,
# Gatekeeper asks once before opening it.
#
# The disk image is laid out with dmgbuild, run through uv, because writing
# the window layout with only the tools macOS ships needs Finder scripting,
# which needs a logged in session and a permission prompt.

set -eu

repository=$(cd "$(dirname "$0")/.." && pwd)
output="$repository/target/release/bundle"
bundle="$output/Euphoria.app"
frameworks="$bundle/Contents/Frameworks"
binary="$repository/target/release/euphoria-desktop"
icon_artwork="$repository/assets/logo/app-icon.svg"
# The line of the full size artwork thins out to nothing at 16 and 32 pixels,
# so those sizes are drawn from the favicon, which has a heavier line.
small_icon_artwork="$repository/assets/logo/favicon.svg"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repository/crates/euphoria-desktop/Cargo.toml" | head -n 1)
dmgbuild_version="1.6.7"
make_dmg=yes
build=yes

for argument in "$@"; do
  case "$argument" in
    --no-dmg) make_dmg=no ;;
    --skip-build) build=no ;;
    *)
      echo "error: unknown option $argument (expected --no-dmg or --skip-build)" >&2
      exit 1
      ;;
  esac
done

if [ "$(uname -s)" != "Darwin" ]; then
  echo "error: application bundles can only be built on macOS" >&2
  exit 1
fi
if [ -z "$version" ]; then
  echo "error: no version found in crates/euphoria-desktop/Cargo.toml" >&2
  exit 1
fi
if [ "$make_dmg" = yes ] && ! command -v uv > /dev/null; then
  echo "error: uv is needed to lay out the disk image (https://docs.astral.sh/uv/), or pass --no-dmg" >&2
  exit 1
fi

if [ "$build" = yes ]; then
  cargo build --release --locked -p euphoria --manifest-path "$repository/Cargo.toml"
fi

rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources" "$frameworks"
executable="$bundle/Contents/MacOS/Euphoria"
cp "$binary" "$executable"
cp "$repository/LICENSE" "$bundle/Contents/Resources/LICENSE"
mkdir -p "$bundle/Contents/Resources/licenses"
cp "$repository/licenses/webpki-root-certs-CDLA-Permissive-2.0.txt" "$repository/licenses/demucs-NOTICE.txt" "$bundle/Contents/Resources/licenses/"
chmod u+w "$executable"

# Copies every library the executable needs from outside the system into
# Contents/Frameworks, following install names the way dyld does, and fails
# if any of them does not resolve.
python3 "$repository/scripts/macos_libraries.py" bundle \
  --root "$bundle" \
  --executable "$executable" \
  --libraries "$frameworks" \
  --run-path "@executable_path/../Frameworks"

# Every binary in the bundle needs at least the macOS version it was built
# for, in every architecture it holds. Homebrew builds its libraries for the
# macOS it runs on, so the app can need a newer macOS than Rust's own
# default, 11.0.
minimum_system=$(python3 "$repository/scripts/macos_libraries.py" minimum-macos --root "$bundle" --floor 11.0)

iconset=$(mktemp -d)/AppIcon.iconset
mkdir -p "$iconset"
# Writes one icon size: render <pixels> <file name> <artwork>.
render() {
  sips -s format png -z "$1" "$1" "$3" --out "$iconset/$2" > /dev/null
}
render 16 icon_16x16.png "$small_icon_artwork"
render 32 icon_16x16@2x.png "$small_icon_artwork"
render 32 icon_32x32.png "$small_icon_artwork"
render 64 icon_32x32@2x.png "$icon_artwork"
render 128 icon_128x128.png "$icon_artwork"
render 256 icon_128x128@2x.png "$icon_artwork"
render 256 icon_256x256.png "$icon_artwork"
render 512 icon_256x256@2x.png "$icon_artwork"
render 512 icon_512x512.png "$icon_artwork"
render 1024 icon_512x512@2x.png "$icon_artwork"
iconutil --convert icns --output "$bundle/Contents/Resources/AppIcon.icns" "$iconset"
rm -rf "$(dirname "$iconset")"

cat > "$bundle/Contents/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>Euphoria</string>
  <key>CFBundleDisplayName</key>
  <string>Euphoria</string>
  <key>CFBundleIdentifier</key>
  <string>io.github.lvndry.euphoria</string>
  <key>CFBundleExecutable</key>
  <string>Euphoria</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$version</string>
  <key>CFBundleVersion</key>
  <string>$version</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>LSMinimumSystemVersion</key>
  <string>$minimum_system</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.music</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSHumanReadableCopyright</key>
  <string>Copyright 2026 The Euphoria contributors. GPL-3.0-or-later.</string>
</dict>
</plist>
PLIST
plutil -lint "$bundle/Contents/Info.plist" > /dev/null

# Libraries first, then the bundle, which seals them in its signature.
for file in "$frameworks"/*.dylib; do
  [ -e "$file" ] || continue
  codesign --force --sign - --timestamp=none "$file"
done
codesign --force --sign - --timestamp=none "$bundle"
codesign --verify --strict --verbose=1 "$bundle"

# Proves the result rather than trusting how it was made: every library of
# every binary in the bundle is a system library or a file in the bundle.
python3 "$repository/scripts/macos_libraries.py" verify --root "$bundle" --executable "$executable"
echo "Built $bundle (macOS $minimum_system or later)"

if [ "$make_dmg" = no ]; then
  exit 0
fi

dmg="$output/Euphoria-$version.dmg"
rm -f "$dmg"
uv tool run --quiet --from "dmgbuild==$dmgbuild_version" dmgbuild \
  -s "$repository/scripts/dmg-settings.py" \
  -D "app=$bundle" \
  -D "icon=$bundle/Contents/Resources/AppIcon.icns" \
  "Euphoria $version" "$dmg"
hdiutil verify "$dmg" > /dev/null
echo "Built $dmg"
