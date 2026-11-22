#!/bin/sh
# Copyright 2026 The Euphoria contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Installs the newest Euphoria release, the command line tool and the desktop
# app, for the current user:
#
#   curl -fsSL https://github.com/lvndry/euphoria/releases/latest/download/install.sh | sh
#
# Run with --help for the options. Every file is checked against the
# release's SHA256SUMS before anything is installed, and the files installed
# are listed in a manifest so that --uninstall removes exactly those. When
# the release carries minisign signatures, the files are verified against the
# release's minisign.pub, whose key is pinned in this script, so that a
# changed release and a changed public key do not pass each other off as the
# real ones. Releases without signatures are checked with the checksums only.


# The key that signs the release's files, pinned here so that a changed
# release and a changed minisign.pub would not pass each other off as the
# real ones. See the "Signing the release's files" section of
# docs/development/releasing.md.
minisign_pinned_key="RWTXpSIgOpSVBK7N301TWI11jjnnhB9peLGu/RNf5J7TcjJ44MQfNNr7"

set -eu

repository_slug="lvndry/euphoria"
releases_page="https://github.com/$repository_slug/releases"
source_guide="https://github.com/$repository_slug/blob/main/docs/getting-started/installation.md"
app_bundle_identifier="io.github.lvndry.euphoria"

install_prefix=${EUPHORIA_INSTALL_DIR:-$HOME/.local}
releases_api=${EUPHORIA_RELEASES_API:-https://api.github.com/repos/$repository_slug/releases}
download_base_override=${EUPHORIA_DOWNLOAD_BASE:-}
requested_version=${EUPHORIA_VERSION:-}

bin_dir="$install_prefix/bin"
lib_dir="$install_prefix/lib"
share_dir="$install_prefix/share"
state_dir="$share_dir/euphoria"
manifest="$state_dir/install-manifest.txt"

temporary_dir=""
mount_point=""

say() {
  printf '%s\n' "$*"
}

fail() {
  printf 'euphoria-install: error: %s\n' "$*" >&2
  exit 1
}

cleanup() {
  if [ -n "$mount_point" ]; then
    hdiutil detach -quiet "$mount_point" 2> /dev/null || hdiutil detach -quiet -force "$mount_point" 2> /dev/null || true
    mount_point=""
  fi
  if [ -n "$temporary_dir" ] && [ -d "$temporary_dir" ]; then
    rm -rf "$temporary_dir"
  fi
}

trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

usage() {
  cat << USAGE
Install Euphoria, the command line tool and the desktop app, for this user.

Usage: install.sh [--cli-only | --app-only] [--uninstall] [--help]

  --cli-only    Install only the euphoria command line tool.
  --app-only    Install only the desktop app.
  --uninstall   Remove every file an earlier run of this script installed,
                and nothing else.
  --help        Show this help.

Running the script again upgrades what it installed to the newest release.

What it installs:
  The command line tool   \$EUPHORIA_INSTALL_DIR/bin/euphoria and its libraries in
                          \$EUPHORIA_INSTALL_DIR/lib (default ~/.local).
  The app on macOS        Euphoria.app in /Applications when you can write
                          there without sudo, otherwise ~/Applications.
  The app on Linux        The AppImage as \$EUPHORIA_INSTALL_DIR/bin/euphoria-desktop,
                          with a launcher entry and icon in \$EUPHORIA_INSTALL_DIR/share.
  The list of files       \$EUPHORIA_INSTALL_DIR/share/euphoria/install-manifest.txt

Environment:
  EUPHORIA_VERSION        Install this version, such as 2026.11.5, instead of
                          the newest.
  EUPHORIA_INSTALL_DIR    Where the command line tool goes (default ~/.local).
                          Pass the same value to --uninstall.
  EUPHORIA_APP_DIR        Where Euphoria.app goes on macOS.
  EUPHORIA_RELEASES_API   The releases API to read (for testing).
  EUPHORIA_DOWNLOAD_BASE  A folder or URL holding the release files (for testing).

Supported: macOS on Apple Silicon and Linux on x86_64. For anything else,
see $releases_page or build from source: $source_guide
USAGE
}

want_cli=yes
want_app=yes
uninstall=no
for argument in "$@"; do
  case "$argument" in
    --cli-only) want_app=no ;;
    --app-only) want_cli=no ;;
    --uninstall) uninstall=yes ;;
    -h | --help)
      usage
      exit 0
      ;;
    *) fail "unknown option $argument (see --help)" ;;
  esac
