use async_trait::async_trait;

use crate::cli::{CommandContext, CommandTrait};
use crate::pipeline::{
    PipelineArgs, PipelineMode, PreparedDeployment, execute_pipeline, prepare_deployment,
};

#[derive(clap::Args, Clone, Debug)]
/// Prepare app files in the workspace without running the provider.
pub struct CheckArgs {
    #[command(flatten)]
    pub pipeline: PipelineArgs,
}

pub struct CheckCommand;

#[async_trait]
impl CommandTrait for CheckCommand {
    type Args = CheckArgs;

    async fn run(args: CheckArgs, ctx: CommandContext) -> anyhow::Result<()> {
        let args = args.pipeline;
        let prepared: PreparedDeployment = prepare_deployment(
            &ctx.home,
            args.provider.clone(),
            args.workspace,
            args.node,
            args.apps,
        )
        .await?;

        execute_pipeline(&ctx.home, prepared, "Starting check...", PipelineMode::Check).await
    }
}
