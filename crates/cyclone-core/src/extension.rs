use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::manifest::ImageManifest;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvisionTarget {
    pub id: String,
    #[serde(default)]
    pub labels: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvisionStep {
    pub action: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProvisionPlan {
    pub target: ProvisionTarget,
    pub steps: Vec<ProvisionStep>,
}

/// Cloud/VM adapters implement this trait; core stays SDK-free.
pub trait Extension: Send + Sync {
    fn name(&self) -> &'static str;

    fn plan(&self, manifest: &ImageManifest, target: &ProvisionTarget) -> Result<ProvisionPlan>;

    fn apply(&self, plan: &ProvisionPlan, dry_run: bool) -> Result<Vec<String>>;
}
