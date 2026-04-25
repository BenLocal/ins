use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::anyhow;
use inquire::Select;

use crate::app::dependency::{DEFAULT_NAMESPACE, validate_namespace_name};
use crate::app::parse::load_app_record;
use crate::cli::CommandContext;
use crate::cli::node::nodes_file;
use crate::config::{InsConfig, config_file, persist_node_workspace_if_missing};
use crate::node::list::load_all_nodes;
use crate::node::types::NodeRecord;
use crate::provider::DeploymentTarget;
use crate::store::duck::{
    InstalledServiceRecord, find_service_namespace_on_node, load_installed_service_configs,
};

use super::target::{
    app_qa_file, apply_cli_values, build_deployment_targets, load_app_records_by_names,
    parse_cli_value_overrides, resolve_apps,
};
use super::{PipelineArgs, PreparedDeployment};
use super::{node_label, node_name};

const DEFAULT_PROVIDER: &str = "docker-compose";

pub(crate) fn resolve_namespace(input: Option<String>) -> anyhow::Result<String> {
    let raw = input
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_NAMESPACE.to_string());
    validate_namespace_name(&raw)?;
    Ok(raw)
}

pub(crate) async fn check_namespace_conflicts(
    home: &Path,
    node: &NodeRecord,
    namespace: &str,
    targets: &[DeploymentTarget],
) -> anyhow::Result<()> {
    for target in targets {
        let Some(existing) = find_service_namespace_on_node(home, node, &target.service).await?
        else {
            continue;
        };
        if existing != namespace {
            return Err(anyhow!(
                "service '{}' already exists on node '{}' under namespace '{}'; \
                 cannot deploy under namespace '{}'. Either redeploy under namespace '{}' \
                 or manually remove the existing record from the deploy history (\
                 `<home>/store/deploy_history.duckdb`).",
                target.service,
                node_name(node),
                existing,
                namespace,
                existing,
            ));
        }
    }
    Ok(())
}

/// Bundle of everything `prepare_deployment` needs: shared CLI state
/// (`home`, `config`) plus the command-specific `PipelineArgs`. Keeps the
/// callsite to a single line and avoids the 8-argument signature we used to
/// have.
#[derive(Clone, Debug)]
pub struct PipelineContext {
    pub home: PathBuf,
    pub config: Arc<InsConfig>,
    pub args: PipelineArgs,
}

impl PipelineContext {
    pub fn new(cmd_ctx: &CommandContext, args: PipelineArgs) -> Self {
        Self {
            home: cmd_ctx.home.clone(),
            config: cmd_ctx.config.clone(),
            args,
        }
    }
}

/// Read `[defaults] local_extern_ip` from `config.toml`. The local node has
/// no other way to know its public-facing address, so when this is missing
/// the deploy aborts with a message naming the exact key + file the user
/// needs to edit. Remote nodes use their registered `ip` directly and never
/// touch this resolver.
fn resolve_local_extern_ip(home: &Path, config: &InsConfig) -> anyhow::Result<String> {
    if let Some(ip) = config.local_extern_ip() {
        return Ok(ip.to_string());
    }
    Err(anyhow!(
        "local_extern_ip is not configured. \
         Add `[defaults] local_extern_ip = \"<external IP>\"` to {} and re-run.",
        config_file(home).display()
    ))
}

pub async fn prepare_deployment(ctx: PipelineContext) -> anyhow::Result<PreparedDeployment> {
    let PipelineContext {
        home,
        config,
        args:
            PipelineArgs {
                provider,
                workspace,
                node: requested_node,
                namespace: requested_namespace,
                values: requested_values,
                defaults: use_defaults,
                apps: requested_apps,
            },
    } = ctx;

    let namespace = resolve_namespace(requested_namespace)?;

    let nodes = load_all_nodes(&nodes_file(&home)).await?;
    let node = select_node(&nodes, requested_node.as_deref())?;
    let node_name_str = node_name(&node).to_string();

    let provider = resolve_provider(provider, &config, &node_name_str);
    let cli_workspace = workspace;
    let config_has_node_workspace = config.has_node_workspace(&node_name_str);
    let workspace = resolve_workspace(cli_workspace.clone(), &config, &node_name_str)?;

    // First-use learning: if the user typed --workspace for a node that doesn't yet
    // have a per-node entry in config.toml, record the resolved path so subsequent
    // runs can omit the flag.
    if cli_workspace.is_some() && !config_has_node_workspace {
        let absolute = workspace.to_string_lossy().to_string();
        persist_node_workspace_if_missing(&home, &node_name_str, &absolute).await?;
    }

    let app_home = resolve_app_home(&home, &config);
    let user_env = config.env_for(&node_name_str);

    let local_extern_ip = match &node {
        NodeRecord::Local() => Some(resolve_local_extern_ip(&home, &config)?),
        NodeRecord::Remote(_) => None,
    };

    let app_names = resolve_apps(requested_apps, &app_home).await?;
    let mut apps = load_app_records_by_names(&app_names, &app_home, &user_env).await?;
    let value_overrides = parse_cli_value_overrides(&requested_values)?;
    apply_cli_values(&mut apps, &value_overrides)?;
    let targets =
        build_deployment_targets(apps, &home, &node, &workspace, &namespace, use_defaults).await?;

    check_namespace_conflicts(&home, &node, &namespace, &targets).await?;

    Ok(PreparedDeployment {
        provider,
        node,
        namespace,
        local_extern_ip,
        app_names,
        app_home,
        workspace,
        targets,
        user_env,
    })
}

