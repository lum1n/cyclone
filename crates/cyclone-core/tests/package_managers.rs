//! Capture/restore of native package-manager declarations (not binaries).
//!
//! Capture queries the live manager (`brew list`, `pacman -Qe`, …) via a scripted
//! CLI probe. User-maintained `.list` files are not the source.

use std::path::{Path, PathBuf};

use cyclone_core::{
    capture, restore, CaptureOptions, CommandProbe, ComponentKind, Distro, PackageCaptureMode,
    PackageCaptureOptions, PackageManager, PackageRestoreOptions, Platform, Profile,
    ProfileComponent, RestoreActionKind, RestoreOptions, ScriptedCommands,
};
use tempfile::tempdir;

const PASSPHRASE: &str = "cyclone-test-passphrase-ci";

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

fn capture_live(
    home: &Path,
    image: &Path,
    profile: &Profile,
    platform: Platform,
    distro: Distro,
    commands: CommandProbe,
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
            mode: PackageCaptureMode::Live,
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

/// Mock list-installed output. No fixture `.list` files.
fn scripted_list(manager: PackageManager) -> ScriptedCommands {
    match manager {
        PackageManager::Homebrew => ScriptedCommands::new()
            .stdout("brew", &["tap"], "homebrew/cask-fonts\n")
            .stdout(
                "brew",
                &["list", "--formula", "--versions"],
                "git 2.47.0\nripgrep 14.1.1\n",
            )
            .stdout(
                "brew",
                &["list", "--cask", "--versions"],
                "firefox 131.0.3\n",
            ),
        PackageManager::Apt => ScriptedCommands::new()
            .allow("apt-get")
            .allow("apt")
            .stdout("apt-mark", &["showmanual"], "git\ncurl\nbuild-essential\n")
            .stdout(
                "dpkg-query",
                &["-W", "-f=${Package}\t${Version}\n"],
                "git\t1:2.43.0-1ubuntu1\ncurl\t8.5.0-2\nbuild-essential\t12.10ubuntu1\n",
            ),
        PackageManager::Dnf => ScriptedCommands::new().stdout(
            "dnf",
            &[
                "repoquery",
                "--userinstalled",
                "--queryformat",
                "%{name}=%{version}-%{release}\n",
            ],
            "git=2.47.0-1.fc41\nripgrep=14.1.1-1.fc41\n",
        ),
        PackageManager::Pacman => {
            ScriptedCommands::new().stdout("pacman", &["-Qe"], "git 2.47.0-1\ncurl 8.11.0-1\n")
        }
        PackageManager::Yay => ScriptedCommands::new().allow("pacman").stdout(
            "yay",
            &["-Qme"],
            "visual-studio-code-bin 1.95.3-1\nspotify 1:1.2.52-1\n",
        ),
        PackageManager::Paru => ScriptedCommands::new().allow("pacman").stdout(
            "paru",
            &["-Qme"],
            "visual-studio-code-bin 1.95.3-1\ngoogle-chrome 131.0.6778.85-1\n",
        ),
        PackageManager::Apk => {
            ScriptedCommands::new().stdout("apk", &["info", "-e"], "git\ncurl\n")
        }
        PackageManager::Zypper => ScriptedCommands::new().stdout(
            "zypper",
            &[
                "--non-interactive",
                "--quiet",
                "search",
                "--installed-only",
                "--type",
                "package",
            ],
            "git=2.47.0\ncurl=8.11.0\n",
        ),
    }
}

struct LiveCase {
    rel: &'static str,
    id: &'static str,
    manager: PackageManager,
    platform: Platform,
    distro_id: &'static str,
    needle: &'static str,
}

fn live_cases() -> &'static [LiveCase] {
    &[
        LiveCase {
            rel: "Brewfile",
            id: "brewfile",
            manager: PackageManager::Homebrew,
            platform: Platform::MacOs,
            distro_id: "macos",
            needle: "git",
        },
        LiveCase {
            rel: ".config/apt/packages.list",
            id: "apt-packages",
            manager: PackageManager::Apt,
            platform: Platform::Linux,
            distro_id: "ubuntu",
            needle: "git",
        },
        LiveCase {
            rel: ".config/dnf/packages.list",
            id: "dnf-packages",
            manager: PackageManager::Dnf,
            platform: Platform::Linux,
            distro_id: "fedora",
            needle: "git",
        },
        LiveCase {
            rel: ".config/pacman/packages.list",
            id: "pacman-packages",
            manager: PackageManager::Pacman,
            platform: Platform::Linux,
            distro_id: "arch",
            needle: "git",
        },
        LiveCase {
            rel: ".config/yay/packages.list",
            id: "yay-packages",
            manager: PackageManager::Yay,
            platform: Platform::Linux,
            distro_id: "arch",
            needle: "visual-studio-code-bin",
        },
        LiveCase {
            rel: ".config/paru/packages.list",
            id: "paru-packages",
            manager: PackageManager::Paru,
            platform: Platform::Linux,
            distro_id: "arch",
            needle: "google-chrome",
        },
        LiveCase {
            rel: ".config/apk/world",
            id: "apk-packages",
            manager: PackageManager::Apk,
            platform: Platform::Linux,
            distro_id: "alpine",
            needle: "git",
        },
        LiveCase {
            rel: ".config/zypper/packages.list",
            id: "zypper-packages",
            manager: PackageManager::Zypper,
            platform: Platform::Linux,
            distro_id: "opensuse-tumbleweed",
            needle: "git",
        },
    ]
}

