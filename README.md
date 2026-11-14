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

## Walkthrough — profile, capture on A, restore on B

This is the MVP loop: **profile → capture on machine A → copy the image → dry-run restore on machine B → apply**.

### Machine A (capture)

```bash
# 1. First-run profiler. Scans ~/.config and ~/ for dotfiles and config-like files,
#    then writes an editable profile (XDG: ~/.config/cyclone/profile.toml).
#    Non-interactive mode includes discovered paths that already exist
#    (SSH private keys, keychains, browser data, and large caches stay opt-in).
cyclone init --non-interactive

# Review or change capture scope (or open the TOML in an editor).
cyclone profile
# cyclone profile --edit          # re-run the profiler
# $EDITOR ~/.config/cyclone/profile.toml

# 2. See what would be captured (no image, no passphrase).
cyclone capture --output ./dev-box.cyclone --dry-run

# 3. Capture for real. The whole image is encrypted with a user-held key.
export CYCLONE_PASSPHRASE='choose-a-strong-secret'
cyclone capture --output ./dev-box.cyclone
```

Copy `dev-box.cyclone/` to machine B (`scp -r`, a USB drive, …). Registry `push`/`pull` is not in this release.

### Machine B (restore)

```bash
export CYCLONE_PASSPHRASE='choose-a-strong-secret'   # same key as on A

# Inspect requires the key. Secret *values* are never printed (names only).
cyclone inspect ./dev-box.cyclone

# Dry-run first: path mapping, package-manager diffs, missing brew/apt, OS mismatches.
cyclone restore ./dev-box.cyclone --dry-run

# Apply into a sandbox (recommended the first time), then into $HOME when the diff looks right.
cyclone restore ./dev-box.cyclone --target-root ./restored --overwrite
# cyclone restore ./dev-box.cyclone --overwrite
```

To preview a **Linux → macOS** (or the reverse) restore from either machine:

```bash
cyclone restore ./dev-box.cyclone --dry-run --assume-platform macos
# Distro/manager mismatches (e.g. a Brewfile on Ubuntu, or apt pins on Fedora):
cyclone restore ./dev-box.cyclone --dry-run --assume-platform linux --assume-distro fedora --assume-manager dnf
```

VS Code settings, for example, map `~/.config/Code/User/settings.json` (Linux) to `~/Library/Application Support/Code/User/settings.json` (macOS). Package declarations that do not match the destination manager are skipped with an **actionable diff** (package names that will not be installed, and whether to recapture on this distro).

A representative fixture home used in CI lives at [`fixtures/home`](fixtures/home). It contains **no real secrets**.

## Walkthrough — provision an exe.dev VM

Same encrypted image, different machine: decrypt locally, then bootstrap a VM. **Dry-run first** (that is also the default). Cloud HTTP lives in [`cyclone-ext-exe`](crates/cyclone-ext-exe); core stays SDK-free.

```bash
export CYCLONE_PASSPHRASE='choose-a-strong-secret'   # never sent to exe.dev

# Plan only: decrypt the image, print POST /exec commands, write nothing.
cyclone provision ./dev-box.cyclone --ext exe-dev --target my-vm
cyclone provision ./dev-box.cyclone --ext exe-dev --target my-vm --dry-run

# Apply: create or reuse the VM and restore Linux-mapped files under $HOME.
# Mint a token (ssh is not in exe.dev's default --cmds):
#   ssh exe.dev ssh-key generate-api-key --exp=30d --cmds=ls,new,ssh --label cyclone
export EXE_TOKEN='…'   # official env var used by the exe.dev Python SDK
cyclone provision ./dev-box.cyclone --ext exe-dev --target my-vm --apply
```

`EXE_TOKEN` is required only for `--apply`. There is no `--token` flag (argv shows up in `ps`). Optional `EXE_DEV_EXEC_URL` overrides `https://exe.dev/exec` for tests.

Details, documented API mapping, and 64KiB `/exec` limits: [`crates/cyclone-ext-exe/README.md`](crates/cyclone-ext-exe/README.md).

