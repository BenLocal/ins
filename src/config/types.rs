use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InsConfig {
    #[serde(default, skip_serializing_if = "Defaults::is_empty")]
    pub(crate) defaults: Defaults,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) nodes: BTreeMap<String, NodeConfig>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Defaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) workspace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) app_home: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provider: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) env: BTreeMap<String, String>,
}

impl Defaults {
    fn is_empty(&self) -> bool {
        self.workspace.is_none()
            && self.app_home.is_none()
            && self.provider.is_none()
            && self.env.is_empty()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NodeConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) workspace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provider: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) env: BTreeMap<String, String>,
}

impl InsConfig {
    /// Per-node workspace override, falling back to [defaults].workspace, otherwise None.
    pub(crate) fn workspace_for(&self, node: &str) -> Option<&str> {
        self.nodes
            .get(node)
            .and_then(|n| n.workspace.as_deref())
            .or(self.defaults.workspace.as_deref())
    }

    /// True if config has a per-node workspace entry (regardless of defaults).
    pub(crate) fn has_node_workspace(&self, node: &str) -> bool {
        self.nodes.get(node).is_some_and(|n| n.workspace.is_some())
    }

    /// Per-node provider override, falling back to [defaults].provider, otherwise None.
    pub(crate) fn provider_for(&self, node: &str) -> Option<&str> {
        self.nodes
            .get(node)
            .and_then(|n| n.provider.as_deref())
            .or(self.defaults.provider.as_deref())
    }

    /// Absolute path resolution for the configured app home if present.
    pub(crate) fn app_home_override(&self) -> Option<&str> {
        self.defaults.app_home.as_deref()
    }

    /// Merged user env for the given node: [defaults.env] overlaid by [nodes.<n>.env].
    /// Node-specific keys win on collision.
    pub(crate) fn env_for(&self, node: &str) -> BTreeMap<String, String> {
        let mut merged = self.defaults.env.clone();
        if let Some(cfg) = self.nodes.get(node) {
            for (k, v) in &cfg.env {
                merged.insert(k.clone(), v.clone());
            }
        }
        merged
    }
}
