use anyhow::{Context, anyhow};
use async_trait::async_trait;
use tokio::process::Child;
use tokio::process::Command;
use tokio::signal;
use tokio::time::{Duration, sleep};

use crate::node::types::NodeRecord;
use crate::provider::{ProviderContext, ProviderTrait};

pub struct DockerComposeProvider;

#[async_trait]
impl ProviderTrait for DockerComposeProvider {
    async fn validate(&self, ctx: ProviderContext) -> anyhow::Result<()> {
        println!("Provider '{}': validating deployment", ctx.provider);

        match &ctx.node {
            NodeRecord::Local() => {
                for target in &ctx.targets {
                    let app_dir = ctx.workspace.join(&target.service);
                    let compose_file = compose_file_for_target(&app_dir, &target.app.name)?;

                    println!(
                        "Validating app '{}' as service '{}' from {}",
                        target.app.name,
                        target.service,
                        app_dir.display()
                    );

                    let status = Command::new("docker")
                        .arg("compose")
                        .arg("-f")
                        .arg(&compose_file)
                        .arg("config")
                        .arg("-q")
                        .current_dir(&app_dir)
                        .status()
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
            NodeRecord::Remote(remote) => Err(anyhow!(
                "remote nodes are not yet supported for docker-compose provider (node '{}')",
                remote.name
            )),
        }
    }

    async fn run(&self, ctx: ProviderContext) -> anyhow::Result<()> {
        println!("Provider '{}': starting deployment", ctx.provider);

        match &ctx.node {
            NodeRecord::Local() => {
                for target in &ctx.targets {
                    let app_dir = ctx.workspace.join(&target.service);
                    let compose_file = compose_file_for_target(&app_dir, &target.app.name)?;

                    println!(
                        "Deploying app '{}' as service '{}' from {}",
                        target.app.name,
                        target.service,
                        app_dir.display()
                    );

                    let mut child = Command::new("docker")
                        .arg("compose")
                        .arg("-f")
                        .arg(&compose_file)
                        .arg("up")
                        .arg("-d")
                        .current_dir(&app_dir)
                        .kill_on_drop(true)
                        .process_group(0)
                        .spawn()
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
            NodeRecord::Remote(remote) => Err(anyhow!(
                "remote nodes are not yet supported for docker-compose provider (node '{}')",
                remote.name
            )),
        }
    }
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
