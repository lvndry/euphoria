---
title: Installation
description: Install Euphoria on macOS or Linux.
---

# Installation

On a Mac with Apple Silicon, download the app and the command line tool from the [releases page](https://github.com/lvndry/euphoria/releases). On Linux, or to build the newest code, build from source, which takes a few minutes. Homebrew packages are on the way.

## Download for macOS

The app:

1. Download [Euphoria.dmg](https://github.com/lvndry/euphoria/releases/latest/download/Euphoria.dmg), the newest release. Every release is also on the releases page as `Euphoria-<version>.dmg`.
2. Open it and drag Euphoria onto Applications.
3. Open Euphoria from Applications the first time as described [below](#opening-the-app-for-the-first-time).

The app needs a Mac with Apple Silicon, and at least the macOS version the release notes give, which is the newest minimum of any binary in the app.

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

From the same folder, build the app and a disk image to install it from, as the releases are built. This also needs [uv](https://docs.astral.sh/uv/), which lays out the disk image window:

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

## Debian and Ubuntu

These distributions do not package libkeyfinder, so a script in the repository builds it first:

```
sudo apt-get install cargo cmake pkg-config libfftw3-dev
git clone https://github.com/lvndry/euphoria.git
cd euphoria
./.github/scripts/install-libkeyfinder.sh
cargo install --path crates/euphoria-cli
```

## Check the installation

```
euphoria --version
```

Next: the [quick start](quick-start.md).
