//! Scanner coverage against the repository fixture `$HOME`.

use std::path::PathBuf;

use cyclone_core::{discover_defaults, scan_home, OptInReason, Platform};

fn fixture_home() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/home")
        .canonicalize()
        .expect("fixtures/home")
}

#[test]
fn fixture_home_scan_offers_more_than_catalog() {
    let home = fixture_home();
    let hits = scan_home(&home).expect("scan");

    assert!(
        hits.iter().any(|h| h.logical == "~/.tmux.conf"),
        "scan should list non-catalog dotfiles: {hits:?}"
    );
    assert!(hits.iter().any(|h| h.logical == "~/.config/htop"));
    assert!(hits.iter().any(|h| h.logical == "~/.gitconfig"));
    assert!(hits.iter().any(|h| h.logical == "~/.config/nvim"));
    assert!(hits.iter().any(|h| h.logical == "~/Brewfile"));
    assert!(hits.iter().all(|h| h.logical != "~/Documents"));
    assert!(hits.iter().all(|h| h.logical != "~/Documents/notes.txt"));
}

#[test]
fn fixture_home_exclusions_are_visible_as_opt_in() {
    let home = fixture_home();
    let hits = scan_home(&home).expect("scan");

    let key = hits
        .iter()
        .find(|h| h.logical == "~/.ssh/id_ed25519")
        .expect("ssh private key should still be listed");
    assert_eq!(key.opt_in, Some(OptInReason::SshPrivateKey));

    assert_eq!(
        hits.iter()
            .find(|h| h.logical == "~/.ssh/config")
            .expect("ssh config")
            .opt_in,
        None
    );

    let chrome = hits
        .iter()
        .find(|h| h.logical == "~/.config/google-chrome")
        .expect("browser data should still be listed");
    assert_eq!(chrome.opt_in, Some(OptInReason::BrowserData));

    let cache = hits
        .iter()
        .find(|h| h.logical == "~/.cache")
        .expect("cache should still be listed");
    assert_eq!(cache.opt_in, Some(OptInReason::LargeCache));

    let gnupg = hits
        .iter()
        .find(|h| h.logical == "~/.gnupg")
        .expect("keychain dir should still be listed");
    assert_eq!(gnupg.opt_in, Some(OptInReason::Keychain));

    let keyring = hits
        .iter()
        .find(|h| h.logical == "~/.local/share/keyrings")
        .expect("nested keyring should still be listed");
    assert_eq!(keyring.opt_in, Some(OptInReason::Keychain));
}

#[test]
fn fixture_home_defaults_include_scan_hits_not_exclusions() {
    let home = fixture_home();
    let profile = discover_defaults(&home, Platform::Linux).expect("profile");

    let included: Vec<&str> = profile
        .included()
        .filter_map(|c| c.linux.as_deref())
        .collect();
    assert!(included.contains(&"~/.tmux.conf"));
    assert!(included.contains(&"~/.config/htop"));
    assert!(included.contains(&"~/.gitconfig"));
    assert!(included.contains(&"~/.ssh/config"));
    assert!(
        !included.contains(&"~/.ssh/id_ed25519"),
        "private keys must not be included by default: {included:?}"
    );
    assert!(!included.contains(&"~/.config/google-chrome"));
    assert!(!included.contains(&"~/.cache"));
    assert!(!included.contains(&"~/.gnupg"));
    assert!(!included.contains(&"~/.local/share/keyrings"));
    assert!(!included.contains(&"~/Documents/notes.txt"));

    let key = profile
        .components
        .iter()
        .find(|c| c.linux.as_deref() == Some("~/.ssh/id_ed25519"))
        .expect("key still in profile as opt-in");
    assert!(!key.include);
    assert_eq!(key.opt_in, Some(OptInReason::SshPrivateKey));
}
