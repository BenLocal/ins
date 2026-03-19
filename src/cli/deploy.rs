use anyhow::anyhow;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use inquire::{Confirm, MultiSelect, Select, Text};
use minijinja::{Environment, UndefinedBehavior, context};
use serde_json::{Map, Value};
use tokio::fs;

use crate::app::parse::load_app_record;
use crate::app::types::{AppRecord, AppValue};
use crate::cli::{CommandContext, CommandTrait, node::nodes_file};
use crate::file::FileTrait;
use crate::file::local::LocalFile;
use crate::file::remote::RemoteFile;
use crate::node::list::load_all_nodes;
use crate::node::types::NodeRecord;
use crate::provider::docker_compose::DockerComposeProvider;
use crate::provider::{DeploymentTarget, ProviderContext, ProviderTrait as _};
use crate::store::duck::{
    StoredDeploymentRecord, load_latest_deployment_record, save_deployment_record,
};
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
        let targets = build_deployment_targets(apps, &ctx.home, &node, &args.workspace).await?;

        println!("Starting deployment...");
        println!("Provider Name: {}", &args.provider);
        println!("Node Name: {}", node_name(&node));
        println!("Apps: {}", &app_names.join(", "));
        println!("Workspace: {}", args.workspace.display());
        println!("Deployment Targets:");
        for target in &targets {
            println!(
                "  {} -> service {} -> {}",
                target.app.name,
                target.service,
                args.workspace.join(&target.service).display()
            );
        }

        println!("Copying apps to workspace...");
        copy_apps_to_workspace(&ctx.home, &targets, &app_home, &args.workspace, &node).await?;

        println!("Running provider...");
        provider
            .run(ProviderContext::new(
                args.provider,
                node,
                targets,
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
    home: &Path,
    targets: &[DeploymentTarget],
    app_home: &Path,
    workspace: &Path,
    node: &NodeRecord,
) -> anyhow::Result<()> {
    target_file_for_node(node).create_dir_all(workspace).await?;

    for target in targets {
        let source_dir = app_home.join(&target.app.name);
        let qa_file = app_qa_file(&source_dir);
        let target_dir = workspace.join(&target.service);
        println!(
            "\r  Copying app '{}' into service '{}' at {}",
            target.app.name,
            target.service,
            target_dir.display()
        );
        copy_dir_recursive(&source_dir, &target_dir, &target.app, node).await?;

        let qa_yaml = fs::read_to_string(&qa_file)
            .await
            .map_err(|e| anyhow!("read qa file {}: {}", qa_file.display(), e))?;
        save_deployment_record(home, node, workspace, target, &qa_yaml).await?;
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
    node: &NodeRecord,
) -> anyhow::Result<()> {
    let mut stack = vec![(source.to_path_buf(), target.to_path_buf())];
    let template_values = build_template_values(app_record)?;
    let target_file = target_file_for_node(node);

    while let Some((current_source, current_target)) = stack.pop() {
        target_file.create_dir_all(&current_target).await?;

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
                println!("\r    Creating directory {}", target_path.display());
                stack.push((source_path, target_path));
                continue;
            }

            copy_file_to_workspace(
                &source_path,
                &current_target,
                &file_name.to_string_lossy(),
                &template_values,
                target_file.as_ref(),
            )
            .await?;
        }
    }

    Ok(())
}