fn resolve_provider(cli: Option<String>, config: &InsConfig, node_name: &str) -> String {
    cli.or_else(|| config.provider_for(node_name).map(str::to_string))
        .unwrap_or_else(|| DEFAULT_PROVIDER.to_string())
}

fn resolve_workspace(
    cli: Option<PathBuf>,
    config: &InsConfig,
    node_name: &str,
) -> anyhow::Result<PathBuf> {
    let raw = cli
        .or_else(|| config.workspace_for(node_name).map(PathBuf::from))
        .ok_or_else(|| {
            anyhow!(
                "--workspace not provided and no config default for node '{}'",
                node_name
            )
        })?;
    absolute_workspace(&raw)
}

pub(super) fn resolve_app_home(home: &Path, config: &InsConfig) -> PathBuf {
    match config.app_home_override() {
        Some(path) => {
            let p = PathBuf::from(path);
            if p.is_absolute() { p } else { home.join(p) }
        }
        None => home.join("app"),
    }
}

fn absolute_workspace(workspace: &Path) -> anyhow::Result<PathBuf> {
    std::path::absolute(workspace).map_err(|e| {
        anyhow!(
            "resolve absolute workspace path {}: {}",
            workspace.display(),
            e
        )
    })
}

pub async fn prepare_installed_service_deployment(
    home: &Path,
    config: &InsConfig,
    provider: Option<String>,
    service: &InstalledServiceRecord,
) -> anyhow::Result<PreparedDeployment> {
    let node = load_all_nodes(&nodes_file(home))
        .await?
        .into_iter()
        .find(|node| node_name(node) == service.node_name)
        .ok_or_else(|| {
            anyhow!(
                "node '{}' not found for service '{}'",
                service.node_name,
                service.service
            )
        })?;

    let app_home = resolve_app_home(home, config);
    let qa_file = app_qa_file(&app_home.join(&service.app_name));
    let user_env = config.env_for(&service.node_name);
    let mut app = load_app_record(&qa_file, &user_env).await?;
    let stored_config = load_installed_service_configs(home)
        .await?
        .into_iter()
        .find(|record| {
            record.service == service.service
                && record.namespace == service.namespace
                && record.node_name == service.node_name
        })
        .ok_or_else(|| {
            anyhow!(
                "service '{}' (namespace '{}', node '{}') config not found",
                service.service,
                service.namespace,
                service.node_name
            )
        })?;

    for value in &mut app.values {
        if let Some(stored) = stored_config.app_values.get(&value.name) {
            value.value = Some(stored.clone());
        }
    }

    let target = DeploymentTarget::new(app, service.service.clone());
    let provider = resolve_provider(provider, config, &service.node_name);
    let user_env = config.env_for(&service.node_name);

    let local_extern_ip = match &node {
        NodeRecord::Local() => Some(resolve_local_extern_ip(home, config)?),
        NodeRecord::Remote(_) => None,
    };

    Ok(PreparedDeployment {
        provider,
        node,
        namespace: service.namespace.clone(),
        local_extern_ip,
        app_names: vec![service.app_name.clone()],
        app_home,
        workspace: absolute_workspace(Path::new(&service.workspace))?,
        targets: vec![target],
        user_env,
    })
}

pub(crate) fn select_node(
    nodes: &[NodeRecord],
    requested: Option<&str>,
) -> anyhow::Result<NodeRecord> {
    if nodes.is_empty() {
        return Err(anyhow!("no nodes found, please add a node first"));
    }

    if let Some(name) = requested
        && let Some(node) = nodes.iter().find(|node| node_name(node) == name)
    {
        return Ok(node.clone());
    }

    let options: Vec<String> = nodes.iter().map(node_label).collect();

    let answer = Select::new("Select a node", options).prompt()?;
    let selected_name = answer
        .split_once(" (")
        .map(|(name, _)| name)
        .unwrap_or(answer.as_str());

    nodes
        .iter()
        .find(|node| node_name(node) == selected_name)
        .cloned()
        .ok_or_else(|| anyhow!("selected node '{}' not found", selected_name))
}

#[cfg(test)]
#[path = "prepare_test.rs"]
mod prepare_test;
