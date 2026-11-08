//! Home / config discovery used by `cyclone init` and `cyclone profile --edit`.
//!
//! The scanner walks `$HOME` (top-level dotfiles and config-like names) and
//! `$HOME/.config` (immediate children). Sensitive paths are still returned so
//! the user can opt in; they are not included by default.

use std::collections::BTreeSet;
use std::path::Path;

use crate::error::Result;
use crate::paths::to_logical;
use crate::profile::{ComponentKind, OptInReason};

/// A path found under `$HOME` or `$HOME/.config`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanHit {
    /// Logical path (`~/...`) relative to the scanned home.
    pub logical: String,
    pub kind: ComponentKind,
    /// `None` for ordinary candidates; set for default exclusions.
    pub opt_in: Option<OptInReason>,
}

/// Well-known nested locations that are easy to miss with a top-level walk.
/// These are **opt-in exclusions**, not a hardcoded include list.
const NESTED_EXCLUSION_PATHS: &[(&str, OptInReason)] = &[
    (".local/share/keyrings", OptInReason::Keychain),
    ("Library/Keychains", OptInReason::Keychain),
    ("Library/Caches", OptInReason::LargeCache),
    (
        "Library/Application Support/Google/Chrome",
        OptInReason::BrowserData,
    ),
    (
        "Library/Application Support/Firefox",
        OptInReason::BrowserData,
    ),
    (
        "Library/Application Support/BraveSoftware",
        OptInReason::BrowserData,
    ),
];

const JUNK_NAMES: &[&str] = &[".DS_Store", ".localized", ".CFUserTextEncoding"];

const CONFIG_LIKE_NAMES: &[&str] = &["Brewfile", "brewfile"];

const BROWSER_NAMES: &[&str] = &[
    "google-chrome",
    "google-chrome-beta",
    "google-chrome-unstable",
    "chromium",
    "chromium-browser",
    "chrome",
    "BraveSoftware",
    "brave",
    "microsoft-edge",
    "microsoft-edge-beta",
    "microsoft-edge-dev",
    "firefox",
    ".mozilla",
    "mozilla",
    "opera",
    "vivaldi",
    "safari",
];

const KEYCHAIN_NAMES: &[&str] = &["keychain", "Keychains", "keyrings", ".gnupg", "gnupg"];

const LARGE_CACHE_NAMES: &[&str] = &[
    ".cache",
    "Cache",
    "GPUCache",
    "CachedData",
    "Code Cache",
    "node_modules",
    ".npm",
    ".yarn",
    ".pnpm-store",
    ".pnpm",
    ".Trash",
    "Trash",
    ".rustup",
    ".nvm",
    ".pyenv",
    ".rbenv",
    ".linuxbrew",
    ".cargo",
    ".local",
    ".git",
];

const SSH_CONFIG_NAMES: &[&str] = &[
    "config",
    "config.d",
    "known_hosts",
    "known_hosts.old",
    "authorized_keys",
];

/// Walk `$HOME` and `$HOME/.config` for dotfiles and config-like paths.
///
/// Does not recurse into the whole home tree. `.ssh` is expanded one level so
/// private keys can be opted into separately from `config` / `*.pub`.
pub fn scan_home(home: &Path) -> Result<Vec<ScanHit>> {
    let mut hits = Vec::new();
    let mut seen = BTreeSet::new();

    if home.is_dir() {
        scan_dir_entries(home, home, ScanMode::HomeTop, &mut hits, &mut seen)?;
    }

    let config = home.join(".config");
    if config.is_dir() {
        scan_dir_entries(
            home,
            &config,
            ScanMode::ConfigChildren,
            &mut hits,
            &mut seen,
        )?;
    }

    let ssh = home.join(".ssh");
    if ssh.is_dir() {
        scan_dir_entries(home, &ssh, ScanMode::SshChildren, &mut hits, &mut seen)?;
    }

    for (rel, reason) in NESTED_EXCLUSION_PATHS {
        let abs = home.join(rel);
        if abs.exists() {
            push_hit(home, &abs, Some(*reason), &mut hits, &mut seen);
        }
    }

    hits.sort_by(|a, b| a.logical.cmp(&b.logical));
    Ok(hits)
}

#[derive(Clone, Copy)]
enum ScanMode {
    HomeTop,
    ConfigChildren,
    SshChildren,
}

