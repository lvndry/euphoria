---
title: Releasing
description: How a release of Euphoria is numbered, built and published.
---

# Releasing

A release is one button: the **Release** workflow builds the macOS and Linux apps and command line tools on GitHub's runners, tags the commit and publishes a GitHub Release with the files. Only the repository owner can run it, and only from `main`.

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
   - `euphoria-<version>-aarch64-apple-darwin.tar.gz`, with `scripts/release/package-cli.sh`: the command line tool in `bin/`, any library it links from outside macOS in `lib/` (none today), the licence, the licences of the vendored Signalsmith libraries and of LAME in `licenses/`, and the README. It runs from wherever it is unpacked, and a Homebrew formula can install `bin/` and `lib/` as they are.
   - `Euphoria.dmg`, the same disk image under a name without the version.
   - `SHA256SUMS`, the SHA-256 checksum of each file.
3. **Build for Linux** (`ubuntu-22.04`, x86_64), at the same time as the macOS build. Sets the version the same way, installs the system libraries and ONNX Runtime with `.github/scripts/install-linux-dependencies.sh`, and builds:
   - `Euphoria-<version>-x86_64.AppImage`, with `scripts/bundle-linux.sh`.
   - `euphoria-<version>-x86_64-unknown-linux-gnu.tar.gz`, with `scripts/release/package-cli.sh`: the same layout as the macOS tarball, with the run path `$ORIGIN/../lib`.
   - `Euphoria-x86_64.AppImage`, the same AppImage under a name without the version.

   It then runs `scripts/release/check-linux-packages.sh`, which analyzes a test file with the command line tool in bare Ubuntu 22.04 and Fedora containers and starts the AppImage in an Ubuntu 22.04 container that has only the libraries every desktop has, under a virtual X server, where its window must appear. It reads the oldest glibc the files run on from their symbol versions, for the release notes, and writes the checksums of its files. The runner is the oldest Ubuntu LTS GitHub offers, because a Linux program needs at least the glibc it was built against.
