use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use async_trait::async_trait;
use tokio::fs;

use crate::app::parse::load_app_record;
use crate::cli::{CommandContext, CommandTrait};

#[derive(clap::Args, Clone, Debug)]
/// Manage applications in the cluster.
pub struct AppArgs {
    #[command(subcommand)]
    pub command: AppSubcommand,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum AppSubcommand {
    /// List apps under the home app directory.
    List(AppListArgs),
    /// Show the qa.yaml for one app.
    Inspect(AppInspectArgs),
}

#[derive(clap::Args, Clone, Debug, Default)]
/// List all application records.
pub struct AppListArgs {}

#[derive(clap::Args, Clone, Debug)]
/// Inspect one application record.
pub struct AppInspectArgs {
    /// App name.
    #[arg(short, long)]
    pub name: String,
}

pub struct AppCommand;

#[async_trait]
impl CommandTrait for AppCommand {
    type Args = AppArgs;

    async fn run(args: AppArgs, ctx: CommandContext) -> anyhow::Result<()> {
        let app_home = ctx.home.join("app");
        fs::create_dir_all(&app_home)
            .await
            .with_context(|| format!("create app home {}", app_home.display()))?;

        match args.command {
            AppSubcommand::List(_args) => list_apps(&app_home).await,
            AppSubcommand::Inspect(args) => inspect_app(&app_home, args).await,
        }
    }
}

async fn list_apps(app_home: &Path) -> anyhow::Result<()> {
    let mut entries = fs::read_dir(app_home)
        .await
        .with_context(|| format!("read app home {}", app_home.display()))?;
    let mut apps = Vec::new();

    while let Some(entry) = entries
        .next_entry()
        .await
        .with_context(|| format!("iterate app home {}", app_home.display()))?
    {
        let path = entry.path();
        if !entry
            .file_type()
            .await
            .with_context(|| format!("read file type {}", path.display()))?
            .is_dir()
        {
            continue;
        }

        let qa_file = qa_file(&path);
        if !fs::try_exists(&qa_file)
            .await
            .with_context(|| format!("check app file {}", qa_file.display()))?
        {
            continue;
        }

        let record = load_app_record(&qa_file).await?;
        apps.push(record);
    }

    apps.sort_by(|left, right| left.name.cmp(&right.name));

    if apps.is_empty() {
        println!("no apps found");
        return Ok(());
    }

    println!("{}", serde_json::to_string_pretty(&apps)?);
    Ok(())
}

async fn inspect_app(app_home: &Path, args: AppInspectArgs) -> anyhow::Result<()> {
    validate_app_name(&args.name)?;

    let qa_file = qa_file(&app_home.join(&args.name));
    let content = fs::read_to_string(&qa_file)
        .await
        .with_context(|| format!("read app file {}", qa_file.display()))?;
    print!("{content}");
    Ok(())
}

fn qa_file(app_dir: &Path) -> PathBuf {
    app_dir.join("qa.yaml")
}

fn validate_app_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() || name.contains('/') || name.contains("..") {
        bail!("invalid app name '{}'", name);
    }
    Ok(())
}