## Image format

An image is a **directory**. Every byte of user data is encrypted at rest. Plaintext images are not produced.

| File | Purpose |
|------|---------|
| `header.json` | Envelope only: magic, format version, Argon2id parameters, AEAD name. No manifest, paths, or file bytes. |
| `bundle.bin.enc` | ChaCha20-Poly1305 ciphertext of the full inner bundle (manifest **and** artifacts) |

Unlock with `--passphrase`, `--key-file`, or `CYCLONE_PASSPHRASE`. Argon2id salt is random per image (stored in the envelope). Inspect, restore, and provision all require the key.

## Commands

| Command | Description |
|---------|-------------|
| `init` | First-run profiler; scans `~/.config` and `~/`, writes editable `profile.toml` |
| `profile` | Print the saved profile; `--edit` re-scans and re-runs the profiler |
| `capture` | Build an encrypted image (`--dry-run`, `--packages live|files|auto`) |
| `inspect <path>` | Decrypt and summarize (secret names only) |
| `restore <path>` | Decrypt, write artifacts, and install matching package declarations (`--dry-run`, `--target-root`, `--overwrite`, `--assume-platform`, `--assume-distro`, `--assume-manager`, `--install-packages` / `--no-install-packages`) |
| `push` / `pull` | Registry transport — **not implemented**; copy the directory |
| `provision --ext exe-dev` | Bootstrap an exe.dev VM from the encrypted image (**dry-run by default**; `--apply` calls the API) |

Other options:

- `init` / `profile`: `--output <path>` writes the profile somewhere other than the XDG default; `init --force` overwrites an existing profile; `profile --edit --non-interactive` re-scans without prompts.
- `capture`: `--profile <path>` uses a profile other than the discovered one. `--packages` can also be set with `CYCLONE_PACKAGE_CAPTURE`.
- `init`, `profile`, `capture`, `restore`: `--home <dir>` (or `CYCLONE_HOME`) sets the directory treated as `~`.
- `capture`, `inspect`, `restore`, `provision`: `--passphrase` / `CYCLONE_PASSPHRASE` or `--key-file <path>` (trailing newline trimmed; not both).

## What is captured

`cyclone init` and `cyclone profile --edit` **scan** `$HOME/.config` and `$HOME` for dotfiles, config trees, and config-like files (for example `Brewfile`). Anything in that scan can be added to the profile — not only a built-in list. Interactive init prints the full candidate list; type component ids or paths to toggle/include. Non-interactive init includes ordinary discovered paths that already exist.

A small catalog still supplies stable ids and per-OS mappings for well-known tools:

| Component | Linux | macOS |
|-----------|-------|-------|
| `gitconfig`, `zshrc`, `bashrc`, `vimrc` | `~/.<file>` | same |
| `nvim` | `~/.config/nvim` | same |
| `cargo-config` | `~/.cargo/config.toml` | same |
| `vscode-settings` | `~/.config/Code/User/settings.json` | `~/Library/Application Support/Code/User/settings.json` |
| `brewfile` | live `brew list` (stored as `~/Brewfile` in the image) | same |
| `apt-packages` | live `apt-mark` / `dpkg-query` (stored as `~/.config/apt/packages.list`) | *(Linux only)* |
| `dnf-packages` | live `dnf repoquery --userinstalled` | *(Linux only)* |
| `pacman-packages` | live `pacman -Qe` / `-Qne` | *(Linux only)* |
| `yay-packages` | live `yay -Qme` | *(Linux / Arch AUR)* |
| `paru-packages` | live `paru -Qme` | *(Linux / Arch AUR)* |
| `apk-packages` | live `apk info` | *(Linux only)* |
| `zypper-packages` | live `zypper search --installed-only` | *(Linux only)* |

Extra files the scan finds (`.tmux.conf`, `~/.config/htop`, …) get generated ids and the same path on both OSes unless you edit the profile.

