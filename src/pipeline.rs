use anyhow::anyhow;
use chrono::{DateTime, Utc};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use inquire::{Confirm, MultiSelect, Select, Text};
use minijinja::{Environment, UndefinedBehavior, context};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::fs;
use tokio::task::JoinSet;

use crate::app::parse::load_app_record;
use crate::app::types::{AppRecord, AppValue};
use crate::cli::node::nodes_file;
use crate::env::build_provider_envs;
use crate::file::local::LocalFile;
use crate::file::remote::RemoteFile;
use crate::file::{FileTrait, ProgressFn};
use crate::node::list::load_all_nodes;
use crate::node::types::NodeRecord;
use crate::provider::docker_compose::DockerComposeProvider;
use crate::provider::{DeploymentTarget, ProviderContext, ProviderTrait};
use crate::store::duck::{
    StoredDeploymentRecord, load_installed_service_configs, load_latest_deployment_record,
    save_deployment_record,
};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

const COPY_CONCURRENCY: usize = 3;

#[derive(clap::Args, Clone, Debug)]
pub struct PipelineArgs {
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

#[derive(Clone, Debug)]
pub struct PreparedDeployment {
    pub provider: String,
    pub node: NodeRecord,
    pub app_names: Vec<String>,
    pub app_home: PathBuf,
    pub workspace: PathBuf,
    pub targets: Vec<DeploymentTarget>,
}

pub enum PipelineMode {
    Check,
    Deploy,
}

pub fn ensure_supported_provider(
    provider: &str,
) -> anyhow::Result<Box<dyn ProviderTrait + Send + Sync>> {
    match provider {
        "docker-compose" => Ok(Box::new(DockerComposeProvider)),
        _ => Err(anyhow!("unsupported provider: {}", provider)),
    }
}

pub async fn prepare_deployment(
    home: &Path,
    provider: String,
    workspace: PathBuf,
    requested_node: Option<String>,
    requested_apps: Option<Vec<String>>,
) -> anyhow::Result<PreparedDeployment> {
    let nodes = load_all_nodes(&nodes_file(home)).await?;
    let node = select_node(&nodes, requested_node.as_deref())?;
    let app_home = home.join("app");
    let app_names = resolve_apps(requested_apps, &app_home).await?;
    let apps = load_app_records_by_names(&app_names, &app_home).await?;
    let targets = build_deployment_targets(apps, home, &node, &workspace).await?;

    Ok(PreparedDeployment {
        provider,
        node,
        app_names,
        app_home,
        workspace,
        targets,
    })
}

pub fn print_prepared_deployment(title: &str, prepared: &PreparedDeployment) {
    println!("{}", title);
    println!("Provider Name: {}", prepared.provider);
    println!("Node Name: {}", node_name(&prepared.node));
    println!("Apps: {}", prepared.app_names.join(", "));
    println!("Workspace: {}", prepared.workspace.display());
    println!("Deployment Targets:");
    for target in &prepared.targets {
        println!(
            "  {} -> service {} -> {}",
            target.app.name,
            target.service,
            prepared.workspace.join(&target.service).display()
        );
    }
}

pub async fn copy_prepared_apps_to_workspace(
    home: &Path,
    prepared: &PreparedDeployment,
) -> anyhow::Result<()> {
    copy_apps_to_workspace(
        home,
        &prepared.targets,
        &prepared.app_home,
        &prepared.workspace,
        &prepared.node,
    )
    .await
}

pub async fn execute_pipeline(
    home: &Path,
    prepared: PreparedDeployment,
    title: &str,
    mode: PipelineMode,
) -> anyhow::Result<()> {
    let provider = ensure_supported_provider(&prepared.provider)?;

    print_prepared_deployment(title, &prepared);
    copy_prepared_apps_to_workspace(home, &prepared).await?;

    let provider_ctx = ProviderContext::new(
        prepared.provider.clone(),
        prepared.node.clone(),
        prepared.targets.clone(),
        prepared.workspace,
        build_provider_envs(
            &prepared.targets,
            &prepared.node,
            &load_installed_service_configs(home).await?,
        )?,
    );

    match mode {
        PipelineMode::Check => {
            println!("Validating with provider...");
            provider.validate(provider_ctx).await?;
            println!("Check completed.");
            Ok(())
        }
        PipelineMode::Deploy => {
            println!("Running provider...");
            provider.run(provider_ctx).await
        }
    }
}

pub fn select_node(nodes: &[NodeRecord], requested: Option<&str>) -> anyhow::Result<NodeRecord> {
    if nodes.is_empty() {
        return Err(anyhow!("no nodes found, please add a node first"));
    }

    if let Some(name) = requested
        && let Some(node) = nodes.iter().find(|node| node_name(node) == name)
    {
        return Ok(node.clone());
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

pub async fn resolve_apps(
    requested: Option<Vec<String>>,
    app_home: &Path,
) -> anyhow::Result<Vec<String>> {
    if let Some(apps) = requested.filter(|apps| !apps.is_empty()) {
        return Ok(apps);
    }

    let available_apps = load_available_app_choices(app_home).await?;
    if available_apps.is_empty() {
        return Err(anyhow!("no apps found, please add an app first"));
    }

    let labels: Vec<String> = available_apps
        .iter()
        .map(|choice| choice.label.clone())
        .collect();
    let selected = MultiSelect::new("Select apps to deploy", labels.clone()).prompt()?;
    if selected.is_empty() {
        return Err(anyhow!("no apps selected"));
    }

    let mut selected_apps = Vec::new();
    for label in selected {
        let choice = available_apps
            .iter()
            .find(|choice| choice.label == label)
            .ok_or_else(|| anyhow!("selected app option '{}' not found", label))?;
        selected_apps.push(choice.name.clone());
    }

    Ok(selected_apps)
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

pub async fn copy_apps_to_workspace(
    home: &Path,
    targets: &[DeploymentTarget],
    app_home: &Path,
    workspace: &Path,
    node: &NodeRecord,
) -> anyhow::Result<()> {
    println!("Saving deployment records...");
    for target in targets {
        let source_dir = app_home.join(&target.app.name);
        let qa_file = app_qa_file(&source_dir);
        // save deployment record
        let qa_yaml = fs::read_to_string(&qa_file)
            .await
            .map_err(|e| anyhow!("read qa file {}: {}", qa_file.display(), e))?;
        println!(
            "Save deployment record for app '{}' into service '{}'",
            target.app.name, target.service
        );
        save_deployment_record(home, node, workspace, target, &qa_yaml).await?;
    }

    target_file_for_node(node).create_dir_all(workspace).await?;

    println!("Copying app files to workspace...");
    for target in targets {
        let source_dir = app_home.join(&target.app.name);
        let target_dir = workspace.join(&target.service);

        if let Some(progress) =
            CopyAppProgress::new(&target.app.name, &target.service, &source_dir, &target_dir)
                .await?
        {
            copy_dir_recursive(
                &source_dir,
                &target_dir,
                &target.app,
                node,
                Some(progress.clone()),
            )
            .await?;
            progress.finish();
        } else {
            println!(
                "  Copying app '{}' into service '{}' at {}",
                target.app.name,
                target.service,
                target_dir.display()
            );
            copy_dir_recursive(&source_dir, &target_dir, &target.app, node, None).await?;
        }
    }

    Ok(())
}

async fn load_available_app_choices(app_home: &Path) -> anyhow::Result<Vec<AppChoice>> {
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
        apps.push(AppChoice {
            label: app_choice_label(&app),
            name: app.name,
        });
    }

    apps.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(apps)
}

fn app_qa_file(app_dir: &Path) -> PathBuf {
    app_dir.join("qa.yaml")
}

#[derive(Clone, Debug)]
struct AppChoice {
    name: String,
    label: String,
}

pub(crate) fn app_choice_label(app: &AppRecord) -> String {
    let mut parts = vec![app.name.clone()];
    if let Some(description) = app
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(description.to_string());
    }
    if let Some(author) = author_display(app) {
        parts.push(author);
    }
    parts.join(" - ")
}

fn author_display(app: &AppRecord) -> Option<String> {
    let author_name = app
        .author_name
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    let author_email = app
        .author_email
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();

    match (author_name.is_empty(), author_email.is_empty()) {
        (true, true) => None,
        (false, true) => Some(author_name.to_string()),
        (true, false) => Some(format!("作者({author_email})")),
        (false, false) => Some(format!("{author_name}({author_email})")),
    }
}

async fn copy_dir_recursive(
    source: &Path,
    target: &Path,
    app_record: &AppRecord,
    node: &NodeRecord,
    progress: Option<Arc<CopyAppProgress>>,
) -> anyhow::Result<()> {
    let template_values = build_template_values(app_record)?;
    let jobs = collect_copy_jobs(source, target).await?;
    target_file_for_node(node).create_dir_all(target).await?;

    if jobs.is_empty() {
        return Ok(());
    }

    let mut join_set = JoinSet::new();
    let mut next_job = 0usize;
    let mut available_slots: Vec<usize> = (0..COPY_CONCURRENCY.min(jobs.len())).rev().collect();

    loop {
        while next_job < jobs.len() && !available_slots.is_empty() {
            let slot = available_slots.pop().expect("slot available");
            let job = jobs[next_job].clone();
            let template_values = template_values.clone();
            let node = node.clone();
            let slot_progress = progress.as_ref().map(|progress| progress.slot(slot));
            join_set.spawn(async move {
                let result =
                    copy_file_to_workspace(job, &template_values, &node, slot_progress).await;
                (slot, result)
            });
            next_job += 1;
        }

        let Some(joined) = join_set.join_next().await else {
            break;
        };
        let (slot, result) = joined.map_err(|e| anyhow!("copy task join error: {}", e))?;
        result?;
        available_slots.push(slot);
    }

    Ok(())
}

pub fn build_template_values(app_record: &AppRecord) -> anyhow::Result<Value> {
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

pub fn build_deployment_target(
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

pub fn apply_stored_values(app: &mut AppRecord, preset: &StoredDeploymentRecord) {
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
    if let Some(preset) = preset
        && std::io::stdin().is_terminal()
    {
        let options = vec![
            format!("Use previous service ({})", preset.service),
            format!("Use app name ({})", app.name),
            "Enter service name manually".to_string(),
        ];
        let answer = Select::new(
            &format!("Service name for app '{}'", app.name),
            options.clone(),
        )
        .prompt()?;
        if answer == options[0] {
            return Ok(preset.service.clone());
        }
        if answer == options[1] {
            return Ok(app.name.clone());
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
        .map(|value| resolve_app_value(value, preset.and_then(|p| p.app_values.get(&value.name))))
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

pub fn parse_number_value(raw: &str, value_name: &str) -> anyhow::Result<Value> {
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
    job: CopyJob,
    template_values: &serde_json::Value,
    node: &NodeRecord,
    progress: Option<CopyProgressSlot>,
) -> anyhow::Result<()> {
    let source_file = LocalFile;
    let target_file = target_file_for_node(node);

    if job.render_as_template {
        if let Some(progress) = progress.as_ref() {
            progress.start_template(&job.target_path);
        } else {
            println!(
                "    Rendering template {} -> {}",
                job.source_path.display(),
                job.target_path.display()
            );
        }
        let source = source_file
            .read(&job.source_path, None)
            .await
            .map_err(|e| anyhow!("read template {}: {}", job.source_path.display(), e))?;
        let rendered = render_template(&source, template_values)?;
        let rendered =
            maybe_inject_compose_labels(&job.target_path, &rendered, template_values, node)?;
        let rendered_size = rendered.len() as u64;
        if let Some(progress) = progress.as_ref() {
            progress.begin_write_phase(rendered_size);
        }
        let progress_write = progress.as_ref().map(|progress| progress.write_progress());
        target_file
            .write(&job.target_path, &rendered, progress_write.as_ref())
            .await
            .map_err(|e| anyhow!("write rendered file {}: {}", job.target_path.display(), e))?;
        if let Some(progress) = progress.as_ref() {
            progress.finish_file();
        }
        return Ok(());
    }

    let source_meta = fs::metadata(&job.source_path)
        .await
        .map_err(|e| anyhow!("metadata source file {}: {}", job.source_path.display(), e))?;
    let source_size = source_meta.len();
    if let Some(progress) = progress.as_ref() {
        progress.start_copy(&job.target_path, source_size);
    } else {
        println!(
            "    Copying file {} -> {}",
            job.source_path.display(),
            job.target_path.display()
        );
    }
    let source_bytes = source_file
        .read_bytes(&job.source_path, None)
        .await
        .map_err(|e| anyhow!("read source file {}: {}", job.source_path.display(), e))?;
    if is_docker_compose_file(&job.target_path) {
        let source = String::from_utf8(source_bytes).map_err(|e| {
            anyhow!(
                "read compose file {} as utf-8: {}",
                job.source_path.display(),
                e
            )
        })?;
        let rendered =
            maybe_inject_compose_labels(&job.target_path, &source, template_values, node)?;
        let rendered_size = rendered.len() as u64;
        if let Some(progress) = progress.as_ref() {
            progress.begin_write_phase(rendered_size);
        }
        let progress_write = progress.as_ref().map(|progress| progress.write_progress());
        target_file
            .write(&job.target_path, &rendered, progress_write.as_ref())
            .await
            .map_err(|e| anyhow!("write compose file {}: {}", job.target_path.display(), e))?;
        if let Some(progress) = progress.as_ref() {
            progress.finish_file();
        }
        return Ok(());
    }
    if let Some(progress) = progress.as_ref() {
        progress.begin_write_phase(source_size);
    }
    let progress_write = progress.as_ref().map(|progress| progress.write_progress());
    target_file
        .write_bytes(&job.target_path, &source_bytes, progress_write.as_ref())
        .await
        .map_err(|e| {
            anyhow!(
                "copy {} to {}: {}",
                job.source_path.display(),
                job.target_path.display(),
                e
            )
        })?;
    if let Some(progress) = progress.as_ref() {
        progress.finish_file();
    }
    Ok(())
}

fn is_docker_compose_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("docker-compose.yml" | "docker-compose.yaml")
    )
}

fn maybe_inject_compose_labels(
    path: &Path,
    content: &str,
    template_values: &serde_json::Value,
    node: &NodeRecord,
) -> anyhow::Result<String> {
    if !is_docker_compose_file(path) {
        return Ok(content.to_string());
    }

    inject_compose_labels(
        content,
        &build_compose_metadata_labels(template_values, node),
    )
}

fn inject_compose_labels(
    content: &str,
    metadata_labels: &BTreeMap<String, String>,
) -> anyhow::Result<String> {
    let mut document: serde_yaml::Value =
        serde_yaml::from_str(content).map_err(|e| anyhow!("parse compose yaml: {}", e))?;

    let Some(root) = document.as_mapping_mut() else {
        return Ok(content.to_string());
    };
    let Some(services) = root
        .get_mut(serde_yaml::Value::String("services".into()))
        .and_then(serde_yaml::Value::as_mapping_mut)
    else {
        return Ok(content.to_string());
    };

    for service in services.values_mut() {
        let Some(service_mapping) = service.as_mapping_mut() else {
            continue;
        };
        let labels_key = serde_yaml::Value::String("labels".into());
        let existing = service_mapping.remove(&labels_key);
        let mut labels = labels_value_to_mapping(existing)?;

        for (key, value) in metadata_labels {
            labels.insert(
                serde_yaml::Value::String(key.clone()),
                serde_yaml::Value::String(value.clone()),
            );
        }

        service_mapping.insert(labels_key, serde_yaml::Value::Mapping(labels));
    }

    serde_yaml::to_string(&document).map_err(|e| anyhow!("serialize compose yaml: {}", e))
}

fn labels_value_to_mapping(
    value: Option<serde_yaml::Value>,
) -> anyhow::Result<serde_yaml::Mapping> {
    let mut mapping = serde_yaml::Mapping::new();
    let Some(value) = value else {
        return Ok(mapping);
    };

    match value {
        serde_yaml::Value::Null => Ok(mapping),
        serde_yaml::Value::Mapping(existing) => Ok(existing),
        serde_yaml::Value::Sequence(items) => {
            for item in items {
                let Some(text) = item.as_str() else {
                    return Err(anyhow!("compose labels sequence entries must be strings"));
                };
                let (key, value) = text.split_once('=').unwrap_or((text, ""));
                mapping.insert(
                    serde_yaml::Value::String(key.to_string()),
                    serde_yaml::Value::String(value.to_string()),
                );
            }
            Ok(mapping)
        }
        _ => Err(anyhow!("compose labels must be a mapping or sequence")),
    }
}

pub(crate) fn build_compose_metadata_labels(
    template_values: &serde_json::Value,
    node: &NodeRecord,
) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    labels.insert("ins.node_name".into(), node_name(node).to_string());

    if let Some(app) = template_values.get("app") {
        insert_compose_label(&mut labels, "ins.name", app.get("name"));
        insert_compose_label(&mut labels, "ins.description", app.get("description"));
        insert_compose_label(&mut labels, "ins.author_name", app.get("author_name"));
        insert_compose_label(&mut labels, "ins.author_email", app.get("author_email"));
        insert_compose_label(&mut labels, "ins.version", app.get("version"));
    }

    labels
}

fn insert_compose_label(
    labels: &mut BTreeMap<String, String>,
    key: &str,
    value: Option<&serde_json::Value>,
) {
    let Some(value) = value else {
        return;
    };
    if value.is_null() {
        return;
    }
    let text = value
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| value.to_string());
    labels.insert(key.to_string(), text);
}

#[derive(Clone, Debug)]
struct CopyJob {
    source_path: PathBuf,
    target_path: PathBuf,
    render_as_template: bool,
}

struct CopyAppProgress {
    _multi: Arc<MultiProgress>,
    total_files: u64,
    app_bar: ProgressBar,
    file_bars: Vec<ProgressBar>,
}

#[derive(Clone)]
struct CopyProgressSlot {
    app_bar: ProgressBar,
    file_bar: ProgressBar,
}

impl CopyAppProgress {
    async fn new(
        app_name: &str,
        _service: &str,
        source_dir: &Path,
        target_dir: &Path,
    ) -> anyhow::Result<Option<Arc<Self>>> {
        if !std::io::stdout().is_terminal() {
            return Ok(None);
        }

        let total_files = count_files_recursive(source_dir).await?;
        let multi = Arc::new(MultiProgress::new());
        let app_bar = multi.add(ProgressBar::new(total_files.max(1)));
        app_bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} {msg:<24} [{bar:24.cyan/blue}] {pos}/{len} files {elapsed_precise}",
            )
            .unwrap()
            .progress_chars("█▉▊▋▌▍▎▏ "),
        );
        app_bar.set_message(format!("{app_name} -> {}", target_dir.display()));

        let mut file_bars = Vec::new();
        for index in 0..COPY_CONCURRENCY.min(total_files.max(1) as usize) {
            let file_bar = multi.add(ProgressBar::new_spinner());
            file_bar.set_style(
                ProgressStyle::with_template(
                    "   {spinner:.green} {msg:<64} {bytes}/{total_bytes} {elapsed_precise}",
                )
                .unwrap(),
            );
            file_bar.set_message(format!(
                "Waiting {}/{} in {}",
                index + 1,
                COPY_CONCURRENCY,
                target_dir.display()
            ));
            file_bar.finish_and_clear();
            file_bars.push(file_bar);
        }

        Ok(Some(Arc::new(Self {
            _multi: multi,
            total_files: total_files.max(1),
            app_bar,
            file_bars,
        })))
    }

