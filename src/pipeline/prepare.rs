use std::path::{Path, PathBuf};

use anyhow::anyhow;
use inquire::Select;

use crate::app::parse::load_app_record;
use crate::cli::node::nodes_file;
use crate::node::list::load_all_nodes;
use crate::node::types::NodeRecord;
use crate::provider::DeploymentTarget;
use crate::store::duck::{InstalledServiceRecord, load_installed_service_configs};

use super::PreparedDeployment;
use super::target::{
    app_qa_file, apply_cli_values, build_deployment_targets, load_app_records_by_names,
    parse_cli_value_overrides, resolve_apps,
};
use super::{node_label, node_name};

pub async fn prepare_deployment(
    home: &Path,
    provider: String,
    workspace: PathBuf,
    requested_node: Option<String>,
    requested_values: Vec<String>,
    requested_apps: Option<Vec<String>>,
) -> anyhow::Result<PreparedDeployment> {
    let workspace = absolute_workspace(&workspace)?;
    let nodes = load_all_nodes(&nodes_file(home)).await?;
    let node = select_node(&nodes, requested_node.as_deref())?;
    let app_home = home.join("app");
    let app_names = resolve_apps(requested_apps, &app_home).await?;
    let mut apps = load_app_records_by_names(&app_names, &app_home).await?;
    let value_overrides = parse_cli_value_overrides(&requested_values)?;
    apply_cli_values(&mut apps, &value_overrides)?;
    let targets = build_deployment_targets(apps, home, &node, &workspace).await?;

    Ok(PreparedDeployment {
        provider,
        node,
        app_names,
        app_home,
        workspace,
        targets,
    })
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
    provider: String,
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

    let app_home = home.join("app");
    let qa_file = app_qa_file(&app_home.join(&service.app_name));
    let mut app = load_app_record(&qa_file).await?;
    let stored_config = load_installed_service_configs(home)
        .await?
        .into_iter()
        .find(|record| record.service == service.service)
        .ok_or_else(|| anyhow!("service '{}' config not found", service.service))?;

    for value in &mut app.values {
        if let Some(stored) = stored_config.app_values.get(&value.name) {
            value.value = Some(stored.clone());
        }
    }

    let target = DeploymentTarget::new(app, service.service.clone());

    Ok(PreparedDeployment {
        provider,
        node,
        app_names: vec![service.app_name.clone()],
        app_home,
        workspace: absolute_workspace(Path::new(&service.workspace))?,
        targets: vec![target],
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
mod tests {
    use super::absolute_workspace;
    use std::path::Path;

    #[test]
    fn absolute_workspace_resolves_relative_path_against_cwd() {
        let resolved = absolute_workspace(Path::new("./workspace")).expect("absolute");
        assert!(
            resolved.is_absolute(),
            "expected absolute, got {:?}",
            resolved
        );
        assert!(resolved.ends_with("workspace"));
    }

    #[test]
    fn absolute_workspace_preserves_already_absolute_path() {
        let resolved = absolute_workspace(Path::new("/srv/ins-ws")).expect("absolute");
        assert_eq!(resolved, Path::new("/srv/ins-ws"));
    }
}