4. **Tag and publish** (Linux runner). Checks that both builds were made from the same files. Makes the commit `chore(release): <version>` with the same version changes on top of the commit that was built, and checks that its files are exactly those that were built. Tags it `v<version>`, pushes the tag, checks each build's files against its checksums, joins the checksums into one `SHA256SUMS`, and publishes the GitHub Release with the files and notes, marked as the latest release.
5. **Publish to Homebrew** (Linux runner), from `Publish to Homebrew`. Regenerates the formula and the cask of the release with `scripts/release/publish_homebrew.py`, which downloads each file and checks it against the `SHA256SUMS` the release published, and pushes them to the tap [lvndry/homebrew-euphoria](https://github.com/lvndry/homebrew-euphoria) with the workflow's token, which writes to every repository of the same owner. The tap is public, because brew fetches taps over public https. A failed run of this job leaves the release published; run `Publish to Homebrew` again by hand with the version, and the tap catches up, or a later release overwrites both files.

The release commit is not pushed to `main`. `main` is protected by a ruleset that requires a pull request for every change, which the workflow's token cannot bypass, and a release should not need one. The tag is not covered by that ruleset, so the workflow pushes only the tag, and the commit is reachable through it. The ruleset needs no change.

## When a run fails

Run the workflow again. Nothing has to be cleaned up first:

- A run that failed before pushing the tag left nothing behind, and the next run releases the same version.
- A run that failed after pushing the tag, for example while uploading the files, left a tag without a release, or a release without all its files. The next run sees that the newest tag is unfinished and finishes it instead of starting a new version. It checks that the tag points at a commit `chore(release): <version>` with exactly one parent, that the parent is a commit of `main` on which CI passed, and that the tagged files are exactly the parent's files with the version set. Then it rebuilds that version from the parent, checks the build against the tag, and publishes it, completing a release that is missing files. The tag is never moved or pushed again, so a version that was downloaded once always means the same files.
- A tag that fails those checks stops the run with the reason. It was not made by this workflow, and the owner decides what to do with it.

A release that is finished in a later month keeps its version: it is the version of the tag.

A release is finished with the files its tag can build. A tag whose files hold `scripts/bundle-linux.sh` needs the Linux files too; an older one, such as `v2026.11.0`, is complete with the macOS files, and finishing it skips the Linux build.

`scripts/release/plan_release.py` makes this choice, and `python3 -m unittest discover -s scripts/release -p 'test_*.py'` tests it together with the version, the manifest changes and the release notes.

## Release notes

`scripts/release/release_notes.py` writes the notes from the commit messages since the previous tag, which follow Conventional Commits. It groups them under Features (`feat`), Fixes (`fix`), Performance (`perf`) and Documentation (`docs`), and lists breaking changes first: those marked with `!` after the type, or with a `BREAKING CHANGE:` footer, whose text is shown under the subject. Each line links its pull request. Commits that make no difference to someone using Euphoria (`chore`, `ci`, `test`, `refactor`, `style`, `build`) are left out. The notes start with how to install the app.

To see the notes for the next release before running the workflow:

```
python3 scripts/release/release_notes.py --version 2026.11.1 --since v2026.11.0 --until origin/main
```

## Download links

Every release has these files (the two Linux ones from the first release after 2026.11.0):

```
https://github.com/lvndry/euphoria/releases/download/v<version>/Euphoria-<version>.dmg
https://github.com/lvndry/euphoria/releases/download/v<version>/euphoria-<version>-aarch64-apple-darwin.tar.gz
https://github.com/lvndry/euphoria/releases/download/v<version>/Euphoria-<version>-x86_64.AppImage
https://github.com/lvndry/euphoria/releases/download/v<version>/euphoria-<version>-x86_64-unknown-linux-gnu.tar.gz
https://github.com/lvndry/euphoria/releases/download/v<version>/SHA256SUMS
```

These links always lead to the disk image and the AppImage of the newest release, for the README and a website:

```
https://github.com/lvndry/euphoria/releases/latest/download/Euphoria.dmg
https://github.com/lvndry/euphoria/releases/latest/download/Euphoria-x86_64.AppImage
```

## Linux on x86_64 only

The Linux files are built for x86_64. An arm64 build would work the same way, since `scripts/bundle-linux.sh` pins the AppImage tools for both, but needs an arm64 runner in the workflow. The files need the glibc of Ubuntu 22.04, the release runner, or a newer one; the release notes give the exact version, which `scripts/linux_libraries.py minimum-glibc` reads from the symbol versions the files ask for.

## Apple Silicon only

The release is built for Apple Silicon. Apple no longer sells Intel Macs. The release notes and the app's `Info.plist` give the oldest macOS it runs on.

## Signing and notarization

The macOS job signs the app, the disk image and the command line tool with a Developer ID Application certificate and the hardened runtime, and has Apple notarize them. A notarized app opens without the "Apple could not verify" warning, also after a Homebrew install or upgrade.

When the six secrets below are set, the job:

1. Imports the certificate into a temporary keychain with a random password, which an `if: always()` step deletes at the end, together with the notary key. It fails unless the certificate holds exactly one Developer ID Application identity, with the team in `APPLE_TEAM_ID`.
2. Signs every Mach-O file in the app from the inside out with `--options runtime --timestamp`: the dylibs in `Contents/Frameworks` first, deepest folders first, then the app (`scripts/macos_sign.py`; `scripts/test_macos_sign.py` tests the order and flags).
3. Submits the app to `xcrun notarytool submit --wait` and staples the ticket, so the app inside the disk image is stapled. Then builds the disk image, signs it, notarizes it and staples it (`scripts/release/notarize-macos.sh`). The job fails unless Apple answers Accepted.
4. Signs `bin/euphoria` and `lib/*.dylib` of the command line tarball the same way and submits a zip of `bin/` and `lib/`. **Command line files cannot be stapled**: Gatekeeper looks up the ticket at Apple the first time it runs a downloaded file, so the first run needs an internet connection. The `@executable_path/../lib` and `@rpath` loading is unchanged by signing, because the run path is written before signing and the job runs the unpacked tool afterwards.
5. Verifies, and fails the job if any check fails: `codesign --verify --deep --strict` on the app, `spctl -a -vvv -t install` on the disk image, `spctl -a -vvv` and `stapler validate` on the app inside it, and the hardened runtime flag on every signed file.
6. Only then computes `SHA256SUMS`, over the final stapled disk image and the signed tarball. `publish_homebrew.py` hashes the files it downloads and checks them against `SHA256SUMS`, so the tap needs no change.

Without the secrets, as in a fork, the job prints a notice naming the missing ones and signs ad hoc, as before. The release notes then say that the app is not notarized.

### When signing or notarization fails

A release is never held back by Apple. If the Developer ID path fails in the macOS job, for instance because the Apple Developer Program membership lapsed, an agreement is unsigned, the certificate was revoked or notarytool rejects a file, the job prints a `::warning::`, deletes everything it built, and builds the app, the disk image and the tarball again with the ad hoc signature. The release notes then say "This release is not notarized: macOS will ask you to confirm the first time you open it", so no Developer ID signed app is ever left in an un-notarized disk image that claims otherwise. A real problem, such as a binary that fails `codesign --verify` after the ad hoc signing, fails the job as before.

Notarizing needs an active Apple Developer Program membership at the time of every release, and renewing it is yours to do. Releases notarized earlier keep working after the membership lapses.

### Entitlements

**The app: none.** It is signed with the hardened runtime and no entitlements file, because nothing needs an exception:

- No JIT and no unsigned executable memory: ONNX Runtime is linked statically and runs its kernels as compiled code, and nothing generates code at run time.
- Library validation passes: the app links no library from outside macOS, and any dylib a future build bundles is re-signed with the same Developer ID as the app, so it is loaded by a binary of the same team. The Homebrew cask copies the app as it is.
- No audio input: the app only plays audio, so `com.apple.security.device.audio-input` is not needed.
- No App Sandbox: it is not distributed through the Mac App Store.

**The command line tool: `com.apple.security.cs.disable-library-validation`**, from `scripts/release/cli-entitlements.plist`. Homebrew rewrites the load paths of the tarball's libraries when it installs them and signs them again ad hoc, so they no longer carry Euphoria's team. Under the hardened runtime, library validation then refuses to load them and `euphoria` exits at once with "different Team IDs". 2026.11.2 shipped without the entitlement and its Homebrew command line tool did not start. The libraries themselves keep their Developer ID signature in the tarball, and the tool is still notarized.

### Secrets

Set these repository secrets (Settings > Secrets and variables > Actions, or `gh secret set NAME < file`). Names are exact.

| Secret | Holds |
| --- | --- |
| `APPLE_DEVELOPER_ID_P12_BASE64` | the Developer ID Application certificate with its private key, as a base64 encoded `.p12` |
| `APPLE_DEVELOPER_ID_P12_PASSWORD` | the password chosen when exporting the `.p12` |
| `APPLE_TEAM_ID` | the 10 character Team ID |
| `APPLE_NOTARY_KEY_P8_BASE64` | the App Store Connect API key `.p8`, base64 encoded |
| `APPLE_NOTARY_KEY_ID` | the key's ID |
| `APPLE_NOTARY_ISSUER_ID` | the issuer ID of the App Store Connect team |

Step by step:

1. **Certificate.** In Xcode, open Settings > Accounts, choose the team, Manage Certificates, and add a **Developer ID Application** certificate. Or on developer.apple.com, Certificates, Identifiers & Profiles > Certificates > +, choose **Developer ID Application**, upload a certificate signing request made with Keychain Access and install the downloaded certificate. Only the Account Holder can create it. The private key stays on the Mac that made the request.
2. **Export.** In Keychain Access, under My Certificates, select "Developer ID Application: Name (TEAMID)" with its private key below it, choose File > Export Items and save a `.p12` with a strong password.
3. **Encode and store.**
   ```
   base64 -i DeveloperID.p12 -o DeveloperID.p12.base64
   gh secret set APPLE_DEVELOPER_ID_P12_BASE64 < DeveloperID.p12.base64
   gh secret set APPLE_DEVELOPER_ID_P12_PASSWORD
   ```
   The second command asks for the password. Delete the local `.p12` and `.base64` files afterwards, or keep them in a password manager.
4. **Team ID.** developer.apple.com > Account > Membership details, Team ID. `gh secret set APPLE_TEAM_ID`.
5. **Notary key.** On appstoreconnect.apple.com, open Users and Access > Integrations > App Store Connect API > Team Keys, choose Generate API Key (or +), name it, give it the **Developer** role and generate. Download the `AuthKey_<KeyID>.p8` now: Apple offers it once. The Key ID is in the key's row and the Issuer ID at the top of the page.
   ```
   base64 -i AuthKey_XXXXXXXXXX.p8 -o AuthKey.p8.base64
   gh secret set APPLE_NOTARY_KEY_P8_BASE64 < AuthKey.p8.base64
   gh secret set APPLE_NOTARY_KEY_ID
   gh secret set APPLE_NOTARY_ISSUER_ID
   ```

The Developer ID certificate has its own expiry date, which can come sooner than the usual five years. Read it with `openssl pkcs12 -in DeveloperID.p12 -nokeys -legacy | openssl x509 -noout -enddate`, or in Keychain Access. Before it expires, make a new certificate the same way and replace `APPLE_DEVELOPER_ID_P12_BASE64` and `APPLE_DEVELOPER_ID_P12_PASSWORD`. Releases signed before stay valid after it expires, because each signature carries a secure timestamp. Revoking the API key does not affect notarization tickets already issued.

## Not done yet
- **Homebrew.** A cask for the app and a formula for the command line tool need a tap repository, such as `lvndry/homebrew-euphoria`, which is the owner's call. The formula can install the tarball's `bin/` and `lib/`. The workflow would then update the tap after each release, with a token that can push to it.
