use anyhow::{Context, anyhow};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::process::Child;
use tokio::process::Command;
use tokio::signal;
use tokio::time::{Duration, sleep};

use crate::env::{shell_exports, shell_quote};
use crate::execution_output::ExecutionOutput;
use crate::file::remote::RemoteFile;
use crate::node::types::NodeRecord;
use crate::provider::{ProviderContext, ProviderTrait};
use crate::volume::types::ResolvedVolume;

pub struct DockerComposeProvider;

#[derive(Clone, Copy, Debug)]
enum ComposeCommandKind {
    DockerComposePlugin,
    DockerComposeBinary,
}

#[async_trait]
impl ProviderTrait for DockerComposeProvider {
    async fn validate(&self, ctx: ProviderContext) -> anyhow::Result<()> {
        ctx.output.line(format!(
            "Provider '{}': validating deployment",
            ctx.provider
        ));

        match &ctx.node {
            NodeRecord::Local() => {
                let compose_command = resolve_local_compose_command(&ctx.output).await?;

                for target in &ctx.targets {
                    let app_dir = ctx.workspace.join(&target.service);
                    let compose_file = compose_file_for_target(&app_dir, &target.app.name)?;
                    let envs = ctx.env_for_target(&target.service);

                    ctx.output.line(format!(
                        "Validating app '{}' as service '{}' from {}",
                        target.app.name,
                        target.service,
                        app_dir.display()
                    ));

                    if ctx.output.echo_enabled() {
                        let status = run_local_compose_command_streaming(
                            compose_command,
                            &compose_file,
                            &app_dir,
                            &envs,
                            &["config", "-q"],
                        )
                        .await
                        .with_context(|| {
                            format!(
                                "failed to validate docker compose for app '{}' service '{}'",
                                target.app.name, target.service
                            )
                        })?;

                        if !status.success() {
                            return Err(anyhow!(
                                "❌ docker compose validation failed for app '{}' service '{}' (exit code {:?})",
                                target.app.name,
                                target.service,
                                status.code()
                            ));
                        }
                    } else {
                        let command_output = run_local_compose_command_capture(
                            compose_command,
                            &compose_file,
                            &app_dir,
                            &envs,
                            &["config", "-q"],
                        )
                        .await
                        .with_context(|| {
                            format!(
                                "failed to validate docker compose for app '{}' service '{}'",
                                target.app.name, target.service
                            )
                        })?;

                        append_command_output(
                            &ctx.output,
                            &command_output.stdout,
                            &command_output.stderr,
                        );

                        if !command_output.status.success() {
                            return Err(anyhow!(
                                "❌ docker compose validation failed for app '{}' service '{}' (exit code {:?})",
                                target.app.name,
                                target.service,
                                command_output.status.code()
                            ));
                        }
                    }

                    ctx.output.line(format!(
                        "✅ docker compose validation passed for app '{}' service '{}'",
                        target.app.name, target.service
                    ));
                }

                Ok(())
            }
            NodeRecord::Remote(remote) => {
                let remote_file = remote_file(remote);
                let compose_command = resolve_remote_compose_command(&remote_file).await?;

                for target in &ctx.targets {
                    let app_dir = ctx.workspace.join(&target.service);
                    let envs = ctx.env_for_target(&target.service);
                    let command =
                        docker_compose_shell_command(compose_command, &app_dir, &envs, "config -q");

                    ctx.output.line(format!(
                        "Validating app '{}' as service '{}' from {} on remote node '{}'",
                        target.app.name,
                        target.service,
                        app_dir.display(),
                        remote.name
                    ));

                    let output = remote_file.exec(&command).await.with_context(|| {
                        format!(
                            "failed to validate docker compose for app '{}' service '{}' on node '{}'",
                            target.app.name, target.service, remote.name
                        )
                    })?;
                    let rendered = render_remote_output(&output.stdout, &output.stderr);
                    if rendered != "no remote output" {
                        ctx.output.line(rendered.clone());
                    }

                    if output.exit_status != 0 {
                        return Err(anyhow!(
                            "❌ remote docker compose validation failed for app '{}' service '{}' on node '{}' (exit code {})\n{}",
                            target.app.name,
                            target.service,
                            remote.name,
                            output.exit_status,
                            rendered
                        ));
                    }

                    ctx.output.line(format!(
                        "✅ docker compose validation passed for app '{}' service '{}' on remote node '{}'",
                        target.app.name, target.service, remote.name
                    ));
                }

                Ok(())
            }
        }
    }

