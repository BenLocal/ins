use anyhow::anyhow;
use serde_json::Value;
use std::collections::BTreeMap;

use crate::app::types::AppRecord;
use crate::node::types::NodeRecord;
use crate::provider::DeploymentTarget;
use crate::store::duck::InstalledServiceConfigRecord;

pub(crate) fn build_provider_envs(
    targets: &[DeploymentTarget],
    node: &NodeRecord,
    installed_services: &[InstalledServiceConfigRecord],
) -> anyhow::Result<BTreeMap<String, BTreeMap<String, String>>> {
    let mut envs = BTreeMap::new();

    for target in targets {
        let mut target_envs = build_target_envs(&target.app, &target.service, node)?;
        append_installed_service_envs(
            &mut target_envs,
            installed_services,
            &target.service,
            &target.app.dependencies,
        );
        envs.insert(target.service.clone(), target_envs);
    }

    Ok(envs)
}

fn build_target_envs(
    app: &AppRecord,
    service: &str,
    node: &NodeRecord,
) -> anyhow::Result<BTreeMap<String, String>> {
    let resolved_values = resolve_app_values_for_env(app)?;
    let mut envs = BTreeMap::new();

    envs.insert("INS_APP_NAME".into(), app.name.clone());
    envs.insert("INS_SERVICE_NAME".into(), service.to_string());
    envs.insert("INS_NODE_NAME".into(), node_name(node).to_string());

    if let Some(version) = &app.version {
        envs.insert("INS_VERSION".into(), version.clone());
    }
    if let Some(description) = &app.description {
        envs.insert("INS_DESCRIPTION".into(), description.clone());
    }
    if let Some(author_name) = &app.author_name {
        envs.insert("INS_AUTHOR_NAME".into(), author_name.clone());
    }
    if let Some(author_email) = &app.author_email {
        envs.insert("INS_AUTHOR_EMAIL".into(), author_email.clone());
    }

    for (name, value) in resolved_values {
        envs.insert(env_key_for_value_name(&name), provider_env_value(&value));
    }

    Ok(envs)
}

fn append_installed_service_envs(
    envs: &mut BTreeMap<String, String>,
    installed_services: &[InstalledServiceConfigRecord],
    current_service: &str,
    dependencies: &[String],
) {
    for service in installed_services {
        if service.service == current_service {
            continue;
        }
        if !dependencies
            .iter()
            .any(|dependency| dependency == &service.service)
        {
            continue;
        }

        let prefix = format!("INS_SERVICE_{}", env_key_for_value_name(&service.service));
        envs.insert(format!("{prefix}_SERVICE"), service.service.clone());
        envs.insert(format!("{prefix}_APP_NAME"), service.app_name.clone());
        envs.insert(format!("{prefix}_NODE_NAME"), service.node_name.clone());
        envs.insert(format!("{prefix}_WORKSPACE"), service.workspace.clone());
        envs.insert(
            format!("{prefix}_CREATED_AT_MS"),
            service.created_at_ms.to_string(),
        );
        //envs.insert(format!("{prefix}_QA_YAML"), service.qa_yaml.clone());

        for (name, value) in &service.app_values {
            envs.insert(
                format!("{prefix}_{}", env_key_for_value_name(name)),
                provider_env_value(value),
            );
        }
    }
}

fn resolve_app_values_for_env(app: &AppRecord) -> anyhow::Result<Vec<(String, Value)>> {
    let mut values = Vec::with_capacity(app.values.len());

    for value in &app.values {
        let resolved = value
            .value
            .clone()
            .or_else(|| value.default.clone())
            .or_else(|| {
                value
                    .options
                    .first()
                    .and_then(|option| option.value.clone())
            })
            .ok_or_else(|| anyhow!("missing value for '{}'", value.name))?;
        values.push((value.name.clone(), resolved));
    }

    Ok(values)
}

fn env_key_for_value_name(name: &str) -> String {
    let mut key = String::new();

    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            key.push(ch.to_ascii_uppercase());
        } else {
            key.push('_');
        }
    }

    if key.is_empty() || key.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        key.insert(0, '_');
    }

    key
}

fn provider_env_value(value: &Value) -> String {
    value
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| value.to_string())
}

pub(crate) fn shell_exports(envs: &BTreeMap<String, String>) -> String {
    envs.iter()
        .map(|(key, value)| format!("{key}={}", shell_quote(value)))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn node_name(node: &NodeRecord) -> &str {
    match node {
        NodeRecord::Local() => "local",
        NodeRecord::Remote(node) => &node.name,
    }
}

#[cfg(test)]
#[path = "env_test.rs"]
mod env_test;