    fn slot(&self, index: usize) -> CopyProgressSlot {
        CopyProgressSlot {
            app_bar: self.app_bar.clone(),
            file_bar: self.file_bars[index].clone(),
        }
    }

    fn finish(&self) {
        for file_bar in &self.file_bars {
            file_bar.finish_and_clear();
        }
        if self.total_files == 0 {
            self.app_bar.set_length(0);
        }
        self.app_bar.finish_with_message("Copy complete");
    }
}

impl CopyProgressSlot {
    fn start_copy(&self, path: &Path, size: u64) {
        self.file_bar.reset();
        self.file_bar.reset_elapsed();
        self.file_bar
            .enable_steady_tick(std::time::Duration::from_millis(100));
        self.file_bar.set_length(size.max(1));
        self.file_bar.set_position(0);
        self.file_bar
            .set_message(format!("Copying {}", path.display()));
    }

    fn start_template(&self, path: &Path) {
        self.file_bar.reset();
        self.file_bar.reset_elapsed();
        self.file_bar
            .enable_steady_tick(std::time::Duration::from_millis(100));
        self.file_bar.set_length(0);
        self.file_bar.set_position(0);
        self.file_bar
            .set_message(format!("Rendering {}", path.display()));
    }

    fn begin_write_phase(&self, size: u64) {
        self.file_bar.set_length(size.max(1));
        self.file_bar.set_position(0);
    }

