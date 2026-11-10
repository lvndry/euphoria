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
