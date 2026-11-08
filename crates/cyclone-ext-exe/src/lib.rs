//! # cyclone-ext-exe
//!
//! First cloud extension: bootstrap an [exe.dev](https://exe.dev) VM from an **encrypted**
//! Cyclone image. Cloud HTTP lives here; [`cyclone_core`] stays SDK-free.
//!
//! The CLI id is **`exe-dev`** (`cyclone provision --ext exe-dev`).
//!
//! ## HTTPS API (public docs)
//!
//! All control-plane calls are `POST https://exe.dev/exec` with
//! `Authorization: Bearer $EXE_TOKEN` and a body that is the same command you would
//! type over SSH ([HTTPS API](https://exe.dev/docs/https-api)):
//!
//! | Step | Command |
//! |------|---------|
//! | List VM | `ls --json <name>` |
//! | Create VM | `new --name <name> --tag cyclone --json` |
//! | Write files | `ssh <name> 'python3 -c "…"'` ([run on VM](https://exe.dev/docs/https-api-run-on-vm)) |
//!
//! `POST /exec` has **no stdin**, a **64KiB** body limit, and a **30s** timeout.
//! Files are therefore written as chunked base64 over `ssh`, not piped through the lobby.
//!
//! ## Environment
//!
//! | Variable | Required | Purpose |
//! |----------|----------|---------|
//! | `EXE_TOKEN` | apply only | Bearer token. Official name used by the exe.dev Python SDK. |
//! | `EXE_DEV_EXEC_URL` | no | Override of `https://exe.dev/exec` (tests). |
//!
//! Mint a token that can list, create, and ssh:
//!
//! ```text
//! ssh exe.dev ssh-key generate-api-key --exp=30d --cmds=ls,new,ssh --label cyclone
//! export EXE_TOKEN='…'   # never commit this
//! ```
//!
//! Cyclone does **not** send `CYCLONE_PASSPHRASE` to exe.dev. The image is decrypted
//! locally; only restored file bytes go to the VM you named.

mod client;
mod error;
mod remote;

use cyclone_core::{
    ApplyOptions, Extension, InnerBundle, Platform, ProvisionPlan, ProvisionResult, ProvisionStep,
    ProvisionTarget, Result,
};

pub use client::{
    parse_vms, ExeDevClient, ExecTransport, MockExecTransport, UreqTransport, VmInfo,
    DEFAULT_EXEC_URL, EXEC_BODY_LIMIT, EXEC_URL_ENV, TOKEN_ENV,
};
pub use error::ExeDevError;
pub use remote::{ls_command, new_command, planned_writes, validate_vm_name, write_file_commands};

/// User-facing extension id (`cyclone provision --ext exe-dev`).
pub const EXTENSION_NAME: &str = "exe-dev";

/// exe.dev adapter. Credentials live on the optional client, not in core.
pub struct ExeDevExtension {
    client: Option<ExeDevClient>,
}

impl std::fmt::Debug for ExeDevExtension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExeDevExtension")
            .field("client", &self.client)
            .finish()
    }
}

impl Default for ExeDevExtension {
    fn default() -> Self {
        Self::new()
    }
}

impl ExeDevExtension {
    /// Plan and dry-run without an API token.
    pub fn new() -> Self {
        Self { client: None }
    }

    pub fn with_client(client: ExeDevClient) -> Self {
        Self {
            client: Some(client),
        }
    }

    /// Reads [`TOKEN_ENV`] and optional [`EXEC_URL_ENV`].
    pub fn from_env() -> Result<Self> {
        Ok(Self::with_client(ExeDevClient::from_env()?))
    }
}

