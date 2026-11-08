# Contributing to Cyclone

Thank you for helping improve Cyclone. This document is written for contributors who are new to the repo.

## Code of conduct

Be respectful and constructive in issues and PRs.

## Development setup

1. Install [Rust 1.85+](https://rustup.rs/). The repo pins the toolchain via `rust-toolchain.toml`.
2. Clone the repository and from the workspace root:

   ```bash
   cargo build
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   cargo fmt --all
   ```

3. Optional: `export RUST_LOG=cyclone=debug` when exercising the CLI.

## Project layout

- **`crates/cyclone-core`** — Library used by the CLI and extensions. No cloud SDKs.
- **`crates/cyclone`** — `clap`-based binary; keep it thin (parse args, call core).
- **`crates/cyclone-ext-*`** — Optional adapters implementing `cyclone_core::Extension`. `cyclone-ext-exe` talks to exe.dev via `POST /exec`; tests replay fixture HTTP under `crates/cyclone-ext-exe/tests/fixtures/` (no live credentials).
- **`fixtures/home`** — Fake `$HOME` for Linux CI (dotfiles, extra config trees, and opt-in exclusion placeholders; **no real secrets**). Used by the profiler scanner tests as well as capture/restore.
- **`fixtures/packages`** — Native package-manager dumps (Brewfile, apt, dnf, pacman, apk, zypper) used by capture/restore tests. **`fixtures/os-release`** — Sample `/etc/os-release` files so Linux distro detection is not hardcoded to Ubuntu.

## Pull request checklist

- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` pass locally.
- [ ] New behavior has tests in `cyclone-core` when possible (integration tests under `crates/cyclone-core/tests/`).
- [ ] Public API changes include rustdoc on types and functions.
- [ ] User-facing behavior changes are reflected in `README.md`.
- [ ] No secret values or real credentials in fixtures or snapshots.

## Design principles

1. **Explicit security** — Never log or persist secret values; dry-run before destructive restore.
2. **Core stays portable** — macOS + Linux only in core; cloud specifics live in extensions.
3. **Small diffs** — Prefer focused PRs over sweeping refactors.

## Commit messages

Use clear, imperative subjects (e.g. `Add restore --target-root flag`). Reference issues when applicable.

## Licensing

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
