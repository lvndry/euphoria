---
title: Installation
description: Install Euphoria on macOS or Linux.
---

# Installation

On a Mac with Apple Silicon, install the app and the command line tool with [the install script](#install-with-the-install-script) or [Homebrew](#install-with-homebrew), or download them from the [releases page](https://github.com/lvndry/euphoria/releases). On a 64-bit Intel or AMD PC running Linux, use the install script or download the app and the command line tool from the releases page. To build the newest code, build from source, which takes a few minutes.

## Install with the install script

One command installs the newest release, both the command line tool and the app, for your user only, without `sudo`:

```
curl -fsSL https://github.com/lvndry/euphoria/releases/latest/download/install.sh | sh
```

The script is attached to every release, so this URL gives the script of the newest release, which passed CI with it. The same script is also at `https://raw.githubusercontent.com/lvndry/euphoria/main/scripts/install.sh`, which follows main. To read it before running it, download it first:

```
curl -fsSLO https://github.com/lvndry/euphoria/releases/latest/download/install.sh
sh install.sh --help
sh install.sh
```

It works on a Mac with Apple Silicon and on Linux on x86_64, and stops with a message on anything else. It checks every file it downloads against the release's `SHA256SUMS` and installs nothing if one does not match. It puts:

| What | macOS | Linux |
| --- | --- | --- |
| The command line tool | `~/.local/bin/euphoria`, its libraries in `~/.local/lib` | the same |
| The app | `Euphoria.app` in `/Applications` when you can write there without `sudo`, otherwise in `~/Applications`, copied from the notarized disk image | the AppImage as `~/.local/bin/euphoria-desktop`, with a launcher entry in `~/.local/share/applications` and its icon in `~/.local/share/icons` |
| The list of installed files | `~/.local/share/euphoria/install-manifest.txt` | the same |

If `~/.local/bin` is not on your `PATH`, the script prints the line to add to your shell's startup file. It does not edit that file itself.

Options:

- `--cli-only` installs only the command line tool, `--app-only` only the app. Without either, it installs both.
- `EUPHORIA_VERSION=2026.11.5` installs that version instead of the newest: `curl -fsSL <url> | EUPHORIA_VERSION=2026.11.5 sh`.
- `EUPHORIA_INSTALL_DIR` installs the command line tool somewhere else than `~/.local`, such as `EUPHORIA_INSTALL_DIR=/opt/euphoria`.

Running it again upgrades what it installed to the newest release. It refuses to overwrite a file in those folders that it did not install, such as a tool you unpacked there by hand, and tells you which.

To uninstall, run it with `--uninstall`. It removes exactly the files listed in its manifest and nothing else, and needs the same `EUPHORIA_INSTALL_DIR` if you set one:

```
curl -fsSL https://github.com/lvndry/euphoria/releases/latest/download/install.sh | sh -s -- --uninstall
```

Your library, analysis cache and settings are not touched.

## Install with Homebrew

Add the Euphoria tap once:

```
brew tap lvndry/homebrew-euphoria
```

Then install the command line tool, for a Mac with Apple Silicon:

```
brew install euphoria
```

and the app:

```
brew install --cask euphoria
```

The formula also installs tab completion for bash, zsh and fish. Without Homebrew, see [`euphoria completions`](../reference/cli.md#euphoria-completions).

The tap, [lvndry/homebrew-euphoria](https://github.com/lvndry/homebrew-euphoria), is updated after each release. Releases are signed with a Developer ID and notarized by Apple, so Homebrew installs and upgrades open without a warning. Only releases made before notarization need the steps [below](#opening-the-app-for-the-first-time).

## Download for macOS

The app:

1. Download [Euphoria.dmg](https://github.com/lvndry/euphoria/releases/latest/download/Euphoria.dmg), the newest release. Every release is also on the releases page as `Euphoria-<version>.dmg`.
2. Open it and drag Euphoria onto Applications.
3. Open Euphoria from Applications. Releases are notarized by Apple, so it opens without a warning. [Builds from before notarization and copies you built yourself](#opening-the-app-for-the-first-time) need one extra step.

The app needs a Mac with Apple Silicon, and at least the macOS version the release notes give, which is the newest minimum of any binary in the app: macOS 15 for the current release.

The command line tool is in `euphoria-<version>-aarch64-apple-darwin.tar.gz` on the same page. It needs nothing else:

```
tar -xzf euphoria-*-aarch64-apple-darwin.tar.gz
cd euphoria-*-aarch64-apple-darwin
./bin/euphoria --version
```

Keep `bin` and `lib` together, and add the `bin` folder to your `PATH` to run `euphoria` from anywhere.

The files of the tarball are signed with a Developer ID and notarized, but a command line tool cannot carry the notarization ticket inside it, so the first time macOS runs a file downloaded with a browser, it checks Apple's service online and needs an internet connection. Releases made before notarization are not notarized at all, and macOS holds back their tool. A download made with `curl` is not marked as coming from the internet, so the simplest way is:

```
curl -LO https://github.com/lvndry/euphoria/releases/download/v<version>/SHA256SUMS
curl -LO https://github.com/lvndry/euphoria/releases/download/v<version>/euphoria-<version>-aarch64-apple-darwin.tar.gz
shasum -a 256 --check --ignore-missing SHA256SUMS
```

The last command must print `OK` for the tarball before you unpack it: it proves the file is the one the release published. If you downloaded with a browser instead and macOS refuses to run the tool, first check the tarball against `SHA256SUMS` the same way. Then it is your choice whether to remove the internet mark from the unpacked files with `xattr -d com.apple.quarantine bin/euphoria lib/*.dylib`. That turns off macOS's check of these files only, and is safe only for files whose checksum you checked.

## Download for Linux

The Linux files are built for x86_64 and need glibc 2.35 or later, which Ubuntu 22.04, Debian 12, Fedora 36 and newer have; the release notes give the exact minimum of each release, read from the files themselves. `ldd --version` shows the glibc you have. They carry their own copy of the libraries they need beyond the base system, so nothing has to be installed first.

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

Install Rust with [Homebrew](https://brew.sh), and the Xcode Command Line Tools (`xcode-select --install`, already there if Homebrew is) for the C++ compiler:

```
brew install rust
```

Then build Euphoria:

```
git clone https://github.com/lvndry/euphoria.git
cd euphoria
cargo install --path apps/euphoria-cli
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

The app needs no library from Homebrew, so you can copy it to another Mac. It needs the macOS version the script prints when it finishes: the newest minimum macOS of any binary in the bundle, which is also written into the app as `LSMinimumSystemVersion`.

## Opening the app for the first time

These steps apply only to releases made before notarization and to copies you built yourself without a Developer ID, which carry an ad hoc signature and are not notarized by Apple. Current releases from GitHub or Homebrew open without them. An app you built yourself opens like any other on the Mac that built it. Elsewhere, macOS says the first time that it cannot check the app for malicious software. To open it anyway:

1. In Finder, open Applications, right click Euphoria and choose **Open**.
2. Choose **Open** in the dialog.

If the dialog has no **Open** button, as on recent macOS versions, choose **Done**, then open **System Settings > Privacy & Security**, scroll to the message about Euphoria and choose **Open Anyway**. macOS remembers the choice, and the app opens normally from then on.

## Build from source on Debian and Ubuntu

Install Rust with [rustup](https://rustup.rs), because the distribution's `cargo` is often older than Euphoria needs, then:

```
sudo apt-get install build-essential
git clone https://github.com/lvndry/euphoria.git
cd euphoria
cargo install --path apps/euphoria-cli
```

### The desktop app on Linux

The app also needs the libraries its window toolkit, GPUI, links against for X11 and Wayland, ALSA for sound, and libclang:

```
sudo apt-get install libasound2-dev libfontconfig-dev libfreetype-dev libwayland-dev \
  libx11-xcb-dev libxkbcommon-x11-dev libclang-dev
cargo build --release -p euphoria-desktop
./target/release/euphoria-desktop
```

To build the AppImage the releases carry, which also needs `patchelf` and `librsvg2-bin`:

```
sudo apt-get install patchelf librsvg2-bin
./scripts/bundle-linux.sh
```

It writes `target/release/bundle/Euphoria-<version>-<architecture>.AppImage`. A build made on a newer distribution needs that distribution's glibc or newer; the script prints the minimum when it finishes.

On other distributions, install the same libraries under their names there (Fedora: `alsa-lib-devel fontconfig-devel freetype-devel wayland-devel libxcb-devel libxkbcommon-x11-devel clang-devel`).

## Check the installation

```
euphoria --version
```

Next: the [quick start](quick-start.md).
