use std::path::{Path, PathBuf};

use crate::error::{CycloneError, Result};
use crate::platform::Platform;

/// Expand `~` / `~/...` against `home`. Absolute and other relative paths are unchanged.
pub fn expand_tilde(path: &str, home: &Path) -> PathBuf {
    if path == "~" {
        home.to_path_buf()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home.join(rest)
    } else if let Some(rest) = path.strip_prefix("~\\") {
        home.join(rest)
    } else {
        PathBuf::from(path)
    }
}

/// Render `abs` as `~/...` when it lives under `home`.
pub fn to_logical(abs: &Path, home: &Path) -> String {
    match abs.strip_prefix(home) {
        Ok(rel) if rel.as_os_str().is_empty() => "~".to_string(),
        Ok(rel) => format!("~/{}", normalize_rel(rel)),
        Err(_) => abs.display().to_string(),
    }
}

/// Join a component-relative path (`/` separators) onto an expanded component root.
pub fn join_relative(root: &Path, relative: &str) -> PathBuf {
    if relative.is_empty() {
        root.to_path_buf()
    } else {
        let mut out = root.to_path_buf();
        for part in relative.split('/') {
            if part.is_empty() || part == "." {
                continue;
            }
            out.push(part);
        }
        out
    }
}

pub fn normalize_rel(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Resolve the home directory used for `~` expansion.
pub fn default_home() -> Result<PathBuf> {
    if let Ok(h) = std::env::var("CYCLONE_HOME") {
        if !h.is_empty() {
            return Ok(PathBuf::from(h));
        }
    }
    directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .ok_or_else(|| CycloneError::User("cannot resolve home directory".into()))
}

/// Map a captured component onto the destination OS home.
pub fn mapped_dest(dest_logical_root: &str, relative: &str, dest_home: &Path) -> PathBuf {
    join_relative(&expand_tilde(dest_logical_root, dest_home), relative)
}

pub fn logical_root_for(
    linux: Option<&str>,
    macos: Option<&str>,
    platform: Platform,
) -> Option<String> {
    match platform {
        Platform::Linux => linux.map(str::to_string),
        Platform::MacOs => macos.map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_and_round_trips_tilde() {
        let home = Path::new("/home/ada");
        let p = expand_tilde("~/.config/nvim", home);
        assert_eq!(p, PathBuf::from("/home/ada/.config/nvim"));
        assert_eq!(to_logical(&p, home), "~/.config/nvim");
    }

    #[test]
    fn maps_vscode_settings_linux_to_macos() {
        let dest_home = Path::new("/Users/ada");
        let dest = mapped_dest(
            "~/Library/Application Support/Code/User/settings.json",
            "",
            dest_home,
        );
        assert_eq!(
            dest,
            PathBuf::from("/Users/ada/Library/Application Support/Code/User/settings.json")
        );
    }

    #[test]
    fn maps_config_tree_children() {
        let dest = mapped_dest("~/.config/nvim", "lua/init.lua", Path::new("/home/b"));
        assert_eq!(dest, PathBuf::from("/home/b/.config/nvim/lua/init.lua"));
    }
}
