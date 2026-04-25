use super::{ComposeCommandKind, docker_compose_shell_command};
use crate::volume::types::ResolvedVolume;
use std::collections::BTreeMap;
use std::path::Path;

fn resolved(name: &str, opts: &[(&str, &str)]) -> ResolvedVolume {
    let mut map = BTreeMap::new();
    for (k, v) in opts {
        map.insert((*k).into(), (*v).into());
    }
    ResolvedVolume {
        docker_name: name.into(),
        driver: "local".into(),
        driver_opts: map,
    }
}

#[test]
fn docker_compose_shell_command_prefixes_env_exports() {
    let command = docker_compose_shell_command(
        ComposeCommandKind::DockerComposePlugin,
        Path::new("/tmp/app"),
        &BTreeMap::from([
            ("IMAGE_TAG".into(), "v1".into()),
            ("INS_NODE_NAME".into(), "node-a".into()),
        ]),
        "config -q",
    );

    assert!(command.contains("IMAGE_TAG='v1'"));
    assert!(command.contains("INS_NODE_NAME='node-a'"));
    assert!(command.contains("docker compose -f \"$compose_file\" config -q"));
}

#[test]
fn docker_volume_create_command_includes_all_opts_for_filesystem() {
    let volume = resolved(
        "ins_data",
        &[("type", "none"), ("o", "bind"), ("device", "/mnt/data")],
    );
    let cmd = super::docker_volume_create_shell_command(&volume);
    assert!(cmd.contains("docker volume create"));
    assert!(cmd.contains("--driver 'local'"));
    assert!(cmd.contains("--opt 'type=none'"));
    assert!(cmd.contains("--opt 'o=bind'"));
    assert!(cmd.contains("--opt 'device=/mnt/data'"));
    assert!(cmd.contains("'ins_data'"));
}

#[test]
fn docker_volume_create_command_quotes_cifs_credentials() {
    let volume = resolved(
        "ins_secret",
        &[
            ("type", "cifs"),
            ("o", "username=alice,password=pa ss!word"),
            ("device", "//10.0.0.5/share"),
        ],
    );
    let cmd = super::docker_volume_create_shell_command(&volume);
    assert!(cmd.contains("--opt 'o=username=alice,password=pa ss!word'"));
    assert!(cmd.contains("--opt 'device=//10.0.0.5/share'"));
}

#[test]
fn docker_volume_ensure_remote_shell_command_has_inspect_guard() {
    let volume = resolved(
        "ins_data",
        &[("type", "none"), ("o", "bind"), ("device", "/mnt/data")],
    );
    let cmd = super::docker_volume_ensure_shell_command(&volume);
    assert!(cmd.contains("docker volume inspect 'ins_data'"));
    assert!(cmd.contains("docker volume create"));
    assert!(cmd.contains("--opt 'device=/mnt/data'"));
}
