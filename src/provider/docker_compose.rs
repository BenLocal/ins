use anyhow::{Context, anyhow};
use async_trait::async_trait;
use tokio::process::Command;

use crate::node::types::NodeRecord;
use crate::provider::{ProviderContext, ProviderTrait};

pub struct DockerComposeProvider;

#[async_trait]
impl ProviderTrait for DockerComposeProvider {
    async fn run(&self, ctx: ProviderContext) -> anyhow::Result<()> {
        println!("Provider '{}': starting deployment", ctx.provider);

        match &ctx.node {
            NodeRecord::Local() => {
                for target in &ctx.targets {
                    let app_dir = ctx.workspace.join(&target.service);
                    let compose_yml = app_dir.join("docker-compose.yml");
                    let compose_yaml = app_dir.join("docker-compose.yaml");

                    let compose_file = if compose_yml.exists() {
                        compose_yml
                    } else if compose_yaml.exists() {
                        compose_yaml
                    } else {
                        return Err(anyhow!(
                            "docker compose file not found for app '{}' in {}",
                            target.app.name,
                            app_dir.display()
                        ));
                    };

                    println!(
                        "Deploying app '{}' as service '{}' from {}",
                        target.app.name,
                        target.service,
                        app_dir.display()
                    );

                    let status = Command::new("docker")
                        .arg("compose")
                        .arg("-f")
                        .arg(&compose_file)
                        .arg("up")
                        .arg("-d")
                        .current_dir(&app_dir)
                        .status()
                        .await
                        .with_context(|| {
                            format!(
                                "failed to run docker compose for app '{}' service '{}' (file {})",
                                target.app.name,
                                target.service,
                                compose_file.display()
                            )
                        })?;

                    if !status.success() {
                        return Err(anyhow!(
                            "docker compose up failed for app '{}' service '{}' (exit code {:?})",
                            target.app.name,
                            target.service,
                            status.code()
                        ));
                    }
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
