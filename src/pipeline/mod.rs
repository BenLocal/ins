mod copy;
mod labels;
mod prepare;
mod progress;
mod target;
mod template;

use anyhow::anyhow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::env::build_provider_envs;
use crate::execution_output::ExecutionOutput;
use crate::node::types::NodeRecord;
use crate::provider::docker_compose::DockerComposeProvider;
use crate::provider::{DeploymentTarget, ProviderContext, ProviderTrait};
use crate::store::duck::load_installed_service_configs;
#[cfg(test)]
pub(crate) use copy::copy_apps_to_workspace;
#[cfg(test)]
pub(crate) use copy::copy_apps_to_workspace_with_output;
use copy::copy_prepared_apps_to_workspace_with_output;
#[cfg(test)]
pub(crate) use labels::build_compose_metadata_labels;
#[cfg(test)]
pub(crate) use prepare::select_node;
pub use prepare::{prepare_deployment, prepare_installed_service_deployment};
#[cfg(test)]
pub(crate) use target::{
    app_choice_label, apply_cli_values, apply_stored_values, build_deployment_target,
    parse_cli_value_overrides, parse_number_value, resolve_apps,
};
#[cfg(test)]
pub(crate) use template::{build_template_values, is_template_file, rendered_template_name};

const COPY_CONCURRENCY: usize = 3;

#[derive(clap::Args, Clone, Debug)]
pub struct PipelineArgs {
    /// Provider name. Falls back to config.toml, then "docker-compose".
    #[arg(short, long)]
    pub provider: Option<String>,
    /// Workspace directory for copied app files. Falls back to config.toml per-node or defaults.
    #[arg(short, long)]
    pub workspace: Option<PathBuf>,
    /// Target node name.
    #[arg(short, long)]
    pub node: Option<String>,
    /// Override qa values. Can be specified multiple times as key=value.
    #[arg(short = 'v', long = "value", value_name = "KEY=VALUE")]
    pub values: Vec<String>,
    /// Application names to deploy.
    pub apps: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct PreparedDeployment {
    pub provider: String,
    pub node: NodeRecord,
    pub app_names: Vec<String>,
    pub app_home: PathBuf,
    pub workspace: PathBuf,
    pub targets: Vec<DeploymentTarget>,
    pub user_env: BTreeMap<String, String>,
    pub(crate) node_info: crate::node::info::NodeInfo,
}

#[derive(Clone, Copy, Debug)]
pub enum PipelineMode {
    Check,
    Deploy,
}

pub fn ensure_supported_provider(
    provider: &str,
) -> anyhow::Result<Box<dyn ProviderTrait + Send + Sync>> {
    match provider {
        "docker-compose" => Ok(Box::new(DockerComposeProvider)),
        _ => Err(anyhow!("unsupported provider: {}", provider)),
    }
}

#[allow(dead_code)]
pub fn print_prepared_deployment(title: &str, prepared: &PreparedDeployment) {
    let output = ExecutionOutput::stdout();
    print_prepared_deployment_to_output(title, prepared, &output);
}

pub fn print_prepared_deployment_to_output(
    title: &str,
    prepared: &PreparedDeployment,
    output: &ExecutionOutput,
) {
    output.line(title);
    output.line(format!("Provider Name: {}", prepared.provider));
    output.line(format!("Node Name: {}", node_name(&prepared.node)));
    output.line(format!("Apps: {}", prepared.app_names.join(", ")));
    output.line(format!("Workspace: {}", prepared.workspace.display()));
    output.line("Deployment Targets:");
    for target in &prepared.targets {
        output.line(format!(
            "  {} -> service {} -> {}",
            target.app.name,
            target.service,
            prepared.workspace.join(&target.service).display()
        ));
    }
}

pub async fn execute_pipeline(
    home: &Path,
    prepared: PreparedDeployment,
    title: &str,
    mode: PipelineMode,
) -> anyhow::Result<()> {
    let output = ExecutionOutput::stdout();
    execute_pipeline_with_output(home, prepared, title, mode, output).await
}

pub async fn execute_pipeline_with_output(
    home: &Path,
    prepared: PreparedDeployment,
    title: &str,
    mode: PipelineMode,
    output: ExecutionOutput,
) -> anyhow::Result<()> {
    let provider = ensure_supported_provider(&prepared.provider)?;

    print_prepared_deployment_to_output(title, &prepared, &output);
    let resolved_volumes =
        copy_prepared_apps_to_workspace_with_output(home, &prepared, &output).await?;

    let provider_ctx = ProviderContext::new(
        prepared.provider.clone(),
        prepared.node.clone(),
        prepared.targets.clone(),
        prepared.workspace,
        build_provider_envs(
            &prepared.targets,
            &prepared.node,
            &load_installed_service_configs(home).await?,
            &merge_user_env_with_node_info(&prepared.user_env, &prepared.node_info),
        )?,
        output.clone(),
        resolved_volumes,
    );

    match mode {
        PipelineMode::Check => {
            print_provider_envs(&provider_ctx.envs, &output);
            output.line("Validating with provider...");
            provider.validate(provider_ctx).await?;
            output.line("Check completed.");
            Ok(())
        }
        PipelineMode::Deploy => {
            output.line("Running provider...");
            provider.run(provider_ctx).await
        }
    }
}

fn print_provider_envs(
    envs: &BTreeMap<String, BTreeMap<String, String>>,
    output: &ExecutionOutput,
) {
    output.line("");
    output.line("");
    output.line("--------------------------------");
    output.line("Provider Environment Variables:");

    if envs.is_empty() {
        output.line("  (none)");
        return;
    }

    for (service, service_envs) in envs {
        output.line(format!("  [{service}]"));
        if service_envs.is_empty() {
            output.line("    (none)");
            continue;
        }

        for (key, value) in service_envs {
            output.line(format!("    {key}={value}"));
        }
    }
    output.line("--------------------------------");
}

fn merge_user_env_with_node_info(
    user_env: &BTreeMap<String, String>,
    node_info: &crate::node::info::NodeInfo,
) -> BTreeMap<String, String> {
    let mut merged = user_env.clone();
    // INS_NODE_* overlays user env (built-in metadata wins on key collision).
    for (k, v) in node_info.to_env_pairs() {
        merged.insert(k, v);
    }
    merged
}

pub(crate) fn node_name(node: &NodeRecord) -> &str {
    match node {
        NodeRecord::Local() => "local",
        NodeRecord::Remote(node) => &node.name,
    }
}

pub(crate) fn node_label(node: &NodeRecord) -> String {
    match node {
        NodeRecord::Local() => "local (127.0.0.1)".to_string(),
        NodeRecord::Remote(node) => format!("{} ({})", node.name, node.ip),
    }
}

#[cfg(test)]
#[path = "pipeline_test.rs"]
mod pipeline_test;
