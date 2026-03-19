use async_trait::async_trait;

use crate::cli::deploy::{
    PreparedDeployment, copy_prepared_apps_to_workspace, prepare_deployment,
    print_prepared_deployment,
};
use crate::cli::{CommandContext, CommandTrait};
use crate::provider::docker_compose::DockerComposeProvider;
use crate::provider::{ProviderContext, ProviderTrait as _};

#[derive(clap::Args, Clone, Debug)]
/// Prepare app files in the workspace without running the provider.
pub struct CheckArgs {
    /// Workspace directory for copied app files.
    #[arg(short, long)]
    pub workspace: std::path::PathBuf,
    /// Target node name.
    #[arg(short, long)]
    pub node: Option<String>,
    /// Application names to check.
    pub apps: Option<Vec<String>>,
}

pub struct CheckCommand;

#[async_trait]
impl CommandTrait for CheckCommand {
    type Args = CheckArgs;

    async fn run(args: CheckArgs, ctx: CommandContext) -> anyhow::Result<()> {
        let prepared: PreparedDeployment = prepare_deployment(
            &ctx.home,
            None,
            args.workspace,
            args.node,
            args.apps,
        )
        .await?;

        print_prepared_deployment("Starting check...", &prepared);
        copy_prepared_apps_to_workspace(&ctx.home, &prepared).await?;
        println!("Validating with provider...");
        DockerComposeProvider
            .validate(ProviderContext::new(
                None,
                prepared.node,
                prepared.targets,
                prepared.workspace,
            ))
            .await?;
        println!("Check completed.");
        Ok(())
    }
}
