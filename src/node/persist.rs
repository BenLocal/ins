use anyhow::Context;

// Re-exported for use by `src/web/handlers/nodes.rs` (Task 11).
// pub(crate) (not pub) because the underlying items in cli::node are pub(crate).
#[allow(unused_imports)]
pub(crate) use crate::cli::node::{
    NodeAddArgs, NodeSetArgs, add_node_record, delete_node_record, list_node_records, nodes_file,
    set_node_record,
};
use crate::tui::state::{NodeFormInput, NodeFormState};

pub fn parse_node_form(form: NodeFormState) -> anyhow::Result<NodeFormInput> {
    let port = form
        .port
        .trim()
        .parse::<u16>()
        .with_context(|| format!("invalid port '{}'", form.port.trim()))?;
    Ok(NodeFormInput {
        mode: form.mode,
        name: form.name.trim().into(),
        ip: form.ip.trim().into(),
        port,
        user: form.user.trim().into(),
        password: form.password,
        key_path: normalize_optional(&form.key_path),
    })
}

fn normalize_optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
#[path = "persist_test.rs"]
mod persist_test;