impl Extension for ExeDevExtension {
    fn name(&self) -> &'static str {
        EXTENSION_NAME
    }

    fn plan(&self, image: &InnerBundle, target: &ProvisionTarget) -> Result<ProvisionPlan> {
        validate_vm_name(&target.id)?;
        let writes = planned_writes(image)?;
        let skipped: Vec<&str> = image
            .manifest
            .components
            .iter()
            .filter(|c| !c.supports(Platform::Linux) || c.path_for(Platform::Linux).is_none())
            .map(|c| c.id.as_str())
            .collect();

        let mut steps = vec![
            ProvisionStep {
                action: "validate-image".into(),
                detail: format!(
                    "decrypted Cyclone image schema v{}, captured {} on {}/{}, {} components, {} files to restore on linux",
                    image.manifest.schema_version,
                    image.manifest.captured_at,
                    image.manifest.host_platform,
                    image.manifest.host_arch,
                    image.manifest.components.len(),
                    writes.len()
                ),
            },
            ProvisionStep {
                action: "exe.dev/ls".into(),
                detail: format!(
                    "POST {}  {}",
                    self.endpoint_display(),
                    ls_command(&target.id)
                ),
            },
            ProvisionStep {
                action: "exe.dev/new".into(),
                detail: format!(
                    "POST {}  {}  (skipped if ls already lists the VM)",
                    self.endpoint_display(),
                    new_command(&target.id)
                ),
            },
            ProvisionStep {
                action: "restore-files".into(),
                detail: format!(
                    "write {} file(s) under $HOME via ssh (chunked; POST /exec body limit {} bytes)",
                    writes.len(),
                    EXEC_BODY_LIMIT
                ),
            },
        ];
        if !skipped.is_empty() {
            steps.push(ProvisionStep {
                action: "skip-unsupported".into(),
                detail: format!("not restored on exe.dev (linux): {}", skipped.join(", ")),
            });
        }
        Ok(ProvisionPlan {
            target: target.clone(),
            steps,
        })
    }

    fn apply(
        &self,
        image: &InnerBundle,
        target: &ProvisionTarget,
        opts: &ApplyOptions,
    ) -> Result<ProvisionResult> {
        let plan = self.plan(image, target)?;
        let writes = planned_writes(image)?;
        let mut log: Vec<String> = plan
            .steps
            .iter()
            .map(|s| {
                if opts.dry_run {
                    format!("[dry-run] {}: {}", s.action, s.detail)
                } else {
                    format!("{}: {}", s.action, s.detail)
                }
            })
            .collect();

        if opts.dry_run {
            for w in &writes {
                log.push(format!(
                    "[dry-run] write ~/{} ({} bytes, component {})",
                    w.home_relative,
                    w.bytes.len(),
                    w.component_id
                ));
            }
            log.push(
                "[dry-run] no POST /exec; pass --apply with EXE_TOKEN set to provision".into(),
            );
            return Ok(ProvisionResult { dry_run: true, log });
        }

        let client = self.client.as_ref().ok_or_else(|| {
            cyclone_core::CycloneError::from(ExeDevError::MissingToken(TOKEN_ENV))
        })?;

        let ls_cmd = ls_command(&target.id);
        let ls = match client.exec(&ls_cmd) {
            Ok(r) => r,
            Err(ExeDevError::Http { status: 404, .. }) => {
                log.push("ls: VM not listed (HTTP 404); will create".into());
                client::ExecResponse {
                    status: 404,
                    body: "{}".into(),
                }
            }
            Err(e) => return Err(e.into()),
        };
        let vms = if ls.status == 404 {
            Vec::new()
        } else {
            client::parse_vms(&ls.body)?
        };
        let existing = client::find_vm(&vms, &target.id).cloned();

        match existing {
            Some(vm) if vm.status.as_deref() == Some("paused") => {
                return Err(ExeDevError::Message(format!(
                    "VM `{}` is paused; resume it (`ssh exe.dev resume {}` or POST /exec `resume {}`) and retry",
                    target.id, target.id, target.id
                ))
                .into());
            }
            Some(vm) => {
                log.push(format!(
                    "using existing VM `{}` status={} url={}",
                    vm.name,
                    vm.status.as_deref().unwrap_or("unknown"),
                    vm.https_url.as_deref().unwrap_or("-")
                ));
            }
            None => {
                let created = client.exec(&new_command(&target.id))?;
                let info = client::parse_vms(&created.body)?
                    .into_iter()
                    .next()
                    .or_else(|| {
                        client.exec(&ls_cmd).ok().and_then(|r| {
                            client::parse_vms(&r.body)
                                .ok()
                                .and_then(|vms| client::find_vm(&vms, &target.id).cloned())
                        })
                    });
                match info {
                    Some(vm) => log.push(format!(
                        "created VM `{}` url={}",
                        vm.name,
                        vm.https_url.as_deref().unwrap_or("-")
                    )),
                    None => log.push(format!("created VM `{}`", target.id)),
                }
            }
        }

        for w in &writes {
            let commands = write_file_commands(&target.id, w)?;
            for cmd in commands {
                client.exec(&cmd)?;
            }
            log.push(format!(
                "wrote ~/{} ({} bytes)",
                w.home_relative,
                w.bytes.len()
            ));
        }

        Ok(ProvisionResult {
            dry_run: false,
            log,
        })
    }
}

