---
title: Releasing
description: How a release of Euphoria is numbered, built and published.
---

# Releasing

A release is one button: the **Release** workflow builds the macOS app and the command line tool on GitHub's runners, tags the commit and publishes a GitHub Release with the files. Only the repository owner can run it, and only from `main`.

## Version numbers

Euphoria uses calendar versions, `YEAR.MONTH.N`:

- `YEAR` and `MONTH` are the year and month in UTC when the release is made.
- `N` counts the releases made in that month, from 0.
- No part has a leading zero.

| Release | Version | Tag |
| --- | --- | --- |
| First of November 2026 | `2026.11.0` | `v2026.11.0` |
| Second of November 2026 | `2026.11.1` | `v2026.11.1` |
| First of December 2026 | `2026.12.0` | `v2026.12.0` |
| First of January 2027 | `2027.1.0` | `v2027.1.0` |

A version says when it was made, not how much changed, so no one has to decide how big a release is. It is still three dot separated numbers, which keeps it valid where versions are checked: Cargo reads it as semver, macOS accepts it as `CFBundleShortVersionString` and `CFBundleVersion` in the app's `Info.plist`, and versions sort in release order when compared number by number.

The workflow works the number out itself from the date and the existing tags, with `scripts/release/next_version.py`. It refuses to run if the newest tag is from a later month than the current one, which would mean a clock or a tag is wrong.

`main` keeps the version `0.1.0` in its Cargo manifests. The release version is set only in the commit each tag points at, described below.

## Cut a release

1. Check that the last CI run on `main` passed. The workflow checks this too and stops if it has not.
2. On GitHub, open **Actions > Release** and choose **Run workflow** on `main`. There is nothing to fill in.
3. Wait about twenty minutes. The release appears under **Releases** with its notes and files.

## What the workflow does

1. **Check and choose the version** (Linux runner). Stops unless it was started by the owner on `main`, and unless CI passed on that commit. Works out the next version and the previous tag, and stops if the tag exists already.
2. **Build for macOS** (`macos-15`, Apple Silicon). Sets the version in every crate's `Cargo.toml`, in the workspace's dependency on `euphoria-core` and in `Cargo.lock`, with `scripts/release/set_version.py`, and checks that `Cargo.lock` still matches. Then it builds:
   - `Euphoria-<version>.dmg`, with `scripts/bundle-macos.sh`.
   - `euphoria-<version>-aarch64-apple-darwin.tar.gz`, with `scripts/release/package-cli.sh`: the command line tool in `bin/`, libkeyfinder and FFTW in `lib/`, the licence and the README. It runs from wherever it is unpacked, and a Homebrew formula can install `bin/` and `lib/` as they are.
   - `Euphoria.dmg`, the same disk image under a name without the version.
   - `SHA256SUMS`, the SHA-256 checksum of each file.
3. **Tag and publish** (Linux runner). Makes the commit `chore(release): <version>` with the same version changes on top of the commit that was built, and checks that its files are exactly those that were built. Tags it `v<version>`, pushes the tag, and publishes the GitHub Release with the files and notes, marked as the latest release.

The release commit is not pushed to `main`. `main` is protected by a ruleset that requires a pull request for every change, which the workflow's token cannot bypass, and a release should not need one. The tag is not covered by that ruleset, so the workflow pushes only the tag, and the commit is reachable through it. The ruleset needs no change.

## When a run fails

Run the workflow again. Nothing has to be cleaned up first:

- A run that failed before pushing the tag left nothing behind, and the next run releases the same version.
- A run that failed after pushing the tag, for example while uploading the files, left a tag without a release, or a release without all its files. The next run sees that the newest tag is unfinished and finishes it instead of starting a new version. It checks that the tag points at a commit `chore(release): <version>` with exactly one parent, that the parent is a commit of `main` on which CI passed, and that the tagged files are exactly the parent's files with the version set. Then it rebuilds that version from the parent, checks the build against the tag, and publishes it, completing a release that is missing files. The tag is never moved or pushed again, so a version that was downloaded once always means the same files.
- A tag that fails those checks stops the run with the reason. It was not made by this workflow, and the owner decides what to do with it.

A release that is finished in a later month keeps its version: it is the version of the tag.

`scripts/release/plan_release.py` makes this choice, and `python3 -m unittest discover -s scripts/release -p 'test_*.py'` tests it together with the version, the manifest changes and the release notes.

## Release notes

`scripts/release/release_notes.py` writes the notes from the commit messages since the previous tag, which follow Conventional Commits. It groups them under Features (`feat`), Fixes (`fix`), Performance (`perf`) and Documentation (`docs`), and lists breaking changes first: those marked with `!` after the type, or with a `BREAKING CHANGE:` footer, whose text is shown under the subject. Each line links its pull request. Commits that make no difference to someone using Euphoria (`chore`, `ci`, `test`, `refactor`, `style`, `build`) are left out. The notes start with how to install the app.

To see the notes for the next release before running the workflow:

```
python3 scripts/release/release_notes.py --version 2026.11.1 --since v2026.11.0 --until origin/main
```

## Download links

Every release has these files:

```
https://github.com/lvndry/euphoria/releases/download/v<version>/Euphoria-<version>.dmg
https://github.com/lvndry/euphoria/releases/download/v<version>/euphoria-<version>-aarch64-apple-darwin.tar.gz
https://github.com/lvndry/euphoria/releases/download/v<version>/SHA256SUMS
```

This link always leads to the disk image of the newest release, for the README and a website:

```
https://github.com/lvndry/euphoria/releases/latest/download/Euphoria.dmg
```

## Apple Silicon only

The release is built for Apple Silicon. A build for Intel Macs would need libkeyfinder and FFTW built for Intel too, which Homebrew on the Apple Silicon runners does not provide, and a universal binary would need both. Apple no longer sells Intel Macs. The app needs at least the macOS version of the runner, because Homebrew builds its libraries for the macOS it runs on; the release notes and the app's `Info.plist` say which.

## Not done yet

- **Notarization.** Without it, macOS shows a warning the first time the app is opened on a Mac other than the one that built it, and the user has to choose Open from the right click menu. Notarizing needs a paid Apple Developer Program membership, a Developer ID Application certificate and an app specific password or API key, stored as repository secrets. The workflow would then sign with that certificate and the hardened runtime, submit the disk image with `xcrun notarytool` and staple the result.
- **Homebrew.** A cask for the app and a formula for the command line tool need a tap repository, such as `lvndry/homebrew-euphoria`, which is the owner's call. The formula can install the tarball's `bin/` and `lib/`. The workflow would then update the tap after each release, with a token that can push to it.
