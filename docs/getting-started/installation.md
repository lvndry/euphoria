---
title: Installation
description: Install Euphoria on macOS or Linux.
---

# Installation

On a Mac with Apple Silicon, or a 64-bit Intel or AMD PC running Linux, download the app and the command line tool from the [releases page](https://github.com/lvndry/euphoria/releases). To build the newest code, build from source, which takes a few minutes. Homebrew packages are on the way.

## Download for macOS

The app:

1. Download [Euphoria.dmg](https://github.com/lvndry/euphoria/releases/latest/download/Euphoria.dmg), the newest release. Every release is also on the releases page as `Euphoria-<version>.dmg`.
2. Open it and drag Euphoria onto Applications.
3. Open Euphoria from Applications the first time as described [below](#opening-the-app-for-the-first-time).

The app needs a Mac with Apple Silicon, and at least the macOS version the release notes give, which is the newest minimum of any binary in the app: macOS 15 for the current release.

The command line tool is in `euphoria-<version>-aarch64-apple-darwin.tar.gz` on the same page. It carries its own copy of libkeyfinder, so it needs nothing else:

```
tar -xzf euphoria-*-aarch64-apple-darwin.tar.gz
cd euphoria-*-aarch64-apple-darwin
./bin/euphoria --version
```

Keep `bin` and `lib` together, and add the `bin` folder to your `PATH` to run `euphoria` from anywhere.

A tarball downloaded with a browser is marked as coming from the internet, and macOS holds back a program from it that is not notarized, as the command line tool is not. A download made with `curl` is not marked, so the simplest way is:

```
curl -LO https://github.com/lvndry/euphoria/releases/download/v<version>/SHA256SUMS
curl -LO https://github.com/lvndry/euphoria/releases/download/v<version>/euphoria-<version>-aarch64-apple-darwin.tar.gz
shasum -a 256 --check --ignore-missing SHA256SUMS
```

The last command must print `OK` for the tarball before you unpack it: it proves the file is the one the release published. If you downloaded with a browser instead and macOS refuses to run the tool, first check the tarball against `SHA256SUMS` the same way. Then it is your choice whether to remove the internet mark from the unpacked files with `xattr -d com.apple.quarantine bin/euphoria lib/*.dylib`. That turns off macOS's check of these files only, and is safe only for files whose checksum you checked.

## Download for Linux

The Linux files are built for x86_64 and need glibc 2.35 or later, which Ubuntu 22.04, Debian 12, Fedora 36 and newer have; the release notes give the exact minimum of each release, read from the files themselves. `ldd --version` shows the glibc you have. They carry their own copy of libkeyfinder and FFTW, so nothing has to be installed first.

The app is an AppImage, one file that runs without installing:

```
curl -LO https://github.com/lvndry/euphoria/releases/latest/download/Euphoria-x86_64.AppImage
chmod +x Euphoria-x86_64.AppImage
./Euphoria-x86_64.AppImage
```

Every release is also on the releases page as `Euphoria-<version>-x86_64.AppImage`. The [AppImage runtime includes its FUSE library](https://github.com/AppImage/type2-runtime/blob/20251108/README.md). If FUSE mounting is unavailable or disallowed on your system, run it without FUSE: `./Euphoria-x86_64.AppImage --appimage-extract-and-run`.

The app also uses what every desktop provides: X11 or Wayland, Vulkan for drawing (`libvulkan1` and your graphics driver's Vulkan support, which desktop installs include), ALSA, which PulseAudio and PipeWire serve, for playback, and the XDG desktop portal (`xdg-desktop-portal` with the GTK, GNOME or KDE back end) for the folder picker and the save dialog. Without the portal, **Add Folder** says it cannot open the picker; dragging a folder onto the window still works.

To have the app in your launcher, move the AppImage where you keep programs, such as `~/Applications`, and let an AppImage integration tool such as Gear Lever or AppImageLauncher add it, or extract its launcher entry and icon yourself with Python 3:

```
./Euphoria-x86_64.AppImage --appimage-extract 'usr/share/*'
mkdir -p "$HOME/.local/share"
cp -r squashfs-root/usr/share/applications squashfs-root/usr/share/icons "$HOME/.local/share/"
python3 - <<'PY'
from pathlib import Path

launcher = Path.home() / ".local/share/applications/euphoria.desktop"
# The launcher expands HOME at runtime; its value stays one quoted path.
command = r'Exec=/bin/sh -c "exec \\"\\$HOME/Applications/Euphoria-x86_64.AppImage\\""'
lines = [command if line.startswith('Exec=') else line for line in launcher.read_text().splitlines()]
launcher.write_text('\n'.join(lines) + '\n')
PY
rm -r squashfs-root
```

The command line tool is in `euphoria-<version>-x86_64-unknown-linux-gnu.tar.gz` on the same page:

```
version=$(curl -sSfL -o /dev/null -w '%{url_effective}' https://github.com/lvndry/euphoria/releases/latest | sed 's|.*/tag/v||')
curl -LO "https://github.com/lvndry/euphoria/releases/download/v$version/SHA256SUMS"
curl -LO "https://github.com/lvndry/euphoria/releases/download/v$version/euphoria-$version-x86_64-unknown-linux-gnu.tar.gz"
sha256sum --check --ignore-missing SHA256SUMS
tar -xzf "euphoria-$version-x86_64-unknown-linux-gnu.tar.gz"
cd "euphoria-$version-x86_64-unknown-linux-gnu"
./bin/euphoria --version
```

The first line reads the newest version from the releases page; set `version` yourself, such as `version=2026.12.0`, for another one.

`sha256sum` must print `OK` for the tarball before you unpack it, and the same check works for the AppImage. Keep `bin` and `lib` together, and add the `bin` folder to your `PATH` to run `euphoria` from anywhere.

## Build from source on macOS

Install the build tools and libkeyfinder, the key detection library Euphoria uses, with [Homebrew](https://brew.sh):

```
brew install rust cmake libkeyfinder
```

Then build Euphoria:

```
git clone https://github.com/lvndry/euphoria.git
cd euphoria
cargo install --path crates/euphoria-cli
```

This puts the `euphoria` command in `~/.cargo/bin`, which the Rust installer adds to your `PATH`.

### The desktop app

From the same folder, build the app and a disk image to install it from, as the releases are built. This also needs [uv](https://docs.astral.sh/uv/), which lays out the disk image window, and the Xcode Command Line Tools (`xcode-select --install`, already there if Homebrew is), whose libclang generates the bindings of the time stretcher the decks use:

```
brew install uv
./scripts/bundle-macos.sh
```

It writes `target/release/bundle/Euphoria.app` and `target/release/bundle/Euphoria-<version>.dmg`. Open the disk image and drag Euphoria onto Applications, or copy the app directly:

```
cp -R target/release/bundle/Euphoria.app /Applications/
```

The app carries its own copy of libkeyfinder and FFTW, so it keeps working if you uninstall them from Homebrew, and you can copy it to another Mac. It needs the macOS version the script prints when it finishes: the newest minimum macOS of any binary in the bundle, which is also written into the app as `LSMinimumSystemVersion`. The libraries copied from Homebrew usually set it, because Homebrew builds them for the macOS it is installed on, so a bundle built on a newer Mac can need a newer macOS.

## Opening the app for the first time

The app is signed only with an ad hoc signature, not with an Apple Developer ID, and it is not notarized by Apple. An app you built yourself opens like any other. A downloaded app, or one built on another Mac, makes macOS say the first time that it cannot check the app for malicious software. To open it anyway:

1. In Finder, open Applications, right click Euphoria and choose **Open**.
2. Choose **Open** in the dialog.

If the dialog has no **Open** button, as on recent macOS versions, choose **Done**, then open **System Settings > Privacy & Security**, scroll to the message about Euphoria and choose **Open Anyway**. macOS remembers the choice, and the app opens normally from then on.

## Build from source on Debian and Ubuntu

These distributions do not package libkeyfinder, so a script in the repository builds it first. Install Rust with [rustup](https://rustup.rs), because the distribution's `cargo` is often older than Euphoria needs, then:

```
sudo apt-get install build-essential cmake pkg-config libfftw3-dev
git clone https://github.com/lvndry/euphoria.git
cd euphoria
./.github/scripts/install-libkeyfinder.sh
cargo install --path crates/euphoria-cli
```

### The desktop app on Linux

The app also needs the libraries its window toolkit, GPUI, links against for X11 and Wayland, ALSA for sound, and libclang:

```
sudo apt-get install libasound2-dev libfontconfig-dev libfreetype-dev libwayland-dev \
  libx11-xcb-dev libxkbcommon-x11-dev libclang-dev
cargo build --release -p euphoria
./target/release/euphoria-desktop
```

To build the AppImage the releases carry, which also needs `patchelf` and `librsvg2-bin`:

```
sudo apt-get install patchelf librsvg2-bin
./scripts/bundle-linux.sh
```

It writes `target/release/bundle/Euphoria-<version>-<architecture>.AppImage`. A build made on a newer distribution needs that distribution's glibc or newer; the script prints the minimum when it finishes.

On other distributions, install the same libraries under their names there (Fedora: `fftw-devel alsa-lib-devel fontconfig-devel freetype-devel wayland-devel libxcb-devel libxkbcommon-x11-devel clang-devel`).

## Check the installation

```
euphoria --version
```

Next: the [quick start](quick-start.md).
