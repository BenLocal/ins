use async_trait::async_trait;

use crate::cli::{CommandContext, CommandTrait};
use crate::pipeline::{
    PipelineArgs, PipelineContext, PipelineMode, PreparedDeployment, execute_pipeline,
    prepare_deployment,
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
        let prepared: PreparedDeployment =
            prepare_deployment(PipelineContext::new(&ctx, args.pipeline)).await?;

        execute_pipeline(
            &ctx.home,
            prepared,
            "Starting check...",
            PipelineMode::Check,
        )
        .await
    }
}

#[cfg(test)]
#[path = "check_test.rs"]
mod check_test;
