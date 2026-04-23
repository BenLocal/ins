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
#[path = "output_test.rs"]
mod output_test;
