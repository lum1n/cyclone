use std::path::{Path, PathBuf};

use cyclone_core::{
    capture, inspect, restore, CaptureOptions, ImageBundle, Platform, Profile, RestoreActionKind,
    RestoreOptions,
};
use tempfile::tempdir;

fn fixture_home() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/home")
        .canonicalize()
        .expect("fixtures/home")
}

fn profile_for_fixture(home: &Path) -> Profile {
    cyclone_core::discover_defaults(home, Platform::Linux).expect("profile")
}

const PASSPHRASE: &str = "cyclone-test-passphrase-ci";
const UNIQUE: &str = "FIXTURE_UNIQUE_DOTFILE_TOKEN";
const EXCLUDED: &str = "FIXTURE_EXCLUDED_TOKEN";
const SSH_PRIVATE: &str = "FIXTURE_SSH_PRIVATE_KEY_TOKEN";
const BROWSER: &str = "FIXTURE_BROWSER_TOKEN";
const KEYCHAIN: &str = "FIXTURE_KEYCHAIN_TOKEN";

fn capture_fixture(image: &Path, profile: &Profile, home: &Path) {
    let report = capture(CaptureOptions {
        profile,
        output: image,
        passphrase: Some(PASSPHRASE),
        home: home.to_path_buf(),
        platform: Platform::Linux,
        dry_run: false,
        fast_kdf: true,
    })
    .expect("capture");
    assert!(
        report.component_count >= 4,
        "expected gitconfig/zshrc/nvim/vscode plus tools, got {}",
        report.component_count
    );
}

#[test]
fn fixture_home_round_trip_is_encrypted() {
    let home = fixture_home();
    let dir = tempdir().unwrap();
    let image = dir.path().join("dev-box.cyclone");
    let mut profile = profile_for_fixture(&home);
    profile
        .secret_references
        .push(cyclone_core::SecretReference {
            name: "GITHUB_TOKEN".into(),
            provider: "prompt".into(),
            path_hint: None,
        });

    let dry = capture(CaptureOptions {
        profile: &profile,
        output: &image,
        passphrase: None,
        home: home.clone(),
        platform: Platform::Linux,
        dry_run: true,
        fast_kdf: true,
    })
    .unwrap();
    assert!(dry.dry_run);
    assert!(!image.exists(), "dry-run must not write an image");

    capture_fixture(&image, &profile, &home);

    let bundle = ImageBundle::open(&image).unwrap();
    let raw = bundle.raw_bytes().unwrap();
    let raw_s = String::from_utf8_lossy(&raw);
    assert!(
        !raw_s.contains(UNIQUE),
        "captured file contents must not appear in the on-disk image"
    );
    let header = std::fs::read_to_string(image.join("header.json")).unwrap();
    assert!(!header.contains(UNIQUE));
    assert!(!header.contains("GITHUB_TOKEN"));
    assert!(!header.contains(EXCLUDED));
    assert!(header.contains("argon2id"));
    assert!(image.join("bundle.bin.enc").is_file());
    assert!(!image.join("manifest.json").exists());

    assert!(inspect(&image, "wrong-passphrase").is_err());

    let inspected = inspect(&image, PASSPHRASE).unwrap();
    assert!(inspected.redacted);
    assert_eq!(inspected.secret_names, vec!["GITHUB_TOKEN".to_string()]);
    assert!(inspected.components.iter().any(|c| c.id == "gitconfig"));
    assert!(inspected
        .components
        .iter()
        .any(|c| c.id == "vscode-settings"));
    assert!(inspected.components.iter().any(|c| c.id == "tmux-conf"));
    assert!(inspected.components.iter().any(|c| c.id == "htop"));
    assert!(inspected
        .components
        .iter()
        .all(|c| c.id != "ssh-id-ed25519"));
    assert!(inspected.components.iter().all(|c| c.id != "google-chrome"));

    let inner = bundle.decrypt(PASSPHRASE).unwrap();
    let captured: Vec<String> = inner
        .payload
        .artifacts
        .iter()
        .map(|a| String::from_utf8_lossy(&a.bytes).into_owned())
        .collect();
    assert!(
        captured.iter().any(|b| b.contains(UNIQUE)),
        "gitconfig token missing from decrypted payload"
    );
    assert!(
        captured.iter().all(|b| !b.contains(EXCLUDED)),
        "excluded cache file leaked into payload"
    );
    assert!(
        captured.iter().all(|b| !b.contains(SSH_PRIVATE)),
        "ssh private key leaked into payload"
    );
    assert!(
        captured.iter().all(|b| !b.contains(BROWSER)),
        "browser data leaked into payload"
    );
    assert!(
        captured.iter().all(|b| !b.contains(KEYCHAIN)),
        "keychain data leaked into payload"
    );
}

