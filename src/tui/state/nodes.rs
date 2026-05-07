use anyhow::bail;

use crate::{
    cli::node::{
        NodeAddArgs, NodeSetArgs, add_node_record, delete_node_record, list_node_records,
        nodes_file, set_node_record,
    },
    node::types::{NodeRecord, RemoteNodeRecord},
    tui::state::{ActiveSection, OverlayState, TuiState, clamp_index},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeFormMode {
    Add,
    Edit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeFormField {
    Name,
    Ip,
    Port,
    User,
    Password,
    KeyPath,
}

impl NodeFormField {
    pub const ALL: [Self; 6] = [
        Self::Name,
        Self::Ip,
        Self::Port,
        Self::User,
        Self::Password,
        Self::KeyPath,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Ip => "IP",
            Self::Port => "Port",
            Self::User => "User",
            Self::Password => "Password",
            Self::KeyPath => "Key Path",
        }
    }
}

#[derive(Clone, Debug)]
pub struct NodeFormState {
    pub mode: NodeFormMode,
    pub active_field: NodeFormField,
    pub name: String,
    pub ip: String,
    pub port: String,
    pub user: String,
    pub password: String,
    pub key_path: String,
}

#[derive(Clone, Debug)]
pub struct NodeFormInput {
    pub mode: NodeFormMode,
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub key_path: Option<String>,
}

impl NodeFormState {
    pub fn blank(mode: NodeFormMode) -> Self {
        Self {
            mode,
            active_field: NodeFormField::Name,
            name: String::new(),
            ip: String::new(),
            port: "22".into(),
            user: "root".into(),
            password: String::new(),
            key_path: String::new(),
        }
    }

    pub fn for_node(node: &RemoteNodeRecord) -> Self {
        Self {
            mode: NodeFormMode::Edit,
            active_field: NodeFormField::Ip,
            name: node.name.clone(),
            ip: node.ip.clone(),
            port: node.port.to_string(),
            user: node.user.clone(),
            password: node.password.clone(),
            key_path: node.key_path.clone().unwrap_or_default(),
        }
    }

    pub fn active_value_mut(&mut self) -> &mut String {
        match self.active_field {
            NodeFormField::Name => &mut self.name,
            NodeFormField::Ip => &mut self.ip,
            NodeFormField::Port => &mut self.port,
            NodeFormField::User => &mut self.user,
            NodeFormField::Password => &mut self.password,
            NodeFormField::KeyPath => &mut self.key_path,
        }
    }

    pub fn next_field(&mut self) {
        let index = NodeFormField::ALL
            .iter()
            .position(|field| *field == self.active_field)
            .unwrap_or(0);
        self.active_field = NodeFormField::ALL[(index + 1) % NodeFormField::ALL.len()];
    }

    pub fn previous_field(&mut self) {
        let index = NodeFormField::ALL
            .iter()
            .position(|field| *field == self.active_field)
            .unwrap_or(0);
        self.active_field =
            NodeFormField::ALL[(index + NodeFormField::ALL.len() - 1) % NodeFormField::ALL.len()];
    }
}

pub fn node_label(node: &NodeRecord) -> String {
    match node {
        NodeRecord::Local() => "local".into(),
        NodeRecord::Remote(node) => format!("{} ({})", node.name, node.ip),
    }
}

pub fn node_detail(node: &NodeRecord) -> String {
    match node {
        NodeRecord::Local() => "name: local\ntype: local".into(),
        NodeRecord::Remote(node) => format!(
            "name: {}\ntype: remote\nip: {}\nport: {}\nuser: {}\nauth: {}",
            node.name,
            node.ip,
            node.port,
            node.user,
            node.key_path
                .as_ref()
                .map(|path| format!("key:{path}"))
                .unwrap_or_else(|| {
                    if node.password.is_empty() {
                        "password:<empty>".into()
                    } else {
                        "password".into()
                    }
                })
        ),
    }
}

impl TuiState {
    pub fn open_add_node_form(&mut self) {
        self.overlay = Some(OverlayState::NodeForm(NodeFormState::blank(
            NodeFormMode::Add,
        )));
        self.status = Some("Adding a new node".into());
    }

    pub fn open_edit_node_form(&mut self) -> anyhow::Result<()> {
        let Some(node) = self.nodes.get(self.node_index) else {
            bail!("no node selected");
        };

        match node {
            NodeRecord::Local() => bail!("local node cannot be edited"),
            NodeRecord::Remote(node) => {
                self.overlay = Some(OverlayState::NodeForm(NodeFormState::for_node(node)));
                self.status = Some(format!("Editing node '{}'", node.name));
                Ok(())
            }
        }
    }

    pub fn open_delete_node_confirmation(&mut self) -> anyhow::Result<()> {
        let Some(node) = self.nodes.get(self.node_index) else {
            bail!("no node selected");
        };

        match node {
            NodeRecord::Local() => bail!("local node cannot be deleted"),
            NodeRecord::Remote(node) => {
                self.overlay = Some(OverlayState::DeleteConfirm(super::DeleteTarget::Node {
                    name: node.name.clone(),
                }));
                self.status = Some(format!("Confirm delete for node '{}'", node.name));
                Ok(())
            }
        }
    }

    pub fn node_form(&self) -> Option<&NodeFormState> {
        match self.overlay.as_ref() {
            Some(OverlayState::NodeForm(form)) => Some(form),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn node_form_mode(&self) -> Option<NodeFormMode> {
        self.node_form().map(|form| form.mode)
    }

    pub async fn apply_node_form(&mut self, input: NodeFormInput) -> anyhow::Result<()> {
        let nodes_path = nodes_file(&self.home);
        match input.mode {
            NodeFormMode::Add => {
                add_node_record(
                    nodes_path.as_path(),
                    NodeAddArgs {
                        name: input.name.clone(),
                        ip: input.ip,
                        port: input.port,
                        user: input.user,
                        password: input.password,
                        key_path: input.key_path,
                    },
                )
                .await?;
            }
            NodeFormMode::Edit => {
                set_node_record(
                    nodes_path.as_path(),
                    NodeSetArgs {
                        name: input.name.clone(),
                        ip: input.ip,
                        port: input.port,
                        user: input.user,
                        password: input.password,
                        key_path: input.key_path,
                    },
                )
                .await?;
            }
        }

        self.reload_nodes().await?;
        self.node_index = self
            .nodes
            .iter()
            .position(
                |node| matches!(node, NodeRecord::Remote(remote) if remote.name == input.name),
            )
            .unwrap_or(0);
        self.overlay = None;
        self.active_section = ActiveSection::Nodes;
        self.status = Some(match input.mode {
            NodeFormMode::Add => format!("Added node '{}'", input.name),
            NodeFormMode::Edit => format!("Updated node '{}'", input.name),
        });
        self.sync_selection();
        Ok(())
    }

    pub async fn delete_node(&mut self, name: String) -> anyhow::Result<()> {
        delete_node_record(nodes_file(&self.home).as_path(), &name).await?;
        self.reload_nodes().await?;
        self.node_index = clamp_index(self.node_index, self.nodes.len());
        self.overlay = None;
        self.active_section = ActiveSection::Nodes;
        self.status = Some(format!("Deleted node '{name}'"));
        self.sync_selection();
        Ok(())
    }

    pub async fn reload_nodes(&mut self) -> anyhow::Result<()> {
        self.nodes = list_node_records(&nodes_file(&self.home)).await?;
        self.node_details = self.nodes.iter().map(node_detail).collect();
        Ok(())
    }

    pub fn build_node_input_from_form(form: NodeFormState) -> anyhow::Result<NodeFormInput> {
        crate::node::persist::parse_node_form(form)
    }
}
