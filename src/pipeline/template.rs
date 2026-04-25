use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use minijinja::{Environment, UndefinedBehavior, context};
use serde_json::{Map, Value};
use tokio::runtime::Handle;
use tokio::sync::OnceCell;
use tokio::task;
use tokio::time::timeout;

use crate::app::types::AppRecord;
use crate::execution_output::ExecutionOutput;
use crate::node::info::{NodeInfoProbe, gpu::GpuProbe, system::SystemProbe};
use crate::node::types::NodeRecord;
use crate::provider::DeploymentTarget;
use crate::store::duck::InstalledServiceConfigRecord;
use crate::volume::compose::resolve_target_volumes;
use crate::volume::types::VolumeRecord;

use crate::node::list::lookup_node_ips;

use super::node_name;
use super::target::resolve_app_values;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Deferred per-probe cache: each `get_or_init` runs the SSH probe at most
/// once for the lifetime of the cache. Shared across all template renders
/// within a single deployment so the second file's `{{ system_info() }}`
/// reuses the result from the first file's call.
pub(crate) struct ProbeCache {
    node: NodeRecord,
    system: OnceCell<Value>,
    gpu: OnceCell<Value>,
}

impl ProbeCache {
    pub(crate) fn new(node: NodeRecord) -> Self {
        Self {
            node,
            system: OnceCell::new(),
            gpu: OnceCell::new(),
        }
    }

    async fn system(&self) -> Value {
        self.system
            .get_or_init(|| async {
                match timeout(PROBE_TIMEOUT, SystemProbe.probe(&self.node)).await {
                    Ok(Ok(v)) => v,
                    Ok(Err(e)) => {
                        eprintln!("warning: system_info probe failed: {e}");
                        serde_json::json!({})
                    }
                    Err(_) => {
                        eprintln!(
                            "warning: system_info probe timed out after {:?}",
                            PROBE_TIMEOUT
                        );
                        serde_json::json!({})
                    }
                }
            })
            .await
            .clone()
    }

    async fn gpu(&self) -> Value {
        self.gpu
            .get_or_init(|| async {
                match timeout(PROBE_TIMEOUT, GpuProbe.probe(&self.node)).await {
                    Ok(Ok(v)) => v,
                    Ok(Err(e)) => {
                        eprintln!("warning: gpu_info probe failed: {e}");
                        no_gpu_value()
                    }
                    Err(_) => {
                        eprintln!(
                            "warning: gpu_info probe timed out after {:?}",
                            PROBE_TIMEOUT
                        );
                        no_gpu_value()
                    }
                }
            })
            .await
            .clone()
    }
}

/// `{{ node }}` exposes the deploying node's name, IP, and external IP.
/// Local nodes always report `ip=127.0.0.1`; `extern_ip` is the value
/// resolved from `[defaults] local_extern_ip` in config.toml and must be
/// provided by the caller (panics if `None` for a local node). Remote nodes
/// use the registered IP from `nodes.json` for both `ip` and `extern_ip`.
/// Other `RemoteNodeRecord` fields (port, user, password, key_path) are
/// intentionally not exposed — password/key_path are secrets and port/user
/// are rarely useful from inside a container.
fn node_template_value(node: &NodeRecord, local_extern_ip: Option<&str>) -> Value {
    match node {
        NodeRecord::Local() => {
            let extern_ip = local_extern_ip
                .expect("local_extern_ip must be resolved before template build for local node");
            serde_json::json!({
                "name": "local",
                "ip": "127.0.0.1",
                "extern_ip": extern_ip,
            })
        }
        NodeRecord::Remote(remote) => serde_json::json!({
            "name": remote.name,
            "ip": remote.ip,
            "extern_ip": remote.ip,
        }),
    }
}

