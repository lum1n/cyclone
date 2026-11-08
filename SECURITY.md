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

Cyclone handles local filesystem paths and user-supplied passphrases. Report issues involving:

- Secret material leaking into images, logs, or manifests
- Cryptographic misuse in `cyclone-core`
- Unsafe restore behavior (overwrites outside declared targets)

Out of scope: third-party cloud providers implemented in extension crates unless the bug is in core’s extension API contract.
