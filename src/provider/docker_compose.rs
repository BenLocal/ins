use anyhow::{Context, anyhow};
use async_trait::async_trait;
use tokio::process::Child;
use tokio::process::Command;
use tokio::signal;
use tokio::time::{Duration, sleep};

use crate::file::remote::RemoteFile;
use crate::node::types::NodeRecord;
use crate::provider::{ProviderContext, ProviderTrait};

pub struct DockerComposeProvider;

#[derive(Clone, Copy, Debug)]
enum ComposeCommandKind {
    DockerComposePlugin,
    DockerComposeBinary,
}

#[async_trait]
impl ProviderTrait for DockerComposeProvider {
    async fn validate(&self, ctx: ProviderContext) -> anyhow::Result<()> {
        println!("Provider '{}': validating deployment", ctx.provider);

        match &ctx.node {
            NodeRecord::Local() => {
                let compose_command = resolve_local_compose_command().await?;

                for target in &ctx.targets {
                    let app_dir = ctx.workspace.join(&target.service);
                    let compose_file = compose_file_for_target(&app_dir, &target.app.name)?;

                    println!(
                        "Validating app '{}' as service '{}' from {}",
                        target.app.name,
                        target.service,
                        app_dir.display()
                    );

                    let status = run_local_compose_command(
                        compose_command,
                        &compose_file,
                        &app_dir,
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

                    println!(
                        "✅ docker compose validation passed for app '{}' service '{}'",
                        target.app.name, target.service
                    );
                }

                Ok(())
            }
            NodeRecord::Remote(remote) => {
                let remote_file = remote_file(remote);
                let compose_command = resolve_remote_compose_command(&remote_file).await?;

                for target in &ctx.targets {
                    let app_dir = ctx.workspace.join(&target.service);
                    let command =
                        docker_compose_shell_command(compose_command, &app_dir, "config -q");

                    println!(
                        "Validating app '{}' as service '{}' from {} on remote node '{}'",
                        target.app.name,
                        target.service,
                        app_dir.display(),
                        remote.name
                    );

                    let output =
                        remote_file.exec(&command).await.with_context(|| {
                            format!(
                                "failed to validate docker compose for app '{}' service '{}' on node '{}'",
                                target.app.name, target.service, remote.name
                            )
                        })?;

                    if output.exit_status != 0 {
                        return Err(anyhow!(
                            "❌ remote docker compose validation failed for app '{}' service '{}' on node '{}' (exit code {})\n{}",
                            target.app.name,
                            target.service,
                            remote.name,
                            output.exit_status,
                            render_remote_output(&output.stdout, &output.stderr)
                        ));
                    }

                    println!(
                        "✅ docker compose validation passed for app '{}' service '{}' on remote node '{}'",
                        target.app.name, target.service, remote.name
                    );
                }

                Ok(())
            }
        }
    }

    async fn run(&self, ctx: ProviderContext) -> anyhow::Result<()> {
        println!("Provider '{}': starting deployment", ctx.provider);

        match &ctx.node {
            NodeRecord::Local() => {
                let compose_command = resolve_local_compose_command().await?;

                for target in &ctx.targets {
                    let app_dir = ctx.workspace.join(&target.service);
                    let compose_file = compose_file_for_target(&app_dir, &target.app.name)?;

                    println!(
                        "Deploying app '{}' as service '{}' from {}",
                        target.app.name,
                        target.service,
                        app_dir.display()
                    );

                    let mut child = spawn_local_compose_command(
                        compose_command,
                        &compose_file,
                        &app_dir,
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

                    println!(
                        "✅ docker compose up succeeded for app '{}' service '{}'",
                        target.app.name, target.service
                    );
                }

                Ok(())
            }
            NodeRecord::Remote(remote) => {
                let remote_file = remote_file(remote);
                let compose_command = resolve_remote_compose_command(&remote_file).await?;

                for target in &ctx.targets {
                    let app_dir = ctx.workspace.join(&target.service);
                    let command = docker_compose_shell_command(compose_command, &app_dir, "up -d");

                    println!(
                        "Deploying app '{}' as service '{}' from {} on remote node '{}'",
                        target.app.name,
                        target.service,
                        app_dir.display(),
                        remote.name
                    );

                    let output = remote_file.tty_exec(&command).await.with_context(|| {
                        format!(
                            "failed to run docker compose for app '{}' service '{}' on node '{}'",
                            target.app.name, target.service, remote.name
                        )
                    })?;

                    if output.exit_status != 0 {
                        return Err(anyhow!(
                            "❌ remote docker compose up failed for app '{}' service '{}' on node '{}' (exit code {})\n{}",
                            target.app.name,
                            target.service,
                            remote.name,
                            output.exit_status,
                            render_remote_output(&output.stdout, &output.stderr)
                        ));
                    }

                    println!(
                        "✅ docker compose up succeeded for app '{}' service '{}' on remote node '{}'",
                        target.app.name, target.service, remote.name
                    );
                }

                Ok(())
            }
        }
    }
}

async fn resolve_local_compose_command() -> anyhow::Result<ComposeCommandKind> {
    if which_local("docker").await? {
        let status = Command::new("docker")
            .arg("compose")
            .arg("version")
            .status()
            .await
            .context("run 'docker compose version'")?;
        if status.success() {
            return Ok(ComposeCommandKind::DockerComposePlugin);
        }
    }

    if which_local("docker-compose").await? {
        let status = Command::new("docker-compose")
            .arg("--version")
            .status()
            .await
            .context("run 'docker-compose --version'")?;
        if status.success() {
            return Ok(ComposeCommandKind::DockerComposeBinary);
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

async fn run_local_compose_command(
    compose_command: ComposeCommandKind,
    compose_file: &std::path::Path,
    app_dir: &std::path::Path,
    args: &[&str],
) -> anyhow::Result<std::process::ExitStatus> {
    let mut command = build_local_compose_command(compose_command, compose_file, app_dir, args);
    command.status().await.map_err(anyhow::Error::from)
}

async fn spawn_local_compose_command(
    compose_command: ComposeCommandKind,
    compose_file: &std::path::Path,
    app_dir: &std::path::Path,
    args: &[&str],
) -> anyhow::Result<Child> {
    let mut command = build_local_compose_command(compose_command, compose_file, app_dir, args);
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
    docker_compose_args: &str,
) -> String {
    let app_dir = shell_quote(&app_dir.display().to_string());
    let compose_command = match compose_command {
        ComposeCommandKind::DockerComposePlugin => "docker compose",
        ComposeCommandKind::DockerComposeBinary => "docker-compose",
    };
    format!(
        "cd {app_dir} && if [ -f docker-compose.yml ]; then compose_file=docker-compose.yml; \
elif [ -f docker-compose.yaml ]; then compose_file=docker-compose.yaml; \
else echo \"docker compose file not found in $(pwd)\" >&2; exit 127; fi; \
{compose_command} -f \"$compose_file\" {docker_compose_args}"
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
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
        .map(|line| line.split('\r').last().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
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
