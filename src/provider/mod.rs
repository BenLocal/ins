use async_trait::async_trait;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::execution_output::ExecutionOutput;
use crate::volume::types::ResolvedVolume;
use crate::{app::types::AppRecord, node::types::NodeRecord};

pub mod docker_compose;

#[derive(Clone, Debug)]
pub struct DeploymentTarget {
    pub app: AppRecord,
    pub service: String,
}

impl DeploymentTarget {
    pub fn new(app: AppRecord, service: String) -> Self {
        Self { app, service }
    }
}

#[derive(Clone, Debug)]
pub struct ProviderContext {
    pub provider: String,
    pub node: NodeRecord,
    pub targets: Vec<DeploymentTarget>,
    pub workspace: PathBuf,
    pub envs: BTreeMap<String, BTreeMap<String, String>>,
    pub output: ExecutionOutput,
    #[allow(dead_code)]
    pub volumes: Vec<ResolvedVolume>,
}

impl ProviderContext {
    pub fn new(
        provider: String,
        node: NodeRecord,
        targets: Vec<DeploymentTarget>,
        workspace: PathBuf,
        envs: BTreeMap<String, BTreeMap<String, String>>,
        output: ExecutionOutput,
        volumes: Vec<ResolvedVolume>,
    ) -> Self {
        Self {
            provider,
            node,
            targets,
            workspace,
            envs,
            output,
            volumes,
        }
    }

    pub fn env_for_target(&self, service: &str) -> BTreeMap<String, String> {
        self.envs.get(service).cloned().unwrap_or_default()
    }
}

#[async_trait]
pub trait ProviderTrait {
    async fn validate(&self, ctx: ProviderContext) -> anyhow::Result<()>;
    async fn run(&self, ctx: ProviderContext) -> anyhow::Result<()>;
}
