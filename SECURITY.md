# Security Policy

## Supported versions

| Version | Supported |
|---------|-----------|
| `main` / latest release | Yes |

## Reporting a vulnerability

Please **do not** open public GitHub issues for security problems.

Email the maintainer with:

- Description of the issue and impact
- Steps to reproduce
- Affected version/commit

We will acknowledge receipt and work on a fix. We appreciate responsible disclosure.

## Scope

Cyclone handles local filesystem paths and user-supplied passphrases. Images are **encrypted at rest in full** (Argon2id + ChaCha20-Poly1305). Report issues involving:

- Secret material leaking into images, logs, `header.json`, or the git repository
- Cryptographic misuse in `cyclone-core` (KDF, AEAD, key lifetime)
- Unsafe restore behavior (overwrites outside declared targets)

Out of scope: third-party cloud providers implemented in extension crates unless the bug is in core’s extension API contract.

## Key handling

- Passphrases are provided with `--passphrase`, `--key-file`, or `CYCLONE_PASSPHRASE`.
- Cyclone does not escrow keys. An image without the user-held key cannot be decrypted.
- Do not commit passphrases, key files, or decrypted bundles.
- `cyclone provision --apply` reads `EXE_TOKEN` (exe.dev bearer token; the same name the official Python SDK uses). Cyclone does not log it, does not accept it as a CLI flag, and does not send `CYCLONE_PASSPHRASE` to exe.dev. The image is decrypted locally; only restored file bytes go to the VM you named.
