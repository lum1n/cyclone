//! Native package-manager declarations (capture dumps, restore installs).
//!
//! Each manager keeps its own dump format and install command. Cyclone does not
//! invent a unified package API — a Brewfile is not rewritten as an apt list.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::{CycloneError, Result};
use crate::platform::{command_on_path, Distro, Platform};

const HEADER_MANAGER: &str = "# cyclone.package-manager:";
const HEADER_DISTRO: &str = "# cyclone.distro:";

/// Native package manager on a capture or restore host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageManager {
    Homebrew,
    Apt,
    Dnf,
    Pacman,
    /// Arch User Repository helper (`yay`). Captured separately from official pacman repos.
    Yay,
    /// Arch User Repository helper (`paru`). Captured separately from official pacman repos.
    Paru,
    Apk,
    Zypper,
}

impl PackageManager {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Homebrew => "homebrew",
            Self::Apt => "apt",
            Self::Dnf => "dnf",
            Self::Pacman => "pacman",
            Self::Yay => "yay",
            Self::Paru => "paru",
            Self::Apk => "apk",
            Self::Zypper => "zypper",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Homebrew => "Homebrew",
            Self::Apt => "apt",
            Self::Dnf => "dnf",
            Self::Pacman => "pacman",
            Self::Yay => "yay",
            Self::Paru => "paru",
            Self::Apk => "apk",
            Self::Zypper => "zypper",
        }
    }

    /// Primary executable used to dump or install.
    pub fn command(self) -> &'static str {
        match self {
            Self::Homebrew => "brew",
            Self::Apt => "apt-get",
            Self::Dnf => "dnf",
            Self::Pacman => "pacman",
            Self::Yay => "yay",
            Self::Paru => "paru",
            Self::Apk => "apk",
            Self::Zypper => "zypper",
        }
    }

    pub fn catalog_id(self) -> &'static str {
        match self {
            Self::Homebrew => "brewfile",
            Self::Apt => "apt-packages",
            Self::Dnf => "dnf-packages",
            Self::Pacman => "pacman-packages",
            Self::Yay => "yay-packages",
            Self::Paru => "paru-packages",
            Self::Apk => "apk-packages",
            Self::Zypper => "zypper-packages",
        }
    }

    pub fn declaration_path(self) -> &'static str {
        match self {
            Self::Homebrew => "~/Brewfile",
            Self::Apt => "~/.config/apt/packages.list",
            Self::Dnf => "~/.config/dnf/packages.list",
            Self::Pacman => "~/.config/pacman/packages.list",
            Self::Yay => "~/.config/yay/packages.list",
            Self::Paru => "~/.config/paru/packages.list",
            Self::Apk => "~/.config/apk/world",
            Self::Zypper => "~/.config/zypper/packages.list",
        }
    }

    pub fn supported_platforms(self) -> &'static [Platform] {
        match self {
            Self::Homebrew => &[Platform::MacOs, Platform::Linux],
            _ => &[Platform::Linux],
        }
    }

    /// Privilege needed to install (Homebrew and AUR helpers sudo internally).
    pub fn install_needs_root(self) -> bool {
        !matches!(self, Self::Homebrew | Self::Yay | Self::Paru)
    }

    /// `yay` / `paru` — AUR helpers, not the official pacman repos.
    pub fn is_aur_helper(self) -> bool {
        matches!(self, Self::Yay | Self::Paru)
    }

    pub fn from_component_id(id: &str) -> Option<Self> {
        match id {
            "brewfile" | "homebrew" | "brew" => Some(Self::Homebrew),
            "apt-packages" | "apt" => Some(Self::Apt),
            "dnf-packages" | "dnf" | "yum-packages" | "yum" => Some(Self::Dnf),
            "pacman-packages" | "pacman" => Some(Self::Pacman),
            "yay-packages" | "yay" => Some(Self::Yay),
            "paru-packages" | "paru" => Some(Self::Paru),
            "apk-packages" | "apk" => Some(Self::Apk),
            "zypper-packages" | "zypper" => Some(Self::Zypper),
            _ => None,
        }
    }

    pub fn from_declaration_path(path: &str) -> Option<Self> {
        let name = path.rsplit('/').next().unwrap_or(path);
        if name.eq_ignore_ascii_case("Brewfile") {
            return Some(Self::Homebrew);
        }
        if path.contains("/apt/") || name.eq_ignore_ascii_case("apt.list") {
            return Some(Self::Apt);
        }
        if path.contains("/dnf/") {
            return Some(Self::Dnf);
        }
        if path.contains("/pacman/") {
            return Some(Self::Pacman);
        }
        if path.contains("/yay/") {
            return Some(Self::Yay);
        }
        if path.contains("/paru/") {
            return Some(Self::Paru);
        }
        if path.contains("/apk/") || name.eq_ignore_ascii_case("world") && path.contains("apk") {
            return Some(Self::Apk);
        }
        if path.contains("/zypper/") {
            return Some(Self::Zypper);
        }
        None
    }

    pub fn present_on(self, commands: &CommandProbe) -> bool {
        match self {
            Self::Homebrew => commands.has("brew"),
            Self::Apt => commands.has("apt-get") || commands.has("apt"),
            Self::Dnf => commands.has("dnf") || commands.has("yum"),
            Self::Pacman => commands.has("pacman"),
            Self::Yay => commands.has("yay"),
            Self::Paru => commands.has("paru"),
            Self::Apk => commands.has("apk"),
            Self::Zypper => commands.has("zypper"),
        }
    }
}

