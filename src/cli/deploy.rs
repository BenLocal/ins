use anyhow::anyhow;
use async_trait::async_trait;
use inquire::{Confirm, CustomType, MultiSelect, Select, Text};
use minijinja::{Environment, UndefinedBehavior, context};
use serde_json::{Map, Value};
use tokio::fs;

use crate::app::parse::load_app_record;
use crate::app::types::{AppRecord, AppValue};
use crate::cli::{CommandContext, CommandTrait, node::nodes_file};
use crate::node::list::load_all_nodes;
use crate::node::types::NodeRecord;
use crate::provider::docker_compose::DockerComposeProvider;
use crate::provider::{ProviderContext, ProviderTrait as _};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

#[derive(clap::Args, Clone, Debug)]
/// Deploy a container with the given runtime settings.
pub struct DeployArgs {
    /// Provider name.
    #[arg(short, long, default_value = "docker-compose")]
    pub provider: String,
    /// Workspace directory for copied app files.
    #[arg(short, long)]
    pub workspace: PathBuf,
    /// Target node name.
    #[arg(short, long)]
    pub node: Option<String>,
    /// Application names to deploy.
    pub apps: Option<Vec<String>>,
}

pub struct DeployCommand;

#[async_trait]
impl CommandTrait for DeployCommand {
    type Args = DeployArgs;

    async fn run(args: DeployArgs, ctx: CommandContext) -> anyhow::Result<()> {
        let provider = match args.provider.as_str() {
            "docker-compose" => DockerComposeProvider,
            _ => return Err(anyhow!("unsupported provider: {}", &args.provider)),
        };

        let nodes = load_all_nodes(&nodes_file(&ctx.home)).await?;
        let node = select_node(&nodes, args.node.as_deref())?;
        let app_home = ctx.home.join("app");
        let apps = resolve_apps(args.apps, &app_home).await?;

        println!("Starting deployment...");
        println!("Provider Name: {}", &args.provider);
        println!("Node Name: {}", node_name(&node));
        println!("Apps: {}", &apps.join(", "));
        println!("Workspace: {}", args.workspace.display());

        println!("Copying apps to workspace...");
        copy_apps_to_workspace(&apps, &app_home, &args.workspace).await?;

        println!("Running provider...");
        provider
            .run(ProviderContext {
                provider: args.provider,
                node,
                apps,
            })
            .await?;

        Ok(())
    }
}

fn select_node(nodes: &[NodeRecord], requested: Option<&str>) -> anyhow::Result<NodeRecord> {
    if nodes.is_empty() {
        return Err(anyhow!("no nodes found, please add a node first"));
    }

    if let Some(name) = requested {
        if let Some(node) = nodes.iter().find(|node| node_name(node) == name) {
            return Ok(node.clone());
        }
    }

    let options: Vec<String> = nodes.iter().map(node_label).collect();

    let answer = Select::new("Select a node", options).prompt()?;
    let selected_name = answer
        .split_once(" (")
        .map(|(name, _)| name)
        .unwrap_or(answer.as_str());

    nodes
        .iter()
        .find(|node| node_name(node) == selected_name)
        .cloned()
        .ok_or_else(|| anyhow!("selected node '{}' not found", selected_name))
}

async fn resolve_apps(
    requested: Option<Vec<String>>,
    app_home: &Path,
) -> anyhow::Result<Vec<String>> {
    if let Some(apps) = requested.filter(|apps| !apps.is_empty()) {
        return Ok(apps);
    }

    let available_apps = load_available_apps(app_home).await?;
    if available_apps.is_empty() {
        return Err(anyhow!("no apps found, please add an app first"));
    }

    let selected = MultiSelect::new("Select apps to deploy", available_apps).prompt()?;
    if selected.is_empty() {
        return Err(anyhow!("no apps selected"));
    }

    Ok(selected)
}

async fn copy_apps_to_workspace(
    apps: &[String],
    app_home: &Path,
    workspace: &Path,
) -> anyhow::Result<()> {
    fs::create_dir_all(workspace)
        .await
        .map_err(|e| anyhow!("create workspace {}: {}", workspace.display(), e))?;

    for app in apps {
        let source_dir = app_home.join(app);
        let qa_file = app_qa_file(&source_dir);
        let app_record = load_app_record(&qa_file).await?;
        let target_dir = workspace.join(app);
        copy_dir_recursive(&source_dir, &target_dir, &app_record).await?;
    }

    Ok(())
}

