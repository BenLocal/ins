use std::collections::BTreeMap;
use std::path::Path;

use anyhow::anyhow;

use crate::node::types::NodeRecord;

use super::node_name;

pub(crate) struct ComposeRewriteOutcome {
    pub content: String,
    pub has_build: bool,
}

pub(crate) fn is_docker_compose_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("docker-compose.yml" | "docker-compose.yaml")
    )
}

pub(crate) fn maybe_inject_compose_labels(
    path: &Path,
    content: &str,
    template_values: &serde_json::Value,
    node: &NodeRecord,
) -> anyhow::Result<ComposeRewriteOutcome> {
    if !is_docker_compose_file(path) {
        return Ok(ComposeRewriteOutcome {
            content: content.to_string(),
            has_build: false,
        });
    }
    inject_compose_labels(
        content,
        &build_compose_metadata_labels(template_values, node),
    )
}

fn inject_compose_labels(
    content: &str,
    metadata_labels: &BTreeMap<String, String>,
) -> anyhow::Result<ComposeRewriteOutcome> {
    let mut document: serde_yaml::Value =
        serde_yaml::from_str(content).map_err(|e| anyhow!("parse compose yaml: {}", e))?;

    let Some(root) = document.as_mapping_mut() else {
        return Ok(ComposeRewriteOutcome {
            content: content.to_string(),
            has_build: false,
        });
    };
    let Some(services) = root
        .get_mut(serde_yaml::Value::String("services".into()))
        .and_then(serde_yaml::Value::as_mapping_mut)
    else {
        return Ok(ComposeRewriteOutcome {
            content: content.to_string(),
            has_build: false,
        });
    };

    let mut has_build = false;
    for service in services.values_mut() {
        let Some(service_mapping) = service.as_mapping_mut() else {
            continue;
        };
        if service_mapping.contains_key(serde_yaml::Value::String("build".into())) {
            has_build = true;
        }
        let labels_key = serde_yaml::Value::String("labels".into());
        let existing = service_mapping.remove(&labels_key);
        let mut labels = labels_value_to_mapping(existing)?;

        for (key, value) in metadata_labels {
            labels.insert(
                serde_yaml::Value::String(key.clone()),
                serde_yaml::Value::String(value.clone()),
            );
        }

        service_mapping.insert(labels_key, serde_yaml::Value::Mapping(labels));
    }

    let content =
        serde_yaml::to_string(&document).map_err(|e| anyhow!("serialize compose yaml: {}", e))?;
    Ok(ComposeRewriteOutcome { content, has_build })
}

fn labels_value_to_mapping(
    value: Option<serde_yaml::Value>,
) -> anyhow::Result<serde_yaml::Mapping> {
    let mut mapping = serde_yaml::Mapping::new();
    let Some(value) = value else {
        return Ok(mapping);
    };

    match value {
        serde_yaml::Value::Null => Ok(mapping),
        serde_yaml::Value::Mapping(existing) => Ok(existing),
        serde_yaml::Value::Sequence(items) => {
            for item in items {
                let Some(text) = item.as_str() else {
                    return Err(anyhow!("compose labels sequence entries must be strings"));
                };
                let (key, value) = text.split_once('=').unwrap_or((text, ""));
                mapping.insert(
                    serde_yaml::Value::String(key.to_string()),
                    serde_yaml::Value::String(value.to_string()),
                );
            }
            Ok(mapping)
        }
        _ => Err(anyhow!("compose labels must be a mapping or sequence")),
    }
}

pub(crate) fn build_compose_metadata_labels(
    template_values: &serde_json::Value,
    node: &NodeRecord,
) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    labels.insert("ins.node_name".into(), node_name(node).to_string());
    insert_compose_label(&mut labels, "ins.service", template_values.get("service"));
    insert_compose_label(
        &mut labels,
        "ins.namespace",
        template_values.get("namespace"),
    );

    if let Some(app) = template_values.get("app") {
        insert_compose_label(&mut labels, "ins.name", app.get("name"));
        insert_compose_label(&mut labels, "ins.description", app.get("description"));
        insert_compose_label(&mut labels, "ins.author_name", app.get("author_name"));
        insert_compose_label(&mut labels, "ins.author_email", app.get("author_email"));
        insert_compose_label(&mut labels, "ins.version", app.get("version"));
    }

    labels
}

fn insert_compose_label(
    labels: &mut BTreeMap<String, String>,
    key: &str,
    value: Option<&serde_json::Value>,
) {
    let Some(value) = value else {
        return;
    };
    if value.is_null() {
        return;
    }
    let text = value
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| value.to_string());
    labels.insert(key.to_string(), text);
}

#[cfg(test)]
#[path = "labels_test.rs"]
mod labels_test;
