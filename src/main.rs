use clap::{CommandFactory, Parser};
use std::{env, fs, path::PathBuf};

use crate::cli::{
    CommandContext, CommandTrait, app::AppCommand, check::CheckCommand, deploy::DeployCommand,
    node::NodeCommand,
};

mod app;
mod cli;
mod file;
mod node;
mod provider;
mod store;

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

    let res = match cli.command {
        Some(Command::Deploy(args)) => DeployCommand::run(args, CommandContext { home }).await,
        Some(Command::Check(args)) => CheckCommand::run(args, CommandContext { home }).await,
        Some(Command::Node(args)) => NodeCommand::run(args, CommandContext { home }).await,
        Some(Command::App(args)) => AppCommand::run(args, CommandContext { home }).await,
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
        return env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".ins");
    }

    env::var_os("HOME")
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

    #[command(subcommand)]
    command: Option<Command>,
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
}
