use std::{
    env,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;
use tokio::fs;

use crate::app::parse::load_app_record;

const QA_TEMPLATE: &str = include_str!("../../template/qa.yaml");

#[tokio::test]
async fn load_app_record_parses_template_yaml() -> anyhow::Result<()> {
    let test_dir = unique_test_dir("parse-template");
    let qa_file = test_dir.join("qa.yaml");

    fs::create_dir_all(&test_dir).await?;
    fs::write(&qa_file, QA_TEMPLATE).await?;

    let record = load_app_record(&qa_file).await?;

    assert_eq!(record.name, "<name>");
    assert_eq!(record.description.as_deref(), Some("<description>"));
    assert_eq!(record.before.shell.as_deref(), Some("bash"));
    assert_eq!(record.before.script.as_deref(), Some("./before.sh"));
    assert_eq!(record.after.shell.as_deref(), Some("bash"));
    assert_eq!(record.after.script.as_deref(), Some("./after.sh"));
    assert_eq!(record.values.len(), 1);
    assert_eq!(record.values[0].name, "<name>");
    assert_eq!(record.values[0].value_type, "string");
    assert_eq!(record.values[0].description.as_deref(), Some("<description>"));
    assert_eq!(record.values[0].options.len(), 1);
    assert_eq!(record.values[0].options[0].name, "<name>");
    assert_eq!(
        record.values[0].options[0].description.as_deref(),
        Some("<description>")
    );
    assert_eq!(record.values[0].options[0].value, Some(json!("<value>")));

    fs::remove_dir_all(&test_dir).await?;
    Ok(())
}

#[tokio::test]
async fn load_app_record_parses_value_and_default_fields() -> anyhow::Result<()> {
    let test_dir = unique_test_dir("parse-values");
    let qa_file = test_dir.join("qa.yaml");
    let qa = r#"
name: demo
values:
  - name: replicas
    type: number
    default: 3
  - name: feature_enabled
    type: boolean
    value: true
"#;

    fs::create_dir_all(&test_dir).await?;
    fs::write(&qa_file, qa.trim_start()).await?;

    let record = load_app_record(&qa_file).await?;

    assert_eq!(record.values.len(), 2);
    assert_eq!(record.values[0].default, Some(json!(3)));
    assert_eq!(record.values[1].value, Some(json!(true)));

    fs::remove_dir_all(&test_dir).await?;
    Ok(())
}

#[tokio::test]
async fn load_app_record_collects_sibling_files_and_directories() -> anyhow::Result<()> {
    let test_dir = unique_test_dir("parse-files");
    let qa_file = test_dir.join("qa.yaml");
    let child_dir = test_dir.join("scripts");
    let child_file = test_dir.join("README.md");

    fs::create_dir_all(&child_dir).await?;
    fs::write(&qa_file, QA_TEMPLATE).await?;
    fs::write(&child_file, "demo").await?;

    let record = load_app_record(&qa_file).await?;
    let files = record.files.expect("files should be populated");

    assert_eq!(files.len(), 3);
    assert_eq!(files[0].name, "README.md");
    assert_eq!(files[0].path, child_file.display().to_string());
    assert!(!files[0].is_dir);
    assert_eq!(files[1].name, "qa.yaml");
    assert_eq!(files[1].path, qa_file.display().to_string());
    assert!(!files[1].is_dir);
    assert_eq!(files[2].name, "scripts");
    assert_eq!(files[2].path, child_dir.display().to_string());
    assert!(files[2].is_dir);

    fs::remove_dir_all(&test_dir).await?;
    Ok(())
}

fn unique_test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("docker-ins-{name}-{}-{nanos}", std::process::id()))
}