#[test]
fn live_query_capture_and_restore_per_manager() {
    for case in live_cases() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("empty-home");
        std::fs::create_dir_all(&home).unwrap();
        assert!(
            !home.join(case.rel).exists(),
            "{} must not require a pre-existing list file",
            case.id
        );
        let logical = format!("~/{}", case.rel);
        let macos = if case.manager == PackageManager::Homebrew {
            Some(logical.as_str())
        } else {
            None
        };
        let profile = profile_one(case.id, &logical, macos, case.manager, vec![case.platform]);
        let image = dir.path().join("img.cyclone");
        let scripted = scripted_list(case.manager);
        capture_live(
            &home,
            &image,
            &profile,
            case.platform,
            Distro::from_id(case.distro_id),
            CommandProbe::Scripted(scripted.clone()),
        );

        let dest = dir.path().join("dest");
        let dry = restore(RestoreOptions {
            image: &image,
            passphrase: PASSPHRASE,
            dest_home: dest.clone(),
            dest_platform: case.platform,
            dry_run: true,
            overwrite: true,
            packages: PackageRestoreOptions {
                distro: Some(Distro::from_id(case.distro_id)),
                manager: Some(case.manager),
                install: false,
                commands: CommandProbe::Scripted(scripted.clone()),
            },
        })
        .unwrap();
        assert!(
            dry.planned_actions.iter().any(|a| {
                a.component_id == case.id && a.kind == RestoreActionKind::WouldInstall
            }),
            "{} missing WouldInstall: {:?}",
            case.id,
            dry.planned_actions
        );
        assert!(
            dry.planned_actions
                .iter()
                .any(|a| a.detail.contains(case.needle)),
            "{} dry-run should name packages: {:?}",
            case.id,
            dry.planned_actions
        );

        restore(RestoreOptions {
            image: &image,
            passphrase: PASSPHRASE,
            dest_home: dest.clone(),
            dest_platform: case.platform,
            dry_run: false,
            overwrite: true,
            packages: PackageRestoreOptions {
                distro: Some(Distro::from_id(case.distro_id)),
                manager: Some(case.manager),
                install: true,
                commands: CommandProbe::Scripted(scripted.clone()),
            },
        })
        .unwrap();
        let restored = std::fs::read_to_string(dest.join(case.rel)).unwrap();
        assert!(
            restored.contains(case.needle),
            "{} declaration should be written from the live dump",
            case.id
        );
        let ran = scripted.ran();
        assert!(
            ran.iter().any(|c| {
                c.contains(case.manager.command())
                    || c.starts_with("sudo")
                    || c.contains("apt-mark")
                    || c.contains("dpkg-query")
            }),
            "{} should invoke {}: {ran:?}",
            case.id,
            case.manager.command()
        );
    }
}