fn scan_dir_entries(
    home: &Path,
    dir: &Path,
    mode: ScanMode,
    hits: &mut Vec<ScanHit>,
    seen: &mut BTreeSet<String>,
) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.file_name();
        let name_s = name.to_string_lossy();
        if name_s == "." || name_s == ".." || is_junk(&name_s) {
            continue;
        }
        let path = entry.path();

        match mode {
            ScanMode::HomeTop => {
                if name_s == ".config" || name_s == ".ssh" {
                    // Walked separately so children become individual candidates.
                    continue;
                }
                if !(name_s.starts_with('.') || is_config_like_name(&name_s)) {
                    continue;
                }
            }
            ScanMode::ConfigChildren | ScanMode::SshChildren => {}
        }

        if !path.is_file() && !path.is_dir() {
            continue;
        }
        push_hit(home, &path, None, hits, seen);
    }
    Ok(())
}

fn push_hit(
    home: &Path,
    abs: &Path,
    forced: Option<OptInReason>,
    hits: &mut Vec<ScanHit>,
    seen: &mut BTreeSet<String>,
) {
    let logical = to_logical(abs, home);
    if !seen.insert(logical.clone()) {
        if let Some(reason) = forced {
            if let Some(existing) = hits.iter_mut().find(|h| h.logical == logical) {
                if existing.opt_in.is_none() {
                    existing.opt_in = Some(reason);
                }
            }
        }
        return;
    }
    let kind = kind_for(abs, &logical);
    let opt_in = forced.or_else(|| classify(&logical));
    hits.push(ScanHit {
        logical,
        kind,
        opt_in,
    });
}

fn kind_for(abs: &Path, logical: &str) -> ComponentKind {
    if abs.is_dir() {
        ComponentKind::ConfigTree
    } else if is_tool_manifest_name(logical) {
        ComponentKind::ToolManifest
    } else {
        ComponentKind::Dotfile
    }
}

fn is_tool_manifest_name(logical: &str) -> bool {
    let name = logical.rsplit('/').next().unwrap_or(logical);
    name.eq_ignore_ascii_case("Brewfile") || name.eq_ignore_ascii_case("packages.list")
}

/// Classify a logical path (`~/...` or relative) against the default exclusions.
pub fn classify(logical: &str) -> Option<OptInReason> {
    let rel = logical.strip_prefix("~/").unwrap_or(logical);
    let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }

    if parts[0] == ".ssh" {
        if parts.len() == 1 {
            return Some(OptInReason::SshPrivateKey);
        }
        let name = parts[parts.len() - 1];
        if is_ssh_public_or_config(name) {
            return None;
        }
        return Some(OptInReason::SshPrivateKey);
    }

    for part in &parts {
        if name_in(part, KEYCHAIN_NAMES) {
            return Some(OptInReason::Keychain);
        }
        if name_in(part, BROWSER_NAMES) {
            return Some(OptInReason::BrowserData);
        }
        if name_in(part, LARGE_CACHE_NAMES) {
            return Some(OptInReason::LargeCache);
        }
    }
    None
}

/// Stable kebab-case id derived from a logical path, unique within `used`.
pub(crate) fn component_id_for(logical: &str, used: &BTreeSet<String>) -> String {
    let rel = logical.strip_prefix("~/").unwrap_or(logical);
    let trimmed = if let Some(rest) = rel.strip_prefix(".config/") {
        rest
    } else if let Some(rest) = rel.strip_prefix('.') {
        rest
    } else {
        rel
    };
    let slug: String = trimmed
        .chars()
        .map(|c| match c {
            '/' | '.' | ' ' | '_' => '-',
            c => c,
        })
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>()
        .trim_matches('-')
        .to_ascii_lowercase();
    let base = if slug.is_empty() {
        "path".to_string()
    } else {
        slug
    };
    if !used.contains(&base) {
        return base;
    }
    for i in 2.. {
        let candidate = format!("{base}-{i}");
        if !used.contains(&candidate) {
            return candidate;
        }
    }
    base
}

/// True when `parent` is a strict ancestor of `child` (`~/a` vs `~/a/b`).
pub(crate) fn logical_is_prefix(parent: &str, child: &str) -> bool {
    let parent = parent.trim_end_matches('/');
    let child = child.trim_end_matches('/');
    child.starts_with(parent) && child.as_bytes().get(parent.len()) == Some(&b'/')
}

fn is_ssh_public_or_config(name: &str) -> bool {
    name.ends_with(".pub") || name_in(name, SSH_CONFIG_NAMES)
}

fn is_junk(name: &str) -> bool {
    JUNK_NAMES.iter().any(|n| name.eq_ignore_ascii_case(n))
}

fn is_config_like_name(name: &str) -> bool {
    CONFIG_LIKE_NAMES
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n))
}

fn name_in(part: &str, names: &[&str]) -> bool {
    names.iter().any(|n| part.eq_ignore_ascii_case(n))
}

