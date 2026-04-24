use std::collections::BTreeMap;
use std::path::Path;

use super::build_remote_hook_command;

#[test]
fn remote_hook_command_quotes_paths_and_exports_env() {
    let mut envs = BTreeMap::new();
    envs.insert("INS_APP_NAME".into(), "mysql".into());
    envs.insert("MYSQL_PASSWORD".into(), "p w d".into());

    let cmd = build_remote_hook_command(
        Path::new("/srv/mysql workspace"),
        "bash",
        "./after.sh",
        &envs,
    );

    assert!(
        cmd.contains("cd '/srv/mysql workspace'"),
        "app_dir should be shell-quoted: {cmd}"
    );
    assert!(
        cmd.contains("'bash' './after.sh'"),
        "shell + script should be quoted: {cmd}"
    );
    assert!(cmd.contains("INS_APP_NAME="), "env exports missing: {cmd}");
    assert!(
        cmd.contains("MYSQL_PASSWORD='p w d'"),
        "env with spaces must be quoted: {cmd}"
    );
}

#[test]
fn remote_hook_command_handles_empty_env() {
    let cmd = build_remote_hook_command(Path::new("/srv/app"), "sh", "run.sh", &BTreeMap::new());
    assert_eq!(cmd, "cd '/srv/app' && 'sh' 'run.sh'");
}
