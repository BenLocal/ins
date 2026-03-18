use crate::cli::{CommandContext, CommandTrait};
use async_trait::async_trait;

#[derive(clap::Args, Clone, Debug)]
/// Deploy a container with the given runtime settings.
pub struct DeployArgs {}

pub struct DeployCommand;

#[async_trait]
impl CommandTrait for DeployCommand {
    type Args = DeployArgs;

    async fn run(_args: DeployArgs, _ctx: CommandContext) -> anyhow::Result<()> {
        println!("deploy");
        Ok(())
    }
}
