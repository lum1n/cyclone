//! Provision an encrypted Cyclone image against fixture HTTP (no live exe.dev).

use std::path::PathBuf;
use std::sync::Arc;

use cyclone_core::{
    capture, ApplyOptions, CaptureOptions, Extension, ImageBundle, Platform, Profile,
    ProvisionTarget,
};
use cyclone_ext_exe::{ExeDevClient, ExeDevExtension, MockExecTransport};
use tempfile::tempdir;

fn fixture_home() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/home")
        .canonicalize()
        .expect("fixtures/home")
}

const PASSPHRASE: &str = "cyclone-test-passphrase-ci";

fn capture_encrypted(image: &std::path::Path) {
    let home = fixture_home();
    let profile: Profile =
        cyclone_core::discover_defaults(&home, Platform::Linux).expect("profile");
    capture(CaptureOptions {
        profile: &profile,
        output: image,
        passphrase: Some(PASSPHRASE),
        home,
        platform: Platform::Linux,
        dry_run: false,
        fast_kdf: true,
        packages: Default::default(),
    })
    .expect("capture");
}

#[test]
fn encrypted_image_dry_run_then_apply_against_fixtures() {
    let dir = tempdir().unwrap();
    let image = dir.path().join("dev-box.cyclone");
    capture_encrypted(&image);
    assert!(image.join("bundle.bin.enc").is_file());
    assert!(image.join("header.json").is_file());

    let bundle = ImageBundle::open(&image).unwrap();
    let header = std::fs::read_to_string(image.join("header.json")).unwrap();
    assert!(header.contains("argon2id"));
    assert!(!header.contains(PASSPHRASE));

    let inner = bundle.decrypt(PASSPHRASE).expect("decrypt");
    let target = ProvisionTarget {
        id: "dev-box".into(),
        labels: vec![],
    };

    let dry = ExeDevExtension::new()
        .apply(&inner, &target, &ApplyOptions { dry_run: true })
        .unwrap();
    assert!(dry.dry_run);
    let dry_log = dry.log.join("\n");
    assert!(dry_log.contains("ls --json dev-box"));
    assert!(dry_log.contains("new --name dev-box --tag cyclone --json"));
    assert!(
        !dry_log.contains("FIXTURE_UNIQUE_DOTFILE_TOKEN"),
        "dry-run must not print decrypted dotfile bytes"
    );

    let mock = Arc::new(MockExecTransport::new());
    mock.push(200, include_str!("fixtures/ls_empty.json"));
    mock.push(200, include_str!("fixtures/new_vm.json"));
    // One 200 per restored file (fixture home has several).
    for _ in 0..32 {
        mock.push(200, "");
    }
    let client =
        ExeDevClient::with_transport("https://exe.dev/exec", "fixture-token", mock.clone());
    let applied = ExeDevExtension::with_client(client)
        .apply(&inner, &target, &ApplyOptions { dry_run: false })
        .unwrap();
    assert!(!applied.dry_run);
    let recorded = mock.recorded();
    assert_eq!(recorded[0], "ls --json dev-box");
    assert_eq!(recorded[1], "new --name dev-box --tag cyclone --json");
    assert!(recorded
        .iter()
        .skip(2)
        .all(|c| c.starts_with("ssh dev-box ")));
    assert!(recorded.iter().any(|c| c.contains(".gitconfig")));
    let joined = recorded.join("\n");
    assert!(
        joined.contains("FIXTURE_UNIQUE") || joined.contains("base64"),
        "file bytes travel as base64 in the ssh command, not as a second channel"
    );
}
