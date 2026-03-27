use async_trait::async_trait;

use crate::cli::{CommandContext, CommandTrait};
use crate::pipeline::{
    PipelineArgs, PipelineMode, PreparedDeployment, execute_pipeline, prepare_deployment,
};

#[cfg(test)]
use crate::pipeline::{
    app_choice_label, apply_cli_values, apply_stored_values, build_compose_metadata_labels,
    build_deployment_target, build_template_values, copy_apps_to_workspace, is_template_file,
    parse_cli_value_overrides, parse_number_value, rendered_template_name, resolve_apps,
    select_node,
};

#[derive(clap::Args, Clone, Debug)]
/// Deploy a container with the given runtime settings.
pub struct DeployArgs {
    #[command(flatten)]
    pub pipeline: PipelineArgs,
}

pub struct DeployCommand;

#[async_trait]
impl CommandTrait for DeployCommand {
    type Args = DeployArgs;

    async fn run(args: DeployArgs, ctx: CommandContext) -> anyhow::Result<()> {
        let args = args.pipeline;
        let prepared: PreparedDeployment = prepare_deployment(
            &ctx.home,
            args.provider.clone(),
            args.workspace,
            args.node,
            args.values,
            args.apps,
        )
        .await?;

        execute_pipeline(
            &ctx.home,
            prepared,
            "Starting deployment...",
            PipelineMode::Deploy,
        )
        .await
    }
}

#[cfg(test)]
#[path = "deploy_test.rs"]
mod deploy_test;
