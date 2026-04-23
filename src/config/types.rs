use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InsConfig {
    #[serde(default)]
    pub(crate) defaults: Defaults,
    #[serde(default)]
    pub(crate) nodes: BTreeMap<String, NodeConfig>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Defaults {
    pub(crate) workspace: Option<String>,
    pub(crate) app_home: Option<String>,
    pub(crate) provider: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NodeConfig {
    pub(crate) workspace: Option<String>,
    pub(crate) provider: Option<String>,
}

impl InsConfig {
    /// Per-node workspace override, falling back to [defaults].workspace, otherwise None.
    pub(crate) fn workspace_for(&self, node: &str) -> Option<&str> {
        self.nodes
            .get(node)
            .and_then(|n| n.workspace.as_deref())
            .or(self.defaults.workspace.as_deref())
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
}