**Listed, but not captured unless you opt in:** SSH private keys, keychains, browser profiles, and large caches (`~/.cache`, `node_modules`, `~/.cargo`, …). They appear in the profiler as opt-in rows (and as `opt_in = "…"` in `profile.toml`) so you can add them on purpose.

## Package managers (supported vs warned)

Cyclone captures **declarations** by querying the live manager (`brew list`, `pacman -Qe`, …) — never installed binaries, and never a user-maintained `.list` file. Distro is read from `/etc/os-release`; Ubuntu is not assumed. Each manager keeps its own dump format and install command.

| Host | Capture | Restore |
|------|---------|---------|
| macOS with Homebrew | `brew list` / `brew tap` (or `brew bundle dump` if `list` is empty). A handwritten `~/Brewfile` is captured only if you opt it in as a **dotfile**. | `brew bundle --file …` |
| Debian/Ubuntu (`apt`) | `apt-mark showmanual` + `dpkg-query` | `sudo apt-get install -y …` |
| Fedora/RHEL (`dnf`) | `dnf repoquery --userinstalled` | `sudo dnf install -y …` |
| Arch (`pacman`) | Official-repo packages (`pacman -Qne` when an AUR helper is present, else `-Qe` / `-Q`) | `sudo pacman -S --needed --noconfirm …` |
| Arch + `yay` | AUR/foreign packages only (`yay -Qme`), stored apart from pacman | `yay -S --needed --noconfirm …` |
| Arch + `paru` | AUR/foreign packages only (`paru -Qme`), stored apart from pacman | `paru -S --needed --noconfirm …` |
| Alpine (`apk`) | `apk info` | `sudo apk add …` |
| openSUSE (`zypper`) | `zypper search --installed-only` | `sudo zypper --non-interactive install …` |

`cyclone init` includes a manager when its **command is on `PATH`**. You do not create `~/.config/pacman/packages.list` (or apt/dnf/yay/paru/zypper equivalents). Default `cyclone capture` is `--packages live`. `--packages files` reads profile paths only (air-gapped).

**Restore apply** runs the matching install command when you restore into `$HOME` (pass `--no-install-packages` to write the declaration file only). `--target-root` sandboxes files and does **not** install packages unless you also pass `--install-packages`.

**Warned (dry-run prints an actionable diff, nothing is installed):**

- Brewfile on a Linux host whose manager is apt/dnf/pacman/… (or the reverse)
- apt pins on Fedora, pacman dump on Ubuntu, Alpine world on Debian, …
- AUR lists (`yay` / `paru`) on a non-Arch distro, or on Arch without that helper on `PATH`
- Matching manager but `brew` / `apt-get` / … missing from `PATH`
- Component not declared for the destination OS (apt lists on macOS)

Images stay **encrypted on local disk**. `push`/`pull` is not in this release.

Secret **references** in the profile are names and providers only. Any secret *values* that happen to sit in a captured file live inside the encrypted payload, never in `header.json` or logs.

## Workspace

| Crate | Role |
|-------|------|
| [`cyclone`](crates/cyclone) | CLI |
| [`cyclone-core`](crates/cyclone-core) | Profiles, capture/restore, whole-image encryption, `Extension` trait |
| [`cyclone-ext-exe`](crates/cyclone-ext-exe) | exe.dev VM bootstrap (`--ext exe-dev`) |

Run `cargo test --workspace` and `cargo doc --workspace --open` from the repo root.

## Security model

- **Fail closed:** no passphrase → no encrypt, inspect, or restore (capture `--dry-run` is the exception).
- **Whole-image encryption:** manifest and file bytes are inside `bundle.bin.enc`.
- **User-held key:** derived with Argon2id; Cyclone does not upload or store it. Losing the passphrase means losing the image.
- **No inline secrets in git:** fixtures and examples use names and placeholder tokens only.

See [SECURITY.md](SECURITY.md) for reporting vulnerabilities.

## Contributing

We welcome issues and PRs. Read [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, CI expectations, and code guidelines.

## License

[MIT](LICENSE) — Copyright (c) 2026 lum1n