done
if [ "$want_cli" = no ] && [ "$want_app" = no ]; then
  fail "--cli-only and --app-only cannot be used together"
fi

# Manifest lines are "<component> <absolute path>", component cli or app.

manifest_paths() {
  if [ -f "$manifest" ]; then
    sed -n "s/^$1 //p" "$manifest"
  fi
}

remove_installed_path() {
  case "$1" in
    /*) ;;
    *)
      say "Skipping $1 in the manifest: not an absolute path."
      return 0
      ;;
  esac
  case "$1" in
    *"/../"* | *"/..")
      say "Skipping $1 in the manifest: it climbs out of a folder."
      return 0
      ;;
  esac
  if [ -d "$1" ] && [ ! -L "$1" ]; then
    case "$1" in
      */Euphoria.app) rm -rf "$1" ;;
      *) say "Skipping $1 in the manifest: a folder this script does not install." ;;
    esac
  elif [ -e "$1" ] || [ -L "$1" ]; then
    rm -f "$1"
  fi
}

refresh_desktop_database() {
  if command -v update-desktop-database > /dev/null 2>&1 && [ -d "$share_dir/applications" ]; then
    update-desktop-database "$share_dir/applications" > /dev/null 2>&1 || true
  fi
}

if [ "$uninstall" = yes ]; then
  if [ ! -f "$manifest" ]; then
    fail "nothing to uninstall: $manifest does not exist. If you set EUPHORIA_INSTALL_DIR when installing, set it to the same folder now."
  fi
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    path=${line#* }
    remove_installed_path "$path"
    say "Removed $path"
  done < "$manifest"
  rm -f "$manifest"
  rmdir "$state_dir" "$share_dir/doc/euphoria" 2> /dev/null || true
  refresh_desktop_database
  say "Euphoria is uninstalled."
  exit 0
fi

# Platform

system=$(uname -s)
machine=$(uname -m)
case "$system" in
  Darwin)
    if [ "$machine" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2> /dev/null || echo 0)" = 1 ]; then
      machine=arm64
    fi
    case "$machine" in
      arm64 | aarch64) platform=macos-arm64 ;;
      x86_64) platform=macos-intel ;;
      *) platform=unsupported ;;
    esac
    ;;
  Linux)
    case "$machine" in
      x86_64 | amd64) platform=linux-x86_64 ;;
      *) platform=unsupported ;;
    esac
    ;;
  *) platform=unsupported ;;
esac
if [ "$platform" = unsupported ]; then
  fail "Euphoria has no release build for $system on $machine. Builds exist for macOS on Apple Silicon and Linux on x86_64. See $releases_page, or build from source: $source_guide"
fi

# Tools

if command -v curl > /dev/null 2>&1; then
  downloader=curl
elif command -v wget > /dev/null 2>&1; then
  downloader=wget
else
  fail "neither curl nor wget is installed; install one of them and run this again"
fi

if command -v sha256sum > /dev/null 2>&1; then
  checksum_tool="sha256sum"
elif command -v shasum > /dev/null 2>&1; then
  checksum_tool="shasum -a 256"
else
  fail "neither sha256sum nor shasum is installed, so the downloads cannot be checked"
fi

for required in tar mktemp sed awk; do
  command -v "$required" > /dev/null 2>&1 || fail "$required is not installed"
done
if [ "$platform" = macos-arm64 ] && [ "$want_app" = yes ]; then
  command -v hdiutil > /dev/null 2>&1 || fail "hdiutil is missing, so the disk image cannot be opened"
  command -v ditto > /dev/null 2>&1 || fail "ditto is missing, so the app cannot be copied"
