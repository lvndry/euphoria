#!/bin/sh
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Builds the desktop app in release mode and wraps it in a macOS application
# bundle with the Euphoria icon:
#
#   ./scripts/bundle-macos.sh
#   open target/release/bundle/Euphoria.app
#
# The bundle is for use on the machine that built it. It links against the
# libkeyfinder installed there and is neither signed nor notarized.

set -eu

repository=$(cd "$(dirname "$0")/.." && pwd)
bundle="$repository/target/release/bundle/Euphoria.app"
icon_artwork="$repository/assets/logo/app-icon.svg"
# The line of the full size artwork thins out to nothing at 16 and 32 pixels,
# so those sizes are drawn from the favicon, which has a heavier line.
small_icon_artwork="$repository/assets/logo/favicon.svg"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repository/crates/euphoria-app/Cargo.toml" | head -n 1)

if [ "$(uname -s)" != "Darwin" ]; then
  echo "error: application bundles can only be built on macOS" >&2
  exit 1
fi
if [ -z "$version" ]; then
  echo "error: no version found in crates/euphoria-app/Cargo.toml" >&2
  exit 1
fi

cargo build --release --locked -p euphoria-app --manifest-path "$repository/Cargo.toml"

rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$repository/target/release/euphoria-app" "$bundle/Contents/MacOS/Euphoria"

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

cat > "$bundle/Contents/Info.plist" <<PLIST
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
  <string>11.0</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.music</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSHumanReadableCopyright</key>
  <string>Copyright 2026 The Euphoria contributors. GPL-3.0-or-later.</string>
</dict>
</plist>
PLIST

echo "Built $bundle"
