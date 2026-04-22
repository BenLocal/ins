use anyhow::Context;
use serde::Serialize;

use crate::OutputFormat;
use crate::app::types::AppRecord;
use crate::node::types::{NodeRecord, RemoteNodeRecord};
use crate::store::duck::InstalledServiceRecord;
use crate::volume::types::VolumeRecord;

pub(crate) trait TableRenderable {
    fn headers() -> &'static [&'static str];
    fn row(&self) -> Vec<String>;
}

pub(crate) fn print_structured_list<T>(
    items: &[T],
    output: OutputFormat,
    empty_message: &str,
) -> anyhow::Result<()>
where
    T: Serialize + TableRenderable,
{
    let rendered = render_structured_list(items, output, empty_message)?;
    println!("{rendered}");
    Ok(())
}

fn render_structured_list<T>(
    items: &[T],
    output: OutputFormat,
    empty_message: &str,
) -> anyhow::Result<String>
where
    T: Serialize + TableRenderable,
{
    if items.is_empty() {
        return Ok(empty_message.to_string());
    }

    match output {
        OutputFormat::Json => {
            serde_json::to_string_pretty(items).context("serialize structured output")
        }
        OutputFormat::Table => Ok(render_table(items)),
    }
}

fn render_table<T>(items: &[T]) -> String
where
    T: TableRenderable,
{
    let headers = T::headers();
    let rows = items.iter().map(T::row).collect::<Vec<_>>();
    let widths = column_widths(headers, &rows);

    let header = format_row(headers.iter().copied(), &widths);
    let separator = widths
        .iter()
        .map(|width| "-".repeat(*width))
        .collect::<Vec<_>>()
        .join("  ");
    let body = rows
        .iter()
        .map(|row| format_row(row.iter().map(String::as_str), &widths))
        .collect::<Vec<_>>();

    std::iter::once(header)
        .chain(std::iter::once(separator))
        .chain(body)
        .collect::<Vec<_>>()
        .join("\n")
}

fn column_widths(headers: &[&str], rows: &[Vec<String>]) -> Vec<usize> {
    headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            let row_width = rows
                .iter()
                .filter_map(|row| row.get(index))
                .map(String::len)
                .max()
                .unwrap_or(0);
            header.len().max(row_width)
        })
        .collect()
}

fn format_row<'a>(cells: impl Iterator<Item = &'a str>, widths: &[usize]) -> String {
    cells
        .enumerate()
        .map(|(index, cell)| format!("{cell:<width$}", width = widths[index]))
        .collect::<Vec<_>>()
        .join("  ")
}

impl TableRenderable for NodeRecord {
    fn headers() -> &'static [&'static str] {
        &["name", "type", "ip", "port", "user", "auth"]
    }

    fn row(&self) -> Vec<String> {
        match self {
            NodeRecord::Local() => vec![
                "local".into(),
                "local".into(),
                "-".into(),
                "-".into(),
                "-".into(),
                "-".into(),
            ],
            NodeRecord::Remote(node) => vec![
                node.name.clone(),
                "remote".into(),
                node.ip.clone(),
                node.port.to_string(),
                node.user.clone(),
                remote_auth_label(node),
            ],
        }
    }
}

impl TableRenderable for AppRecord {
    fn headers() -> &'static [&'static str] {
        &["name", "version", "dependencies", "author", "description"]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.name.clone(),
            self.version.clone().unwrap_or_else(|| "-".into()),
            join_or_dash(&self.dependencies),
            app_author_label(self),
            self.description.clone().unwrap_or_else(|| "-".into()),
        ]
    }
}

impl TableRenderable for InstalledServiceRecord {
    fn headers() -> &'static [&'static str] {
        &["service", "app", "node", "workspace", "created_at_ms"]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.service.clone(),
            self.app_name.clone(),
            self.node_name.clone(),
            self.workspace.clone(),
            self.created_at_ms.to_string(),
        ]
    }
}

impl TableRenderable for VolumeRecord {
    fn headers() -> &'static [&'static str] {
        &["name", "node", "type", "detail"]
    }

    fn row(&self) -> Vec<String> {
        vec![
            self.name().into(),
            self.node().into(),
            self.kind_label().into(),
            self.detail_label(),
        ]
    }
}

fn remote_auth_label(node: &RemoteNodeRecord) -> String {
    match &node.key_path {
        Some(path) => format!("key:{path}"),
        None if node.password.is_empty() => "password:<empty>".into(),
        None => "password".into(),
    }
}

fn app_author_label(app: &AppRecord) -> String {
    match (&app.author_name, &app.author_email) {
        (Some(name), Some(email)) => format!("{name} <{email}>"),
        (Some(name), None) => name.clone(),
        (None, Some(email)) => email.clone(),
        (None, None) => "-".into(),
    }
}

fn join_or_dash(items: &[String]) -> String {
    if items.is_empty() {
        "-".into()
    } else {
        items.join(",")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::render_structured_list;
    use crate::OutputFormat;
    use crate::app::types::{AppRecord, AppValue, ScriptHook};
    use crate::node::types::{NodeRecord, RemoteNodeRecord};
    use crate::store::duck::InstalledServiceRecord;

    #[test]
    fn render_structured_list_formats_nodes_as_table() {
        let nodes = vec![
            NodeRecord::Local(),
            NodeRecord::Remote(RemoteNodeRecord {
                name: "node-a".into(),
                ip: "10.0.0.1".into(),
                port: 22,
                user: "root".into(),
                password: "secret".into(),
                key_path: None,
            }),
        ];

        let rendered =
            render_structured_list(&nodes, OutputFormat::Table, "no nodes found").expect("table");

        assert!(rendered.contains("type"));
        assert!(rendered.contains("local"));
        assert!(rendered.contains("node-a"));
        assert!(rendered.contains("password"));
    }

    #[test]
    fn render_structured_list_formats_apps_as_json_when_requested() {
        let apps = vec![AppRecord {
            name: "demo".into(),
            version: Some("1.0.0".into()),
            description: Some("sample".into()),
            author_name: Some("Alice".into()),
            author_email: Some("alice@example.com".into()),
            dependencies: vec!["redis".into()],
            before: ScriptHook::default(),
            after: ScriptHook::default(),
            files: None,
            values: vec![AppValue {
                name: "image".into(),
                value_type: "string".into(),
                description: None,
                value: Some(json!("nginx:latest")),
                default: None,
                options: vec![],
            }],
        }];

        let rendered =
            render_structured_list(&apps, OutputFormat::Json, "no apps found").expect("json");

        assert!(rendered.contains("\"name\": \"demo\""));
        assert!(rendered.contains("\"dependencies\": ["));
    }

    #[test]
    fn render_structured_list_uses_empty_message() {
        let services: Vec<InstalledServiceRecord> = Vec::new();

        let rendered = render_structured_list(&services, OutputFormat::Table, "no services found")
            .expect("empty message");

        assert_eq!(rendered, "no services found");
    }
}