fi

fetch() {
  if [ "$downloader" = curl ]; then
    curl --fail --silent --show-error --location --retry 3 --output "$2" "$1"
  else
    wget --quiet --tries=3 --output-document="$2" "$1"
  fi
}

temporary_dir=$(mktemp -d 2> /dev/null || mktemp -d -t euphoria-install) || fail "could not create a temporary folder"

# Release

requested_version=${requested_version#v}
if [ -n "$requested_version" ]; then
  release_url="$releases_api/tags/v$requested_version"
  release_label="release v$requested_version"
else
  release_url="$releases_api/latest"
  release_label="the latest release"
fi
release_json="$temporary_dir/release.json"
fetch "$release_url" "$release_json" || fail "could not read $release_label from $release_url. Check the version and your connection; see $releases_page"

tag=$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$release_json" | head -n 1)
[ -n "$tag" ] || fail "$release_url did not describe a release"
version=${tag#v}
tr ',' '\n' < "$release_json" | sed -n 's/.*"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]*\/\([^"/]*\)".*/\1/p' > "$temporary_dir/assets.txt"

has_asset() {
  grep -Fqx "$1" "$temporary_dir/assets.txt"
}

if [ "$platform" = macos-intel ]; then
  if grep -q 'x86_64-apple-darwin' "$temporary_dir/assets.txt"; then
    fail "Euphoria $version has a build for Intel Macs, but this script installs only the Apple Silicon one. Download it from $releases_page/tag/$tag"
  fi
  fail "Euphoria $version has no build for Intel Macs: its releases run on Apple Silicon only. You can build it from source: $source_guide"
fi

case "$platform" in
  macos-arm64)
    cli_asset="euphoria-$version-aarch64-apple-darwin.tar.gz"
    app_asset="Euphoria-$version.dmg"
    ;;
  linux-x86_64)
    cli_asset="euphoria-$version-x86_64-unknown-linux-gnu.tar.gz"
    app_asset="Euphoria-$version-x86_64.AppImage"
    ;;
esac

wanted_assets=""
[ "$want_cli" = yes ] && wanted_assets="$cli_asset"
[ "$want_app" = yes ] && wanted_assets="$wanted_assets $app_asset"
has_asset SHA256SUMS || fail "Euphoria $version has no SHA256SUMS, so its files cannot be checked; nothing was installed"
for asset in $wanted_assets; do
  has_asset "$asset" || fail "Euphoria $version has no $asset for this system; nothing was installed. See $releases_page/tag/$tag"
done

# Download and verify

if [ -n "$download_base_override" ]; then
  download_base=$download_base_override
else
  download_base="https://github.com/$repository_slug/releases/download/$tag"
fi
say "Downloading Euphoria $version"
downloads="$temporary_dir/downloads"
mkdir "$downloads"
for asset in SHA256SUMS $wanted_assets; do
  if ! fetch "$download_base/$asset" "$downloads/$asset"; then
    fail "could not download $download_base/$asset. Check your connection and try again"
  fi
done
for asset in $wanted_assets; do
  expected=$(awk -v name="$asset" '{ file = $2; sub(/^\*/, "", file); if (file == name) { print $1; exit } }' "$downloads/SHA256SUMS")
  [ -n "$expected" ] || fail "SHA256SUMS does not list $asset; nothing was installed"
  actual=$($checksum_tool "$downloads/$asset" | awk '{ print $1 }')
  if [ "$actual" != "$expected" ]; then
    fail "checksum mismatch for $asset (expected $expected, got $actual). The download is damaged or was tampered with; nothing was installed"
  fi
  say "Verified $asset"
done

# Minisign signatures complement the checksums: a checksum proves a file is
# the one the release says, and the signature proves the release says it,
# because the key that signs is pinned in this script, so that a changed
# release and a changed minisign.pub do not pass each other off as the real
# ones. The verification runs the minisign binary the release itself ships,
# which is checked against SHA256SUMS like every other file. A release made
# before the signatures exist goes without them, with one warning.