impl std::fmt::Display for PackageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PackageManager {
    type Err = CycloneError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "homebrew" | "brew" | "brewfile" => Ok(Self::Homebrew),
            "apt" | "apt-get" | "dpkg" => Ok(Self::Apt),
            "dnf" | "yum" => Ok(Self::Dnf),
            "pacman" => Ok(Self::Pacman),
            "yay" => Ok(Self::Yay),
            "paru" => Ok(Self::Paru),
            "apk" => Ok(Self::Apk),
            "zypper" => Ok(Self::Zypper),
            _ => Err(CycloneError::User(format!(
                "unknown package manager `{s}` (homebrew, apt, dnf, pacman, yay, paru, apk, zypper)"
            ))),
        }
    }
}

/// One package pin in a native declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Manager-specific extra: `cask`, `tap`, `formula`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

impl PackageSpec {
    pub fn package(name: impl Into<String>, version: Option<String>) -> Self {
        Self {
            name: name.into(),
            version,
            kind: None,
        }
    }

    pub fn install_token(&self, manager: PackageManager) -> String {
        match manager {
            PackageManager::Homebrew => self.name.clone(),
            PackageManager::Pacman => match &self.version {
                Some(v) => format!("{}={}", self.name, v),
                None => self.name.clone(),
            },
            PackageManager::Yay | PackageManager::Paru => self.name.clone(),
            PackageManager::Dnf => match &self.version {
                Some(v) => format!("{}-{}", self.name, v),
                None => self.name.clone(),
            },
            _ => match &self.version {
                Some(v) => format!("{}={}", self.name, v),
                None => self.name.clone(),
            },
        }
    }
}

/// Parsed native dump (text + pins). Not a fat binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageDeclaration {
    pub manager: PackageManager,
    pub distro_id: String,
    pub body: String,
    pub packages: Vec<PackageSpec>,
}

/// How capture obtains package declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageCaptureMode {
    /// Query the live manager (`brew list`, `pacman -Qe`, …). Same as [`Self::Live`].
    Auto,
    /// Only read profile paths (air-gapped / explicit). Not the default.
    Files,
    /// Dump installed packages from the manager command. Does not read user `.list` files.
    Live,
}

impl FromStr for PackageCaptureMode {
    type Err = CycloneError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "files" | "file" => Ok(Self::Files),
            "live" => Ok(Self::Live),
            _ => Err(CycloneError::User(format!(
                "unknown --packages mode `{s}` (auto, files, live)"
            ))),
        }
    }
}

impl Default for PackageCaptureMode {
    fn default() -> Self {
        Self::Live
    }
}

/// Capture-time package options.
#[derive(Debug, Clone)]
pub struct PackageCaptureOptions {
    pub mode: PackageCaptureMode,
    pub distro: Option<Distro>,
    pub commands: CommandProbe,
}

impl Default for PackageCaptureOptions {
    fn default() -> Self {
        Self {
            mode: PackageCaptureMode::Live,
            distro: None,
            commands: CommandProbe::System,
        }
    }
}

/// Restore-time package options.
#[derive(Debug, Clone)]
pub struct PackageRestoreOptions {
    pub distro: Option<Distro>,
    pub manager: Option<PackageManager>,
    /// Run the matching manager's install command (not just write the declaration file).
    pub install: bool,
    pub commands: CommandProbe,
}

impl Default for PackageRestoreOptions {
    fn default() -> Self {
        Self {
            distro: None,
            manager: None,
            install: false,
            commands: CommandProbe::System,
        }
    }
}

/// OS + distro + native manager. Distro is detected; Ubuntu is not assumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostEnv {
    pub platform: Platform,
    pub distro: Distro,
    pub manager: Option<PackageManager>,
    /// `yay` / `paru` on PATH. Not a substitute for pacman; AUR is captured separately.
    pub aur_helpers: Vec<PackageManager>,
}

impl HostEnv {
    pub fn detect() -> Result<Self> {
        Self::detect_for(Platform::current()?, &CommandProbe::System)
    }

    pub fn detect_for(platform: Platform, commands: &CommandProbe) -> Result<Self> {
        let distro = distro_override().unwrap_or_else(|| match platform {
            Platform::MacOs => Distro::macos(),
            Platform::Linux => Distro::detect_linux(),
        });
        let manager =
            manager_override().unwrap_or_else(|| detect_manager(platform, &distro, commands));
        Ok(Self {
            platform,
            distro,
            manager,
            aur_helpers: detect_aur_helpers(commands),
        })
    }

    pub fn assumed(
        platform: Platform,
        distro: Option<Distro>,
        manager: Option<PackageManager>,
        commands: &CommandProbe,
    ) -> Self {
        let distro = distro
            .or_else(distro_override)
            .unwrap_or_else(|| match platform {
                Platform::MacOs => Distro::macos(),
                Platform::Linux => Distro::detect_linux(),
            });
        let manager = manager.or_else(|| {
            manager_override().unwrap_or_else(|| match platform {
                Platform::MacOs => Some(PackageManager::Homebrew),
                Platform::Linux => detect_manager(platform, &distro, commands),
            })
        });
        Self {
            platform,
            distro,
            manager,
            aur_helpers: detect_aur_helpers(commands),
        }
    }

    pub fn manager_present(&self, commands: &CommandProbe) -> bool {
        self.manager
            .map(|m| m.present_on(commands))
            .unwrap_or(false)
    }

    pub fn has_helper(&self, pm: PackageManager) -> bool {
        self.aur_helpers.contains(&pm)
    }
}

