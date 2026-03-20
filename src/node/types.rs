use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) enum NodeRecord {
    Local(),
    Remote(RemoteNodeRecord),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct RemoteNodeRecord {
    pub(crate) name: String,
    pub(crate) ip: String,
    pub(crate) port: u16,
    pub(crate) user: String,
    pub(crate) password: String,
    pub(crate) key_path: Option<String>,
}
