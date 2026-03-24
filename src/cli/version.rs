use async_trait::async_trait;

use crate::cli::{CommandContext, CommandTrait};
use crate::version::VersionInfo;

#[derive(clap::Args, Clone, Debug, Default)]
/// Show version details.
pub struct VersionArgs {}

pub struct VersionCommand;

#[async_trait]
impl CommandTrait for VersionCommand {
    type Args = VersionArgs;

    async fn run(_args: VersionArgs, _ctx: CommandContext) -> anyhow::Result<()> {
        print!("{}", VersionInfo::current().render());
        Ok(())
    }
}