    async fn run(&self, ctx: ProviderContext) -> anyhow::Result<()> {
        ctx.output
            .line(format!("Provider '{}': starting deployment", ctx.provider));

        match &ctx.node {
            NodeRecord::Local() => {
                let compose_command = resolve_local_compose_command(&ctx.output).await?;

                ensure_volumes_local(&ctx.volumes, &ctx.output).await?;

                for target in &ctx.targets {
                    let app_dir = ctx.workspace.join(&target.service);
                    let compose_file = compose_file_for_target(&app_dir, &target.app.name)?;
                    let envs = ctx.env_for_target(&target.service);

                    ctx.output.line(format!(
                        "Deploying app '{}' as service '{}' from {}",
                        target.app.name,
                        target.service,
                        app_dir.display()
                    ));

                    if ctx.output.echo_enabled() {
                        let mut child = spawn_local_compose_command(
                            compose_command,
                            &compose_file,
                            &app_dir,
                            &envs,
                            &["up", "-d"],
                        )
                        .await
                        .with_context(|| {
                            format!(
                                "failed to run docker compose for app '{}' service '{}' (file {})",
                                target.app.name,
                                target.service,
                                compose_file.display()
                            )
                        })?;

                        let status =
                            wait_for_child_or_ctrl_c(&mut child, &target.app.name, &target.service)
                                .await?;

                        if !status.success() {
                            return Err(anyhow!(
                                "❌ docker compose up failed for app '{}' service '{}' (exit code {:?})",
                                target.app.name,
                                target.service,
                                status.code()
                            ));
                        }
                    } else {
                        let command_output = run_local_compose_command_capture(
                            compose_command,
                            &compose_file,
                            &app_dir,
                            &envs,
                            &["up", "-d"],
                        )
                        .await
                        .with_context(|| {
                            format!(
                                "failed to run docker compose for app '{}' service '{}' (file {})",
                                target.app.name,
                                target.service,
                                compose_file.display()
                            )
                        })?;

                        append_command_output(
                            &ctx.output,
                            &command_output.stdout,
                            &command_output.stderr,
                        );

                        if !command_output.status.success() {
                            return Err(anyhow!(
                                "❌ docker compose up failed for app '{}' service '{}' (exit code {:?})",
                                target.app.name,
                                target.service,
                                command_output.status.code()
                            ));
                        }
                    }

                    ctx.output.line(format!(
                        "✅ docker compose up succeeded for app '{}' service '{}'",
                        target.app.name, target.service
                    ));
                }

                Ok(())
            }
            NodeRecord::Remote(remote) => {
                let remote_file = remote_file(remote);
                let compose_command = resolve_remote_compose_command(&remote_file).await?;

                ensure_volumes_remote(&remote_file, &remote.name, &ctx.volumes, &ctx.output)
                    .await?;

                for target in &ctx.targets {
                    let app_dir = ctx.workspace.join(&target.service);
                    let envs = ctx.env_for_target(&target.service);
                    let command =
                        docker_compose_shell_command(compose_command, &app_dir, &envs, "up -d");

                    ctx.output.line(format!(
                        "Deploying app '{}' as service '{}' from {} on remote node '{}'",
                        target.app.name,
                        target.service,
                        app_dir.display(),
                        remote.name
                    ));

                    let output = remote_file.tty_exec(&command).await.with_context(|| {
                        format!(
                            "failed to run docker compose for app '{}' service '{}' on node '{}'",
                            target.app.name, target.service, remote.name
                        )
                    })?;
                    let rendered = render_remote_output(&output.stdout, &output.stderr);
                    if rendered != "no remote output" {
                        ctx.output.line(rendered.clone());
                    }

                    if output.exit_status != 0 {
                        return Err(anyhow!(
                            "❌ remote docker compose up failed for app '{}' service '{}' on node '{}' (exit code {})\n{}",
                            target.app.name,
                            target.service,
                            remote.name,
                            output.exit_status,
                            rendered
                        ));
                    }

                    ctx.output.line(format!(
                        "✅ docker compose up succeeded for app '{}' service '{}' on remote node '{}'",
                        target.app.name, target.service, remote.name
                    ));
                }

                Ok(())
            }
        }
    }
}

