---
title: Continuous integration
description: Which checks run on a pull request, which run only on main, and how to run all of them on a branch.
---

# Continuous integration

The **CI** workflow (`.github/workflows/ci.yml`) runs on every pull request, on every push to `main`, and by hand. A pull request gets the checks that catch most breakage in a few minutes. The integration tests, the Linux tests and packaging take longer, so they run on `main` and on manual runs only.

## On every pull request

| Check | What it runs |
|---|---|
| Formatting | `cargo fmt`, `taplo fmt` and the release and CI helper script tests |
| Dependency advisories and licenses | `cargo deny` and the checksums of the vendored C++ |
| Release and benchmark script tests | the Python tests in `scripts/` and `bench/` |
| Documentation links | relative links and headings in the docs |
| Workflow files | `actionlint` |
| Lint | Clippy on macOS, with and without the core's default features |
| Unit and headless UI tests on macOS | the unit tests of every crate and the desktop app's headless UI tests, from one build |
| Build and Clippy on Linux | a build of every crate and Clippy, which also lints the code only Linux compiles |
| Tests on macOS | passes when both macOS test jobs passed or were skipped |

A pull request that changes only documentation runs the documentation and workflow checks; the Rust jobs show as skipped.

## On main and on manual runs only

| Check | What it runs |
|---|---|
| Integration and packaging tests on macOS | the command line and core integration tests, which analyse generated audio end to end, the macOS library bundling tests, and the core without its default features |
| Tests on Linux | every crate's unit and integration tests except the desktop app's, the core without its default features, and the Linux library bundling tests |
| Desktop tests on Linux | the desktop app's unit and headless UI tests |
| macOS app and tarball with the ad hoc signature | the release build, the disk image and the command line tarball |
| Linux AppImage and tarball on a clean system | the packages, checked on clean systems |

These jobs show as skipped on a pull request, not as pending, so nothing waits for them. A break they catch shows up on the next run on `main`, and the [Release](releasing.md) workflow refuses to run until CI on `main` has passed.

## Running everything on a pull request

Start the workflow by hand on the branch, from **Actions > CI > Run workflow**, or with:

```
gh workflow run ci.yml --ref <branch>
```

A manual run treats every path as changed, so it runs every job, including packaging. Its result shows in the Actions tab and on the branch's commit, not in the pull request's list of checks.

## Speed

CI builds the tests with `opt-level = 1`, set by `CARGO_PROFILE_DEV_OPT_LEVEL` in the workflow. Many tests analyse audio end to end, which is many times slower without optimisation; debug assertions and overflow checks stay on. The same setting speeds up a local run:

```
CARGO_PROFILE_DEV_OPT_LEVEL=1 cargo test --workspace
```

Only runs on `main` save the Rust build cache. Pull requests restore `main`'s cache, so they reuse its compiled dependencies instead of filling the 10 GB cache limit with copies of them. A push to a pull request cancels the run of the push before it; runs on `main` are never cancelled.
