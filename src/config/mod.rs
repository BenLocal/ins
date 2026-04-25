pub(crate) mod load;
pub(crate) mod types;

pub(crate) use load::config_file;
pub(crate) use load::{load_config, persist_node_workspace_if_missing};
pub(crate) use types::InsConfig;

#[cfg(test)]
#[path = "config_test.rs"]
mod config_test;
