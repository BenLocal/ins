use async_trait::async_trait;
use anyhow::{anyhow, Context};
use tokio::process::Command;

use crate::node::types::NodeRecord;
use crate::provider::{ProviderContext, ProviderTrait};

pub struct DockerComposeProvider;

#[async_trait]
impl ProviderTrait for DockerComposeProvider {
    async fn run(&self, ctx: ProviderContext) -> anyhow::Result<()> {
        match &ctx.node {
            NodeRecord::Local() => {
                for app in &ctx.apps {
                    let app_dir = ctx.workspace.join(&app.name);
                    let compose_yml = app_dir.join("docker-compose.yml");
                    let compose_yaml = app_dir.join("docker-compose.yaml");

                    let compose_file = if compose_yml.exists() {
                        compose_yml
                    } else if compose_yaml.exists() {
                        compose_yaml
                    } else {
                        return Err(anyhow!(
                            "docker compose file not found for app '{}' in {}",
                            app.name,
                            app_dir.display()
                        ));
                    };

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
                                "failed to run docker compose for app '{}' (file {})",
                                app.name,
                                compose_file.display()
                            )
                        })?;

                    if !status.success() {
                        return Err(anyhow!(
                            "docker compose up failed for app '{}' (exit code {:?})",
                            app.name,
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