minisign_tool_name="minisign-0.12-linux-x86_64"
if has_asset minisign.pub && has_asset "$minisign_tool_name"; then
  for asset in minisign.pub "$minisign_tool_name"; do
    fetch "$download_base/$asset" "$downloads/$asset" || fail "could not download $download_base/$asset"
    expected=$(awk -v name="$asset" '{ file = $2; sub(/^\*/, "", file); if (file == name) { print $1; exit } }' "$downloads/SHA256SUMS")
    [ -n "$expected" ] || fail "SHA256SUMS does not list $asset; nothing was installed"
    actual=$($checksum_tool "$downloads/$asset" | awk '{ print $1 }')
    [ "$actual" = "$expected" ] || fail "checksum mismatch for $asset (expected $expected, got $actual). The download is damaged or was tampered with; nothing was installed"
  done
  # The pinned key: its line is in the public key the release carries, and
  # it is the one this script was written with.
  pub_key=$(awk -v name="$minisign_pinned_key" '$0 == name { found = 1 } END { print found }' "$downloads/minisign.pub")
  if [ "$pub_key" != 1 ]; then
    fail "minisign.pub does not hold the key this install script was written with ($minisign_pinned_key). The release was made by a different key; nothing was installed"
  fi
  chmod 755 "$downloads/$minisign_tool_name"
  say "Checking the signatures with minisign"
  for asset in $wanted_assets; do
    signature="$downloads/$asset.minisig"
    if ! fetch "$download_base/$asset.minisig" "$signature" 2> /dev/null; then
      fail "$asset has no signature, but the release carries minisign.pub; the release is damaged or incomplete; nothing was installed"
    fi
    if [ "$platform" = macos-arm64 ]; then
      # The binary is a Linux x86_64 build; on an Apple Silicon Mac it runs
      # under Rosetta 2, which those Macs carry.
      if ! arch -x86_64 "$downloads/$minisign_tool_name" -V -p "$downloads/minisign.pub" -x "$signature" -m "$downloads/$asset" 2> /dev/null; then
        "$downloads/$minisign_tool_name" -V -p "$downloads/minisign.pub" -x "$signature" -m "$downloads/$asset" 2> /dev/null ||
          fail "the signature of $asset does not match the release's key. The download was tampered with or the release was republished by a different key; nothing was installed"
      fi
    else
      "$downloads/$minisign_tool_name" -V -p "$downloads/minisign.pub" -x "$signature" -m "$downloads/$asset" 2> /dev/null ||
        fail "the signature of $asset does not match the release's key. The download was tampered with or the release was republished by a different key; nothing was installed"
    fi
    say "Signature OK for $asset"
  done
else
  echo "This release has no minisign signatures; its files are checked with their checksums only." >&2
fi


# Prepare everything before touching the install folders, so that a failure
# leaves the previous installation as it was.

new_manifest="$temporary_dir/manifest"
: > "$new_manifest"
manifest_paths cli > "$temporary_dir/old-cli"
manifest_paths app > "$temporary_dir/old-app"

conflicts=""
check_destination() {
  if { [ -e "$2" ] || [ -L "$2" ]; } && ! grep -Fqx "$2" "$temporary_dir/old-$1"; then
    conflicts="$conflicts
  $2"
  fi
}

copy_plan="$temporary_dir/copy-plan"
: > "$copy_plan"
plan_copy() {
  check_destination "$1" "$3"
  printf '%s\t%s\n' "$2" "$3" >> "$copy_plan"
  printf '%s %s\n' "$1" "$3" >> "$new_manifest"
}

