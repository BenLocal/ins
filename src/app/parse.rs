use std::path::Path;

use anyhow::{Context, anyhow};
use tokio::fs;

use crate::app::types::{AppRecord, AppValue, AppValueOption, ScriptHook};

pub async fn load_app_record(qa_file: &Path) -> anyhow::Result<AppRecord> {
    let content = fs::read_to_string(qa_file)
        .await
        .with_context(|| format!("read app file {}", qa_file.display()))?;

    parse_qa_yaml(&content).with_context(|| format!("parse app file {}", qa_file.display()))
}

fn parse_qa_yaml(content: &str) -> anyhow::Result<AppRecord> {
    let mut record = AppRecord::default();
    let mut section: Option<&str> = None;
    let mut current_value: Option<usize> = None;
    let mut in_options = false;
    let mut current_option: Option<usize> = None;

    for raw_line in content.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = raw_line.len() - raw_line.trim_start().len();

        if indent == 0 {
            current_value = None;
            current_option = None;
            in_options = false;

            if trimmed == "before:" {
                section = Some("before");
                continue;
            }
            if trimmed == "after:" {
                section = Some("after");
                continue;
            }
            if trimmed == "values:" {
                section = Some("values");
                continue;
            }

            let Some((key, value)) = trimmed.split_once(':') else {
                return Err(anyhow!("invalid qa.yaml line '{}'", raw_line));
            };
            match key.trim() {
                "name" => record.name = parse_yaml_scalar(value.trim()),
                "description" => record.description = Some(parse_yaml_scalar(value.trim())),
                other => return Err(anyhow!("unsupported top-level field '{}'", other)),
            }
            section = None;
            continue;
        }

        match section {
            Some("before") => parse_hook_line(&mut record.before, trimmed, raw_line)?,
            Some("after") => parse_hook_line(&mut record.after, trimmed, raw_line)?,
            Some("values") => {
                if indent == 2 && trimmed.starts_with("- ") {
                    let mut value_item = AppValue::default();
                    parse_value_line(&mut value_item, &trimmed[2..], raw_line)?;
                    record.values.push(value_item);
                    current_value = Some(record.values.len() - 1);
                    current_option = None;
                    in_options = false;
                    continue;
                }

                let Some(value_index) = current_value else {
                    return Err(anyhow!("value entry missing before '{}'", raw_line));
                };

                if indent == 4 && trimmed == "options:" {
                    in_options = true;
                    current_option = None;
                    continue;
                }

                if indent == 4 && !in_options {
                    parse_value_line(&mut record.values[value_index], trimmed, raw_line)?;
                    continue;
                }

                if indent == 6 && trimmed.starts_with("- ") && in_options {
                    let mut option_item = AppValueOption::default();
                    parse_option_line(&mut option_item, &trimmed[2..], raw_line)?;
                    record.values[value_index].options.push(option_item);
                    current_option = Some(record.values[value_index].options.len() - 1);
                    continue;
                }

                if indent == 8 && in_options {
                    let Some(option_index) = current_option else {
                        return Err(anyhow!("option entry missing before '{}'", raw_line));
                    };
                    parse_option_line(
                        &mut record.values[value_index].options[option_index],
                        trimmed,
                        raw_line,
                    )?;
                    continue;
                }

                return Err(anyhow!("unsupported values line '{}'", raw_line));
            }
            _ => return Err(anyhow!("unsupported nested line '{}'", raw_line)),
        }
    }

    if record.name.is_empty() {
        return Err(anyhow!("missing field 'name'"));
    }

    Ok(record)
}

fn parse_hook_line(hook: &mut ScriptHook, trimmed: &str, raw_line: &str) -> anyhow::Result<()> {
    let Some((key, value)) = trimmed.split_once(':') else {
        return Err(anyhow!("invalid hook line '{}'", raw_line));
    };
    match key.trim() {
        "shell" => hook.shell = Some(parse_yaml_scalar(value.trim())),
        "script" => hook.script = Some(parse_yaml_scalar(value.trim())),
        other => return Err(anyhow!("unsupported hook field '{}'", other)),
    }
    Ok(())
}

fn parse_value_line(
    value_item: &mut AppValue,
    trimmed: &str,
    raw_line: &str,
) -> anyhow::Result<()> {
    let Some((key, value)) = trimmed.split_once(':') else {
        return Err(anyhow!("invalid value line '{}'", raw_line));
    };
    match key.trim() {
        "name" => value_item.name = parse_yaml_scalar(value.trim()),
        "type" => value_item.value_type = parse_yaml_scalar(value.trim()),
        "description" => value_item.description = Some(parse_yaml_scalar(value.trim())),
        other => return Err(anyhow!("unsupported value field '{}'", other)),
    }
    Ok(())
}

fn parse_option_line(
    option_item: &mut AppValueOption,
    trimmed: &str,
    raw_line: &str,
) -> anyhow::Result<()> {
    let Some((key, value)) = trimmed.split_once(':') else {
        return Err(anyhow!("invalid option line '{}'", raw_line));
    };
    match key.trim() {
        "name" => option_item.name = parse_yaml_scalar(value.trim()),
        "description" => option_item.description = Some(parse_yaml_scalar(value.trim())),
        "value" => option_item.value = Some(parse_yaml_scalar(value.trim())),
        other => return Err(anyhow!("unsupported option field '{}'", other)),
    }
    Ok(())
}

fn parse_yaml_scalar(value: &str) -> String {
    if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        inner.replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        value.to_string()
    }
}