async fn load_available_apps(app_home: &Path) -> anyhow::Result<Vec<String>> {
    fs::create_dir_all(app_home)
        .await
        .map_err(|e| anyhow!("create app home {}: {}", app_home.display(), e))?;

    let mut entries = fs::read_dir(app_home)
        .await
        .map_err(|e| anyhow!("read app home {}: {}", app_home.display(), e))?;
    let mut apps = Vec::new();

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| anyhow!("iterate app home {}: {}", app_home.display(), e))?
    {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .await
            .map_err(|e| anyhow!("read file type {}: {}", path.display(), e))?;
        if !file_type.is_dir() {
            continue;
        }

        let qa_file = app_qa_file(&path);
        if !fs::try_exists(&qa_file)
            .await
            .map_err(|e| anyhow!("check app file {}: {}", qa_file.display(), e))?
        {
            continue;
        }

        let app = load_app_record(&qa_file).await?;
        apps.push(app.name);
    }

    apps.sort();
    Ok(apps)
}

fn app_qa_file(app_dir: &Path) -> PathBuf {
    app_dir.join("qa.yaml")
}

async fn copy_dir_recursive(
    source: &Path,
    target: &Path,
    app_record: &AppRecord,
) -> anyhow::Result<()> {
    let mut stack = vec![(source.to_path_buf(), target.to_path_buf())];
    let template_values = build_template_values(app_record)?;

    while let Some((current_source, current_target)) = stack.pop() {
        fs::create_dir_all(&current_target)
            .await
            .map_err(|e| anyhow!("create target dir {}: {}", current_target.display(), e))?;

        let mut entries = fs::read_dir(&current_source)
            .await
            .map_err(|e| anyhow!("read source dir {}: {}", current_source.display(), e))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| anyhow!("iterate source dir {}: {}", current_source.display(), e))?
        {
            let file_name = entry.file_name();
            let source_path = entry.path();
            let file_type = entry
                .file_type()
                .await
                .map_err(|e| anyhow!("read file type {}: {}", source_path.display(), e))?;

            if file_type.is_dir() {
                let target_path = current_target.join(&file_name);
                stack.push((source_path, target_path));
                continue;
            }

            copy_file_to_workspace(
                &source_path,
                &current_target,
                &file_name.to_string_lossy(),
                &template_values,
            )
            .await?;
        }
    }

    Ok(())
}

fn build_template_values(app_record: &AppRecord) -> anyhow::Result<Value> {
    let resolved_values = resolve_app_values(app_record)?;
    let app_value = serde_json::to_value(app_record)
        .map_err(|e| anyhow!("serialize app record for template: {}", e))?;
    let mut vars = Map::new();

    if let Some(values) = app_value.get("values").and_then(|value| value.as_array()) {
        for (index, value) in values.iter().enumerate() {
            let Some(name) = value.get("name").and_then(|value| value.as_str()) else {
                continue;
            };
            let resolved_value = resolved_values
                .get(index)
                .cloned()
                .unwrap_or(Value::Null);
            let mut enriched_value = value.clone();
            if let Some(obj) = enriched_value.as_object_mut() {
                obj.insert("resolved".to_string(), resolved_value.clone());
            }
            vars.insert(name.to_string(), resolved_value);
            vars.insert(format!("{name}_meta"), enriched_value);
        }
    }

    Ok(serde_json::json!({
        "app": app_value,
        "vars": vars,
    }))
}

fn resolve_app_values(app_record: &AppRecord) -> anyhow::Result<Vec<Value>> {
    app_record
        .values
        .iter()
        .map(resolve_app_value)
        .collect()
}

fn resolve_app_value(value: &AppValue) -> anyhow::Result<Value> {
    if let Some(current) = value.value.clone() {
        return Ok(current);
    }

    if let Some(default) = value.default.clone() {
        return Ok(default);
    }

    if !value.options.is_empty() {
        if value.options.len() == 1 || !std::io::stdin().is_terminal() {
            return Ok(value.options[0].value.clone().unwrap_or(Value::Null));
        }

        let labels: Vec<String> = value
            .options
            .iter()
            .map(|option| match &option.description {
                Some(description) => format!("{} - {}", option.name, description),
                None => option.name.clone(),
            })
            .collect();
        let answer = Select::new(&value_prompt(value), labels.clone()).prompt()?;
        let index = labels
            .iter()
            .position(|label| label == &answer)
            .ok_or_else(|| anyhow!("selected value option not found for '{}'", value.name))?;
        return Ok(value.options[index].value.clone().unwrap_or(Value::Null));
    }

    if !std::io::stdin().is_terminal() {
        return Ok(Value::Null);
    }

    prompt_value_by_type(value)
}

