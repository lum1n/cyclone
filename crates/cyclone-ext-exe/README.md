# cyclone-ext-exe

Bootstrap an [exe.dev](https://exe.dev) VM from an **encrypted** Cyclone image.

This crate implements `cyclone_core::Extension` (`plan` / `apply`). It is the only crate that talks to exe.dev. `cyclone-core` stays cloud-SDK-free.

CLI id: **`exe-dev`**.

```bash
# Decrypt locally and print the plan (default; no exe.dev calls).
export CYCLONE_PASSPHRASE='choose-a-strong-secret'
cyclone provision ./dev-box.cyclone --ext exe-dev --target my-vm
cyclone provision ./dev-box.cyclone --ext exe-dev --target my-vm --dry-run

# Create or reuse the VM and restore files onto it.
export EXE_TOKEN='…'   # see below; never commit this
cyclone provision ./dev-box.cyclone --ext exe-dev --target my-vm --apply
```

## Environment variables

| Variable | Required | Purpose |
|----------|----------|---------|
| `CYCLONE_PASSPHRASE` | yes (or `--passphrase` / `--key-file`) | User-held image key. Used **only on this machine** to decrypt `bundle.bin.enc`. Never sent to exe.dev. |
| `EXE_TOKEN` | `--apply` only | Bearer token for `POST https://exe.dev/exec`. This is the **official** name used by the [exe.dev Python SDK](https://exe.dev/deploy/python-sandboxes) (`exedev.Client()` reads `EXE_TOKEN`). |
| `EXE_DEV_EXEC_URL` | no | Override the exec URL. Default `https://exe.dev/exec`. For tests. |

There is **no `--token` flag** so the secret does not appear in `ps`.

### Mint a token

Public docs: [HTTPS API](https://exe.dev/docs/https-api). Default token `--cmds` do **not** include `ssh`, which provision needs in order to write files.

```bash
ssh exe.dev ssh-key generate-api-key --exp=30d --cmds=ls,new,ssh --label cyclone
export EXE_TOKEN='…'   # printed token; starts with `exe1.`
```

Cyclone never generates, stores, or logs this value.

## What `--apply` does

1. Decrypt the image locally (Argon2id + ChaCha20-Poly1305).
2. `POST /exec` body `ls --json <target>` — reuse the VM if it is already running.
3. If missing: `new --name <target> --tag cyclone --json`.
4. For each Linux-mapped file, `ssh <target> 'python3 -c "…"'` writes bytes under `$HOME` (chunked base64).

A **paused** VM is left alone; resume it yourself (`ssh exe.dev resume <name>`) and retry.

exe.dev VMs are Linux (exeuntu). macOS-only components are listed as skipped. Tool manifests (Brewfile, apt lists) are copied as files; Cyclone does not run `brew` or `apt-get` on the VM.

## Honest limits (from public docs)

- `POST https://exe.dev/exec` has **no stdin**, a **64KiB** body, and a **30s** timeout. Large files are split across several `ssh` calls.
- `new --json` field names are only shown partially in public docs. The client accepts the documented `ls` VM object (`vm_name`, `https_url`, `ssh_dest`, `status`) plus obvious aliases (`name`, `url`).
- Tests replay **fixture** HTTP (see `tests/fixtures/`). They do not call the live API and contain no real tokens.

## Library

```rust,ignore
use cyclone_core::{ApplyOptions, Extension};
use cyclone_ext_exe::{ExeDevClient, ExeDevExtension};

let ext = ExeDevExtension::from_env()?;          // reads EXE_TOKEN
let plan = ext.plan(&inner_bundle, &target)?;
let result = ext.apply(&inner_bundle, &target, &ApplyOptions { dry_run: true })?;
```
