use serde::{Deserialize, Serialize};

use crate::bundle::InnerBundle;
use crate::error::Result;

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

/// Options for [`Extension::apply`].
///
/// Cloud credentials stay on the extension instance so `cyclone-core` remains SDK-free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyOptions {
    /// When true, return the work that *would* happen and do not call the provider.
    pub dry_run: bool,
}

impl Default for ApplyOptions {
    fn default() -> Self {
        Self { dry_run: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionResult {
    pub dry_run: bool,
    pub log: Vec<String>,
}

/// Cloud/VM adapters implement this trait; core stays SDK-free.
///
/// Matches the product plan: `plan(image, target) → steps` and
/// `apply(image, target, opts) → result`.
pub trait Extension: Send + Sync {
    fn name(&self) -> &'static str;

    fn plan(&self, image: &InnerBundle, target: &ProvisionTarget) -> Result<ProvisionPlan>;

    fn apply(
        &self,
        image: &InnerBundle,
        target: &ProvisionTarget,
        opts: &ApplyOptions,
    ) -> Result<ProvisionResult>;
}