    fn write_progress(&self) -> ProgressFn {
        let file_bar = self.file_bar.clone();
        Arc::new(move |current, total| {
            let target = total.max(1);
            file_bar.set_length(target);
            file_bar.set_position(current.min(target));
        })
    }

    fn finish_file(&self) {
        self.file_bar.disable_steady_tick();
        self.file_bar.finish_and_clear();
        self.app_bar.inc(1);
    }
}

async fn collect_copy_jobs(source: &Path, target: &Path) -> anyhow::Result<Vec<CopyJob>> {
    let mut jobs = Vec::new();
    let mut stack = vec![(source.to_path_buf(), target.to_path_buf())];

    while let Some((current_source, current_target)) = stack.pop() {
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

            let file_name = file_name.to_string_lossy().to_string();
            let target_path = if is_template_file(&file_name) {
                current_target.join(rendered_template_name(&file_name))
            } else {
                current_target.join(&file_name)
            };
            jobs.push(CopyJob {
                source_path,
                target_path,
                render_as_template: is_template_file(&file_name),
            });
        }
    }

    Ok(jobs)
}

async fn count_files_recursive(root: &Path) -> anyhow::Result<u64> {
    let mut count = 0u64;
    let mut stack = vec![root.to_path_buf()];

    while let Some(current) = stack.pop() {
        let mut entries = fs::read_dir(&current)
            .await
            .map_err(|e| anyhow!("read source dir {}: {}", current.display(), e))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| anyhow!("iterate source dir {}: {}", current.display(), e))?
        {
            let path = entry.path();
            let file_type = entry
                .file_type()
                .await
                .map_err(|e| anyhow!("read file type {}: {}", path.display(), e))?;
            if file_type.is_dir() {
                stack.push(path);
            } else {
                count += 1;
            }
        }
    }

    Ok(count)
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

pub fn is_template_file(file_name: &str) -> bool {
    file_name.ends_with(".j2")
        || file_name.ends_with(".jinja")
        || file_name.ends_with(".jinja2")
        || file_name.ends_with(".tmpl")
}

pub fn rendered_template_name(file_name: &str) -> &str {
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

fn target_file_for_node(node: &NodeRecord) -> Box<dyn FileTrait> {
    match node {
        NodeRecord::Local() => Box::new(LocalFile),
        NodeRecord::Remote(remote) => {
            let remote_file = RemoteFile::new(
                remote.ip.clone(),
                remote.port,
                remote.user.clone(),
                remote.password.clone(),
            );
            let remote_file = if let Some(key_path) = &remote.key_path {
                remote_file.with_key_path(key_path.clone())
            } else {
                remote_file
            };
            Box::new(remote_file)
        }
    }
}