#[test]
fn live_dump_ignores_user_maintained_list_files() {
    let dir = tempdir().unwrap();
    let home = dir.path().join("home");
    write_home_file(
        &home,
        ".config/pacman/packages.list",
        "totally-fake-from-file 0-1\n",
    );
    write_home_file(&home, "Brewfile", "brew \"handwritten-only\"\n");
    write_home_file(&home, ".config/apt/packages.list", "handwritten-apt=1\n");

    struct IgnoreCase {
        id: &'static str,
        logical: &'static str,
        manager: PackageManager,
        platform: Platform,
        distro_id: &'static str,
        scripted: ScriptedCommands,
        keep: &'static str,
        drop: &'static str,
    }
    let cases = [
        IgnoreCase {
            id: "pacman-packages",
            logical: "~/.config/pacman/packages.list",
            manager: PackageManager::Pacman,
            platform: Platform::Linux,
            distro_id: "arch",
            scripted: scripted_list(PackageManager::Pacman),
            keep: "git",
            drop: "totally-fake-from-file",
        },
        IgnoreCase {
            id: "brewfile",
            logical: "~/Brewfile",
            manager: PackageManager::Homebrew,
            platform: Platform::MacOs,
            distro_id: "macos",
            scripted: scripted_list(PackageManager::Homebrew),
            keep: "ripgrep",
            drop: "handwritten-only",
        },
        IgnoreCase {
            id: "apt-packages",
            logical: "~/.config/apt/packages.list",
            manager: PackageManager::Apt,
            platform: Platform::Linux,
            distro_id: "ubuntu",
            scripted: scripted_list(PackageManager::Apt),
            keep: "build-essential",
            drop: "handwritten-apt",
        },
    ];

    for case in cases {
        let IgnoreCase {
            id,
            logical,
            manager,
            platform,
            distro_id,
            scripted,
            keep,
            drop,
        } = case;
        let macos = if manager == PackageManager::Homebrew {
            Some(logical)
        } else {
            None
        };
        let profile = profile_one(id, logical, macos, manager, vec![platform]);
        let image = dir.path().join(format!("{id}.cyclone"));
        capture(CaptureOptions {
            profile: &profile,
            output: &image,
            passphrase: Some(PASSPHRASE),
            home: home.clone(),
            platform,
            dry_run: false,
            fast_kdf: true,
            packages: PackageCaptureOptions {
                mode: PackageCaptureMode::Auto,
                distro: Some(Distro::from_id(distro_id)),
                commands: CommandProbe::Scripted(scripted.clone()),
            },
        })
        .unwrap();

        let dest = dir.path().join(format!("dest-{id}"));
        restore(RestoreOptions {
            image: &image,
            passphrase: PASSPHRASE,
            dest_home: dest.clone(),
            dest_platform: platform,
            dry_run: false,
            overwrite: true,
            packages: PackageRestoreOptions {
                distro: Some(Distro::from_id(distro_id)),
                manager: Some(manager),
                install: false,
                commands: CommandProbe::Scripted(scripted.clone()),
            },
        })
        .unwrap();
        let rel = logical.trim_start_matches("~/");
        let body = std::fs::read_to_string(dest.join(rel)).unwrap();
        assert!(
            body.contains(keep),
            "{id} should store live dump ({keep}): {body}"
        );
        assert!(
            !body.contains(drop),
            "{id} must ignore user list file ({drop}): {body}"
        );
    }
}

#[test]
fn missing_pacman_does_not_look_for_a_list_file() {
    let dir = tempdir().unwrap();
    let home = dir.path().join("empty-home");
    std::fs::create_dir_all(&home).unwrap();
    let profile = profile_one(
        "pacman-packages",
        "~/.config/pacman/packages.list",
        None,
        PackageManager::Pacman,
        vec![Platform::Linux],
    );
    let report = capture(CaptureOptions {
        profile: &profile,
        output: &dir.path().join("unused.cyclone"),
        passphrase: None,
        home,
        platform: Platform::Linux,
        dry_run: true,
        fast_kdf: true,
        packages: PackageCaptureOptions {
            mode: PackageCaptureMode::Live,
            distro: Some(Distro::from_id("arch")),
            commands: CommandProbe::Scripted(ScriptedCommands::new()),
        },
    })
    .unwrap();
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("`pacman`") && (w.contains("PATH") || w.contains("not on PATH"))));
    assert!(report.warnings.iter().all(|w| !w.contains("missing path")));
    assert!(report
        .warnings
        .iter()
        .all(|w| !w.contains("requires a declaration")));
}

