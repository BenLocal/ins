use std::collections::BTreeMap;

use anyhow::{anyhow, bail};

use crate::app::types::AppRecord;
use crate::volume::types::{ResolvedVolume, VolumeRecord};

pub(crate) fn inject_compose_volumes(
    content: &str,
    app: &AppRecord,
    node_name: &str,
    volumes: &[VolumeRecord],
) -> anyhow::Result<(String, Vec<ResolvedVolume>)> {
    let named = resolve_target_volumes(app, node_name, volumes)?;
    if named.is_empty() {
        return Ok((content.to_string(), Vec::new()));
    }

    let mut document: serde_yaml::Value =
        serde_yaml::from_str(content).map_err(|e| anyhow!("parse compose yaml: {}", e))?;

    let Some(root) = document.as_mapping_mut() else {
        bail!("compose file root is not a mapping");
    };

    let volumes_key = serde_yaml::Value::String("volumes".into());
    let needs_reset = root
        .get(&volumes_key)
        .map(|v| !v.is_mapping())
        .unwrap_or(true);
    if needs_reset {
        root.insert(
            volumes_key.clone(),
            serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
        );
    }
    let top_volumes = root
        .get_mut(&volumes_key)
        .and_then(|v| v.as_mapping_mut())
        .expect("top-level volumes mapping just ensured");

    let mut resolved = Vec::with_capacity(named.len());
    for (name, rv) in named {
        let mut replacement = serde_yaml::Mapping::new();
        replacement.insert(
            serde_yaml::Value::String("external".into()),
            serde_yaml::Value::Bool(true),
        );
        replacement.insert(
            serde_yaml::Value::String("name".into()),
            serde_yaml::Value::String(rv.docker_name.clone()),
        );
        top_volumes.insert(
            serde_yaml::Value::String(name),
            serde_yaml::Value::Mapping(replacement),
        );
        resolved.push(rv);
    }

    let rewritten =
        serde_yaml::to_string(&document).map_err(|e| anyhow!("serialize compose yaml: {}", e))?;
    Ok((rewritten, resolved))
}

/// Resolve the list of volumes this app needs on the given node, pairing each
/// logical name with its ResolvedVolume (docker_name, driver, driver_opts).
pub(crate) fn resolve_target_volumes(
    app: &AppRecord,
    node_name: &str,
    volumes: &[VolumeRecord],
) -> anyhow::Result<Vec<(String, ResolvedVolume)>> {
    let required = required_volume_names(app, node_name, volumes);
    let mut resolved = Vec::with_capacity(required.len());
    for name in required {
        let record = volumes
            .iter()
            .find(|v| v.name() == name && v.node() == node_name)
            .ok_or_else(|| {
                anyhow!(
                    "volume '{}' required by app '{}' is not configured on node '{}'",
                    name,
                    app.name,
                    node_name
                )
            })?;
        let docker_name = format!("ins_{}", name);
        let (driver, driver_opts) = driver_opts_for(record);
        resolved.push((
            name,
            ResolvedVolume {
                docker_name,
                driver,
                driver_opts,
            },
        ));
    }
    Ok(resolved)
}

fn required_volume_names(
    app: &AppRecord,
    node_name: &str,
    volumes: &[VolumeRecord],
) -> Vec<String> {
    if app.all_volume {
        let mut names: Vec<String> = volumes
            .iter()
            .filter(|v| v.node() == node_name)
            .map(|v| v.name().to_string())
            .collect();
        names.sort();
        names.dedup();
        names
    } else {
        let mut seen = std::collections::BTreeSet::new();
        let mut names = Vec::new();
        for n in &app.volumes {
            if seen.insert(n.clone()) {
                names.push(n.clone());
            }
        }
        names
    }
}

fn driver_opts_for(record: &VolumeRecord) -> (String, BTreeMap<String, String>) {
    let mut opts = BTreeMap::new();
    match record {
        VolumeRecord::Filesystem(v) => {
            opts.insert("type".into(), "none".into());
            opts.insert("o".into(), "bind".into());
            opts.insert("device".into(), v.path.clone());
        }
        VolumeRecord::Cifs(v) => {
            opts.insert("type".into(), "cifs".into());
            opts.insert(
                "o".into(),
                format!("username={},password={}", v.username, v.password),
            );
            opts.insert("device".into(), v.server.clone());
        }
    }
    ("local".into(), opts)
}

#[cfg(test)]
#[path = "compose_test.rs"]
mod compose_test;
