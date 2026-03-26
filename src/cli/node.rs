use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use async_trait::async_trait;
use tokio::fs;

use crate::cli::{CommandContext, CommandTrait};
use crate::node::list::{load_all_nodes, load_remote_nodes};
use crate::node::types::RemoteNodeRecord;
use crate::output::print_structured_list;

#[derive(clap::Args, Clone, Debug)]
/// Manage nodes in the cluster.
pub struct NodeArgs {
    #[command(subcommand)]
    pub command: NodeSubcommand,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum NodeSubcommand {
    /// Add a node.
    Add(NodeAddArgs),
    /// Update a node.
    Set(NodeSetArgs),
    /// List nodes.
    List(NodeListArgs),
}

#[derive(clap::Args, Clone, Debug)]
/// Add a node to the cluster.
pub struct NodeAddArgs {
    /// Node name
    #[arg(short, long)]
    pub name: String,
    /// Node IP address
    #[arg(short, long)]
    pub ip: String,
    /// Node port, default is 22
    #[arg(short, long, default_value = "22")]
    pub port: u16,
    /// Node user, default is root
    #[arg(long, default_value = "root")]
    pub user: String,
    /// Node password, or private key passphrase when used with --key-path
    #[arg(long, default_value = "")]
    pub password: String,
    /// Private key path for SSH authentication
    #[arg(long)]
    pub key_path: Option<String>,
}

#[derive(clap::Args, Clone, Debug)]
/// Update a node in the cluster.
pub struct NodeSetArgs {
    /// Node name
    #[arg(short, long)]
    pub name: String,
    /// Node IP address
    #[arg(short, long)]
    pub ip: String,
    /// Node port, default is 22
    #[arg(short, long, default_value = "22")]
    pub port: u16,
    /// Node user, default is root
    #[arg(long, default_value = "root")]
    pub user: String,
    /// Node password, or private key passphrase when used with --key-path
    #[arg(long, default_value = "")]
    pub password: String,
    /// Private key path for SSH authentication
    #[arg(long)]
    pub key_path: Option<String>,
}

#[derive(clap::Args, Clone, Debug, Default)]
/// List cluster nodes.
pub struct NodeListArgs {}

pub struct NodeCommand;

#[async_trait]
impl CommandTrait for NodeCommand {
    type Args = NodeArgs;

    async fn run(args: NodeArgs, ctx: CommandContext) -> anyhow::Result<()> {
        let nodes_path = nodes_file(&ctx.home);

        match args.command {
            NodeSubcommand::Add(args) => add_node(&nodes_path, args).await,
            NodeSubcommand::Set(args) => set_node(&nodes_path, args).await,
            NodeSubcommand::List(_args) => list_nodes(&nodes_path, ctx.output).await,
        }
    }
}

async fn add_node(nodes_path: &Path, args: NodeAddArgs) -> anyhow::Result<()> {
    add_node_record(nodes_path, args).await?;
    println!("node add");
    Ok(())
}

pub(crate) async fn add_node_record(nodes_path: &Path, args: NodeAddArgs) -> anyhow::Result<()> {
    let mut nodes = load_remote_nodes(nodes_path).await?;

    if nodes.iter().any(|node| node.name == args.name) {
        bail!("node '{}' already exists", args.name);
    }

    nodes.push(RemoteNodeRecord {
        name: args.name,
        ip: args.ip,
        port: args.port,
        user: args.user,
        password: args.password,
        key_path: args.key_path,
    });

    save_nodes(nodes_path, &nodes).await?;
    Ok(())
}

async fn set_node(nodes_path: &Path, args: NodeSetArgs) -> anyhow::Result<()> {
    set_node_record(nodes_path, args).await?;
    println!("node set");
    Ok(())
}

pub(crate) async fn set_node_record(nodes_path: &Path, args: NodeSetArgs) -> anyhow::Result<()> {
    let mut nodes = load_remote_nodes(nodes_path).await?;
    let Some(node) = nodes.iter_mut().find(|node| node.name == args.name) else {
        bail!("node '{}' not found", args.name);
    };

    node.ip = args.ip;
    node.port = args.port;
    node.user = args.user;
    node.password = args.password;
    node.key_path = args.key_path;

    save_nodes(nodes_path, &nodes).await?;
    Ok(())
}

pub(crate) async fn delete_node_record(nodes_path: &Path, name: &str) -> anyhow::Result<()> {
    let mut nodes = load_remote_nodes(nodes_path).await?;
    let original_len = nodes.len();
    nodes.retain(|node| node.name != name);

    if nodes.len() == original_len {
        bail!("node '{}' not found", name);
    }

    save_nodes(nodes_path, &nodes).await?;
    Ok(())
}

async fn list_nodes(nodes_path: &Path, output: crate::OutputFormat) -> anyhow::Result<()> {
    let nodes = list_node_records(nodes_path).await?;
    print_structured_list(&nodes, output, "no nodes found")
}

pub(crate) fn nodes_file(home: &Path) -> PathBuf {
    home.join("nodes.json")
}

pub(crate) async fn list_node_records(
    nodes_path: &Path,
) -> anyhow::Result<Vec<crate::node::types::NodeRecord>> {
    load_all_nodes(nodes_path).await
}

async fn save_nodes(nodes_path: &Path, nodes: &[RemoteNodeRecord]) -> anyhow::Result<()> {
    let content = serde_json::to_string_pretty(nodes)?;
    fs::write(nodes_path, format!("{content}\n"))
        .await
        .with_context(|| format!("write nodes file {}", nodes_path.display()))?;
    Ok(())
}