async fn resolve_local_compose_command(
    output: &ExecutionOutput,
) -> anyhow::Result<ComposeCommandKind> {
    if which_local("docker").await? {
        if output.echo_enabled() {
            let status = Command::new("docker")
                .arg("compose")
                .arg("version")
                .status()
                .await
                .context("run 'docker compose version'")?;
            if status.success() {
                return Ok(ComposeCommandKind::DockerComposePlugin);
            }
        } else {
            let version_output = Command::new("docker")
                .arg("compose")
                .arg("version")
                .output()
                .await
                .context("run 'docker compose version'")?;
            append_command_output(output, &version_output.stdout, &version_output.stderr);
            if version_output.status.success() {
                return Ok(ComposeCommandKind::DockerComposePlugin);
            }
        }
    }

    if which_local("docker-compose").await? {
        if output.echo_enabled() {
            let status = Command::new("docker-compose")
                .arg("--version")
                .status()
                .await
                .context("run 'docker-compose --version'")?;
            if status.success() {
                return Ok(ComposeCommandKind::DockerComposeBinary);
            }
        } else {
            let version_output = Command::new("docker-compose")
                .arg("--version")
                .output()
                .await
                .context("run 'docker-compose --version'")?;
            append_command_output(output, &version_output.stdout, &version_output.stderr);
            if version_output.status.success() {
                return Ok(ComposeCommandKind::DockerComposeBinary);
            }
        }
    }

    Err(anyhow!(
        "docker compose command not found: neither 'docker compose' nor 'docker-compose' is available"
    ))
}

async fn which_local(command: &str) -> anyhow::Result<bool> {
    match which::which(command) {
        Ok(_) => Ok(true),
        // Match the old behavior of `which <cmd>`: return false when the binary isn't on PATH.
        Err(which::Error::CannotFindBinaryPath) => Ok(false),
        Err(e) => Err(anyhow!("failed to find '{}' in PATH: {}", command, e)),
    }
}

async fn run_local_compose_command_capture(
    compose_command: ComposeCommandKind,
    compose_file: &std::path::Path,
    app_dir: &std::path::Path,
    envs: &BTreeMap<String, String>,
    args: &[&str],
) -> anyhow::Result<std::process::Output> {
    let mut command =
        build_local_compose_command(compose_command, compose_file, app_dir, envs, args);
    command.output().await.map_err(anyhow::Error::from)
}

async fn run_local_compose_command_streaming(
    compose_command: ComposeCommandKind,
    compose_file: &std::path::Path,
    app_dir: &std::path::Path,
    envs: &BTreeMap<String, String>,
    args: &[&str],
) -> anyhow::Result<std::process::ExitStatus> {
    let mut command =
        build_local_compose_command(compose_command, compose_file, app_dir, envs, args);
    command.status().await.map_err(anyhow::Error::from)
}

async fn spawn_local_compose_command(
    compose_command: ComposeCommandKind,
    compose_file: &std::path::Path,
    app_dir: &std::path::Path,
    envs: &BTreeMap<String, String>,
    args: &[&str],
) -> anyhow::Result<Child> {
    let mut command =
        build_local_compose_command(compose_command, compose_file, app_dir, envs, args);
    command
        .kill_on_drop(true)
        .process_group(0)
        .spawn()
        .map_err(anyhow::Error::from)
}

fn build_local_compose_command(
    compose_command: ComposeCommandKind,
    compose_file: &std::path::Path,
    app_dir: &std::path::Path,
    envs: &BTreeMap<String, String>,
    args: &[&str],
) -> Command {
    let mut command = match compose_command {
        ComposeCommandKind::DockerComposePlugin => {
            let mut command = Command::new("docker");
            command.arg("compose");
            command
        }
        ComposeCommandKind::DockerComposeBinary => Command::new("docker-compose"),
    };

    command.arg("-f").arg(compose_file);
    for arg in args {
        command.arg(arg);
    }
    command.envs(envs);
    command.current_dir(app_dir);
    command
}

