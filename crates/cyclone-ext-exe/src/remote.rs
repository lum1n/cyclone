use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cyclone_core::{CapturedArtifact, InnerBundle, ManifestComponent, Platform};

use crate::client::EXEC_BODY_LIMIT;
use crate::error::ExeDevError;

/// Bytes of file content per HTTPS write so the full `POST /exec` body stays under 64KiB.
const RAW_CHUNK: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteWrite {
    pub component_id: String,
    /// Path relative to the VM `$HOME`, using `/` separators.
    pub home_relative: String,
    pub bytes: Vec<u8>,
    pub mode: u32,
}

/// Files that would be restored onto an exe.dev VM (Linux / exeuntu).
pub fn planned_writes(image: &InnerBundle) -> Result<Vec<RemoteWrite>, ExeDevError> {
    let mut out = Vec::new();
    for component in &image.manifest.components {
        if !component.supports(Platform::Linux) {
            continue;
        }
        let Some(logical) = component.path_for(Platform::Linux) else {
            continue;
        };
        for artifact in image.payload.files_for(&component.id) {
            let rel = home_relative(logical, &artifact.relative_path, component)?;
            if !is_safe_rel(&rel) {
                return Err(ExeDevError::Message(format!(
                    "refusing to write unsafe path `{rel}` from component `{}`",
                    component.id
                )));
            }
            out.push(RemoteWrite {
                component_id: component.id.clone(),
                home_relative: rel,
                bytes: artifact.bytes.clone(),
                mode: normalize_mode(artifact),
            });
        }
    }
    Ok(out)
}

fn normalize_mode(artifact: &CapturedArtifact) -> u32 {
    if artifact.mode == 0 {
        0o644
    } else {
        artifact.mode
    }
}

/// Map a Linux logical component path (`~/...`) plus artifact relative path onto `$HOME`.
pub fn home_relative(
    logical: &str,
    relative: &str,
    component: &ManifestComponent,
) -> Result<String, ExeDevError> {
    let rest = logical.strip_prefix("~/").ok_or_else(|| {
        ExeDevError::Message(format!(
            "component `{}` linux path `{logical}` is not $HOME-relative; provision only writes under the VM home",
            component.id
        ))
    })?;
    let mut parts: Vec<&str> = rest
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    if !relative.is_empty() {
        parts.extend(relative.split('/').filter(|p| !p.is_empty() && *p != "."));
    }
    Ok(parts.join("/"))
}

pub fn is_safe_rel(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.contains('\0') {
        return false;
    }
    path.split('/')
        .all(|c| !c.is_empty() && c != "." && c != ".." && !c.contains('\'') && !c.contains('"'))
}

/// Validate an exe.dev VM name (hostname label).
pub fn validate_vm_name(id: &str) -> Result<(), ExeDevError> {
    let valid = (1..=63).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && id
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id
            .chars()
            .last()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(ExeDevError::InvalidTarget(id.to_string()))
    }
}

/// POSIX shell single-quoting (`foo'bar` → `'foo'\''bar'`).
pub fn posix_single_quote(s: &str) -> String {
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
}

pub fn ls_command(vm: &str) -> String {
    format!("ls --json {vm}")
}

pub fn new_command(vm: &str) -> String {
    format!("new --name {vm} --tag cyclone --json")
}

pub fn ssh_command(vm: &str, remote: &str) -> String {
    format!("ssh {vm} {}", posix_single_quote(remote))
}

/// Double-quoted `-c` argument for the VM shell; python source may contain `"`.
fn python3_c(py: &str) -> String {
    debug_assert!(
        !py.contains('\''),
        "python snippet must not contain single quotes (lobby wrapping)"
    );
    let escaped = py.replace('\\', "\\\\").replace('"', "\\\"");
    format!("python3 -c \"{escaped}\"")
}

/// One or more `ssh <vm> 'python3 -c …'` commands that write `bytes` under `$HOME`.
pub fn write_file_commands(vm: &str, write: &RemoteWrite) -> Result<Vec<String>, ExeDevError> {
    let join = python_joinpath(&write.home_relative);
    let chunks: Vec<&[u8]> = if write.bytes.is_empty() {
        Vec::new()
    } else {
        write.bytes.chunks(RAW_CHUNK).collect()
    };
    if chunks.is_empty() {
        let py = format!(
            "from pathlib import Path; p={join}; p.parent.mkdir(parents=True, exist_ok=True); p.write_bytes(b\"\"); p.chmod({:#o})",
            write.mode
        );
        let cmd = ssh_command(vm, &python3_c(&py));
        ensure_fits(&cmd)?;
        return Ok(vec![cmd]);
    }
    let mut commands = Vec::new();
    let last = chunks.len() - 1;
    for (i, chunk) in chunks.iter().enumerate() {
        let b64 = STANDARD.encode(chunk);
        let mode = if i == last {
            format!("; p.chmod({:#o})", write.mode)
        } else {
            String::new()
        };
        let flag = if i == 0 { "wb" } else { "ab" };
        let py = format!(
            "from pathlib import Path; import base64; p={join}; p.parent.mkdir(parents=True, exist_ok=True); open(str(p),\"{flag}\").write(base64.b64decode(\"{b64}\")){mode}"
        );
        let cmd = ssh_command(vm, &python3_c(&py));
        ensure_fits(&cmd)?;
        commands.push(cmd);
    }
    Ok(commands)
}

fn python_joinpath(home_relative: &str) -> String {
    let args: Vec<String> = home_relative
        .split('/')
        .filter(|p| !p.is_empty())
        .map(|p| format!("\"{p}\""))
        .collect();
    format!("Path.home().joinpath({})", args.join(","))
}

fn ensure_fits(command: &str) -> Result<(), ExeDevError> {
    if command.len() > EXEC_BODY_LIMIT {
        Err(ExeDevError::Message(format!(
            "generated write command is {} bytes; POST /exec limit is {EXEC_BODY_LIMIT}",
            command.len()
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_name_rules() {
        assert!(validate_vm_name("dev-box").is_ok());
        assert!(validate_vm_name("a").is_ok());
        assert!(validate_vm_name("MyVM").is_err());
        assert!(validate_vm_name("-bad").is_err());
        assert!(validate_vm_name("bad-").is_err());
    }

    #[test]
    fn posix_quotes_apostrophes() {
        assert_eq!(posix_single_quote("foo"), "'foo'");
        assert_eq!(posix_single_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn write_commands_stay_under_limit() {
        let write = RemoteWrite {
            component_id: "blob".into(),
            home_relative: ".local/share/blob.bin".into(),
            bytes: vec![b'x'; 40_000],
            mode: 0o644,
        };
        let cmds = write_file_commands("dev-box", &write).unwrap();
        assert!(cmds.len() >= 2, "expected chunking, got {}", cmds.len());
        for c in &cmds {
            assert!(c.len() < EXEC_BODY_LIMIT, "len {}", c.len());
            assert!(c.starts_with("ssh dev-box 'python3 -c \""));
            assert!(c.ends_with('\''));
        }
    }

    #[test]
    fn write_command_has_no_plaintext_secret_in_action_name() {
        let write = RemoteWrite {
            component_id: "gitconfig".into(),
            home_relative: ".gitconfig".into(),
            bytes: b"[user]\n\tname = Test\n".to_vec(),
            mode: 0o644,
        };
        let cmds = write_file_commands("dev-box", &write).unwrap();
        assert_eq!(cmds.len(), 1);
        assert!(cmds[0].contains("base64.b64decode"));
    }
}
