use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub(crate) struct AppRecord {
    pub(crate) name: String,
    pub(crate) version: Option<String>,
    pub(crate) description: Option<String>,
    /// Sort key for `ins app list`. Lower numbers come first. Apps without
    /// `order` set are displayed after all ordered apps, sorted by name.
    #[serde(default)]
    pub(crate) order: Option<i64>,
    pub(crate) author_name: Option<String>,
    pub(crate) author_email: Option<String>,
    #[serde(default)]
    pub(crate) dependencies: Vec<String>,
    #[serde(default)]
    pub(crate) before: ScriptHook,
    #[serde(default)]
    pub(crate) after: ScriptHook,
    #[serde(default)]
    pub(crate) values: Vec<AppValue>,
    #[serde(default)]
    pub(crate) volumes: Vec<String>,
    #[serde(default)]
    pub(crate) all_volume: bool,
    #[serde(skip_deserializing)]
    pub(crate) files: Option<Vec<AppFileEntry>>,
}

#[derive(Clone, Debug, Serialize, Default)]
pub(crate) struct AppFileEntry {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) is_dir: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub(crate) struct ScriptHook {
    pub(crate) shell: Option<String>,
    pub(crate) script: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub(crate) struct AppValue {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) value_type: String,
    pub(crate) description: Option<String>,
    pub(crate) value: Option<Value>,
    #[serde(default)]
    pub(crate) default: Option<Value>,
    #[serde(default)]
    pub(crate) options: Vec<AppValueOption>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub(crate) struct AppValueOption {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) value: Option<Value>,
}

impl AppRecord {
    #[allow(dead_code)]
    pub(crate) fn parsed_dependencies(
        &self,
    ) -> anyhow::Result<Vec<crate::app::dependency::DependencyRef>> {
        self.dependencies
            .iter()
            .map(|raw| crate::app::dependency::parse_dependency(raw))
            .collect()
    }
}

/// Sort apps for display. `order` ascending wins; apps without `order` come
/// after all ordered ones; ties break alphabetically by name.
pub(crate) fn sort_apps_for_display(apps: &mut [AppRecord]) {
    apps.sort_by(|a, b| match (a.order, b.order) {
        (Some(x), Some(y)) => x.cmp(&y).then_with(|| a.name.cmp(&b.name)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.name.cmp(&b.name),
    });
}
