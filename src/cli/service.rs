use anyhow::Context;
use async_trait::async_trait;

use crate::cli::{CommandContext, CommandTrait};
use crate::output::print_structured_list;
use crate::store::duck::list_installed_services;

#[derive(clap::Args, Clone, Debug)]
/// Manage installed services.
pub struct ServiceArgs {
    #[command(subcommand)]
    pub command: ServiceSubcommand,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum ServiceSubcommand {
    /// List installed services from deployment history.
    List(ServiceListArgs),
}

#[derive(clap::Args, Clone, Debug, Default)]
/// List installed services.
pub struct ServiceListArgs {}

pub struct ServiceCommand;

#[async_trait]
impl CommandTrait for ServiceCommand {
    type Args = ServiceArgs;

    async fn run(args: ServiceArgs, ctx: CommandContext) -> anyhow::Result<()> {
        match args.command {
            ServiceSubcommand::List(_args) => list_services(&ctx.home, ctx.output).await,
        }
    }
}

async fn list_services(home: &std::path::Path, output: crate::OutputFormat) -> anyhow::Result<()> {
    let services = list_installed_services(home)
        .await
        .with_context(|| format!("list installed services from {}", home.display()))?;
    print_structured_list(&services, output, "no services found")
}