fn distro_override() -> Option<Distro> {
    env::var("CYCLONE_FAKE_DISTRO")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| Distro::from_id(&s))
}

fn manager_override() -> Option<Option<PackageManager>> {
    match env::var("CYCLONE_FAKE_PACKAGE_MANAGER") {
        Ok(s) if s.is_empty() || s == "none" => Some(None),
        Ok(s) => Some(s.parse().ok()),
        Err(_) => None,
    }
}

pub fn preferred_manager(distro: &Distro) -> Option<PackageManager> {
    if distro.id == "macos" {
        return Some(PackageManager::Homebrew);
    }
    for token in distro.tokens() {
        match token {
            "alpine" => return Some(PackageManager::Apk),
            "arch" | "archlinux" | "manjaro" | "endeavouros" | "cachyos" | "garuda" => {
                return Some(PackageManager::Pacman);
            }
            "opensuse-tumbleweed" | "opensuse-leap" | "opensuse" | "sles" | "suse" => {
                return Some(PackageManager::Zypper);
            }
            "fedora" | "rhel" | "centos" | "rocky" | "almalinux" | "ol" | "nobara" | "mageia" => {
                return Some(PackageManager::Dnf);
            }
            "debian" | "ubuntu" | "linuxmint" | "pop" | "elementary" | "raspbian" => {
                return Some(PackageManager::Apt);
            }
            _ => {}
        }
    }
    None
}

pub fn is_arch_family(distro: &Distro) -> bool {
    matches!(preferred_manager(distro), Some(PackageManager::Pacman))
}

fn detect_aur_helpers(commands: &CommandProbe) -> Vec<PackageManager> {
    [PackageManager::Yay, PackageManager::Paru]
        .into_iter()
        .filter(|pm| pm.present_on(commands))
        .collect()
}

fn detect_manager(
    platform: Platform,
    distro: &Distro,
    commands: &CommandProbe,
) -> Option<PackageManager> {
    if platform == Platform::MacOs {
        return Some(PackageManager::Homebrew);
    }
    let preferred = preferred_manager(distro);
    if let Some(pm) = preferred {
        if pm.present_on(commands) {
            return Some(pm);
        }
    }
    [
        PackageManager::Apt,
        PackageManager::Dnf,
        PackageManager::Pacman,
        PackageManager::Apk,
        PackageManager::Zypper,
        PackageManager::Homebrew,
    ]
    .into_iter()
    .find(|pm| pm.present_on(commands))
}

pub fn infer_package_manager(
    id: &str,
    linux: Option<&str>,
    macos: Option<&str>,
    explicit: Option<PackageManager>,
) -> Option<PackageManager> {
    explicit
        .or_else(|| PackageManager::from_component_id(id))
        .or_else(|| linux.and_then(PackageManager::from_declaration_path))
        .or_else(|| macos.and_then(PackageManager::from_declaration_path))
}

/// Process runner used for dumps and installs. Tests inject [`CommandProbe::Scripted`].
#[derive(Debug, Clone)]
pub enum CommandProbe {
    System,
    Scripted(ScriptedCommands),
}

impl Default for CommandProbe {
    fn default() -> Self {
        Self::System
    }
}

impl CommandProbe {
    pub fn has(&self, name: &str) -> bool {
        match self {
            Self::System => command_on_path(name),
            Self::Scripted(s) => s.present.contains(name),
        }
    }

    pub fn output(&self, program: &str, args: &[&str]) -> Result<String> {
        match self {
            Self::System => system_output(program, args),
            Self::Scripted(s) => {
                s.record(program, args);
                s.lookup(program, args)
            }
        }
    }

    pub fn run(&self, program: &str, args: &[&str]) -> Result<()> {
        match self {
            Self::System => {
                let status = std::process::Command::new(program).args(args).status()?;
                if status.success() {
                    Ok(())
                } else {
                    Err(CycloneError::User(format!(
                        "{program} exited with {status}"
                    )))
                }
            }
            Self::Scripted(s) => {
                s.record(program, args);
                if s.present.contains(program) || program == "sudo" {
                    Ok(())
                } else {
                    Err(CycloneError::User(format!(
                        "{program} is not available in the scripted probe"
                    )))
                }
            }
        }
    }
}

