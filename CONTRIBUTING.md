# Contributing to Euphoria

Thanks for helping. Bug reports, wrong analysis results and pull requests are all welcome.

## Reporting a wrong key or tempo

These reports are the most useful thing you can send. Open an issue with the "Wrong analysis result" template and include:

- the track (artist, title, and a link to a store page or a free preview if there is one),
- what Euphoria reported,
- what you expected, and how you know (by ear, a published key, another tool),
- the Euphoria version.

Please do not attach copyrighted audio.

## Setting up

You need a Rust toolchain (1.88 or newer), CMake, pkg-config and libkeyfinder.

macOS:

```
brew install rust cmake libkeyfinder
```

Debian and Ubuntu, which do not package libkeyfinder, so the script builds it from source:

```
sudo apt-get install cargo cmake pkg-config libfftw3-dev
./.github/scripts/install-libkeyfinder.sh
```

Then:

```
cargo test --workspace
```

## Before opening a pull request

Run the same checks as CI:

```
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI also checks TOML formatting with [taplo](https://taplo.tamasfe.dev) and dependencies with [cargo-deny](https://github.com/EmbarkStudios/cargo-deny). You can run both locally with `taplo fmt --check` and `cargo deny check`.

If your change affects key or tempo accuracy, run the benchmark before and after and put both tables in the pull request:

```
./bench/download_datasets.sh
cargo run --release -p euphoria-bench
```

## Pull request guidelines

- Give the pull request a descriptive title in the form `type(scope): summary`, for example `fix(core): handle mono AIFF files` or `feat(cli): add --json output`. Types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `chore`.
- Keep each pull request to one change. Large changes are easier to review as a short series.
- Add tests for new behaviour and for bugs you fix.
- Add a line to the Unreleased section of [CHANGELOG.md](CHANGELOG.md) for anything a user would notice.
- Sign your commits. GitHub explains how in [Signing commits](https://docs.github.com/en/authentication/managing-commit-signature-verification/signing-commits).
- Open the pull request as a draft while it is still in progress.

## Code conventions

- Public items in `euphoria-core` are documented.
- Every `unsafe` block has a `// SAFETY:` comment explaining why it is sound.
- Errors are returned, not swallowed. A batch reports which files failed and why.
- `euphoria-core` has no user interface code and prints nothing. See [ARCHITECTURE.md](ARCHITECTURE.md).

## License

By contributing, you agree that your contributions are licensed under GPL-3.0-or-later, the license of this project.
