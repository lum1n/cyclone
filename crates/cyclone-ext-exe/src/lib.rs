//! # cyclone-ext-exe
//!
//! Extension crate that maps a Cyclone [`ImageManifest`] to
//! exe.dev VM provisioning steps via [`cyclone_core::Extension`].
//!
//! Today this crate implements **plan/apply orchestration only** (no exe.dev API calls yet).
//! It is intended as a reference extension and a hook for future integration work.

use cyclone_core::{
    Extension, ImageManifest, ProvisionPlan, ProvisionStep, ProvisionTarget, Result,
};

#[derive(Debug, Default)]
pub struct ExeDevExtension;

impl ExeDevExtension {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for ExeDevExtension {
    fn name(&self) -> &'static str {
        "exe"
    }

    fn plan(&self, manifest: &ImageManifest, target: &ProvisionTarget) -> Result<ProvisionPlan> {
        Ok(ProvisionPlan {
            target: target.clone(),
            steps: vec![
                ProvisionStep {
                    action: "validate-manifest".into(),
                    detail: format!(
                        "schema v{} captured on {}",
                        manifest.schema_version,
                        manifest.host_platform.as_str()
                    ),
                },
                ProvisionStep {
                    action: "exe.dev/bootstrap".into(),
                    detail: format!("provision VM `{}` (stub)", target.id),
                },
                ProvisionStep {
                    action: "restore-hooks".into(),
                    detail: "run post-restore hooks inside VM (stub)".into(),
                },
            ],
        })
    }

    fn apply(&self, plan: &ProvisionPlan, dry_run: bool) -> Result<Vec<String>> {
        let mut log = Vec::new();
        for step in &plan.steps {
            if dry_run {
                log.push(format!("[dry-run] {}: {}", step.action, step.detail));
            } else {
                log.push(format!("[stub] {}: {}", step.action, step.detail));
            }
        }
        Ok(log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyclone_core::Platform;

    #[test]
    fn plans_steps() {
        let manifest = ImageManifest {
            schema_version: 1,
            captured_at: "1970-01-01T00:00:00Z".into(),
            host_platform: Platform::Linux,
            host_arch: "x86_64".into(),
            components: vec![],
            secret_references: vec![],
            warnings: vec![],
        };
        let ext = ExeDevExtension::new();
        let plan = ext
            .plan(
                &manifest,
                &ProvisionTarget {
                    id: "dev-box".into(),
                    labels: vec![],
                },
            )
            .unwrap();
        assert_eq!(plan.steps.len(), 3);
    }
}