/// Test helper: create a representative fake `$HOME` under `root`.
#[cfg(test)]
pub fn write_fixture_tree(root: &Path) -> std::io::Result<()> {
    write_tree(
        root,
        &[
            (".gitconfig", "[user]\n"),
            (".tmux.conf", "set -g mouse on\n"),
            (".zshrc", "export EDITOR=nvim\n"),
            (".ssh/config", "Host github.com\n"),
            (
                ".ssh/id_ed25519",
                "-----BEGIN OPENSSH PRIVATE KEY-----\nFIXTURE_SSH_PRIVATE_KEY_TOKEN\n-----END OPENSSH PRIVATE KEY-----\n",
            ),
            (".ssh/id_ed25519.pub", "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5 fixture\n"),
            (".config/nvim/init.lua", "-- nvim\n"),
            (".config/htop/htoprc", "sort_key=PERCENT_CPU\n"),
            (
                ".config/google-chrome/Default/Preferences",
                "{\"profile\":\"FIXTURE_BROWSER_TOKEN\"}\n",
            ),
            (".config/apt/packages.list", "ripgrep\n"),
            (".cache/excluded.txt", "FIXTURE_EXCLUDED_TOKEN\n"),
            (".gnupg/trustdb.gpg", "FIXTURE_KEYCHAIN_TOKEN\n"),
            (
                ".local/share/keyrings/login.keyring",
                "FIXTURE_KEYRING_TOKEN\n",
            ),
            ("Brewfile", "brew \"ripgrep\"\n"),
            ("Documents/notes.txt", "not a dotfile\n"),
        ],
    )
}

#[cfg(test)]
fn write_tree(root: &Path, files: &[(&str, &str)]) -> std::io::Result<()> {
    for (rel, body) in files {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, body)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn hit<'a>(hits: &'a [ScanHit], logical: &str) -> &'a ScanHit {
        hits.iter()
            .find(|h| h.logical == logical)
            .unwrap_or_else(|| panic!("missing {logical} in {hits:?}"))
    }

    #[test]
    fn classifies_default_exclusions() {
        assert_eq!(
            classify("~/.ssh/id_ed25519"),
            Some(OptInReason::SshPrivateKey)
        );
        assert_eq!(classify("~/.ssh/id_ed25519.pub"), None);
        assert_eq!(classify("~/.ssh/config"), None);
        assert_eq!(classify("~/.gnupg"), Some(OptInReason::Keychain));
        assert_eq!(
            classify("~/.config/google-chrome"),
            Some(OptInReason::BrowserData)
        );
        assert_eq!(classify("~/.cache"), Some(OptInReason::LargeCache));
        assert_eq!(classify("~/.config/nvim"), None);
        assert_eq!(classify("~/.gitconfig"), None);
    }

    #[test]
    fn scan_temp_home_lists_dotfiles_and_opt_in_exclusions() {
        let dir = tempdir().unwrap();
        write_fixture_tree(dir.path()).unwrap();
        let hits = scan_home(dir.path()).unwrap();

        assert_eq!(hit(&hits, "~/.tmux.conf").opt_in, None);
        assert_eq!(hit(&hits, "~/.gitconfig").opt_in, None);
        assert_eq!(hit(&hits, "~/.config/htop").kind, ComponentKind::ConfigTree);
        assert_eq!(hit(&hits, "~/Brewfile").kind, ComponentKind::ToolManifest);
        assert_eq!(hit(&hits, "~/.ssh/config").opt_in, None);
        assert_eq!(
            hit(&hits, "~/.ssh/id_ed25519").opt_in,
            Some(OptInReason::SshPrivateKey)
        );
        assert_eq!(
            hit(&hits, "~/.config/google-chrome").opt_in,
            Some(OptInReason::BrowserData)
        );
        assert_eq!(hit(&hits, "~/.cache").opt_in, Some(OptInReason::LargeCache));
        assert_eq!(hit(&hits, "~/.gnupg").opt_in, Some(OptInReason::Keychain));
        assert_eq!(
            hit(&hits, "~/.local/share/keyrings").opt_in,
            Some(OptInReason::Keychain)
        );

        assert!(hits.iter().all(|h| h.logical != "~/Documents"));
        assert!(hits.iter().all(|h| h.logical != "~/Documents/notes.txt"));
        assert!(hits.iter().all(|h| h.logical != "~/.config"));
        assert!(hits.iter().all(|h| h.logical != "~/.ssh"));
    }

    #[test]
    fn ids_prefer_last_config_segment() {
        let used = BTreeSet::new();
        assert_eq!(component_id_for("~/.gitconfig", &used), "gitconfig");
        assert_eq!(component_id_for("~/.config/nvim", &used), "nvim");
        assert_eq!(
            component_id_for("~/.ssh/id_ed25519", &used),
            "ssh-id-ed25519"
        );
        assert_eq!(component_id_for("~/.tmux.conf", &used), "tmux-conf");
    }
}
