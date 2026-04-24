use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use inquire::{Confirm, MultiSelect, Select, Text};
use serde_json::Value;
use tokio::fs;

use crate::app::parse::load_app_record;
use crate::app::types::{AppRecord, AppValue};
use crate::node::types::NodeRecord;
use crate::provider::DeploymentTarget;
use crate::store::duck::{StoredDeploymentRecord, load_latest_deployment_record};

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

pub(super) async fn load_app_records_by_names(
    apps: &[String],
    app_home: &Path,
    extra_env: &BTreeMap<String, String>,
) -> anyhow::Result<Vec<AppRecord>> {
    let mut records = Vec::new();

    for app_name in apps {
        let app_dir = app_home.join(app_name);
        let qa_file = app_qa_file(&app_dir);
        let record = load_app_record(&qa_file, extra_env).await?;
        records.push(record);
    }

    Ok(records)
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

        let app = load_app_record(&qa_file, &BTreeMap::new()).await?;
        apps.push(AppChoice {
            label: app_choice_label(&app),
            name: app.name,
        });
    }

    apps.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(apps)
}

pub(super) fn app_qa_file(app_dir: &Path) -> PathBuf {
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
pub(super) async fn build_deployment_targets(
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
        if value.value.is_none()
            && let Some(stored) = preset.app_values.get(&value.name)
        {
            value.value = Some(stored.clone());
        }
    }
}

pub fn parse_cli_value_overrides(
    raw_values: &[String],
) -> anyhow::Result<BTreeMap<String, String>> {
    let mut overrides = BTreeMap::new();

    for raw in raw_values {
        let Some((name, value)) = raw.split_once('=') else {
            return Err(anyhow!(
                "invalid value override '{}', expected key=value",
                raw
            ));
        };
        let key = name.trim();
        if key.is_empty() {
            return Err(anyhow!(
                "invalid value override '{}', key cannot be empty",
                raw
            ));
        }
        overrides.insert(key.to_string(), value.to_string());
    }

    Ok(overrides)
}

pub fn apply_cli_values(
    apps: &mut [AppRecord],
    overrides: &BTreeMap<String, String>,
) -> anyhow::Result<()> {
    if overrides.is_empty() {
        return Ok(());
    }

    let mut matched = BTreeMap::new();

    for app in apps {
        for value in &mut app.values {
            let Some(raw) = overrides.get(&value.name) else {
                continue;
            };
            value.value = Some(parse_cli_value(raw, value)?);
            matched.insert(value.name.clone(), true);
        }
    }

    if let Some(missing) = overrides.keys().find(|key| !matched.contains_key(*key)) {
        return Err(anyhow!("unknown qa value override '{}'", missing));
    }

    Ok(())
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

pub(super) fn resolve_app_values(
    app_record: &AppRecord,
    preset: Option<&StoredDeploymentRecord>,
) -> anyhow::Result<Vec<Value>> {
    app_record
        .values
        .iter()
        .map(|value| resolve_app_value(value, preset.and_then(|p| p.app_values.get(&value.name))))
        .collect()
}

fn parse_cli_value(raw: &str, value: &AppValue) -> anyhow::Result<Value> {
    match value.value_type.as_str() {
        "boolean" => raw
            .parse::<bool>()
            .map(Value::Bool)
            .map_err(|e| anyhow!("invalid boolean for '{}': {}", value.name, e)),
        "number" => parse_number_value(raw, &value.name),
        "json" => serde_json::from_str(raw)
            .map_err(|e| anyhow!("invalid json for '{}': {}", value.name, e)),
        _ => Ok(Value::String(raw.to_string())),
    }
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
