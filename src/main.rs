use clap::{CommandFactory, Parser, ValueEnum};
use std::{env as std_env, fs, path::PathBuf};

use crate::cli::{
    CommandContext, CommandTrait, app::AppCommand, check::CheckCommand, deploy::DeployCommand,
    docker::DockerCommand, node::NodeCommand, service::ServiceCommand, template::TemplateCommand,
    tui::TuiCommand, version::VersionCommand, volume::VolumeCommand,
};

mod app;
mod cli;
mod config;
mod env;
mod execution_output;
mod file;
mod hooks;
mod node;
mod output;
mod pipeline;
mod provider;
mod store;
mod tui;
mod version;
mod volume;

#[tokio::main]
async fn main() {
    let cli = InsCli::parse();

    let home = match ensure_home_dir(&cli.home) {
        Ok(home) => home,
        Err(e) => {
            eprintln!("{:?}", e);
            std::process::exit(1);
        }
    };

    let config = match crate::config::load_config(&home).await {
        Ok(cfg) => std::sync::Arc::new(cfg),
        Err(e) => {
            eprintln!("{:?}", e);
            std::process::exit(1);
        }
    };

    let ctx = CommandContext::new(home, cli.output, config);

    let res = match cli.command {
        Some(Command::Deploy(args)) => DeployCommand::run(args, ctx).await,
        Some(Command::Check(args)) => CheckCommand::run(args, ctx).await,
        Some(Command::Node(args)) => NodeCommand::run(args, ctx).await,
        Some(Command::App(args)) => AppCommand::run(args, ctx).await,
        Some(Command::Service(args)) => ServiceCommand::run(args, ctx).await,
        Some(Command::Template(args)) => TemplateCommand::run(args, ctx).await,
        Some(Command::Tui(args)) => TuiCommand::run(args, ctx).await,
        Some(Command::Version(args)) => VersionCommand::run(args, ctx).await,
        Some(Command::Volume(args)) => VolumeCommand::run(args, ctx).await,
        Some(Command::Docker(args)) => DockerCommand::run(args, ctx).await,
        None => {
            InsCli::command().print_help().expect("print help");
            println!();
            Ok(())
        }
    };

    if let Err(e) = res {
        eprintln!("{:?}", e);
        std::process::exit(1);
    }
}

fn ensure_home_dir(path: &PathBuf) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(path)?;
    Ok(path.clone())
}

fn default_home_dir() -> PathBuf {
    // Prefer local project directory if `.ins/` exists.
    if std::path::Path::new(".ins").is_dir() {
        return std_env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".ins");
    }

    std_env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ins")
}

#[derive(Parser)]
#[command(name = "ins")]
#[command(bin_name = "ins")]
/// Docker deployment helper.
struct InsCli {
    /// Home directory for ins data.
    #[arg(long, global = true, default_value_os_t = default_home_dir())]
    home: PathBuf,

    /// Output format for structured command results.
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Table)]
    output: OutputFormat,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Json,
    #[default]
    Table,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Deploy a container image.
    Deploy(cli::deploy::DeployArgs),
    /// Prepare and validate deployment inputs without running the provider.
    Check(cli::check::CheckArgs),
    /// Manage nodes in the cluster.
    Node(cli::node::NodeArgs),
    /// Manage applications in the cluster.
    App(cli::app::AppArgs),
    /// List installed services.
    Service(cli::service::ServiceArgs),
    /// Manage app templates.
    Template(cli::template::TemplateArgs),
    /// Launch the interactive terminal UI.
    Tui(cli::tui::TuiArgs),
    /// Show version, tag, and git commit information.
    Version(cli::version::VersionArgs),
    /// Manage per-node Docker volume backings.
    Volume(cli::volume::VolumeArgs),
    /// Run a docker command on the selected node.
    Docker(cli::docker::DockerArgs),
}

#[cfg(test)]
#[path = "main_test.rs"]
mod main_test;
