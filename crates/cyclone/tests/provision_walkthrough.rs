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
fn provision_dry_run_default_then_apply_requires_token() {
    let tmp = tempdir().unwrap();
    let xdg = tmp.path().join("xdg-config");
    let image = tmp.path().join("dev-box.cyclone");
    let home = fixture_home();
    let home_s = home.to_str().unwrap();
    let image_s = image.to_str().unwrap();

    cyclone()
        .env("XDG_CONFIG_HOME", &xdg)
        .args(["init", "--non-interactive", "--home", home_s])
        .assert()
        .success();

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

    cyclone()
        .args([
            "provision",
            image_s,
            "--ext",
            "exe-dev",
            "--target",
            "dev-box",
        ])
        .assert()
        .failure()
        .stderr(contains("passphrase required"));

    cyclone()
        .env("CYCLONE_PASSPHRASE", PASSPHRASE)
        .args([
            "provision",
            image_s,
            "--ext",
            "exe-dev",
            "--target",
            "dev-box",
        ])
        .assert()
        .success()
        .stdout(contains("dry-run"))
        .stdout(contains("ls --json dev-box"))
        .stdout(contains("new --name dev-box --tag cyclone --json"))
        .stdout(contains("encrypted image: argon2id"));

    cyclone()
        .env("CYCLONE_PASSPHRASE", PASSPHRASE)
        .args([
            "provision",
            image_s,
            "--ext",
            "exe-dev",
            "--target",
            "dev-box",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(contains("dry-run"));

    cyclone()
        .env("CYCLONE_PASSPHRASE", PASSPHRASE)
        .env_remove("EXE_TOKEN")
        .args([
            "provision",
            image_s,
            "--ext",
            "exe-dev",
            "--target",
            "dev-box",
            "--apply",
        ])
        .assert()
        .failure()
        .stderr(contains("EXE_TOKEN"));

    cyclone()
        .env("CYCLONE_PASSPHRASE", PASSPHRASE)
        .args(["provision", image_s, "--ext", "nope", "--target", "dev-box"])
        .assert()
        .failure()
        .stderr(contains("exe-dev"));
}