fn prompt_value_by_type(value: &AppValue) -> anyhow::Result<Value> {
    match value.value_type.as_str() {
        "boolean" => Ok(Value::Bool(
            Confirm::new(&value_prompt(value)).with_default(false).prompt()?,
        )),
        "number" => Ok(serde_json::Number::from_f64(
            CustomType::<f64>::new(&value_prompt(value)).prompt()?,
        )
        .map(Value::Number)
        .ok_or_else(|| anyhow!("invalid number for '{}'", value.name))?),
        "json" => {
            let raw = Text::new(&format!("{} (JSON)", value_prompt(value))).prompt()?;
            serde_json::from_str(&raw).map_err(|e| anyhow!("invalid json for '{}': {}", value.name, e))
        }
        _ => Ok(Value::String(Text::new(&value_prompt(value)).prompt()?)),
    }
}

fn value_prompt(value: &AppValue) -> String {
    match &value.description {
        Some(description) => format!("{} ({})", value.name, description),
        None => format!("Enter value for {}", value.name),
    }
}

async fn copy_file_to_workspace(
    source_path: &Path,
    target_dir: &Path,
    file_name: &str,
    template_values: &serde_json::Value,
) -> anyhow::Result<()> {
    if is_template_file(file_name) {
        let rendered_name = rendered_template_name(file_name);
        let target_path = target_dir.join(rendered_name);
        let source = fs::read_to_string(source_path)
            .await
            .map_err(|e| anyhow!("read template {}: {}", source_path.display(), e))?;
        let rendered = render_template(&source, template_values)?;
        fs::write(&target_path, rendered)
            .await
            .map_err(|e| anyhow!("write rendered file {}: {}", target_path.display(), e))?;
        return Ok(());
    }

    let target_path = target_dir.join(file_name);
    fs::copy(source_path, &target_path).await.map_err(|e| {
        anyhow!(
            "copy {} to {}: {}",
            source_path.display(),
            target_path.display(),
            e
        )
    })?;
    Ok(())
}

fn render_template(source: &str, template_values: &serde_json::Value) -> anyhow::Result<String> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Lenient);
    env.add_template("app", source)
        .map_err(|e| anyhow!("add template: {}", e))?;
    let template = env
        .get_template("app")
        .map_err(|e| anyhow!("get template: {}", e))?;
    template
        .render(context! {
            app => template_values.get("app").cloned().unwrap_or(serde_json::Value::Null),
            vars => template_values.get("vars").cloned().unwrap_or(serde_json::Value::Null),
        })
        .map_err(|e| anyhow!("render template: {}", e))
}

fn is_template_file(file_name: &str) -> bool {
    file_name.ends_with(".j2")
        || file_name.ends_with(".jinja")
        || file_name.ends_with(".jinja2")
        || file_name.ends_with(".tmpl")
}

fn rendered_template_name(file_name: &str) -> &str {
    file_name
        .strip_suffix(".jinja2")
        .or_else(|| file_name.strip_suffix(".jinja"))
        .or_else(|| file_name.strip_suffix(".tmpl"))
        .or_else(|| file_name.strip_suffix(".j2"))
        .unwrap_or(file_name)
}

fn node_name(node: &NodeRecord) -> &str {
    match node {
        NodeRecord::Local() => "local",
        NodeRecord::Remote(node) => &node.name,
    }
}