if [ "$want_cli" = yes ]; then
  mkdir "$temporary_dir/cli"
  tar -xzf "$downloads/$cli_asset" -C "$temporary_dir/cli" || fail "could not unpack $cli_asset"
  cli_root="$temporary_dir/cli/${cli_asset%.tar.gz}"
  [ -x "$cli_root/bin/euphoria" ] || fail "$cli_asset does not hold bin/euphoria"
  plan_copy cli "$cli_root/bin/euphoria" "$bin_dir/euphoria"
  if [ -d "$cli_root/lib" ]; then
    for library in "$cli_root/lib"/* "$cli_root/lib"/.[!.]*; do
      [ -e "$library" ] || [ -L "$library" ] || continue
      [ -d "$library" ] && [ ! -L "$library" ] && fail "$cli_asset has a folder in lib/, which this script does not install"
      plan_copy cli "$library" "$lib_dir/$(basename "$library")"
    done
  fi
  for notice in "$cli_root/LICENSE" "$cli_root/licenses"/*; do
    [ -f "$notice" ] || continue
    plan_copy cli "$notice" "$share_dir/doc/euphoria/$(basename "$notice")"
  done
fi

if [ "$want_app" = yes ] && [ "$platform" = linux-x86_64 ]; then
  appimage="$downloads/$app_asset"
  chmod +x "$appimage"
  plan_copy app "$appimage" "$bin_dir/euphoria-desktop"
  icons_found=no
  (cd "$temporary_dir" && "$appimage" --appimage-extract 'usr/share/icons/*' > /dev/null 2>&1) || true
  for size in 256x256/apps/euphoria.png scalable/apps/euphoria.svg; do
    icon="$temporary_dir/squashfs-root/usr/share/icons/hicolor/$size"
    if [ -f "$icon" ]; then
      plan_copy app "$icon" "$share_dir/icons/hicolor/$size"
      icons_found=yes
    fi
  done
  if [ "$icons_found" = no ]; then
    say "Warning: could not extract the icon from the AppImage; the launcher entry will show a generic icon."
  fi
  escaped_exec=$(printf '%s' "$bin_dir/euphoria-desktop" | sed 's/[\\"`$]/\\&/g')
  desktop_entry="$temporary_dir/euphoria.desktop"
  cat > "$desktop_entry" << DESKTOP
[Desktop Entry]
Type=Application
Name=Euphoria
GenericName=Key and BPM Analyzer
Comment=Find the key and BPM of every track in your music collection
Exec="$escaped_exec"
Icon=euphoria
Terminal=false
Categories=AudioVideo;Audio;Music;
Keywords=DJ;key;BPM;Camelot;harmonic;mixing;
StartupWMClass=euphoria
DESKTOP
  plan_copy app "$desktop_entry" "$share_dir/applications/euphoria.desktop"
fi

if [ "$want_app" = yes ] && [ "$platform" = macos-arm64 ]; then
  if [ -n "${EUPHORIA_APP_DIR:-}" ]; then
    app_dir=$EUPHORIA_APP_DIR
  elif [ -d /Applications ] && [ -w /Applications ]; then
    app_dir=/Applications
  else
    app_dir="$HOME/Applications"
  fi
  app_destination="$app_dir/Euphoria.app"
  if [ -e "$app_destination" ] && ! grep -Fqx "$app_destination" "$temporary_dir/old-app"; then
    existing_identifier=$(defaults read "$app_destination/Contents/Info" CFBundleIdentifier 2> /dev/null || echo unknown)
    if [ "$existing_identifier" != "$app_bundle_identifier" ]; then
      fail "$app_destination exists and is not Euphoria ($existing_identifier); move it away and run this again. Nothing was installed"
    fi
  fi
  mount_point="$temporary_dir/disk-image"
  mkdir "$mount_point"
  hdiutil attach -quiet -readonly -nobrowse -noautoopen -mountpoint "$mount_point" "$downloads/$app_asset" < /dev/null \
    || {
      mount_point=""
      fail "could not open $app_asset"
    }
  [ -d "$mount_point/Euphoria.app" ] || fail "$app_asset does not hold Euphoria.app"
  printf 'app %s\n' "$app_destination" >> "$new_manifest"
fi

if [ -n "$conflicts" ]; then
  fail "these files exist and were not installed by this script:$conflicts
Move or delete them and run this again. Nothing was installed"
fi

# Install

install_file() {
  mkdir -p "$(dirname "$2")" || fail "could not create $(dirname "$2")"
  staged="$2.euphoria-install.$$"
  if [ -L "$1" ]; then
    rm -f "$staged"
    ln -s "$(readlink "$1")" "$staged" || fail "could not write $staged"
  else
    cp "$1" "$staged" || fail "could not write $staged"
  fi
  mv -f "$staged" "$2" || fail "could not write $2"
}

tab=$(printf '\t')
while IFS="$tab" read -r source destination; do
  install_file "$source" "$destination"
done < "$copy_plan"

if [ -n "$mount_point" ]; then
  mkdir -p "$app_dir" || fail "could not create $app_dir"
  staged_app="$app_dir/.Euphoria.app.euphoria-install.$$"
  ditto "$mount_point/Euphoria.app" "$staged_app" || {
    rm -rf "$staged_app"
    fail "could not copy Euphoria.app to $app_dir"
  }
  if [ -d "$app_destination" ]; then
    rm -rf "$app_destination"
  fi
  mv "$staged_app" "$app_destination" || fail "could not move Euphoria.app into $app_dir"
  hdiutil detach -quiet "$mount_point" 2> /dev/null || true
  mount_point=""
fi

# Files the previous run installed for the same component that this release
# no longer has, such as a library with an older version in its name.
for component in cli app; do
  if [ "$component" = cli ] && [ "$want_cli" = no ]; then
    sed -n 's/^/cli /p' "$temporary_dir/old-cli" >> "$new_manifest"
    continue
  fi
  if [ "$component" = app ] && [ "$want_app" = no ]; then
    sed -n 's/^/app /p' "$temporary_dir/old-app" >> "$new_manifest"
    continue
  fi
  while IFS= read -r old_path; do
    [ -n "$old_path" ] || continue
    if ! grep -Fqx "$component $old_path" "$new_manifest"; then
      remove_installed_path "$old_path"
    fi
  done < "$temporary_dir/old-$component"
done

mkdir -p "$state_dir" || fail "could not create $state_dir"
if ! cp "$new_manifest" "$manifest.new" || ! mv -f "$manifest.new" "$manifest"; then
  fail "could not write $manifest"
fi
if [ "$want_app" = yes ] && [ "$platform" = linux-x86_64 ]; then
  refresh_desktop_database
fi

# Report

say ""
if [ "$want_cli" = yes ]; then
  installed_version=$("$bin_dir/euphoria" --version) || fail "$bin_dir/euphoria was installed but does not run"
  say "Installed $installed_version in $bin_dir/euphoria"
fi
if [ "$want_app" = yes ]; then
  if [ "$platform" = macos-arm64 ]; then
    say "Installed the Euphoria $version app in $app_destination"
  else
    say "Installed the Euphoria $version app as $bin_dir/euphoria-desktop, with a launcher entry"
  fi
fi

case ":${PATH:-}:" in
  *":$bin_dir:"*) ;;
  *)
    if [ "$want_cli" = yes ]; then
      say ""
      say "$bin_dir is not on your PATH. To add it, run:"
      case "$(basename "${SHELL:-sh}")" in
        zsh) say "  echo 'export PATH=\"$bin_dir:\$PATH\"' >> ~/.zshrc" ;;
        bash)
          if [ "$system" = Darwin ]; then
            say "  echo 'export PATH=\"$bin_dir:\$PATH\"' >> ~/.bash_profile"
          else
            say "  echo 'export PATH=\"$bin_dir:\$PATH\"' >> ~/.bashrc"
          fi
          ;;
        fish) say "  fish_add_path '$bin_dir'" ;;
        *) say "  echo 'export PATH=\"$bin_dir:\$PATH\"' >> ~/.profile" ;;
      esac
      say "then open a new terminal."
    fi
    ;;
esac
uninstall_environment=""
if [ -n "${EUPHORIA_INSTALL_DIR:-}" ]; then
  uninstall_environment="EUPHORIA_INSTALL_DIR='$install_prefix' "
fi
say ""
say "To uninstall: curl -fsSL $releases_page/latest/download/install.sh | ${uninstall_environment}sh -s -- --uninstall"