fn compose_file_for_target(
    app_dir: &std::path::Path,
    app_name: &str,
) -> anyhow::Result<std::path::PathBuf> {
    let compose_yml = app_dir.join("docker-compose.yml");
    let compose_yaml = app_dir.join("docker-compose.yaml");

    if compose_yml.exists() {
        Ok(compose_yml)
    } else if compose_yaml.exists() {
        Ok(compose_yaml)
    } else {
        Err(anyhow!(
            "docker compose file not found for app '{}' in {}",
            app_name,
            app_dir.display()
        ))
    }
}

fn remote_file(remote: &crate::node::types::RemoteNodeRecord) -> RemoteFile {
    let remote_file = RemoteFile::new(
        remote.ip.clone(),
        remote.port,
        remote.user.clone(),
        remote.password.clone(),
    );
    if let Some(key_path) = &remote.key_path {
        remote_file.with_key_path(key_path.clone())
    } else {
        remote_file
    }
}

async fn resolve_remote_compose_command(
    remote_file: &RemoteFile,
) -> anyhow::Result<ComposeCommandKind> {
    let output = remote_file
        .exec(
            "if which docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then \
echo docker-compose-plugin; \
elif which docker-compose >/dev/null 2>&1; then \
echo docker-compose-binary; \
else exit 1; fi",
        )
        .await
        .context("detect remote docker compose command")?;

    if output.exit_status != 0 {
        return Err(anyhow!(
            "docker compose command not found on remote node\n{}",
            render_remote_output(&output.stdout, &output.stderr)
        ));
    }

    match output.stdout.trim() {
        "docker-compose-plugin" => Ok(ComposeCommandKind::DockerComposePlugin),
        "docker-compose-binary" => Ok(ComposeCommandKind::DockerComposeBinary),
        other => Err(anyhow!(
            "unknown remote docker compose command probe result '{}'",
            other
        )),
    }
}

fn docker_compose_shell_command(
    compose_command: ComposeCommandKind,
    app_dir: &std::path::Path,
    envs: &BTreeMap<String, String>,
    docker_compose_args: &str,
) -> String {
    let app_dir = shell_quote(&app_dir.display().to_string());
    let compose_command = match compose_command {
        ComposeCommandKind::DockerComposePlugin => "docker compose",
        ComposeCommandKind::DockerComposeBinary => "docker-compose",
    };
    let exports = shell_exports(envs);
    let prefix = if exports.is_empty() {
        String::new()
    } else {
        format!("{exports} ")
    };
    format!(
        "cd {app_dir} && if [ -f docker-compose.yml ]; then compose_file=docker-compose.yml; \
elif [ -f docker-compose.yaml ]; then compose_file=docker-compose.yaml; \
else echo \"docker compose file not found in $(pwd)\" >&2; exit 127; fi; \
{prefix}{compose_command} -f \"$compose_file\" {docker_compose_args}"
    )
}