#[test]
fn restore_dry_run_and_apply_on_second_home() {
    let home = fixture_home();
    let dir = tempdir().unwrap();
    let image = dir.path().join("img.cyclone");
    let profile = profile_for_fixture(&home);
    capture_fixture(&image, &profile, &home);

    let dest = dir.path().join("machine-b");
    std::fs::create_dir_all(&dest).unwrap();

    let dry = restore(RestoreOptions {
        image: &image,
        passphrase: PASSPHRASE,
        dest_home: dest.clone(),
        dest_platform: Platform::Linux,
        dry_run: true,
        overwrite: false,
    })
    .unwrap();
    assert!(dry.dry_run);
    assert_eq!(dry.applied_files, 0);
    assert!(dest.read_dir().unwrap().next().is_none());
    let rendered: Vec<String> = dry.planned_actions.iter().map(|a| a.to_string()).collect();
    assert!(
        rendered
            .iter()
            .any(|a| a.contains("gitconfig") && a.contains("create")),
        "{rendered:?}"
    );

    let applied = restore(RestoreOptions {
        image: &image,
        passphrase: PASSPHRASE,
        dest_home: dest.clone(),
        dest_platform: Platform::Linux,
        dry_run: false,
        overwrite: true,
    })
    .unwrap();
    assert!(applied.applied_files >= 4);
    let git = std::fs::read_to_string(dest.join(".gitconfig")).unwrap();
    assert!(git.contains(UNIQUE));
    let nvim = std::fs::read_to_string(dest.join(".config/nvim/init.lua")).unwrap();
    assert!(nvim.contains("vim.opt.number"));
    assert!(!dest.join(".cache/excluded.txt").exists());

    let second = restore(RestoreOptions {
        image: &image,
        passphrase: PASSPHRASE,
        dest_home: dest.clone(),
        dest_platform: Platform::Linux,
        dry_run: true,
        overwrite: false,
    })
    .unwrap();
    assert!(second
        .planned_actions
        .iter()
        .any(|a| a.kind == RestoreActionKind::Unchanged));
}

#[test]
fn macos_path_mapping_and_tool_diffs() {
    let home = fixture_home();
    let dir = tempdir().unwrap();
    let image = dir.path().join("img.cyclone");
    let profile = profile_for_fixture(&home);
    capture_fixture(&image, &profile, &home);

    let dest = dir.path().join("mac-home");
    let dry = restore(RestoreOptions {
        image: &image,
        passphrase: PASSPHRASE,
        dest_home: dest.clone(),
        dest_platform: Platform::MacOs,
        dry_run: true,
        overwrite: false,
    })
    .unwrap();

    let vscode = dry
        .planned_actions
        .iter()
        .find(|a| a.component_id == "vscode-settings" && a.kind == RestoreActionKind::Map)
        .unwrap_or_else(|| panic!("missing vscode map: {:?}", dry.planned_actions));
    assert!(vscode.detail.contains("~/.config/Code/User/settings.json"));
    assert!(vscode
        .detail
        .contains("~/Library/Application Support/Code/User/settings.json"));

    assert!(dry.planned_actions.iter().any(|a| {
        a.component_id == "apt-packages" && a.kind == RestoreActionKind::SkipUnsupported
    }));

    if !cyclone_core::command_on_path("brew") {
        assert!(dry.planned_actions.iter().any(|a| {
            a.component_id == "brewfile" && a.kind == RestoreActionKind::SkipMissingTool
        }));
    }

    restore(RestoreOptions {
        image: &image,
        passphrase: PASSPHRASE,
        dest_home: dest.clone(),
        dest_platform: Platform::MacOs,
        dry_run: false,
        overwrite: true,
    })
    .unwrap();

    let mapped = dest.join("Library/Application Support/Code/User/settings.json");
    let body = std::fs::read_to_string(&mapped).unwrap();
    assert!(body.contains("editor.fontSize"));
    assert!(!dest.join(".config/apt/packages.list").exists());
}

#[test]
fn edited_profile_changes_capture_scope() {
    let home = fixture_home();
    let dir = tempdir().unwrap();
    let mut profile = profile_for_fixture(&home);
    for c in &mut profile.components {
        if c.id == "nvim" {
            c.include = false;
        }
    }
    let path = dir.path().join("profile.toml");
    profile.save(&path).unwrap();
    let loaded = Profile::load(&path).unwrap();
    assert!(
        !loaded
            .components
            .iter()
            .find(|c| c.id == "nvim")
            .unwrap()
            .include
    );

    let image = dir.path().join("img.cyclone");
    capture_fixture(&image, &loaded, &home);
    let inspected = inspect(&image, PASSPHRASE).unwrap();
    assert!(inspected.components.iter().all(|c| c.id != "nvim"));
    assert!(inspected.components.iter().any(|c| c.id == "gitconfig"));
}
