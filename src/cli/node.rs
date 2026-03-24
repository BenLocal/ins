use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use async_trait::async_trait;
use tokio::fs;

use crate::cli::{CommandContext, CommandTrait};
use crate::node::list::{load_all_nodes, load_remote_nodes};
use crate::node::types::RemoteNodeRecord;

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
            NodeSubcommand::List(_args) => list_nodes(&nodes_path).await,
        }
    }
}

async fn add_node(nodes_path: &PathBuf, args: NodeAddArgs) -> anyhow::Result<()> {
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
    println!("node add");
    Ok(())
}

async fn set_node(nodes_path: &PathBuf, args: NodeSetArgs) -> anyhow::Result<()> {
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
    println!("node set");
    Ok(())
}

async fn list_nodes(nodes_path: &Path) -> anyhow::Result<()> {
    let nodes = load_all_nodes(nodes_path).await?;
    println!("{}", serde_json::to_string_pretty(&nodes)?);
    Ok(())
}

pub(crate) fn nodes_file(home: &Path) -> PathBuf {
    home.join("nodes.json")
}

async fn save_nodes(nodes_path: &PathBuf, nodes: &[RemoteNodeRecord]) -> anyhow::Result<()> {
    let content = serde_json::to_string_pretty(nodes)?;
    fs::write(nodes_path, format!("{content}\n"))
        .await
        .with_context(|| format!("write nodes file {}", nodes_path.display()))?;
    Ok(())
}
