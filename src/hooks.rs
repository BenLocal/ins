//! Lifecycle hook runner for `before` / `after` scripts declared in qa.yaml.
//!
//! Hooks run on the target node (not the ins host) because they typically
//! touch paths and binaries that only exist there. They execute with the same
//! env set the provider passes to `docker compose` (`INS_APP_NAME`,
//! `INS_NODE_NAME`, `INS_SERVICE_<DEP>_*`, and each app value as
//! `<VALUE_NAME>`), so hook scripts can reference `$MYSQL_PASSWORD` directly.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, anyhow};
use tokio::process::Command;

use crate::app::types::ScriptHook;
use crate::env::{shell_exports, shell_quote};
use crate::execution_output::ExecutionOutput;
use crate::file::remote::RemoteFile;

const DEFAULT_HOOK_SHELL: &str = "bash";

/// Run a `before`/`after` hook locally, rooted at `app_dir` so relative
/// `script: ./before.sh` paths resolve correctly. No-op when the hook is unset.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_hook_local(
    hook: &ScriptHook,
    app_dir: &Path,
    envs: &BTreeMap<String, String>,
    output: &ExecutionOutput,
    phase: &str,
    app_name: &str,
    service: &str,
) -> anyhow::Result<()> {
    let Some(script) = hook.script.as_deref() else {
        return Ok(());
    };
    let shell = hook.shell.as_deref().unwrap_or(DEFAULT_HOOK_SHELL);

    output.line(format!(
        "Running {phase} hook for app '{app_name}' service '{service}': {shell} {script}"
    ));

    let command_output = Command::new(shell)
        .arg(script)
        .envs(envs)
        .current_dir(app_dir)
        .output()
        .await
        .with_context(|| format!("run {phase} hook '{shell} {script}' for service '{service}'"))?;

    append_output(output, &command_output.stdout, &command_output.stderr);

    if !command_output.status.success() {
        return Err(anyhow!(
            "❌ {phase} hook failed for app '{}' service '{}' (exit code {:?})",
            app_name,
            service,
            command_output.status.code()
        ));
    }

    output.line(format!(
        "✅ {phase} hook succeeded for app '{app_name}' service '{service}'"
    ));
    Ok(())
}

/// Run a `before`/`after` hook on a remote node over SSH. No-op when unset.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_hook_remote(
    hook: &ScriptHook,
    remote_file: &RemoteFile,
    node_name: &str,
    app_dir: &Path,
    envs: &BTreeMap<String, String>,
    output: &ExecutionOutput,
    phase: &str,
    app_name: &str,
    service: &str,
) -> anyhow::Result<()> {
    let Some(script) = hook.script.as_deref() else {
        return Ok(());
    };
    let shell = hook.shell.as_deref().unwrap_or(DEFAULT_HOOK_SHELL);

    output.line(format!(
        "Running {phase} hook for app '{app_name}' service '{service}' on node '{node_name}': {shell} {script}"
    ));

    let command = build_remote_hook_command(app_dir, shell, script, envs);
    let result = remote_file
        .exec(&command)
        .await
        .with_context(|| format!("run {phase} hook for '{service}' on node '{node_name}'"))?;

    let rendered = render_remote_output(&result.stdout, &result.stderr);
    if rendered != "no remote output" {
        output.line(rendered.clone());
    }

    if result.exit_status != 0 {
        return Err(anyhow!(
            "❌ {phase} hook failed for app '{}' service '{}' on node '{}' (exit code {})\n{}",
            app_name,
            service,
            node_name,
            result.exit_status,
            rendered
        ));
    }

    output.line(format!(
        "✅ {phase} hook succeeded for app '{app_name}' service '{service}' on node '{node_name}'"
    ));
    Ok(())
}

fn build_remote_hook_command(
    app_dir: &Path,
    shell: &str,
    script: &str,
    envs: &BTreeMap<String, String>,
) -> String {
    let app_dir_q = shell_quote(&app_dir.display().to_string());
    let script_q = shell_quote(script);
    let shell_q = shell_quote(shell);
    let exports = shell_exports(envs);
    let prefix = if exports.is_empty() {
        String::new()
    } else {
        format!("{exports} ")
    };
    format!("cd {app_dir_q} && {prefix}{shell_q} {script_q}")
}

fn append_output(output: &ExecutionOutput, stdout: &[u8], stderr: &[u8]) {
    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    if !stdout.is_empty() {
        for line in stdout.lines() {
            output.line(line);
        }
    }
    if !stderr.is_empty() {
        for line in stderr.lines() {
            output.error_line(line);
        }
    }
}

fn render_remote_output(stdout: &str, stderr: &str) -> String {
    let mut lines = Vec::new();
    if !stdout.trim().is_empty() {
        lines.push(format!("stdout:\n{}", stdout.trim_end()));
    }
    if !stderr.trim().is_empty() {
        lines.push(format!("stderr:\n{}", stderr.trim_end()));
    }
    if lines.is_empty() {
        "no remote output".to_string()
    } else {
        lines.join("\n")
    }
}

#[cfg(test)]
#[path = "hooks_test.rs"]
mod hooks_test;
