use std::path::Path;

use anyhow::{Context, bail};
use async_trait::async_trait;
use tokio::fs;

use crate::cli::{CommandContext, CommandTrait};

const QA_TEMPLATE: &str = include_str!("../../template/qa.yaml");
const BEFORE_SCRIPT_TEMPLATE: &str = "#!/usr/bin/env bash\nset -euo pipefail\n\n";
const AFTER_SCRIPT_TEMPLATE: &str = "#!/usr/bin/env bash\nset -euo pipefail\n\n";

#[derive(clap::Args, Clone, Debug)]
/// Manage app templates.
pub struct TemplateArgs {
    #[command(subcommand)]
    pub command: TemplateSubcommand,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum TemplateSubcommand {
    /// Initialize an app template.
    Init(TemplateInitArgs),
}

#[derive(clap::Args, Clone, Debug)]
/// Initialize an app template under the home app directory.
pub struct TemplateInitArgs {
    /// App name.
    #[arg(short, long)]
    pub name: String,
}

pub struct TemplateCommand;

#[async_trait]
impl CommandTrait for TemplateCommand {
    type Args = TemplateArgs;

    async fn run(args: TemplateArgs, ctx: CommandContext) -> anyhow::Result<()> {
        match args.command {
            TemplateSubcommand::Init(args) => init_template(args, ctx).await,
        }
    }
}

async fn init_template(_args: TemplateInitArgs, _ctx: CommandContext) -> anyhow::Result<()> {
    init_app_template(&_ctx.home, &_args.name).await
}

async fn init_app_template(home: &Path, name: &str) -> anyhow::Result<()> {
    validate_template_name(name)?;

    let app_home = home.join("app");
    let app_dir = app_home.join(name);

    fs::create_dir_all(&app_home)
        .await
        .with_context(|| format!("create app home {}", app_home.display()))?;

    if fs::try_exists(&app_dir)
        .await
        .with_context(|| format!("check app dir {}", app_dir.display()))?
    {
        bail!("app template '{}' already exists", name);
    }

    fs::create_dir_all(&app_dir)
        .await
        .with_context(|| format!("create app dir {}", app_dir.display()))?;

    write_template_files(&app_dir, name).await?;

    println!("template initialized at {}", app_dir.display());
    Ok(())
}

async fn write_template_files(app_dir: &Path, name: &str) -> anyhow::Result<()> {
    let qa_content = QA_TEMPLATE.replacen("name: <name>", &format!("name: {name}"), 1);
    let qa_file = app_dir.join("qa.yaml");
    let before_file = app_dir.join("before.sh");
    let after_file = app_dir.join("after.sh");

    fs::write(&qa_file, qa_content)
        .await
        .with_context(|| format!("write app file {}", qa_file.display()))?;
    fs::write(&before_file, BEFORE_SCRIPT_TEMPLATE)
        .await
        .with_context(|| format!("write script {}", before_file.display()))?;
    fs::write(&after_file, AFTER_SCRIPT_TEMPLATE)
        .await
        .with_context(|| format!("write script {}", after_file.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(&before_file, std::fs::Permissions::from_mode(0o755))
            .await
            .with_context(|| format!("chmod script {}", before_file.display()))?;
        fs::set_permissions(&after_file, std::fs::Permissions::from_mode(0o755))
            .await
            .with_context(|| format!("chmod script {}", after_file.display()))?;
    }

    Ok(())
}

fn validate_template_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() || name.contains('/') || name.contains("..") {
        bail!("invalid template name '{}'", name);
    }
    Ok(())
}

#[cfg(test)]
#[path = "template_test.rs"]
mod template_test;
