use std::{
    env,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::fs;

use crate::OutputFormat;
use crate::cli::{
    CommandContext, CommandTrait,
    template::{TemplateArgs, TemplateCommand, TemplateInitArgs, TemplateSubcommand},
};

#[tokio::test]
async fn template_init_creates_app_template_files() -> anyhow::Result<()> {
    let home = unique_test_dir("template-init-home");

    TemplateCommand::run(
        TemplateArgs {
            command: TemplateSubcommand::Init(TemplateInitArgs {
                name: "demo".into(),
            }),
        },
        CommandContext::new(
            home.clone(),
            OutputFormat::Table,
            std::sync::Arc::new(crate::config::InsConfig::default()),
        ),
    )
    .await?;

    let app_dir = home.join("app").join("demo");
    assert!(fs::try_exists(app_dir.join("qa.yaml")).await?);
    assert!(fs::try_exists(app_dir.join("before.sh")).await?);
    assert!(fs::try_exists(app_dir.join("after.sh")).await?);

    let qa = fs::read_to_string(app_dir.join("qa.yaml")).await?;
    assert!(qa.contains("name: demo"));

    fs::remove_dir_all(&home).await?;
    Ok(())
}

#[tokio::test]
async fn template_init_rejects_existing_app_template() -> anyhow::Result<()> {
    let home = unique_test_dir("template-init-existing-home");
    let app_dir = home.join("app").join("demo");

    fs::create_dir_all(&app_dir).await?;

    let err = TemplateCommand::run(
        TemplateArgs {
            command: TemplateSubcommand::Init(TemplateInitArgs {
                name: "demo".into(),
            }),
        },
        CommandContext::new(
            home.clone(),
            OutputFormat::Table,
            std::sync::Arc::new(crate::config::InsConfig::default()),
        ),
    )
    .await
    .expect_err("existing template should fail");

    assert!(err.to_string().contains("already exists"));

    fs::remove_dir_all(&home).await?;
    Ok(())
}

fn unique_test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("ins-{name}-{}-{nanos}", std::process::id()))
}
