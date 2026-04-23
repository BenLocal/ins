pub(crate) mod load;
pub(crate) mod types;

pub(crate) use load::load_config;
pub(crate) use types::InsConfig;

#[cfg(test)]
#[path = "config_test.rs"]
mod config_test;
