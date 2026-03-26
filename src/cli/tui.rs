use async_trait::async_trait;

use crate::cli::{CommandContext, CommandTrait};

#[derive(clap::Args, Clone, Debug, Default)]
/// Launch the interactive terminal UI.
pub struct TuiArgs {}

pub struct TuiCommand;

#[async_trait]
impl CommandTrait for TuiCommand {
    type Args = TuiArgs;

    async fn run(_args: TuiArgs, ctx: CommandContext) -> anyhow::Result<()> {
        crate::tui::run(ctx.home).await
    }
}
