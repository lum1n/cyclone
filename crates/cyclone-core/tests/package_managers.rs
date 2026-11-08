//! Capture/restore of native package-manager declarations (not binaries).

use std::path::{Path, PathBuf};

use cyclone_core::{
    capture, restore, CaptureOptions, CommandProbe, ComponentKind, Distro, PackageCaptureMode,
    PackageCaptureOptions, PackageManager, PackageRestoreOptions, Platform, Profile,
    ProfileComponent, RestoreActionKind, RestoreOptions, ScriptedCommands,
};
use tempfile::tempdir;

const PASSPHRASE: &str = "cyclone-test-passphrase-ci";

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/packages")
}

fn profile_one(
    id: &str,
    linux: &str,
    macos: Option<&str>,
    manager: PackageManager,
    platforms: Vec<Platform>,
) -> Profile {
    Profile {
        schema_version: 1,
        components: vec![ProfileComponent {
            id: id.into(),
            kind: ComponentKind::ToolManifest,
            include: true,
            linux: Some(linux.into()),
            macos: macos.map(str::to_string),
            supported_platforms: platforms,
            package_manager: Some(manager),
            opt_in: None,
        }],
        secret_references: vec![],
        notes: None,
    }
}

fn capture_list(
    home: &Path,
    image: &Path,
    profile: &Profile,
    platform: Platform,
    distro: Distro,
    commands: CommandProbe,
    mode: PackageCaptureMode,
) {
    capture(CaptureOptions {
        profile,
        output: image,
        passphrase: Some(PASSPHRASE),
        home: home.to_path_buf(),
        platform,
        dry_run: false,
        fast_kdf: true,
        packages: PackageCaptureOptions {
            mode,
            distro: Some(distro),
            commands,
        },
    })
    .expect("capture");
}

