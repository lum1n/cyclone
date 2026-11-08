use std::path::PathBuf;

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::tempdir;

fn fixture_home() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/home")
        .canonicalize()
        .expect("fixtures/home")
}

fn cyclone() -> Command {
    Command::cargo_bin("cyclone").expect("bin")
}

const PASSPHRASE: &str = "cyclone-test-passphrase-ci";

#[test]
fn init_capture_inspect_restore_walkthrough() {
    let tmp = tempdir().unwrap();
    let xdg = tmp.path().join("xdg-config");
    let image = tmp.path().join("dev-box.cyclone");
    let machine_b = tmp.path().join("machine-b");
    let home = fixture_home();
    let home_s = home.to_str().unwrap();
    let image_s = image.to_str().unwrap();
    let machine_b_s = machine_b.to_str().unwrap();

    cyclone()
        .env("XDG_CONFIG_HOME", &xdg)
        .args(["init", "--non-interactive", "--home", home_s])
        .assert()
        .success()
        .stdout(contains("Wrote profile"));

    cyclone()
        .env("XDG_CONFIG_HOME", &xdg)
        .arg("profile")
        .assert()
        .success()
        .stdout(contains("gitconfig"))
        .stdout(contains("tmux-conf"))
        .stdout(contains("opt_in = \"ssh-private-key\""));

    cyclone()
        .env("XDG_CONFIG_HOME", &xdg)
        .env("CYCLONE_PASSPHRASE", PASSPHRASE)
        .args([
            "capture",
            "--home",
            home_s,
            "--output",
            image_s,
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(contains("dry-run"));
    assert!(!image.exists());

    cyclone()
        .env("XDG_CONFIG_HOME", &xdg)
        .env("CYCLONE_PASSPHRASE", PASSPHRASE)
        .args([
            "capture",
            "--home",
            home_s,
            "--output",
            image_s,
            "--fast-kdf",
        ])
        .assert()
        .success();
    assert!(image.join("bundle.bin.enc").is_file());

    cyclone()
        .env("XDG_CONFIG_HOME", &xdg)
        .args(["inspect", image_s])
        .assert()
        .failure()
        .stderr(contains("passphrase required"));

    cyclone()
        .env("XDG_CONFIG_HOME", &xdg)
        .env("CYCLONE_PASSPHRASE", PASSPHRASE)
        .args(["inspect", image_s])
        .assert()
        .success()
        .stdout(contains("gitconfig"));

    cyclone()
        .env("XDG_CONFIG_HOME", &xdg)
        .env("CYCLONE_PASSPHRASE", PASSPHRASE)
        .args([
            "restore",
            image_s,
            "--dry-run",
            "--target-root",
            machine_b_s,
        ])
        .assert()
        .success()
        .stdout(contains("dry-run"));
    assert!(!machine_b.join(".gitconfig").exists());

    cyclone()
        .env("XDG_CONFIG_HOME", &xdg)
        .env("CYCLONE_PASSPHRASE", PASSPHRASE)
        .args([
            "restore",
            image_s,
            "--target-root",
            machine_b_s,
            "--overwrite",
        ])
        .assert()
        .success();
    let git = std::fs::read_to_string(machine_b.join(".gitconfig")).unwrap();
    assert!(git.contains("FIXTURE_UNIQUE_DOTFILE_TOKEN"));

    let mac_home = tmp.path().join("mac-home");
    cyclone()
        .env("XDG_CONFIG_HOME", &xdg)
        .env("CYCLONE_PASSPHRASE", PASSPHRASE)
        .args([
            "restore",
            image_s,
            "--dry-run",
            "--assume-platform",
            "macos",
            "--target-root",
            mac_home.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(contains("Library/Application Support/Code"));

    cyclone()
        .env("XDG_CONFIG_HOME", &xdg)
        .env("CYCLONE_PASSPHRASE", PASSPHRASE)
        .args([
            "restore",
            image_s,
            "--dry-run",
            "--assume-platform",
            "linux",
            "--assume-distro",
            "fedora",
            "--assume-manager",
            "dnf",
            "--target-root",
            mac_home.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(contains("dnf"))
        .stdout(contains("recapture"));

    cyclone()
        .args(["push", image_s])
        .assert()
        .failure()
        .stderr(contains("not implemented"));
}