fn build_template_values(app_record: &AppRecord) -> anyhow::Result<Value> {
    let resolved_values = resolve_app_values(app_record, None)?;
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

async fn build_deployment_targets(
    apps: Vec<AppRecord>,
    home: &Path,
    node: &NodeRecord,
    workspace: &Path,
) -> anyhow::Result<Vec<DeploymentTarget>> {
    let mut targets = Vec::with_capacity(apps.len());

    for app in apps {
        let preset = load_latest_deployment_record(home, node, workspace, &app.name).await?;
        let target = build_deployment_target(app, preset.as_ref())?;
        targets.push(target);
    }

    Ok(targets)
}

fn build_deployment_target(
    mut app: AppRecord,
    preset: Option<&StoredDeploymentRecord>,
) -> anyhow::Result<DeploymentTarget> {
    let reuse_stored = should_reuse_stored_settings(&app, preset)?;
    let service = if reuse_stored {
        apply_stored_values(&mut app, preset.expect("preset exists when confirmed"));
        preset
            .expect("preset exists when confirmed")
            .service
            .clone()
    } else {
        resolve_service_name(&app, preset)?
    };

    materialize_app_values(&mut app, if reuse_stored { None } else { preset })?;
    Ok(DeploymentTarget::new(app, service))
}

fn should_reuse_stored_settings(
    app: &AppRecord,
    preset: Option<&StoredDeploymentRecord>,
) -> anyhow::Result<bool> {
    let Some(preset) = preset else {
        return Ok(false);
    };
    if !std::io::stdin().is_terminal() {
        return Ok(false);
    }

    Confirm::new(&format!(
        "Reuse latest settings for app '{}' (service '{}', saved at {})?",
        app.name,
        preset.service,
        format_timestamp_ms(preset.created_at_ms)
    ))
    .with_default(true)
    .prompt()
    .map_err(Into::into)
}

fn apply_stored_values(app: &mut AppRecord, preset: &StoredDeploymentRecord) {
    for value in &mut app.values {
        if let Some(stored) = preset.app_values.get(&value.name) {
            value.value = Some(stored.clone());
        }
    }
}

fn materialize_app_values(
    app: &mut AppRecord,
    preset: Option<&StoredDeploymentRecord>,
) -> anyhow::Result<()> {
    let resolved = resolve_app_values(app, preset)?;
    for (value, resolved_value) in app.values.iter_mut().zip(resolved) {
        value.value = Some(resolved_value);
    }
    Ok(())
}

fn resolve_service_name(
    app: &AppRecord,
    preset: Option<&StoredDeploymentRecord>,
) -> anyhow::Result<String> {
    if let Some(preset) = preset {
        if std::io::stdin().is_terminal() {
            let options = vec![
                format!("Use previous service ({})", preset.service),
                format!("Use app name ({})", app.name),
                "Enter service name manually".to_string(),
            ];
            let answer =
                Select::new(&format!("Service name for app '{}'", app.name), options.clone())
                    .prompt()?;
            if answer == options[0] {
                return Ok(preset.service.clone());
            }
            if answer == options[1] {
                return Ok(app.name.clone());
            }
        }
    }

    if !std::io::stdin().is_terminal() {
        return Ok(app.name.clone());
    }

    let answer = Text::new(&format!("Service name for app '{}'", app.name))
        .with_default(&app.name)
        .prompt()?;
    let trimmed = answer.trim();

    if trimmed.is_empty() {
        return Ok(app.name.clone());
    }

    Ok(trimmed.to_string())
}

fn resolve_app_values(
    app_record: &AppRecord,
    preset: Option<&StoredDeploymentRecord>,
) -> anyhow::Result<Vec<Value>> {
    app_record
        .values
        .iter()
        .map(|value| resolve_app_value(value, preset.and_then(|preset| preset.app_values.get(&value.name))))
        .collect()
}

fn resolve_app_value(value: &AppValue, stored: Option<&Value>) -> anyhow::Result<Value> {
    if let Some(current) = value.value.clone() {
        return Ok(current);
    }

    if let Some(stored) = stored {
        if !std::io::stdin().is_terminal() {
            return Ok(stored.clone());
        }

        let stored_rendered = serde_json::to_string(stored).unwrap_or_else(|_| "<stored>".into());
        let mut options = vec![format!("Use previous value ({stored_rendered})")];
        if let Some(default) = value.default.clone() {
            let default_rendered =
                serde_json::to_string(&default).unwrap_or_else(|_| "<default>".into());
            options.push(format!("Use default ({default_rendered})"));
        }
        options.push("Enter value manually".to_string());

        let answer = Select::new(&value_prompt(value), options.clone()).prompt()?;
        if answer == options[0] {
            return Ok(stored.clone());
        }
        if options.len() == 3 && answer == options[1] {
            return Ok(value.default.clone().unwrap_or(Value::Null));
        }
    }

    if let Some(default) = value.default.clone() {
        if !std::io::stdin().is_terminal() {
            return Ok(default);
        }

        let use_default = Confirm::new(&format!(
            "Use default value for '{}'{}?",
            value.name,
            match serde_json::to_string(&default) {
                Ok(rendered) => format!(" ({rendered})"),
                Err(_) => String::new(),
            }
        ))
        .with_default(true)
        .prompt()?;

        if use_default {
            return Ok(default);
        }
    }

    if !value.options.is_empty() {
        if !std::io::stdin().is_terminal() {
            return Ok(value.options[0].value.clone().unwrap_or(Value::Null));
        }

        if value.options.len() == 1 {
            let option = &value.options[0];
            let use_option = Confirm::new(&format!(
                "Use only available option for '{}' ({})?",
                value.name, option.name
            ))
            .with_default(true)
            .prompt()?;

            if use_option {
                return Ok(option.value.clone().unwrap_or(Value::Null));
            }
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
        "number" => {
            let raw = Text::new(&value_prompt(value)).prompt()?;
            parse_number_value(&raw, &value.name)
        }
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

fn parse_number_value(raw: &str, value_name: &str) -> anyhow::Result<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("invalid number for '{}': empty input", value_name));
    }

    if let Ok(number) = trimmed.parse::<i64>() {
        return Ok(Value::Number(number.into()));
    }
    if let Ok(number) = trimmed.parse::<u64>() {
        return Ok(Value::Number(number.into()));
    }
    if let Ok(number) = trimmed.parse::<f64>() {
        return serde_json::Number::from_f64(number)
            .map(Value::Number)
            .ok_or_else(|| anyhow!("invalid number for '{}'", value_name));
    }

    Err(anyhow!("invalid number for '{}': {}", value_name, raw))
}

fn format_timestamp_ms(timestamp_ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(timestamp_ms)
        .map(|time| time.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| timestamp_ms.to_string())
}

async fn copy_file_to_workspace(
    source_path: &Path,
    target_dir: &Path,
    file_name: &str,
    template_values: &serde_json::Value,
    target_file: &dyn FileTrait,
) -> anyhow::Result<()> {
    let source_file = LocalFile;

    if is_template_file(file_name) {
        let rendered_name = rendered_template_name(file_name);
        let target_path = target_dir.join(rendered_name);
        println!(
            "\r    Rendering template {} -> {}",
            source_path.display(),
            target_path.display()
        );
        let source = source_file
            .read(source_path, None)
            .await
            .map_err(|e| anyhow!("read template {}: {}", source_path.display(), e))?;
        let rendered = render_template(&source, template_values)?;
        target_file
            .write(&target_path, &rendered, None)
            .await
            .map_err(|e| anyhow!("write rendered file {}: {}", target_path.display(), e))?;
        return Ok(());
    }

    let target_path = target_dir.join(file_name);
    println!(
        "\r    Copying file {} -> {}",
        source_path.display(),
        target_path.display()
    );
    let source_bytes = source_file
        .read_bytes(source_path, None)
        .await
        .map_err(|e| anyhow!("read source file {}: {}", source_path.display(), e))?;
    target_file
        .write_bytes(&target_path, &source_bytes, None)
        .await
        .map_err(|e| {
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

fn target_file_for_node(node: &NodeRecord) -> Box<dyn FileTrait> {
    match node {
        NodeRecord::Local() => Box::new(LocalFile),
        NodeRecord::Remote(remote) => Box::new(RemoteFile::new(
            remote.ip.clone(),
            remote.port,
            remote.user.clone(),
            remote.password.clone(),
        )),
    }
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