fn node_label(node: &NodeRecord) -> String {
    match node {
        NodeRecord::Local() => "local (127.0.0.1)".to_string(),
        NodeRecord::Remote(node) => format!("{} ({})", node.name, node.ip),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_template_values, copy_apps_to_workspace, is_template_file, load_available_apps,
        rendered_template_name, resolve_apps, select_node,
    };
    use crate::app::types::{AppRecord, AppValue, AppValueOption, ScriptHook};
    use crate::node::types::{NodeRecord, RemoteNodeRecord};
    use serde_json::json;
    use std::{
        env,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };
    use tokio::fs;

    const QA_TEMPLATE: &str = include_str!("../../template/qa.yaml");

    #[test]
    fn select_node_returns_requested_node_when_it_exists() {
        let nodes = vec![
            NodeRecord::Remote(RemoteNodeRecord {
                name: "node-a".into(),
                ip: "10.0.0.1".into(),
                port: 22,
                user: "root".into(),
                password: "secret".into(),
            }),
            NodeRecord::Remote(RemoteNodeRecord {
                name: "node-b".into(),
                ip: "10.0.0.2".into(),
                port: 22,
                user: "root".into(),
                password: "secret".into(),
            }),
        ];

        let selected = select_node(&nodes, Some("node-b")).expect("node should exist");
        match selected {
            NodeRecord::Remote(node) => assert_eq!(node.name, "node-b"),
            NodeRecord::Local() => panic!("expected remote node"),
        }
    }

    #[test]
    fn select_node_returns_error_when_no_nodes_exist() {
        let err = select_node(&[], None).expect_err("empty nodes should fail");
        assert!(err.to_string().contains("no nodes found"));
    }

    #[tokio::test]
    async fn resolve_apps_returns_requested_apps_when_present() {
        let apps = resolve_apps(
            Some(vec!["app-a".into(), "app-b".into()]),
            PathBuf::from("/tmp/unused").as_path(),
        )
        .await
        .expect("apps should pass through");
        assert_eq!(apps, vec!["app-a", "app-b"]);
    }

    #[tokio::test]
    async fn resolve_apps_returns_error_when_no_apps_exist() -> anyhow::Result<()> {
        let app_home = unique_test_dir("deploy-apps-empty");
        fs::create_dir_all(&app_home).await?;

        let err = resolve_apps(None, &app_home)
            .await
            .expect_err("missing apps should fail");
        assert!(err.to_string().contains("no apps found"));

        fs::remove_dir_all(&app_home).await?;
        Ok(())
    }

    #[tokio::test]
    async fn load_available_apps_reads_apps_from_qa_files() -> anyhow::Result<()> {
        let app_home = unique_test_dir("deploy-apps-list");
        let alpha_dir = app_home.join("alpha");
        let beta_dir = app_home.join("beta");
        fs::create_dir_all(&alpha_dir).await?;
        fs::create_dir_all(&beta_dir).await?;
        fs::write(
            alpha_dir.join("qa.yaml"),
            QA_TEMPLATE.replace("<name>", "alpha"),
        )
        .await?;
        fs::write(
            beta_dir.join("qa.yaml"),
            QA_TEMPLATE.replace("<name>", "beta"),
        )
        .await?;

        let apps = load_available_apps(&app_home).await?;
        assert_eq!(apps, vec!["alpha".to_string(), "beta".to_string()]);

        fs::remove_dir_all(&app_home).await?;
        Ok(())
    }

    #[tokio::test]
    async fn copy_apps_to_workspace_copies_app_files() -> anyhow::Result<()> {
        let app_home = unique_test_dir("deploy-copy-app-home");
        let workspace = unique_test_dir("deploy-copy-workspace");
        let alpha_dir = app_home.join("alpha");
        let scripts_dir = alpha_dir.join("scripts");

        fs::create_dir_all(&scripts_dir).await?;
        fs::write(
            alpha_dir.join("qa.yaml"),
            QA_TEMPLATE.replace("<name>", "alpha"),
        )
        .await?;
        fs::write(alpha_dir.join("README.md"), "hello").await?;
        fs::write(scripts_dir.join("run.sh"), "#!/bin/bash").await?;

        copy_apps_to_workspace(&["alpha".into()], &app_home, &workspace).await?;

        assert!(fs::try_exists(workspace.join("alpha").join("qa.yaml")).await?);
        assert!(fs::try_exists(workspace.join("alpha").join("README.md")).await?);
        assert!(fs::try_exists(workspace.join("alpha").join("scripts").join("run.sh")).await?);

        fs::remove_dir_all(&app_home).await?;
        fs::remove_dir_all(&workspace).await?;
        Ok(())
    }

    #[tokio::test]
    async fn copy_apps_to_workspace_renders_template_files() -> anyhow::Result<()> {
        let app_home = unique_test_dir("deploy-render-app-home");
        let workspace = unique_test_dir("deploy-render-workspace");
        let alpha_dir = app_home.join("alpha");
        let qa = r#"
name: alpha
description: demo
before:
  shell: bash
  script: ./before.sh
after:
  shell: bash
  script: ./after.sh
values:
  - name: image
    type: string
    description: image name
    options:
      - name: nginx
        description: nginx image
        value: nginx:latest
"#;

        fs::create_dir_all(&alpha_dir).await?;
        fs::write(alpha_dir.join("qa.yaml"), qa.trim_start()).await?;
        fs::write(
            alpha_dir.join("docker-compose.yml.j2"),
            "name={{ app.name }}\nimage={{ vars.image }}\n",
        )
        .await?;

        copy_apps_to_workspace(&["alpha".into()], &app_home, &workspace).await?;

        let rendered = fs::read_to_string(workspace.join("alpha").join("docker-compose.yml")).await?;
        assert_eq!(rendered, "name=alpha\nimage=nginx:latest");

        fs::remove_dir_all(&app_home).await?;
        fs::remove_dir_all(&workspace).await?;
        Ok(())
    }

    #[tokio::test]
    async fn copy_apps_to_workspace_allows_missing_template_values() -> anyhow::Result<()> {
        let app_home = unique_test_dir("deploy-render-missing-home");
        let workspace = unique_test_dir("deploy-render-missing-workspace");
        let alpha_dir = app_home.join("alpha");
        let qa = r#"
name: alpha
description: demo
before:
  shell: bash
  script: ./before.sh
after:
  shell: bash
  script: ./after.sh
values: []
"#;

        fs::create_dir_all(&alpha_dir).await?;
        fs::write(alpha_dir.join("qa.yaml"), qa.trim_start()).await?;
        fs::write(
            alpha_dir.join("app.conf.j2"),
            "name={{ app.name }}\nmissing={{ vars.not_found }}\n",
        )
        .await?;

        copy_apps_to_workspace(&["alpha".into()], &app_home, &workspace).await?;

        let rendered = fs::read_to_string(workspace.join("alpha").join("app.conf")).await?;
        assert_eq!(rendered, "name=alpha\nmissing=");

        fs::remove_dir_all(&app_home).await?;
        fs::remove_dir_all(&workspace).await?;
        Ok(())
    }

    #[test]
    fn template_file_detection_and_output_name_work() {
        assert!(is_template_file("a.j2"));
        assert!(is_template_file("a.jinja"));
        assert!(is_template_file("a.jinja2"));
        assert!(is_template_file("a.tmpl"));
        assert!(!is_template_file("a.yaml"));
        assert_eq!(rendered_template_name("a.j2"), "a");
        assert_eq!(rendered_template_name("a.jinja"), "a");
        assert_eq!(rendered_template_name("a.jinja2"), "a");
        assert_eq!(rendered_template_name("a.tmpl"), "a");
    }

    #[test]
    fn build_template_values_prefers_value_then_default_then_option() {
        let record = AppRecord {
            name: "demo".into(),
            description: None,
            before: ScriptHook::default(),
            after: ScriptHook::default(),
            files: None,
            values: vec![
                AppValue {
                    name: "from_value".into(),
                    value_type: "string".into(),
                    description: None,
                    value: Some(json!("explicit")),
                    default: Some(json!("default")),
                    options: vec![AppValueOption {
                        name: "opt".into(),
                        description: None,
                        value: Some(json!("option")),
                    }],
                },
                AppValue {
                    name: "from_default".into(),
                    value_type: "number".into(),
                    description: None,
                    value: None,
                    default: Some(json!(5)),
                    options: vec![],
                },
                AppValue {
                    name: "from_option".into(),
                    value_type: "string".into(),
                    description: None,
                    value: None,
                    default: None,
                    options: vec![AppValueOption {
                        name: "opt".into(),
                        description: None,
                        value: Some(json!("picked")),
                    }],
                },
            ],
        };

        let template_values = build_template_values(&record).expect("template values");
        assert_eq!(template_values["vars"]["from_value"], json!("explicit"));
        assert_eq!(template_values["vars"]["from_default"], json!(5));
        assert_eq!(template_values["vars"]["from_option"], json!("picked"));
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        env::temp_dir().join(format!("docker-ins-{name}-{}-{nanos}", std::process::id()))
    }
}