fn system_output(program: &str, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new(program).args(args).output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(CycloneError::User(format!(
            "{program} failed: {}",
            err.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Recorded command table for tests (no real brew/apt).
#[derive(Debug, Clone, Default)]
pub struct ScriptedCommands {
    present: BTreeSet<String>,
    outputs: BTreeMap<String, String>,
    ran: Arc<Mutex<Vec<String>>>,
}

impl ScriptedCommands {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow(mut self, name: impl Into<String>) -> Self {
        self.present.insert(name.into());
        self
    }

    pub fn stdout(mut self, program: &str, args: &[&str], out: impl Into<String>) -> Self {
        self.present.insert(program.to_string());
        self.outputs.insert(cmd_key(program, args), out.into());
        self
    }

    pub fn ran(&self) -> Vec<String> {
        self.ran.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn record(&self, program: &str, args: &[&str]) {
        if let Ok(mut g) = self.ran.lock() {
            g.push(cmd_key(program, args));
        }
    }

    fn lookup(&self, program: &str, args: &[&str]) -> Result<String> {
        let key = cmd_key(program, args);
        self.outputs
            .get(&key)
            .cloned()
            .ok_or_else(|| CycloneError::User(format!("no scripted output for `{key}`")))
    }
}

fn cmd_key(program: &str, args: &[&str]) -> String {
    let mut s = program.to_string();
    for a in args {
        s.push(' ');
        s.push_str(a);
    }
    s
}

/// Dump live packages into a native declaration, or `None` if the manager is missing.
pub fn dump_live(
    manager: PackageManager,
    distro: &Distro,
    commands: &CommandProbe,
) -> Result<Option<PackageDeclaration>> {
    if !manager.present_on(commands) && !matches!(commands, CommandProbe::Scripted(_)) {
        return Ok(None);
    }
    if let CommandProbe::Scripted(s) = commands {
        if !manager.present_on(commands) && !s.present.contains(manager.command()) {
            return Ok(None);
        }
    }
    let packages = match manager {
        PackageManager::Homebrew => dump_homebrew(commands)?,
        PackageManager::Apt => dump_apt(commands)?,
        PackageManager::Dnf => dump_dnf(commands)?,
        PackageManager::Pacman => dump_pacman(commands)?,
        PackageManager::Yay => dump_aur_helper("yay", commands)?,
        PackageManager::Paru => dump_aur_helper("paru", commands)?,
        PackageManager::Apk => dump_apk(commands)?,
        PackageManager::Zypper => dump_zypper(commands)?,
    };
    Ok(Some(build_declaration(manager, distro, packages)))
}

pub fn parse_declaration(
    manager: PackageManager,
    distro_hint: &str,
    body: &str,
) -> PackageDeclaration {
    let mut distro_id = distro_hint.to_string();
    let mut parsed_manager = manager;
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix(HEADER_MANAGER) {
            if let Ok(m) = rest.trim().parse() {
                parsed_manager = m;
            }
        }
        if let Some(rest) = line.strip_prefix(HEADER_DISTRO) {
            distro_id = rest.trim().to_string();
        }
    }
    let packages = parse_body(parsed_manager, body);
    PackageDeclaration {
        manager: parsed_manager,
        distro_id,
        body: body.to_string(),
        packages,
    }
}

pub fn build_declaration(
    manager: PackageManager,
    distro: &Distro,
    packages: Vec<PackageSpec>,
) -> PackageDeclaration {
    let body = render_declaration(manager, &distro.id, &packages);
    PackageDeclaration {
        manager,
        distro_id: distro.id.clone(),
        body,
        packages,
    }
}

pub fn render_declaration(
    manager: PackageManager,
    distro_id: &str,
    packages: &[PackageSpec],
) -> String {
    let mut out = format!(
        "{HEADER_MANAGER} {}\n{HEADER_DISTRO} {distro_id}\n",
        manager.as_str()
    );
    match manager {
        PackageManager::Homebrew => {
            for p in packages {
                match p.kind.as_deref() {
                    Some("tap") => out.push_str(&format!("tap \"{}\"\n", p.name)),
                    Some("cask") => out.push_str(&format!("cask \"{}\"\n", p.name)),
                    _ => match &p.version {
                        Some(v) => out.push_str(&format!("brew \"{}\" # {v}\n", p.name)),
                        None => out.push_str(&format!("brew \"{}\"\n", p.name)),
                    },
                }
            }
        }
        PackageManager::Pacman | PackageManager::Yay | PackageManager::Paru => {
            for p in packages {
                match &p.version {
                    Some(v) => out.push_str(&format!("{} {v}\n", p.name)),
                    None => out.push_str(&format!("{}\n", p.name)),
                }
            }
        }
        _ => {
            for p in packages {
                match &p.version {
                    Some(v) => out.push_str(&format!("{}={v}\n", p.name)),
                    None => out.push_str(&format!("{}\n", p.name)),
                }
            }
        }
    }
    out
}

fn parse_body(manager: PackageManager, body: &str) -> Vec<PackageSpec> {
    match manager {
        PackageManager::Homebrew => parse_brewfile(body),
        PackageManager::Pacman | PackageManager::Yay | PackageManager::Paru => {
            parse_name_version_lines(body, ' ')
        }
        _ => parse_name_eq_lines(body),
    }
}

fn parse_brewfile(body: &str) -> Vec<PackageSpec> {
    let mut out = Vec::new();
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("tap ") {
            if let Some(name) = brew_quoted(rest) {
                out.push(PackageSpec {
                    name,
                    version: None,
                    kind: Some("tap".into()),
                });
            }
        } else if let Some(rest) = line.strip_prefix("cask ") {
            if let Some(name) = brew_quoted(rest) {
                out.push(PackageSpec {
                    name,
                    version: brew_version_comment(rest),
                    kind: Some("cask".into()),
                });
            }
        } else if let Some(rest) = line.strip_prefix("brew ") {
            if let Some(name) = brew_quoted(rest) {
                out.push(PackageSpec {
                    name,
                    version: brew_version_comment(rest),
                    kind: Some("formula".into()),
                });
            }
        }
    }
    out
}

fn brew_quoted(rest: &str) -> Option<String> {
    let rest = rest.trim();
    let bytes = rest.as_bytes();
    if bytes.first() != Some(&b'"') {
        return rest.split(',').next().map(|s| s.trim().replace('"', ""));
    }
    let end = rest[1..].find('"')?;
    Some(rest[1..1 + end].to_string())
}

fn brew_version_comment(rest: &str) -> Option<String> {
    rest.split_once('#')
        .map(|(_, c)| c.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_name_eq_lines(body: &str) -> Vec<PackageSpec> {
    let mut out = Vec::new();
    for raw in body.lines() {
        let line = strip_comment(raw);
        if line.is_empty() {
            continue;
        }
        if let Some((name, ver)) = line.split_once('=') {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            out.push(PackageSpec::package(
                name,
                Some(ver.trim().to_string()).filter(|s| !s.is_empty()),
            ));
        } else if let Some((name, ver)) = line.split_once('\t') {
            out.push(PackageSpec::package(
                name.trim(),
                Some(ver.trim().to_string()).filter(|s| !s.is_empty()),
            ));
        } else {
            out.push(PackageSpec::package(line, None));
        }
    }
    out
}

fn parse_name_version_lines(body: &str, sep: char) -> Vec<PackageSpec> {
    let mut out = Vec::new();
    for raw in body.lines() {
        let line = strip_comment(raw);
        if line.is_empty() {
            continue;
        }
        if let Some((name, ver)) = line.split_once(sep) {
            out.push(PackageSpec::package(
                name.trim(),
                Some(ver.trim().to_string()).filter(|s| !s.is_empty()),
            ));
        } else {
            out.push(PackageSpec::package(line, None));
        }
    }
    out
}

fn strip_comment(raw: &str) -> &str {
    let line = raw.trim();
    if line.starts_with('#') || line.starts_with("S ") || line.starts_with("---") {
        return "";
    }
    line.split('#').next().unwrap_or(line).trim()
}

fn dump_homebrew(commands: &CommandProbe) -> Result<Vec<PackageSpec>> {
    let mut packages = Vec::new();
    if let Ok(taps) = commands.output("brew", &["tap"]) {
        for tap in taps.lines() {
            let tap = tap.trim();
            if tap.is_empty() || tap == "homebrew/core" || tap == "homebrew/cask" {
                continue;
            }
            packages.push(PackageSpec {
                name: tap.to_string(),
                version: None,
                kind: Some("tap".into()),
            });
        }
    }
    let formulae = commands
        .output("brew", &["list", "--formula", "--versions"])
        .or_else(|_| commands.output("brew", &["list", "--versions"]))
        .or_else(|_| commands.output("brew", &["list"]));
    if let Ok(formulae) = formulae {
        packages.extend(parse_brew_list(&formulae, "formula"));
    }
    let casks = commands
        .output("brew", &["list", "--cask", "--versions"])
        .or_else(|_| commands.output("brew", &["list", "--cask"]));
    if let Ok(casks) = casks {
        packages.extend(parse_brew_list(&casks, "cask"));
    }
    if packages.is_empty() {
        if let Ok(bundle) = commands.output("brew", &["bundle", "dump", "--file", "-"]) {
            packages = parse_brewfile(&bundle);
        }
    }
    Ok(packages)
}

fn parse_brew_list(body: &str, kind: &str) -> Vec<PackageSpec> {
    body.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            let version = parts.next().map(str::to_string);
            Some(PackageSpec {
                name: name.to_string(),
                version,
                kind: Some(kind.to_string()),
            })
        })
        .collect()
}

fn dump_apt(commands: &CommandProbe) -> Result<Vec<PackageSpec>> {
    let manual = commands.output("apt-mark", &["showmanual"])?;
    let versions = commands
        .output("dpkg-query", &["-W", "-f=${Package}\t${Version}\n"])
        .unwrap_or_default();
    let ver_map = dpkg_versions(&versions);
    let mut packages = Vec::new();
    for name in manual.lines() {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        packages.push(PackageSpec::package(name, ver_map.get(name).cloned()));
    }
    Ok(packages)
}

fn dpkg_versions(body: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in body.lines() {
        let mut parts = line.split('\t');
        if let (Some(name), Some(ver)) = (parts.next(), parts.next()) {
            map.insert(name.trim().to_string(), ver.trim().to_string());
        }
    }
    map
}

fn dump_dnf(commands: &CommandProbe) -> Result<Vec<PackageSpec>> {
    let program = if commands.has("dnf") { "dnf" } else { "yum" };
    let body = commands
        .output(
            program,
            &[
                "repoquery",
                "--userinstalled",
                "--queryformat",
                "%{name}=%{version}-%{release}\n",
            ],
        )
        .or_else(|_| commands.output(program, &["repoquery", "--userinstalled"]))?;
    Ok(parse_name_eq_lines(&body))
}

fn dump_pacman(commands: &CommandProbe) -> Result<Vec<PackageSpec>> {
    // When an AUR helper is present, official repo packages only (`-n`).
    // Otherwise keep all explicit packages (`-Qe`) so foreign pkgs are not dropped.
    let args: &[&str] =
        if PackageManager::Yay.present_on(commands) || PackageManager::Paru.present_on(commands) {
            &["-Qne"]
        } else {
            &["-Qe"]
        };
    let body = commands
        .output("pacman", args)
        .or_else(|_| commands.output("pacman", &["-Q"]))?;
    Ok(parse_name_version_lines(&body, ' '))
}

fn dump_aur_helper(program: &str, commands: &CommandProbe) -> Result<Vec<PackageSpec>> {
    // Explicitly installed foreign packages (AUR / local), not official repos.
    let body = commands.output(program, &["-Qme"])?;
    let mut packages = parse_name_version_lines(&body, ' ');
    for p in &mut packages {
        p.kind = Some("aur".into());
    }
    Ok(packages)
}

fn dump_apk(commands: &CommandProbe) -> Result<Vec<PackageSpec>> {
    let body = commands
        .output("apk", &["info", "-e"])
        .or_else(|_| commands.output("apk", &["info"]))?;
    Ok(parse_name_eq_lines(&body))
}

fn dump_zypper(commands: &CommandProbe) -> Result<Vec<PackageSpec>> {
    let body = commands.output(
        "zypper",
        &[
            "--non-interactive",
            "--quiet",
            "search",
            "--installed-only",
            "--type",
            "package",
        ],
    )?;
    if body.contains(HEADER_MANAGER) || body.lines().any(|l| l.contains('=')) {
        return Ok(parse_name_eq_lines(&body));
    }
    Ok(parse_zypper_table(&body))
}

fn parse_zypper_table(body: &str) -> Vec<PackageSpec> {
    let mut out = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if !line.starts_with("i ") && !line.starts_with("i+") {
            continue;
        }
        let cols: Vec<&str> = line.split('|').map(str::trim).collect();
        if cols.len() < 4 {
            continue;
        }
        let name = cols[1];
        let version = cols.get(3).map(|s| s.to_string()).filter(|s| !s.is_empty());
        if name.is_empty() || name == "Name" {
            continue;
        }
        out.push(PackageSpec::package(name, version));
    }
    out
}

/// Shell-style install command for dry-run output.
pub fn install_command_line(
    manager: PackageManager,
    packages: &[PackageSpec],
    file: Option<&Path>,
    use_sudo: bool,
) -> String {
    let (program, args) = install_invocation(manager, packages, file);
    let mut parts = Vec::new();
    if use_sudo && manager.install_needs_root() {
        parts.push("sudo".to_string());
    }
    parts.push(program);
    parts.extend(args);
    parts.join(" ")
}

pub fn install_invocation(
    manager: PackageManager,
    packages: &[PackageSpec],
    file: Option<&Path>,
) -> (String, Vec<String>) {
    match manager {
        PackageManager::Homebrew => {
            let mut args = vec!["bundle".into()];
            if let Some(path) = file {
                args.push("--file".into());
                args.push(path.display().to_string());
            }
            ("brew".into(), args)
        }
        PackageManager::Apt => {
            let mut args = vec!["install".into(), "-y".into()];
            args.extend(install_tokens(manager, packages));
            ("apt-get".into(), args)
        }
        PackageManager::Dnf => {
            let program = "dnf".to_string();
            let mut args = vec!["install".into(), "-y".into()];
            args.extend(install_tokens(manager, packages));
            (program, args)
        }
        PackageManager::Pacman => {
            let mut args = vec!["-S".into(), "--needed".into(), "--noconfirm".into()];
            args.extend(install_tokens(manager, packages));
            ("pacman".into(), args)
        }
        PackageManager::Yay => {
            let mut args = vec!["-S".into(), "--needed".into(), "--noconfirm".into()];
            args.extend(install_tokens(manager, packages));
            ("yay".into(), args)
        }
        PackageManager::Paru => {
            let mut args = vec!["-S".into(), "--needed".into(), "--noconfirm".into()];
            args.extend(install_tokens(manager, packages));
            ("paru".into(), args)
        }
        PackageManager::Apk => {
            let mut args = vec!["add".into()];
            args.extend(install_tokens(manager, packages));
            ("apk".into(), args)
        }
        PackageManager::Zypper => {
            let mut args = vec!["--non-interactive".into(), "install".into()];
            args.extend(install_tokens(manager, packages));
            ("zypper".into(), args)
        }
    }
}

fn install_tokens(manager: PackageManager, packages: &[PackageSpec]) -> Vec<String> {
    packages
        .iter()
        .filter(|p| p.kind.as_deref() != Some("tap"))
        .map(|p| p.install_token(manager))
        .collect()
}

pub fn run_install(
    manager: PackageManager,
    packages: &[PackageSpec],
    file: Option<&Path>,
    commands: &CommandProbe,
) -> Result<()> {
    if packages.is_empty() && !matches!(manager, PackageManager::Homebrew) {
        return Ok(());
    }
    let (program, args) = install_invocation(manager, packages, file);
    let use_sudo = manager.install_needs_root() && !euid_is_zero();
    if use_sudo {
        let mut sudo_args = vec!["-n".to_string(), program];
        sudo_args.extend(args);
        let borrowed: Vec<&str> = sudo_args.iter().map(String::as_str).collect();
        commands.run("sudo", &borrowed)
    } else {
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        commands.run(&program, &borrowed)
    }
}

fn euid_is_zero() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines().find_map(|line| {
                line.strip_prefix("Uid:")
                    .and_then(|rest| rest.split_whitespace().next())
                    .map(|uid| uid == "0")
            })
        })
        .unwrap_or(false)
}

