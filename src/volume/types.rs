use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum VolumeRecord {
    Filesystem(FilesystemVolume),
    Cifs(CifsVolume),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct FilesystemVolume {
    pub(crate) name: String,
    pub(crate) node: String,
    pub(crate) path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct CifsVolume {
    pub(crate) name: String,
    pub(crate) node: String,
    pub(crate) server: String,
    pub(crate) username: String,
    pub(crate) password: String,
}

impl VolumeRecord {
    pub(crate) fn name(&self) -> &str {
        match self {
            VolumeRecord::Filesystem(v) => &v.name,
            VolumeRecord::Cifs(v) => &v.name,
        }
    }

    pub(crate) fn node(&self) -> &str {
        match self {
            VolumeRecord::Filesystem(v) => &v.node,
            VolumeRecord::Cifs(v) => &v.node,
        }
    }

    pub(crate) fn kind_label(&self) -> &'static str {
        match self {
            VolumeRecord::Filesystem(_) => "filesystem",
            VolumeRecord::Cifs(_) => "cifs",
        }
    }

    pub(crate) fn detail_label(&self) -> String {
        match self {
            VolumeRecord::Filesystem(v) => v.path.clone(),
            VolumeRecord::Cifs(v) => format!("{} ({})", v.server, v.username),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedVolume {
    pub docker_name: String,
    pub driver: String,
    pub driver_opts: BTreeMap<String, String>,
}