impl ExeDevExtension {
    fn endpoint_display(&self) -> String {
        self.client
            .as_ref()
            .map(|c| c.endpoint().to_string())
            .unwrap_or_else(|| DEFAULT_EXEC_URL.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::MockExecTransport;
    use cyclone_core::{
        ArtifactKind, CapturedArtifact, CapturedPayload, ComponentKind, ImageManifest,
        ManifestComponent, Platform,
    };
    use std::sync::Arc;

    fn sample_image() -> InnerBundle {
        InnerBundle {
            manifest: ImageManifest {
                schema_version: 1,
                captured_at: "1970-01-01T00:00:00Z".into(),
                host_platform: Platform::Linux,
                host_arch: "x86_64".into(),
                components: vec![
                    ManifestComponent {
                        id: "gitconfig".into(),
                        kind: ComponentKind::Dotfile,
                        linux: Some("~/.gitconfig".into()),
                        macos: Some("~/.gitconfig".into()),
                        captured_path: "~/.gitconfig".into(),
                        content_sha256: "abc".into(),
                        file_count: 1,
                        byte_size: 12,
                        supported_platforms: vec![Platform::Linux, Platform::MacOs],
                    },
                    ManifestComponent {
                        id: "nvim".into(),
                        kind: ComponentKind::ConfigTree,
                        linux: Some("~/.config/nvim".into()),
                        macos: Some("~/.config/nvim".into()),
                        captured_path: "~/.config/nvim".into(),
                        content_sha256: "def".into(),
                        file_count: 1,
                        byte_size: 8,
                        supported_platforms: vec![Platform::Linux, Platform::MacOs],
                    },
                ],
                secret_references: vec![],
                warnings: vec![],
            },
            payload: CapturedPayload::new(vec![
                CapturedArtifact {
                    component_id: "gitconfig".into(),
                    relative_path: String::new(),
                    kind: ArtifactKind::File,
                    bytes: b"[user]\nname = Test\n".to_vec(),
                    mode: 0o644,
                },
                CapturedArtifact {
                    component_id: "nvim".into(),
                    relative_path: "init.lua".into(),
                    kind: ArtifactKind::File,
                    bytes: b"-- nvim\n".to_vec(),
                    mode: 0o644,
                },
            ]),
        }
    }

    fn target(id: &str) -> ProvisionTarget {
        ProvisionTarget {
            id: id.into(),
            labels: vec![],
        }
    }

    #[test]
    fn name_is_exe_dev() {
        assert_eq!(ExeDevExtension::new().name(), "exe-dev");
    }

    #[test]
    fn plan_lists_documented_exec_commands() {
        let plan = ExeDevExtension::new()
            .plan(&sample_image(), &target("dev-box"))
            .unwrap();
        let actions: Vec<_> = plan.steps.iter().map(|s| s.action.as_str()).collect();
        assert!(actions.contains(&"validate-image"));
        assert!(actions.contains(&"exe.dev/ls"));
        assert!(actions.contains(&"exe.dev/new"));
        assert!(actions.contains(&"restore-files"));
        let blob = plan
            .steps
            .iter()
            .map(|s| s.detail.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(blob.contains("ls --json dev-box"));
        assert!(blob.contains("new --name dev-box --tag cyclone --json"));
        assert!(blob.contains("https://exe.dev/exec"));
    }

    #[test]
    fn dry_run_does_not_touch_transport() {
        let mock = Arc::new(MockExecTransport::new());
        let client =
            ExeDevClient::with_transport("https://exe.dev/exec", "fixture-token", mock.clone());
        let ext = ExeDevExtension::with_client(client);
        let result = ext
            .apply(
                &sample_image(),
                &target("dev-box"),
                &ApplyOptions { dry_run: true },
            )
            .unwrap();
        assert!(result.dry_run);
        assert!(mock.recorded().is_empty());
        assert!(result.log.iter().any(|l| l.contains("write ~/.gitconfig")));
        assert!(result
            .log
            .iter()
            .any(|l| l.contains("write ~/.config/nvim/init.lua")));
        assert!(
            !result.log.join("\n").contains("[user]"),
            "dry-run must not log decrypted file bytes"
        );
    }

    #[test]
    fn apply_creates_vm_then_writes_files() {
        let mock = Arc::new(MockExecTransport::new());
        mock.push(200, include_str!("../tests/fixtures/ls_empty.json"));
        mock.push(200, include_str!("../tests/fixtures/new_vm.json"));
        mock.push(200, "");
        mock.push(200, "");
        let client =
            ExeDevClient::with_transport("https://exe.dev/exec", "fixture-token", mock.clone());
        let ext = ExeDevExtension::with_client(client);
        let result = ext
            .apply(
                &sample_image(),
                &target("dev-box"),
                &ApplyOptions { dry_run: false },
            )
            .unwrap();
        assert!(!result.dry_run);
        let recorded = mock.recorded();
        assert_eq!(recorded[0], "ls --json dev-box");
        assert_eq!(recorded[1], "new --name dev-box --tag cyclone --json");
        assert!(recorded[2].starts_with("ssh dev-box "));
        assert!(recorded.iter().any(|c| c.contains(".gitconfig")));
        assert!(recorded.iter().any(|c| c.contains("init.lua")));
        assert!(result.log.iter().any(|l| l.contains("created VM")));
    }

    #[test]
    fn apply_reuses_running_vm() {
        let mock = Arc::new(MockExecTransport::new());
        mock.push(200, include_str!("../tests/fixtures/ls_running.json"));
        mock.push(200, "");
        mock.push(200, "");
        let client =
            ExeDevClient::with_transport("https://exe.dev/exec", "fixture-token", mock.clone());
        let ext = ExeDevExtension::with_client(client);
        ext.apply(
            &sample_image(),
            &target("dev-box"),
            &ApplyOptions { dry_run: false },
        )
        .unwrap();
        let recorded = mock.recorded();
        assert!(recorded.iter().all(|c| !c.starts_with("new ")));
        assert!(recorded[0].starts_with("ls "));
    }

    #[test]
    fn apply_rejects_paused_vm() {
        let mock = Arc::new(MockExecTransport::new());
        mock.push(200, include_str!("../tests/fixtures/ls_paused.json"));
        let client =
            ExeDevClient::with_transport("https://exe.dev/exec", "fixture-token", mock.clone());
        let ext = ExeDevExtension::with_client(client);
        let err = ext
            .apply(
                &sample_image(),
                &target("dev-box"),
                &ApplyOptions { dry_run: false },
            )
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("paused"));
        assert_eq!(mock.recorded().len(), 1);
    }

    #[test]
    fn apply_without_client_asks_for_exe_token() {
        let err = ExeDevExtension::new()
            .apply(
                &sample_image(),
                &target("dev-box"),
                &ApplyOptions { dry_run: false },
            )
            .unwrap_err();
        assert!(err.to_string().contains("EXE_TOKEN"));
    }

    #[test]
    fn plan_rejects_invalid_target() {
        let err = ExeDevExtension::new()
            .plan(&sample_image(), &target("Nope"))
            .unwrap_err();
        assert!(err.to_string().contains("invalid target"));
    }
}
