use std::path::PathBuf;

use anyhow::{Context, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::cli::{CommandContext, CommandTrait};

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
    /// Node password
    #[arg(long)]
    pub password: String,
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
    /// Node password
    #[arg(long)]
    pub password: String,
}

#[derive(clap::Args, Clone, Debug, Default)]
/// List cluster nodes.
pub struct NodeListArgs {}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct NodeRecord {
    name: String,
    ip: String,
    port: u16,
    user: String,
    password: String,
}

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
    let mut nodes = load_nodes(nodes_path).await?;

    if nodes.iter().any(|node| node.name == args.name) {
        bail!("node '{}' already exists", args.name);
    }

    nodes.push(NodeRecord {
        name: args.name,
        ip: args.ip,
        port: args.port,
        user: args.user,
        password: args.password,
    });

    save_nodes(nodes_path, &nodes).await?;
    println!("node add");
    Ok(())
}

async fn set_node(nodes_path: &PathBuf, args: NodeSetArgs) -> anyhow::Result<()> {
    let mut nodes = load_nodes(nodes_path).await?;
    let Some(node) = nodes.iter_mut().find(|node| node.name == args.name) else {
        bail!("node '{}' not found", args.name);
    };

    node.ip = args.ip;
    node.port = args.port;
    node.user = args.user;
    node.password = args.password;

    save_nodes(nodes_path, &nodes).await?;
    println!("node set");
    Ok(())
}

async fn list_nodes(nodes_path: &PathBuf) -> anyhow::Result<()> {
    let nodes = load_nodes(nodes_path).await?;
    println!("{}", serde_json::to_string_pretty(&nodes)?);
    Ok(())
}

fn nodes_file(home: &PathBuf) -> PathBuf {
    home.join("nodes.json")
}

async fn load_nodes(nodes_path: &PathBuf) -> anyhow::Result<Vec<NodeRecord>> {
    if !fs::try_exists(nodes_path)
        .await
        .with_context(|| format!("check nodes file {}", nodes_path.display()))?
    {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(nodes_path)
        .await
        .with_context(|| format!("read nodes file {}", nodes_path.display()))?;

    if content.trim().is_empty() {
        return Ok(Vec::new());
    }

    Ok(serde_json::from_str(&content)
        .with_context(|| format!("parse nodes file {}", nodes_path.display()))?)
}

async fn save_nodes(nodes_path: &PathBuf, nodes: &[NodeRecord]) -> anyhow::Result<()> {
    let content = serde_json::to_string_pretty(nodes)?;
    fs::write(nodes_path, format!("{content}\n"))
        .await
        .with_context(|| format!("write nodes file {}", nodes_path.display()))?;
    Ok(())
}