/// Actionable dry-run text when the destination manager cannot apply a dump.
pub fn mismatch_detail(captured: &PackageDeclaration, dest: &HostEnv) -> String {
    let dest_mgr = dest
        .manager
        .map(PackageManager::display_name)
        .unwrap_or("no matching package manager");
    let names = summarize_names(&captured.packages);
    if captured.manager.is_aur_helper() {
        return format!(
            "{} AUR packages captured on {} cannot be applied on {} ({}); \
need an Arch-family host with {}; skipped packages: {names}",
            captured.manager.display_name(),
            captured.distro_id,
            dest.distro.id,
            dest_mgr,
            captured.manager.command()
        );
    }
    format!(
        "{} declarations captured on {} cannot be applied with {dest_mgr} on {}; \
recapture on this distro, or restore on a host with {}; skipped packages: {names}",
        captured.manager.display_name(),
        captured.distro_id,
        dest.distro.id,
        captured.manager.display_name()
    )
}

pub fn missing_aur_helper_detail(captured: &PackageDeclaration, dest: &HostEnv) -> String {
    format!(
        "{} is not on PATH on {} ({}); AUR packages cannot be applied without {}. packages: {}",
        captured.manager.command(),
        dest.platform,
        dest.distro.id,
        captured.manager.command(),
        summarize_names(&captured.packages)
    )
}