fn render_remote_output(stdout: &str, stderr: &str) -> String {
    let stdout = collapse_carriage_returns(stdout);
    let stderr = collapse_carriage_returns(stderr);

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

/// Normalize carriage-return based progress (e.g. `\r` updating in-place).
///
/// We intentionally keep only the last "segment" after the last `\r` on each `\n`-separated line,
/// so it won't keep re-printing / moving the cursor like a TTY would.
fn collapse_carriage_returns(s: &str) -> String {
    // Make Windows newlines consistent first.
    let s = s.replace("\r\n", "\n");
    s.split('\n')
        .map(|line| line.split('\r').next_back().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

fn append_command_output(output: &ExecutionOutput, stdout: &[u8], stderr: &[u8]) {
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

async fn wait_for_child_or_ctrl_c(
    child: &mut Child,
    app_name: &str,
    service: &str,
) -> anyhow::Result<std::process::ExitStatus> {
    tokio::select! {
        status = child.wait() => {
            status.map_err(|e| anyhow!(
                "failed to wait for docker compose for app '{}' service '{}': {}",
                app_name,
                service,
                e
            ))
        }
        signal_result = signal::ctrl_c() => {
            signal_result.map_err(|e| anyhow!("failed to listen for Ctrl+C: {}", e))?;
            eprintln!("Ctrl+C received, stopping docker compose for app '{}' service '{}'", app_name, service);
            terminate_process_group(child, "TERM");
            sleep(Duration::from_secs(2)).await;
            if child.try_wait().ok().flatten().is_none() {
                eprintln!(
                    "docker compose for app '{}' service '{}' is still running, sending SIGKILL",
                    app_name,
                    service
                );
                terminate_process_group(child, "KILL");
                let _ = child.kill().await;
            }
            let _ = child.wait().await;
            Err(anyhow!("deployment cancelled by Ctrl+C"))
        }
    }
}

fn terminate_process_group(child: &Child, signal: &str) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        match std::process::Command::new("kill")
            .arg(format!("-{signal}"))
            .arg(format!("-{pid}"))
            .status()
        {
            Ok(status) if status.success() => {}
            Ok(status) => eprintln!(
                "failed to send SIG{} to docker compose process group {}: exit {:?}",
                signal,
                pid,
                status.code()
            ),
            Err(err) => eprintln!(
                "failed to send SIG{} to docker compose process group {}: {}",
                signal, pid, err
            ),
        }
    }
}

fn docker_volume_create_shell_command(volume: &ResolvedVolume) -> String {
    let mut parts = vec![
        "docker".to_string(),
        "volume".to_string(),
        "create".to_string(),
        "--driver".to_string(),
        crate::env::shell_quote(&volume.driver),
    ];
    for (k, v) in &volume.driver_opts {
        parts.push("--opt".to_string());
        parts.push(crate::env::shell_quote(&format!("{k}={v}")));
    }
    parts.push(crate::env::shell_quote(&volume.docker_name));
    parts.join(" ")
}

fn docker_volume_ensure_shell_command(volume: &ResolvedVolume) -> String {
    let inspect_name = crate::env::shell_quote(&volume.docker_name);
    let create_cmd = docker_volume_create_shell_command(volume);
    format!(
        "if docker volume inspect {inspect_name} >/dev/null 2>&1; then \
echo \"volume {} already exists\"; \
else {create_cmd}; fi",
        volume.docker_name
    )
}

async fn ensure_volumes_local(
    volumes: &[ResolvedVolume],
    output: &ExecutionOutput,
) -> anyhow::Result<()> {
    for volume in volumes {
        let inspect = Command::new("docker")
            .args(["volume", "inspect", &volume.docker_name])
            .output()
            .await
            .context("run 'docker volume inspect'")?;
        if inspect.status.success() {
            output.line(format!(
                "Reusing existing docker volume '{}'",
                volume.docker_name
            ));
            continue;
        }

        output.line(format!(
            "Creating docker volume '{}' ({})",
            volume.docker_name, volume.driver
        ));

        let mut create = Command::new("docker");
        create
            .arg("volume")
            .arg("create")
            .arg("--driver")
            .arg(&volume.driver);
        for (k, v) in &volume.driver_opts {
            create.arg("--opt").arg(format!("{k}={v}"));
        }
        create.arg(&volume.docker_name);
        let result = create
            .output()
            .await
            .with_context(|| format!("run docker volume create for '{}'", volume.docker_name))?;
        append_command_output(output, &result.stdout, &result.stderr);
        if !result.status.success() {
            return Err(anyhow!(
                "docker volume create failed for '{}' (exit code {:?})",
                volume.docker_name,
                result.status.code()
            ));
        }
    }
    Ok(())
}

async fn ensure_volumes_remote(
    remote_file: &RemoteFile,
    node_name: &str,
    volumes: &[ResolvedVolume],
    output: &ExecutionOutput,
) -> anyhow::Result<()> {
    for volume in volumes {
        output.line(format!(
            "Ensuring docker volume '{}' on remote node '{}'",
            volume.docker_name, node_name
        ));
        let command = docker_volume_ensure_shell_command(volume);
        let result = remote_file.exec(&command).await.with_context(|| {
            format!(
                "ensure docker volume '{}' on node '{}'",
                volume.docker_name, node_name
            )
        })?;
        let rendered = render_remote_output(&result.stdout, &result.stderr);
        if rendered != "no remote output" {
            output.line(rendered.clone());
        }
        if result.exit_status != 0 {
            return Err(anyhow!(
                "ensure docker volume '{}' failed on node '{}' (exit {})\n{}",
                volume.docker_name,
                node_name,
                result.exit_status,
                rendered
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
}
