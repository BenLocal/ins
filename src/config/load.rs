use std::path::{Path, PathBuf};

use anyhow::Context;
use tokio::fs;

use super::types::InsConfig;

pub(crate) fn config_file(home: &Path) -> PathBuf {
    home.join("config.toml")
}

pub(crate) async fn load_config(home: &Path) -> anyhow::Result<InsConfig> {
    let path = config_file(home);
    if !fs::try_exists(&path)
        .await
        .with_context(|| format!("check config file {}", path.display()))?
    {
        return Ok(InsConfig::default());
    }
    let content = fs::read_to_string(&path)
        .await
        .with_context(|| format!("read config file {}", path.display()))?;
    toml::from_str(&content).with_context(|| format!("parse config file {}", path.display()))
}