pub fn summarize_names(packages: &[PackageSpec]) -> String {
    let names: Vec<&str> = packages
        .iter()
        .filter(|p| p.kind.as_deref() != Some("tap"))
        .map(|p| p.name.as_str())
        .collect();
    if names.is_empty() {
        return "(none)".into();
    }
    const LIMIT: usize = 12;
    if names.len() <= LIMIT {
        names.join(", ")
    } else {
        format!(
            "{}, and {} more",
            names[..LIMIT].join(", "),
            names.len() - LIMIT
        )
    }
}

pub fn should_live_dump(mode: PackageCaptureMode) -> bool {
    !matches!(mode, PackageCaptureMode::Files)
}

pub fn should_read_file(mode: PackageCaptureMode, file_exists: bool) -> bool {
    file_exists && matches!(mode, PackageCaptureMode::Files)
}

/// Catalog ids in stable order (Homebrew first, then Linux managers).
pub fn catalog_managers() -> &'static [PackageManager] {
    &[
        PackageManager::Homebrew,
        PackageManager::Apt,
        PackageManager::Dnf,
        PackageManager::Pacman,
        PackageManager::Yay,
        PackageManager::Paru,
        PackageManager::Apk,
        PackageManager::Zypper,
    ]
}

pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/packages")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_fixture(name: &str) -> String {
        std::fs::read_to_string(fixtures_dir().join(name)).expect(name)
    }

    fn os_release(name: &str) -> Distro {
        let text = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/os-release")
                .join(name),
        )
        .expect(name);
        Distro::from_os_release(&text)
    }

    #[test]
    fn parses_fixture_lists_per_manager() {
        let brew = parse_declaration(PackageManager::Homebrew, "macos", &read_fixture("Brewfile"));
        assert_eq!(brew.manager, PackageManager::Homebrew);
        assert!(brew
            .packages
            .iter()
            .any(|p| p.name == "git" && p.kind.as_deref() == Some("formula")));
        assert!(brew
            .packages
            .iter()
            .any(|p| p.name == "firefox" && p.kind.as_deref() == Some("cask")));
        assert!(brew
            .packages
            .iter()
            .any(|p| p.name == "homebrew/cask-fonts"));

        let apt = parse_declaration(PackageManager::Apt, "linux", &read_fixture("apt.list"));
        assert_eq!(apt.distro_id, "ubuntu");
        assert!(apt
            .packages
            .iter()
            .any(|p| p.name == "git" && p.version.as_deref() == Some("1:2.43.0-1ubuntu1")));
        assert!(apt
            .packages
            .iter()
            .any(|p| p.name == "build-essential" && p.version.is_none()));

        let dnf = parse_declaration(PackageManager::Dnf, "linux", &read_fixture("dnf.list"));
        assert_eq!(dnf.distro_id, "fedora");
        assert!(dnf.packages.iter().any(|p| p.name == "ripgrep"));

        let pacman = parse_declaration(
            PackageManager::Pacman,
            "linux",
            &read_fixture("pacman.list"),
        );
        assert_eq!(pacman.distro_id, "arch");
        assert!(pacman
            .packages
            .iter()
            .any(|p| p.name == "curl" && p.version.as_deref() == Some("8.11.0-1")));

        let apk = parse_declaration(PackageManager::Apk, "linux", &read_fixture("apk.world"));
        assert_eq!(apk.distro_id, "alpine");
        assert!(apk.packages.iter().any(|p| p.name == "git"));

        let zypper = parse_declaration(
            PackageManager::Zypper,
            "linux",
            &read_fixture("zypper.list"),
        );
        assert_eq!(zypper.distro_id, "opensuse-tumbleweed");
        assert!(zypper.packages.iter().any(|p| p.name == "curl"));

        let yay = parse_declaration(PackageManager::Yay, "linux", &read_fixture("yay.list"));
        assert_eq!(yay.distro_id, "arch");
        assert!(yay
            .packages
            .iter()
            .any(|p| p.name == "visual-studio-code-bin"));
        let paru = parse_declaration(PackageManager::Paru, "linux", &read_fixture("paru.list"));
        assert_eq!(paru.distro_id, "arch");
        assert!(paru.packages.iter().any(|p| p.name == "google-chrome"));
    }

    #[test]
    fn os_release_does_not_assume_ubuntu() {
        assert_eq!(
            preferred_manager(&os_release("ubuntu")),
            Some(PackageManager::Apt)
        );
        assert_eq!(
            preferred_manager(&os_release("fedora")),
            Some(PackageManager::Dnf)
        );
        assert_eq!(
            preferred_manager(&os_release("arch")),
            Some(PackageManager::Pacman)
        );
        assert_eq!(
            preferred_manager(&os_release("alpine")),
            Some(PackageManager::Apk)
        );
        assert_eq!(
            preferred_manager(&os_release("opensuse")),
            Some(PackageManager::Zypper)
        );
        assert_eq!(os_release("fedora").id, "fedora");
        assert_ne!(os_release("fedora").id, "ubuntu");
    }

    #[test]
    fn live_dump_uses_scripted_manager_output() {
        let commands = CommandProbe::Scripted(
            ScriptedCommands::new()
                .allow("apt-get")
                .stdout("apt-mark", &["showmanual"], "git\ncurl\n")
                .stdout(
                    "dpkg-query",
                    &["-W", "-f=${Package}\t${Version}\n"],
                    "git\t1:2.43.0\ncurl\t8.5.0\n",
                ),
        );
        let decl = dump_live(PackageManager::Apt, &Distro::from_id("debian"), &commands)
            .unwrap()
            .expect("dump");
        assert_eq!(decl.distro_id, "debian");
        assert_eq!(decl.packages.len(), 2);
        assert!(decl.body.contains("git=1:2.43.0"));
    }

    #[test]
    fn mismatch_diff_lists_skipped_packages() {
        let captured =
            parse_declaration(PackageManager::Homebrew, "macos", &read_fixture("Brewfile"));
        let dest = HostEnv {
            platform: Platform::Linux,
            distro: Distro::from_id("ubuntu"),
            manager: Some(PackageManager::Apt),
            aur_helpers: Vec::new(),
        };
        let detail = mismatch_detail(&captured, &dest);
        assert!(detail.contains("Homebrew"));
        assert!(detail.contains("apt"));
        assert!(detail.contains("ubuntu"));
        assert!(detail.contains("git"));
        assert!(detail.contains("ripgrep"));
        assert!(detail.contains("recapture"));
    }

    #[test]
    fn install_line_is_native_not_unified() {
        let pkgs = vec![PackageSpec::package("git", Some("2.47.0".into()))];
        let apt = install_command_line(PackageManager::Apt, &pkgs, None, true);
        assert!(apt.starts_with("sudo apt-get install -y git=2.47.0"));
        let dnf = install_command_line(PackageManager::Dnf, &pkgs, None, true);
        assert!(dnf.contains("dnf install -y git-2.47.0"));
        let brew = install_command_line(
            PackageManager::Homebrew,
            &pkgs,
            Some(Path::new("/tmp/Brewfile")),
            false,
        );
        assert_eq!(brew, "brew bundle --file /tmp/Brewfile");
        let pacman = install_command_line(PackageManager::Pacman, &pkgs, None, true);
        assert!(pacman.contains("pacman -S --needed --noconfirm git=2.47.0"));
        let yay = install_command_line(PackageManager::Yay, &pkgs, None, false);
        assert_eq!(yay, "yay -S --needed --noconfirm git");
        let paru = install_command_line(PackageManager::Paru, &pkgs, None, false);
        assert_eq!(paru, "paru -S --needed --noconfirm git");
    }

    #[test]
    fn live_dump_splits_official_pacman_from_aur_helper() {
        let commands = CommandProbe::Scripted(
            ScriptedCommands::new()
                .allow("pacman")
                .allow("yay")
                .stdout("pacman", &["-Qne"], "git 2.47.0-1\nripgrep 14.1.1-1\n")
                .stdout(
                    "yay",
                    &["-Qme"],
                    "visual-studio-code-bin 1.95.3-1\nspotify 1:1.2.52-1\n",
                ),
        );
        let distro = Distro::from_id("arch");
        let official = dump_live(PackageManager::Pacman, &distro, &commands)
            .unwrap()
            .expect("pacman dump");
        assert!(official.packages.iter().any(|p| p.name == "git"));
        assert!(official
            .packages
            .iter()
            .all(|p| p.name != "visual-studio-code-bin"));

        let aur = dump_live(PackageManager::Yay, &distro, &commands)
            .unwrap()
            .expect("yay dump");
        assert!(aur
            .packages
            .iter()
            .any(|p| p.name == "visual-studio-code-bin"));
        assert!(aur.packages.iter().all(|p| p.name != "git"));
        assert!(aur.body.contains("# cyclone.package-manager: yay"));
    }

    #[test]
    fn aur_mismatch_on_non_arch_names_helper() {
        let captured = parse_declaration(PackageManager::Yay, "arch", &read_fixture("yay.list"));
        let dest = HostEnv {
            platform: Platform::Linux,
            distro: Distro::from_id("ubuntu"),
            manager: Some(PackageManager::Apt),
            aur_helpers: Vec::new(),
        };
        let detail = mismatch_detail(&captured, &dest);
        assert!(detail.contains("AUR"));
        assert!(detail.contains("yay"));
        assert!(detail.contains("ubuntu"));
        assert!(detail.contains("visual-studio-code-bin"));
    }

    #[test]
    fn live_and_auto_query_the_manager_not_list_files() {
        assert!(should_live_dump(PackageCaptureMode::Live));
        assert!(should_live_dump(PackageCaptureMode::Auto));
        assert!(!should_live_dump(PackageCaptureMode::Files));
        assert!(!should_read_file(PackageCaptureMode::Live, true));
        assert!(!should_read_file(PackageCaptureMode::Auto, true));
        assert!(should_read_file(PackageCaptureMode::Files, true));
        assert!(!should_read_file(PackageCaptureMode::Files, false));
    }

    #[test]
    fn live_dump_homebrew_uses_brew_list_not_a_brewfile() {
        let commands = CommandProbe::Scripted(
            ScriptedCommands::new()
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
        );
        let decl = dump_live(
            PackageManager::Homebrew,
            &Distro::from_id("macos"),
            &commands,
        )
        .unwrap()
        .expect("dump");
        assert!(decl.packages.iter().any(|p| p.name == "git"));
        assert!(decl.packages.iter().any(|p| p.name == "firefox"));
        assert!(decl.body.contains("brew \"git\""));
    }
}