fn write_home_file(home: &Path, rel: &str, body: &str) {
    let path = home.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

#[test]
fn fixture_lists_capture_and_restore_per_manager() {
    let cases: &[(&str, &str, &str, PackageManager, Platform, &str)] = &[
        (
            "Brewfile",
            "Brewfile",
            "brewfile",
            PackageManager::Homebrew,
            Platform::MacOs,
            "macos",
        ),
        (
            "apt.list",
            ".config/apt/packages.list",
            "apt-packages",
            PackageManager::Apt,
            Platform::Linux,
            "ubuntu",
        ),
        (
            "dnf.list",
            ".config/dnf/packages.list",
            "dnf-packages",
            PackageManager::Dnf,
            Platform::Linux,
            "fedora",
        ),
        (
            "pacman.list",
            ".config/pacman/packages.list",
            "pacman-packages",
            PackageManager::Pacman,
            Platform::Linux,
            "arch",
        ),
        (
            "yay.list",
            ".config/yay/packages.list",
            "yay-packages",
            PackageManager::Yay,
            Platform::Linux,
            "arch",
        ),
        (
            "paru.list",
            ".config/paru/packages.list",
            "paru-packages",
            PackageManager::Paru,
            Platform::Linux,
            "arch",
        ),
        (
            "apk.world",
            ".config/apk/world",
            "apk-packages",
            PackageManager::Apk,
            Platform::Linux,
            "alpine",
        ),
        (
            "zypper.list",
            ".config/zypper/packages.list",
            "zypper-packages",
            PackageManager::Zypper,
            Platform::Linux,
            "opensuse-tumbleweed",
        ),
    ];

    for (fixture, rel, id, manager, platform, distro_id) in cases {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        let body = std::fs::read_to_string(fixtures().join(fixture)).unwrap();
        write_home_file(&home, rel, &body);
        let logical = format!("~/{rel}");
        let macos = if *manager == PackageManager::Homebrew {
            Some(logical.as_str())
        } else {
            None
        };
        let profile = profile_one(id, &logical, macos, *manager, vec![*platform]);
        let image = dir.path().join("img.cyclone");
        capture_list(
            &home,
            &image,
            &profile,
            *platform,
            Distro::from_id(distro_id),
            CommandProbe::System,
            PackageCaptureMode::Files,
        );

        let dest = dir.path().join("dest");
        let scripted = match manager {
            PackageManager::Homebrew => ScriptedCommands::new().allow("brew"),
            PackageManager::Apt => ScriptedCommands::new().allow("apt-get").allow("apt"),
            PackageManager::Dnf => ScriptedCommands::new().allow("dnf"),
            PackageManager::Pacman => ScriptedCommands::new().allow("pacman"),
            PackageManager::Yay => ScriptedCommands::new().allow("yay"),
            PackageManager::Paru => ScriptedCommands::new().allow("paru"),
            PackageManager::Apk => ScriptedCommands::new().allow("apk"),
            PackageManager::Zypper => ScriptedCommands::new().allow("zypper"),
        };
        let dry = restore(RestoreOptions {
            image: &image,
            passphrase: PASSPHRASE,
            dest_home: dest.clone(),
            dest_platform: *platform,
            dry_run: true,
            overwrite: true,
            packages: PackageRestoreOptions {
                distro: Some(Distro::from_id(distro_id)),
                manager: Some(*manager),
                install: false,
                commands: CommandProbe::Scripted(scripted.clone()),
            },
        })
        .unwrap();
        assert!(
            dry.planned_actions
                .iter()
                .any(|a| { a.component_id == *id && a.kind == RestoreActionKind::WouldInstall }),
            "{id} missing WouldInstall: {:?}",
            dry.planned_actions
        );
        let needle = if manager.is_aur_helper() {
            "visual-studio-code-bin"
        } else {
            "git"
        };
        assert!(
            dry.planned_actions
                .iter()
                .any(|a| a.detail.contains(needle)),
            "{id} dry-run should name packages: {:?}",
            dry.planned_actions
        );

        restore(RestoreOptions {
            image: &image,
            passphrase: PASSPHRASE,
            dest_home: dest.clone(),
            dest_platform: *platform,
            dry_run: false,
            overwrite: true,
            packages: PackageRestoreOptions {
                distro: Some(Distro::from_id(distro_id)),
                manager: Some(*manager),
                install: true,
                commands: CommandProbe::Scripted(scripted.clone()),
            },
        })
        .unwrap();
        let restored = std::fs::read_to_string(dest.join(rel)).unwrap();
        assert!(
            restored.contains(needle),
            "{id} declaration should be written"
        );
        let ran = scripted.ran();
        assert!(
            ran.iter()
                .any(|c| c.contains(manager.command()) || c.starts_with("sudo")),
            "{id} should invoke {}: {ran:?}",
            manager.command()
        );
    }
}

#[test]
fn brewfile_on_linux_apt_is_an_actionable_mismatch() {
    let dir = tempdir().unwrap();
    let home = dir.path().join("home");
    let body = std::fs::read_to_string(fixtures().join("Brewfile")).unwrap();
    write_home_file(&home, "Brewfile", &body);
    let profile = profile_one(
        "brewfile",
        "~/Brewfile",
        Some("~/Brewfile"),
        PackageManager::Homebrew,
        vec![Platform::Linux, Platform::MacOs],
    );
    let image = dir.path().join("img.cyclone");
    capture_list(
        &home,
        &image,
        &profile,
        Platform::MacOs,
        Distro::from_id("macos"),
        CommandProbe::System,
        PackageCaptureMode::Files,
    );

    let dry = restore(RestoreOptions {
        image: &image,
        passphrase: PASSPHRASE,
        dest_home: dir.path().join("linux"),
        dest_platform: Platform::Linux,
        dry_run: true,
        overwrite: false,
        packages: PackageRestoreOptions {
            distro: Some(Distro::from_id("ubuntu")),
            manager: Some(PackageManager::Apt),
            install: false,
            commands: CommandProbe::System,
        },
    })
    .unwrap();
    let mismatch = dry
        .planned_actions
        .iter()
        .find(|a| a.kind == RestoreActionKind::SkipManagerMismatch)
        .unwrap_or_else(|| panic!("expected mismatch: {:?}", dry.planned_actions));
    assert!(mismatch.detail.contains("Homebrew"));
    assert!(mismatch.detail.contains("apt"));
    assert!(mismatch.detail.contains("ubuntu"));
    assert!(mismatch.detail.contains("git"));
    assert!(mismatch.detail.contains("ripgrep"));
    assert!(mismatch.detail.contains("recapture"));
}

#[test]
fn apt_list_on_fedora_dnf_is_an_actionable_mismatch() {
    let dir = tempdir().unwrap();
    let home = dir.path().join("home");
    let body = std::fs::read_to_string(fixtures().join("apt.list")).unwrap();
    write_home_file(&home, ".config/apt/packages.list", &body);
    let profile = profile_one(
        "apt-packages",
        "~/.config/apt/packages.list",
        None,
        PackageManager::Apt,
        vec![Platform::Linux],
    );
    let image = dir.path().join("img.cyclone");
    capture_list(
        &home,
        &image,
        &profile,
        Platform::Linux,
        Distro::from_id("ubuntu"),
        CommandProbe::System,
        PackageCaptureMode::Files,
    );

    let dry = restore(RestoreOptions {
        image: &image,
        passphrase: PASSPHRASE,
        dest_home: dir.path().join("fedora"),
        dest_platform: Platform::Linux,
        dry_run: true,
        overwrite: false,
        packages: PackageRestoreOptions {
            distro: Some(Distro::from_id("fedora")),
            manager: Some(PackageManager::Dnf),
            install: false,
            commands: CommandProbe::Scripted(ScriptedCommands::new().allow("dnf")),
        },
    })
    .unwrap();
    let mismatch = dry
        .planned_actions
        .iter()
        .find(|a| {
            a.component_id == "apt-packages" && a.kind == RestoreActionKind::SkipManagerMismatch
        })
        .unwrap_or_else(|| panic!("{:?}", dry.planned_actions));
    assert!(mismatch.detail.contains("apt"));
    assert!(mismatch.detail.contains("dnf"));
    assert!(mismatch.detail.contains("fedora"));
    assert!(mismatch.detail.contains("build-essential"));
}

#[test]
fn live_apt_dump_uses_scripted_host_not_fat_binaries() {
    let dir = tempdir().unwrap();
    let home = dir.path().join("empty-home");
    std::fs::create_dir_all(&home).unwrap();
    let profile = profile_one(
        "apt-packages",
        "~/.config/apt/packages.list",
        None,
        PackageManager::Apt,
        vec![Platform::Linux],
    );
    let image = dir.path().join("img.cyclone");
    let commands = CommandProbe::Scripted(
        ScriptedCommands::new()
            .allow("apt-get")
            .stdout("apt-mark", &["showmanual"], "ripgrep\nfd-find\n")
            .stdout(
                "dpkg-query",
                &["-W", "-f=${Package}\t${Version}\n"],
                "ripgrep\t14.1.0\nfd-find\t10.2.0\n",
            ),
    );
    capture_list(
        &home,
        &image,
        &profile,
        Platform::Linux,
        Distro::from_id("debian"),
        commands,
        PackageCaptureMode::Live,
    );

    let dest = dir.path().join("dest");
    restore(RestoreOptions {
        image: &image,
        passphrase: PASSPHRASE,
        dest_home: dest.clone(),
        dest_platform: Platform::Linux,
        dry_run: false,
        overwrite: true,
        packages: PackageRestoreOptions {
            distro: Some(Distro::from_id("debian")),
            manager: Some(PackageManager::Apt),
            install: false,
            commands: CommandProbe::System,
        },
    })
    .unwrap();
    let body = std::fs::read_to_string(dest.join(".config/apt/packages.list")).unwrap();
    assert!(body.contains("ripgrep=14.1.0"));
    assert!(body.contains("# cyclone.package-manager: apt"));
    assert!(!body.contains("\0"), "must store text, not binaries");
}

fn capture_yay_image(dir: &Path) -> PathBuf {
    let home = dir.join("home");
    let body = std::fs::read_to_string(fixtures().join("yay.list")).unwrap();
    write_home_file(&home, ".config/yay/packages.list", &body);
    let profile = profile_one(
        "yay-packages",
        "~/.config/yay/packages.list",
        None,
        PackageManager::Yay,
        vec![Platform::Linux],
    );
    let image = dir.join("img.cyclone");
    capture_list(
        &home,
        &image,
        &profile,
        Platform::Linux,
        Distro::from_id("arch"),
        CommandProbe::System,
        PackageCaptureMode::Files,
    );
    image
}

#[test]
fn aur_yay_on_ubuntu_cannot_be_applied() {
    let dir = tempdir().unwrap();
    let image = capture_yay_image(dir.path());
    let dry = restore(RestoreOptions {
        image: &image,
        passphrase: PASSPHRASE,
        dest_home: dir.path().join("ubuntu"),
        dest_platform: Platform::Linux,
        dry_run: true,
        overwrite: false,
        packages: PackageRestoreOptions {
            distro: Some(Distro::from_id("ubuntu")),
            manager: Some(PackageManager::Apt),
            install: false,
            commands: CommandProbe::Scripted(ScriptedCommands::new().allow("yay")),
        },
    })
    .unwrap();
    let mismatch = dry
        .planned_actions
        .iter()
        .find(|a| {
            a.component_id == "yay-packages" && a.kind == RestoreActionKind::SkipManagerMismatch
        })
        .unwrap_or_else(|| panic!("{:?}", dry.planned_actions));
    assert!(mismatch.detail.contains("AUR"));
    assert!(mismatch.detail.contains("ubuntu"));
    assert!(mismatch.detail.contains("visual-studio-code-bin"));
}

#[test]
fn aur_yay_on_arch_without_helper_cannot_be_applied() {
    let dir = tempdir().unwrap();
    let image = capture_yay_image(dir.path());
    let dry = restore(RestoreOptions {
        image: &image,
        passphrase: PASSPHRASE,
        dest_home: dir.path().join("arch"),
        dest_platform: Platform::Linux,
        dry_run: true,
        overwrite: false,
        packages: PackageRestoreOptions {
            distro: Some(Distro::from_id("arch")),
            manager: Some(PackageManager::Pacman),
            install: false,
            commands: CommandProbe::Scripted(ScriptedCommands::new().allow("pacman")),
        },
    })
    .unwrap();
    let missing = dry
        .planned_actions
        .iter()
        .find(|a| a.component_id == "yay-packages" && a.kind == RestoreActionKind::SkipMissingTool)
        .unwrap_or_else(|| panic!("{:?}", dry.planned_actions));
    assert!(missing.detail.contains("AUR"));
    assert!(missing.detail.contains("yay"));
    assert!(missing.detail.contains("visual-studio-code-bin"));
}

#[test]
fn aur_paru_on_arch_with_helper_would_install() {
    let dir = tempdir().unwrap();
    let home = dir.path().join("home");
    let body = std::fs::read_to_string(fixtures().join("paru.list")).unwrap();
    write_home_file(&home, ".config/paru/packages.list", &body);
    let profile = profile_one(
        "paru-packages",
        "~/.config/paru/packages.list",
        None,
        PackageManager::Paru,
        vec![Platform::Linux],
    );
    let image = dir.path().join("img.cyclone");
    capture_list(
        &home,
        &image,
        &profile,
        Platform::Linux,
        Distro::from_id("arch"),
        CommandProbe::System,
        PackageCaptureMode::Files,
    );
    let scripted = ScriptedCommands::new().allow("paru").allow("pacman");
    let dry = restore(RestoreOptions {
        image: &image,
        passphrase: PASSPHRASE,
        dest_home: dir.path().join("arch"),
        dest_platform: Platform::Linux,
        dry_run: true,
        overwrite: false,
        packages: PackageRestoreOptions {
            distro: Some(Distro::from_id("arch")),
            manager: Some(PackageManager::Pacman),
            install: false,
            commands: CommandProbe::Scripted(scripted),
        },
    })
    .unwrap();
    assert!(dry.planned_actions.iter().any(|a| {
        a.component_id == "paru-packages" && a.kind == RestoreActionKind::WouldInstall
    }));
    assert!(dry
        .planned_actions
        .iter()
        .any(|a| a.detail.contains("paru -S") && a.detail.contains("google-chrome")));
}
