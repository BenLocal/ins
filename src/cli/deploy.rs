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
        let app_names = resolve_apps(args.apps, &app_home).await?;
        let apps = load_app_records_by_names(&app_names, &app_home).await?;

        println!("Starting deployment...");
        println!("Provider Name: {}", &args.provider);
        println!("Node Name: {}", node_name(&node));
        println!("Apps: {}", &app_names.join(", "));
        println!("Workspace: {}", args.workspace.display());

        println!("Copying apps to workspace...");
        copy_apps_to_workspace(&app_names, &app_home, &args.workspace).await?;

        println!("Running provider...");
        provider
            .run(ProviderContext::new(
                args.provider,
                node,
                apps,
                args.workspace,
            ))
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

async fn load_app_records_by_names(
    apps: &[String],
    app_home: &Path,
) -> anyhow::Result<Vec<AppRecord>> {
    let mut records = Vec::new();

    for app_name in apps {
        let app_dir = app_home.join(app_name);
        let qa_file = app_qa_file(&app_dir);
        let record = load_app_record(&qa_file).await?;
        records.push(record);
    }

    Ok(records)
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
            let resolved_value = resolved_values.get(index).cloned().unwrap_or(Value::Null);
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
    app_record.values.iter().map(resolve_app_value).collect()
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
            Confirm::new(&value_prompt(value))
                .with_default(false)
                .prompt()?,
        )),
        "number" => Ok(serde_json::Number::from_f64(
            CustomType::<f64>::new(&value_prompt(value)).prompt()?,
        )
        .map(Value::Number)
        .ok_or_else(|| anyhow!("invalid number for '{}'", value.name))?),
        "json" => {
            let raw = Text::new(&format!("{} (JSON)", value_prompt(value))).prompt()?;
            serde_json::from_str(&raw)
                .map_err(|e| anyhow!("invalid json for '{}': {}", value.name, e))
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
#[path = "deploy_test.rs"]
mod deploy_test;
