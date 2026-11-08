# Cyclone

[![CI](https://github.com/lum1n/cyclone/actions/workflows/ci.yml/badge.svg)](https://github.com/lum1n/cyclone/actions/workflows/ci.yml)

Cyclone captures a **declared developer setup** (dotfiles, config trees, tooling metadata) into a **portable, encrypted image** you can inspect, restore, and (via extensions) use to bootstrap remote environments.

**Supported hosts:** macOS and Linux. The core library is cloud-SDK-free; optional `cyclone-ext-*` crates add VM provisioning.

## Install

From this repository (crates are not yet published to crates.io):

```bash
cargo install --path crates/cyclone
```

Requirements: **Rust 1.85+** (see `rust-toolchain.toml`).

## Quick start

```bash
# 1. Build a profile (interactive, or --non-interactive for scripts)
cyclone profile

# 2. Dry-run capture (no writes, no passphrase)
cyclone capture --output ./my-image --dry-run

# 3. Capture for real — passphrase required
export CYCLONE_PASSPHRASE='choose-a-strong-secret'
cyclone capture --output ./my-image

# 4. Inspect metadata (secret values are never shown)
cyclone inspect ./my-image

# 5. Restore — dry-run first, then apply
cyclone restore ./my-image --dry-run
cyclone restore ./my-image --target-root ./restored --overwrite
```

Profiles are stored at the XDG config path, typically `~/.config/cyclone/profile.toml`. Edit that file to add capture roots, tool pins, and secret **references** (names only).

## Image format

An image is a directory:

| File | Purpose |
|------|---------|
| `manifest.json` | Schema version, host platform, component hashes, secret reference names, warnings |
| `payload.bin.enc` | ChaCha20-Poly1305 ciphertext of JSON [`CapturedPayload`](crates/cyclone-core/src/artifacts.rs) |
| `encryption.salt` | Salt hex (reserved for future KDF upgrades; passphrase KDF is documented in code) |

The manifest is intentionally readable so `cyclone inspect` works without your key. **File contents live only in the encrypted payload.**

## Commands

| Command | Description |
|---------|-------------|
| `profile` | First-run profiler; writes editable `profile.toml` |
| `capture` | Build an image from the profile (`--dry-run`) |
| `inspect <path>` | Summarize manifest (redacted secrets) |
| `restore <path>` | Decrypt and write artifacts (`--dry-run`, `--target-root`, `--overwrite`) |
| `provision` | Plan/apply VM bootstrap via an extension (`--ext exe`) |

## Workspace

| Crate | Role |
|-------|------|
| [`cyclone`](crates/cyclone) | CLI |
| [`cyclone-core`](crates/cyclone-core) | Profiles, capture/restore, crypto, `Extension` trait |
| [`cyclone-ext-exe`](crates/cyclone-ext-exe) | exe.dev provisioning adapter (plan/apply stubs today) |

Run `cargo doc --workspace --open` for API documentation.

## Security model

- **Fail closed:** no passphrase → no encrypt/decrypt (except capture/restore dry-run).
- **No inline secrets:** manifests list secret names and providers, not values.
- **User-held key:** derive from `CYCLONE_PASSPHRASE`; Cyclone does not upload or store it.

See [SECURITY.md](SECURITY.md) for reporting vulnerabilities.

## Contributing

We welcome issues and PRs. Read [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, CI expectations, and code guidelines.

## License

[MIT](LICENSE) — Copyright (c) 2026 lum1n