fn no_gpu_value() -> Value {
    let empty: Vec<String> = Vec::new();
    serde_json::json!({
        "vendor": "none",
        "count": 0,
        "models": empty,
        "driver": Value::Null,
    })
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

#[allow(clippy::too_many_arguments)]
pub(super) fn build_target_template_values(
    target: &DeploymentTarget,
    node: &NodeRecord,
    namespace: &str,
    local_extern_ip: Option<&str>,
    volumes_config: &[VolumeRecord],
    installed_services: &[InstalledServiceConfigRecord],
    nodes: &[NodeRecord],
    output: &ExecutionOutput,
) -> anyhow::Result<Value> {
    let mut template_values = build_template_values(&target.app)?;
    if let Some(obj) = template_values.as_object_mut() {
        obj.insert("service".into(), Value::String(target.service.clone()));
        obj.insert("namespace".into(), Value::String(namespace.to_string()));
        obj.insert("node".into(), node_template_value(node, local_extern_ip));
        let services = build_services_template_value(
            &target.app,
            &target.service,
            namespace,
            installed_services,
            nodes,
            local_extern_ip,
            output,
        )?;
        obj.insert("services".into(), services);
        let volumes_json = resolved_volumes_to_json(&target.app, node, volumes_config)?;
        obj.insert("volumes".into(), volumes_json);
    }
    Ok(template_values)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn print_target_template_values(
    target: &DeploymentTarget,
    node: &NodeRecord,
    namespace: &str,
    local_extern_ip: Option<&str>,
    volumes_config: &[VolumeRecord],
    installed_services: &[InstalledServiceConfigRecord],
    nodes: &[NodeRecord],
    output: &ExecutionOutput,
) -> anyhow::Result<()> {
    let template_values = build_target_template_values(
        target,
        node,
        namespace,
        local_extern_ip,
        volumes_config,
        installed_services,
        nodes,
        output,
    )?;
    debug_print_template_values(&target.app.name, &template_values, output);
    Ok(())
}

pub(super) fn build_services_template_value(
    app: &AppRecord,
    current_service: &str,
    current_namespace: &str,
    installed_services: &[InstalledServiceConfigRecord],
    nodes: &[NodeRecord],
    local_extern_ip: Option<&str>,
    output: &ExecutionOutput,
) -> anyhow::Result<Value> {
    let mut services_map = Map::new();
    for dep in app.parsed_dependencies()? {
        if dep.service == current_service && dep.namespace == current_namespace {
            continue;
        }
        let Some(installed) = installed_services
            .iter()
            .find(|s| s.service == dep.service && s.namespace == dep.namespace)
        else {
            continue;
        };

        let key = if dep.explicit_namespace {
            format!(
                "{}_{}",
                dep.namespace.replace('-', "_"),
                dep.service.replace('-', "_")
            )
        } else {
            dep.service.replace('-', "_")
        };

        let (ip, extern_ip) =
            resolve_dep_node_ips(&installed.node_name, nodes, local_extern_ip, output);

        let app_values: Map<String, Value> = installed
            .app_values
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        services_map.insert(
            key,
            serde_json::json!({
                "service": installed.service,
                "namespace": installed.namespace,
                "app_name": installed.app_name,
                "node_name": installed.node_name,
                "workspace": installed.workspace,
                "ip": ip,
                "extern_ip": extern_ip,
                "values": app_values,
            }),
        );
    }
    Ok(Value::Object(services_map))
}

fn resolve_dep_node_ips(
    node_name: &str,
    nodes: &[NodeRecord],
    local_extern_ip: Option<&str>,
    output: &ExecutionOutput,
) -> (String, String) {
    if let Some(pair) = lookup_node_ips(node_name, nodes, local_extern_ip) {
        return pair;
    }
    output.line(format!(
        "warning: dep node '{node_name}' not found in nodes.json; using node_name as ip/extern_ip fallback"
    ));
    (node_name.to_string(), node_name.to_string())
}

/// Schema of the `{{ system_info() }}` / `{{ gpu_info() }}` Jinja functions.
/// Printed at `check` time so users can discover available probe fields.
/// Keep in sync with the probes registered in [`render_template`].
const PROBE_CATALOG: &[(&str, &[&str])] = &[
    (
        "system_info()",
        &["os", "arch", "kernel", "hostname", "cpus"],
    ),
    ("gpu_info()", &["vendor", "count", "models", "driver"]),
];

pub(super) async fn print_probe_catalog(probe_cache: &Arc<ProbeCache>, output: &ExecutionOutput) {
    output.line("----------------------------");
    output.line("Probe function values (cached per deployment):");
    let system_value = probe_cache.system().await;
    let gpu_value = probe_cache.gpu().await;
    for (func, fields) in PROBE_CATALOG {
        let source = match *func {
            "system_info()" => &system_value,
            "gpu_info()" => &gpu_value,
            _ => continue,
        };
        for field in *fields {
            let rendered = source
                .get(field)
                .map(format_probe_value)
                .unwrap_or_else(|| "<missing>".to_string());
            output.line(format!("      {func}.{field}={rendered}"));
        }
    }
    output.line("----------------------------");
}

fn format_probe_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(format_probe_value).collect();
            format!("[{}]", parts.join(", "))
        }
        other => other.to_string(),
    }
}

