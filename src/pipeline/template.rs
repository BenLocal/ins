use anyhow::anyhow;
use minijinja::{Environment, UndefinedBehavior, context};
use serde_json::{Map, Value};

use crate::app::types::AppRecord;
use crate::execution_output::ExecutionOutput;
use crate::provider::DeploymentTarget;

use super::target::resolve_app_values;

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

pub(super) fn build_target_template_values(
    target: &DeploymentTarget,
    output: &ExecutionOutput,
) -> anyhow::Result<Value> {
    let mut template_values = build_template_values(&target.app)?;
    if let Some(obj) = template_values.as_object_mut() {
        obj.insert("service".into(), Value::String(target.service.clone()));
    }
    debug_print_template_values(&target.app.name, &template_values, output);
    Ok(template_values)
}

fn debug_print_template_values(app_name: &str, template_values: &Value, output: &ExecutionOutput) {
    output.line("----------------------------");
    output.line(format!("[debug] template values for app '{app_name}':"));
    for section in ["service", "app", "vars"] {
        let Some(value) = template_values.get(section) else {
            continue;
        };
        let mut lines = Vec::new();
        flatten_template_value(section, value, &mut lines);
        for line in lines {
            output.line(format!("      {line}"));
        }
    }
    output.line("----------------------------");
}

fn flatten_template_value(prefix: &str, value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if map.is_empty() {
                out.push(format!("{prefix}={{}}"));
            } else {
                for (key, v) in map {
                    flatten_template_value(&format!("{prefix}.{key}"), v, out);
                }
            }
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                out.push(format!("{prefix}=[]"));
            } else {
                for (index, v) in arr.iter().enumerate() {
                    flatten_template_value(&format!("{prefix}[{index}]"), v, out);
                }
            }
        }
        Value::String(s) => out.push(format!("{prefix}={s}")),
        Value::Null => out.push(format!("{prefix}=null")),
        other => out.push(format!("{prefix}={other}")),
    }
}

pub(super) fn render_template(source: &str, template_values: &Value) -> anyhow::Result<String> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Lenient);
    env.add_template("app", source)
        .map_err(|e| anyhow!("add template: {}", e))?;
    let template = env
        .get_template("app")
        .map_err(|e| anyhow!("get template: {}", e))?;
    template
        .render(context! {
            app => template_values.get("app").cloned().unwrap_or(Value::Null),
            vars => template_values.get("vars").cloned().unwrap_or(Value::Null),
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