#[test]
fn brewfile_on_linux_apt_is_an_actionable_mismatch() {
    let dir = tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let profile = profile_one(
        "brewfile",
        "~/Brewfile",
        Some("~/Brewfile"),
        PackageManager::Homebrew,
        vec![Platform::Linux, Platform::MacOs],
    );
    let image = dir.path().join("img.cyclone");
    capture_live(
        &home,
        &image,
        &profile,
        Platform::MacOs,
        Distro::from_id("macos"),
        CommandProbe::Scripted(scripted_list(PackageManager::Homebrew)),
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
    std::fs::create_dir_all(&home).unwrap();
    let profile = profile_one(
        "apt-packages",
        "~/.config/apt/packages.list",
        None,
        PackageManager::Apt,
        vec![Platform::Linux],
    );
    let image = dir.path().join("img.cyclone");
    capture_live(
        &home,
        &image,
        &profile,
        Platform::Linux,
        Distro::from_id("ubuntu"),
        CommandProbe::Scripted(scripted_list(PackageManager::Apt)),
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
    capture_live(
        &home,
        &image,
        &profile,
        Platform::Linux,
        Distro::from_id("debian"),
        CommandProbe::Scripted(scripted_list(PackageManager::Apt)),
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
    assert!(body.contains("git=1:2.43.0-1ubuntu1"));
    assert!(body.contains("# cyclone.package-manager: apt"));
    assert!(!body.contains("\0"), "must store text, not binaries");
}

fn capture_yay_image(dir: &Path) -> PathBuf {
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let profile = profile_one(
        "yay-packages",
        "~/.config/yay/packages.list",
        None,
        PackageManager::Yay,
        vec![Platform::Linux],
    );
    let image = dir.join("img.cyclone");
    capture_live(
        &home,
        &image,
        &profile,
        Platform::Linux,
        Distro::from_id("arch"),
        CommandProbe::Scripted(scripted_list(PackageManager::Yay)),
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
    std::fs::create_dir_all(&home).unwrap();
    let profile = profile_one(
        "paru-packages",
        "~/.config/paru/packages.list",
        None,
        PackageManager::Paru,
        vec![Platform::Linux],
    );
    let image = dir.path().join("img.cyclone");
    capture_live(
        &home,
        &image,
        &profile,
        Platform::Linux,
        Distro::from_id("arch"),
        CommandProbe::Scripted(scripted_list(PackageManager::Paru)),
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

#[test]
fn opted_in_brewfile_dotfile_is_captured_when_brew_is_absent() {
    let dir = tempdir().unwrap();
    let home = dir.path().join("home");
    write_home_file(&home, "Brewfile", "brew \"handwritten-ripgrep\"\n");
    let profile = Profile {
        schema_version: 1,
        components: vec![ProfileComponent {
            id: "brewfile".into(),
            kind: ComponentKind::Dotfile,
            include: true,
            linux: Some("~/Brewfile".into()),
            macos: Some("~/Brewfile".into()),
            supported_platforms: vec![Platform::Linux, Platform::MacOs],
            package_manager: None,
            opt_in: None,
        }],
        secret_references: vec![],
        notes: None,
    };
    let image = dir.path().join("img.cyclone");
    capture(CaptureOptions {
        profile: &profile,
        output: &image,
        passphrase: Some(PASSPHRASE),
        home: home.clone(),
        platform: Platform::MacOs,
        dry_run: false,
        fast_kdf: true,
        packages: PackageCaptureOptions {
            mode: PackageCaptureMode::Live,
            distro: Some(Distro::from_id("macos")),
            commands: CommandProbe::Scripted(ScriptedCommands::new()),
        },
    })
    .unwrap();
    let dest = dir.path().join("dest");
    restore(RestoreOptions {
        image: &image,
        passphrase: PASSPHRASE,
        dest_home: dest.clone(),
        dest_platform: Platform::MacOs,
        dry_run: false,
        overwrite: true,
        packages: Default::default(),
    })
    .unwrap();
    let body = std::fs::read_to_string(dest.join("Brewfile")).unwrap();
    assert!(body.contains("handwritten-ripgrep"));
}