fn resolved_volumes_to_json(
    app: &AppRecord,
    node: &NodeRecord,
    volumes_config: &[VolumeRecord],
) -> anyhow::Result<Value> {
    let node_name_str = node_name(node);
    let resolved = resolve_target_volumes(app, node_name_str, volumes_config)?;
    let mut map = Map::new();
    for (name, rv) in resolved {
        let mut driver_opts = Map::new();
        for (k, v) in &rv.driver_opts {
            driver_opts.insert(k.clone(), Value::String(v.clone()));
        }
        map.insert(
            name,
            serde_json::json!({
                "docker_name": rv.docker_name,
                "driver": rv.driver,
                "driver_opts": Value::Object(driver_opts),
            }),
        );
    }
    Ok(Value::Object(map))
}

fn debug_print_template_values(app_name: &str, template_values: &Value, output: &ExecutionOutput) {
    output.line("----------------------------");
    output.line(format!("Template values for app '{app_name}':"));
    for section in [
        "service",
        "namespace",
        "node",
        "services",
        "app",
        "vars",
        "volumes",
    ] {
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

pub(super) fn render_template(
    source: &str,
    template_values: &Value,
    probe_cache: &Arc<ProbeCache>,
) -> anyhow::Result<String> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Lenient);

    {
        let cache = probe_cache.clone();
        env.add_function(
            "system_info",
            move || -> Result<minijinja::Value, minijinja::Error> {
                let value = run_probe_sync(cache.clone(), ProbeKind::System);
                minijinja::Value::from_serialize(&value).pipe(Ok)
            },
        );
    }
    {
        let cache = probe_cache.clone();
        env.add_function(
            "gpu_info",
            move || -> Result<minijinja::Value, minijinja::Error> {
                let value = run_probe_sync(cache.clone(), ProbeKind::Gpu);
                minijinja::Value::from_serialize(&value).pipe(Ok)
            },
        );
    }

    env.add_template("app", source)
        .map_err(|e| anyhow!("add template: {}", e))?;
    let template = env
        .get_template("app")
        .map_err(|e| anyhow!("get template: {}", e))?;
    template
        .render(context! {
            app => template_values.get("app").cloned().unwrap_or(Value::Null),
            vars => template_values.get("vars").cloned().unwrap_or(Value::Null),
            volumes => template_values.get("volumes").cloned().unwrap_or(Value::Null),
            service => template_values.get("service").cloned().unwrap_or(Value::Null),
            namespace => template_values.get("namespace").cloned().unwrap_or(Value::Null),
            node => template_values.get("node").cloned().unwrap_or(Value::Null),
            services => template_values.get("services").cloned().unwrap_or(Value::Null),
        })
        .map_err(|e| anyhow!("render template: {}", e))
}

#[derive(Clone, Copy)]
enum ProbeKind {
    System,
    Gpu,
}

/// Minijinja render is sync; our probes are async. Bridge via the current
/// tokio runtime (the main binary is `#[tokio::main]`, so a handle is
/// always available). `block_in_place` avoids blocking other tasks on the
/// worker thread.
fn run_probe_sync(cache: Arc<ProbeCache>, kind: ProbeKind) -> Value {
    task::block_in_place(|| {
        Handle::current().block_on(async move {
            match kind {
                ProbeKind::System => cache.system().await,
                ProbeKind::Gpu => cache.gpu().await,
            }
        })
    })
}

// Tiny ergonomic helper: `x.pipe(f)` → `f(x)`. Saves one line of binding
// noise in the function-registration closures.
trait Pipe: Sized {
    fn pipe<R>(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }
}
impl<T> Pipe for T {}

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

#[cfg(test)]
#[path = "template_test.rs"]
mod template_test;
