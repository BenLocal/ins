use std::collections::BTreeMap;

use anyhow::{anyhow, bail};

use crate::volume::types::{ResolvedVolume, VolumeRecord};

pub(crate) fn inject_compose_volumes(
    content: &str,
    node_name: &str,
    volumes: &[VolumeRecord],
) -> anyhow::Result<(String, Vec<ResolvedVolume>)> {
    let mut document: serde_yaml::Value =
        serde_yaml::from_str(content).map_err(|e| anyhow!("parse compose yaml: {}", e))?;

    let Some(root) = document.as_mapping_mut() else {
        return Ok((content.to_string(), Vec::new()));
    };

    let volumes_key = serde_yaml::Value::String("volumes".into());
    let Some(top_volumes) = root.get_mut(&volumes_key).and_then(|v| v.as_mapping_mut()) else {
        return Ok((content.to_string(), Vec::new()));
    };

    let mut resolved: Vec<ResolvedVolume> = Vec::new();

    let names: Vec<String> = top_volumes
        .keys()
        .filter_map(|k| k.as_str().map(str::to_string))
        .collect();

    for name in &names {
        let record = volumes
            .iter()
            .find(|v| v.name() == name && v.node() == node_name);
        let Some(record) = record else {
            bail!(
                "volume '{}' is not configured on node '{}'",
                name,
                node_name
            );
        };

        let docker_name = format!("ins_{}", name);
        let (driver, driver_opts) = driver_opts_for(record);

        let mut replacement = serde_yaml::Mapping::new();
        replacement.insert(
            serde_yaml::Value::String("external".into()),
            serde_yaml::Value::Bool(true),
        );
        replacement.insert(
            serde_yaml::Value::String("name".into()),
            serde_yaml::Value::String(docker_name.clone()),
        );
        top_volumes.insert(
            serde_yaml::Value::String(name.clone()),
            serde_yaml::Value::Mapping(replacement),
        );

        resolved.push(ResolvedVolume {
            docker_name,
            driver,
            driver_opts,
        });
    }

    let rewritten =
        serde_yaml::to_string(&document).map_err(|e| anyhow!("serialize compose yaml: {}", e))?;
    Ok((rewritten, resolved))
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
mod tests {
    use super::*;
    use crate::volume::types::{CifsVolume, FilesystemVolume, VolumeRecord};

    fn fs(name: &str, node: &str, path: &str) -> VolumeRecord {
        VolumeRecord::Filesystem(FilesystemVolume {
            name: name.into(),
            node: node.into(),
            path: path.into(),
        })
    }

    fn cifs(name: &str, node: &str, server: &str, username: &str, password: &str) -> VolumeRecord {
        VolumeRecord::Cifs(CifsVolume {
            name: name.into(),
            node: node.into(),
            server: server.into(),
            username: username.into(),
            password: password.into(),
        })
    }

    #[test]
    fn returns_unchanged_when_no_top_level_volumes() {
        let compose = "services:\n  web:\n    image: nginx\n";
        let (rewritten, resolved) = inject_compose_volumes(compose, "node1", &[]).expect("ok");
        assert_eq!(resolved.len(), 0);
        assert!(rewritten.contains("nginx"));
    }

    #[test]
    fn rewrites_filesystem_volume_to_external() {
        let compose = r#"
services:
  web:
    image: nginx
    volumes:
      - data:/var/lib/app
volumes:
  data: {}
"#;
        let volumes = vec![fs("data", "node1", "/mnt/data")];
        let (rewritten, resolved) = inject_compose_volumes(compose, "node1", &volumes).expect("ok");

        let doc: serde_yaml::Value = serde_yaml::from_str(&rewritten).expect("yaml");
        let data = doc
            .get("volumes")
            .and_then(|v| v.get("data"))
            .and_then(|v| v.as_mapping())
            .expect("data mapping");
        assert_eq!(
            data.get(serde_yaml::Value::String("external".into())),
            Some(&serde_yaml::Value::Bool(true))
        );
        assert_eq!(
            data.get(serde_yaml::Value::String("name".into()))
                .and_then(|v| v.as_str()),
            Some("ins_data")
        );

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].docker_name, "ins_data");
        assert_eq!(resolved[0].driver, "local");
        assert_eq!(resolved[0].driver_opts.get("type").unwrap(), "none");
        assert_eq!(resolved[0].driver_opts.get("o").unwrap(), "bind");
        assert_eq!(resolved[0].driver_opts.get("device").unwrap(), "/mnt/data");
    }

    #[test]
    fn rewrites_cifs_volume_with_credentials_in_options() {
        let compose = r#"
services:
  web: { image: nginx }
volumes:
  data: {}
"#;
        let volumes = vec![cifs("data", "node2", "//10.0.0.5/share", "alice", "s3cr3t")];
        let (_rewritten, resolved) =
            inject_compose_volumes(compose, "node2", &volumes).expect("ok");

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].driver_opts.get("type").unwrap(), "cifs");
        assert_eq!(
            resolved[0].driver_opts.get("o").unwrap(),
            "username=alice,password=s3cr3t"
        );
        assert_eq!(
            resolved[0].driver_opts.get("device").unwrap(),
            "//10.0.0.5/share"
        );
    }

    #[test]
    fn errors_when_volume_not_configured_for_node() {
        let compose = r#"
services:
  web: { image: nginx }
volumes:
  data: {}
"#;
        let volumes = vec![fs("data", "other-node", "/mnt/data")];
        let err = inject_compose_volumes(compose, "node1", &volumes)
            .expect_err("missing config should fail");
        let msg = err.to_string();
        assert!(msg.contains("volume 'data'"), "unexpected message: {}", msg);
        assert!(msg.contains("node 'node1'"), "unexpected message: {}", msg);
    }

    #[test]
    fn picks_node_specific_record_when_duplicates_exist() {
        let compose = r#"
services:
  web: { image: nginx }
volumes:
  data: {}
"#;
        let volumes = vec![
            fs("data", "node1", "/mnt/one"),
            fs("data", "node2", "/mnt/two"),
        ];
        let (_rewritten, resolved) =
            inject_compose_volumes(compose, "node2", &volumes).expect("ok");
        assert_eq!(resolved[0].driver_opts.get("device").unwrap(), "/mnt/two");
    }
}
