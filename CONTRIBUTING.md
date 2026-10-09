# Contributing to Jarvis

Thanks for helping out. This page covers how changes get from your machine into `main`.

## Before you start

For anything beyond a small fix, [open an issue](https://github.com/NexorPL/herdr-jarvis/issues) first so
we can agree on the change before you write it.

## Setup

- Rust from https://rustup.rs. The minimum supported version is `rust-version` in `Cargo.toml`.
- Enable the repo's git hooks:

  ```bash
  git config core.hooksPath .githooks
  ```

  The `pre-commit` hook refuses commits made directly on `main`, so work always happens on a branch.
- Point herdr at your checkout to try a local build:

  ```bash
  herdr plugin link "$(pwd)"
  ```

## Workflow

- One branch and one pull request per change. Name branches after the change: `feat/...`, `fix/...`,
  `docs/...`.
- Write commits as [Conventional Commits](https://www.conventionalcommits.org) with a short imperative
  subject: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`. Example: `fix: keep the startup
  hook's pipe out of detached children`.
- `main` changes only through pull requests, merged with a merge commit (no squash, no rebase) once CI
  passes on Linux, macOS and Windows.
- Only the maintainer merges.

## Checks to run locally

CI runs these three commands on all three operating systems. Run them before you push:

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

## Testing a change in herdr

Build and reopen Jarvis:

```bash
cargo build --release
herdr plugin action invoke jarvis.open
```

On Windows the running collector keeps `target/release/jarvis.exe` locked. Move it, and
`target/release/deps/jarvis.exe` (the same file under another name), aside before building; Windows
allows renaming a running executable. Alternatively run `scripts/fetch-or-build.ps1`, which moves both
aside for you. Note that the script first tries to download the release binary for the version in
`Cargo.toml` and builds from source only when there is no such release, so it does not always test
your local code.

If your PR changes the UI or how Jarvis talks to herdr, say in the PR what you checked by hand and on
which OS.

## Privacy

Commits in a public repository show your git email. To keep your address private, use your GitHub
noreply address for this repo:

```bash
git config user.email <id>+<user>@users.noreply.github.com
```

You can find it under GitHub Settings > Emails.

## Releases

For maintainers:

1. Bump `version` in `Cargo.toml`, `Cargo.lock` and `herdr-plugin.toml`, and merge that through a PR.
2. Tag the merge commit on `main` as `vX.Y.Z` and push the tag.

Pushing a `v*` tag runs `.github/workflows/release.yml`. It builds release binaries for Linux (x86_64
and aarch64, musl), macOS (x86_64 and aarch64) and Windows (x86_64), writes a `SHA256SUMS` file and
publishes them all to a GitHub release for that tag.

The version bump matters: the install step (`scripts/fetch-or-build.sh` and `.ps1`) downloads the
release binary named after the version in `Cargo.toml`. If code changes without a new version, users
keep getting the old binary.
