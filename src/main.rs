use clap::{CommandFactory, Parser, ValueEnum};
use std::{env as std_env, fs, path::PathBuf};

use crate::cli::{
    CommandContext, CommandTrait, app::AppCommand, check::CheckCommand, deploy::DeployCommand,
    node::NodeCommand, service::ServiceCommand, template::TemplateCommand, tui::TuiCommand,
    version::VersionCommand,
};

mod app;
mod cli;
mod env;
mod file;
mod node;
mod output;
mod pipeline;
mod provider;
mod store;
mod tui;
mod version;

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
        Some(Command::Deploy(args)) => {
            DeployCommand::run(
                args,
                CommandContext {
                    home,
                    output: cli.output,
                },
            )
            .await
        }
        Some(Command::Check(args)) => {
            CheckCommand::run(
                args,
                CommandContext {
                    home,
                    output: cli.output,
                },
            )
            .await
        }
        Some(Command::Node(args)) => {
            NodeCommand::run(
                args,
                CommandContext {
                    home,
                    output: cli.output,
                },
            )
            .await
        }
        Some(Command::App(args)) => {
            AppCommand::run(
                args,
                CommandContext {
                    home,
                    output: cli.output,
                },
            )
            .await
        }
        Some(Command::Service(args)) => {
            ServiceCommand::run(
                args,
                CommandContext {
                    home,
                    output: cli.output,
                },
            )
            .await
        }
        Some(Command::Template(args)) => {
            TemplateCommand::run(
                args,
                CommandContext {
                    home,
                    output: cli.output,
                },
            )
            .await
        }
        Some(Command::Tui(args)) => {
            TuiCommand::run(
                args,
                CommandContext {
                    home,
                    output: cli.output,
                },
            )
            .await
        }
        Some(Command::Version(args)) => {
            VersionCommand::run(
                args,
                CommandContext {
                    home,
                    output: cli.output,
                },
            )
            .await
        }
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
}

#[cfg(test)]
mod tests {
    use super::{Command, InsCli};
    use clap::Parser;

    #[test]
    fn tui_command_parses_from_cli() {
        let cli = InsCli::try_parse_from(["ins", "tui"]).expect("tui command should parse");
        assert!(matches!(cli.command, Some(Command::Tui(_))));
    }
}
