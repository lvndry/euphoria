---
title: Installation
description: Install Euphoria on macOS or Linux.
---

# Installation

Homebrew packages are on the way. Until then, Euphoria is built from source, which takes a few minutes.

## macOS

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

From the same folder, build the app and a disk image to install it from. This also needs [uv](https://docs.astral.sh/uv/), which lays out the disk image window:

```
brew install uv
./scripts/bundle-macos.sh
```

It writes `target/release/bundle/Euphoria.app` and `target/release/bundle/Euphoria-<version>.dmg`. Open the disk image and drag Euphoria onto Applications, or copy the app directly:

```
cp -R target/release/bundle/Euphoria.app /Applications/
```

The app carries its own copy of libkeyfinder and FFTW, so it keeps working if you uninstall them from Homebrew, and you can copy it to another Mac. It needs the macOS version the script prints when it finishes: the newest minimum macOS of any binary in the bundle, which is also written into the app as `LSMinimumSystemVersion`. The libraries copied from Homebrew usually set it, because Homebrew builds them for the macOS it is installed on, so a bundle built on a newer Mac can need a newer macOS.

### Opening the app for the first time on another Mac

The app is signed only with an ad hoc signature, not with an Apple Developer ID, and it is not notarized by Apple. On the Mac that built it, it opens like any other app. On another Mac, the first time you open it, macOS says it cannot check the app for malicious software. To open it anyway:

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
