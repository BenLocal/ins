use serde::Serialize;

#[derive(Clone, Debug, Serialize, Default)]
pub(crate) struct AppRecord {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) before: ScriptHook,
    pub(crate) after: ScriptHook,
    pub(crate) values: Vec<AppValue>,
}

#[derive(Clone, Debug, Serialize, Default)]
pub(crate) struct ScriptHook {
    pub(crate) shell: Option<String>,
    pub(crate) script: Option<String>,
}

#[derive(Clone, Debug, Serialize, Default)]
pub(crate) struct AppValue {
    pub(crate) name: String,
    pub(crate) value_type: String,
    pub(crate) description: Option<String>,
    pub(crate) options: Vec<AppValueOption>,
}

#[derive(Clone, Debug, Serialize, Default)]
pub(crate) struct AppValueOption {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) value: Option<String>,
}
