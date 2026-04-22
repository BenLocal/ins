use std::path::Path;

use async_trait::async_trait;

use crate::cli::{CommandContext, CommandTrait};
use crate::output::print_structured_list;
use crate::volume::list::{load_volumes, volumes_file};

#[derive(clap::Args, Clone, Debug)]
/// Manage per-node Docker volume backings.
pub struct VolumeArgs {
    #[command(subcommand)]
    pub command: VolumeSubcommand,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum VolumeSubcommand {
    /// List configured volumes.
    List(VolumeListArgs),
}

#[derive(clap::Args, Clone, Debug, Default)]
pub struct VolumeListArgs {}

pub struct VolumeCommand;

#[async_trait]
impl CommandTrait for VolumeCommand {
    type Args = VolumeArgs;

    async fn run(args: VolumeArgs, ctx: CommandContext) -> anyhow::Result<()> {
        match args.command {
            VolumeSubcommand::List(_) => list_volumes(&volumes_file(&ctx.home), ctx.output).await,
        }
    }
}

async fn list_volumes(path: &Path, output: crate::OutputFormat) -> anyhow::Result<()> {
    let volumes = load_volumes(path).await?;
    print_structured_list(&volumes, output, "no volumes found")
}
