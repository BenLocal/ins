use anyhow::{Context, anyhow};
use async_trait::async_trait;
use tokio::process::Command;

use crate::cli::node::nodes_file;
use crate::cli::{CommandContext, CommandTrait};
use crate::env::shell_quote;
use crate::file::remote::RemoteFile;
use crate::node::list::load_all_nodes;
use crate::node::types::{NodeRecord, RemoteNodeRecord};

#[derive(clap::Args, Clone, Debug)]
/// Run a docker command on the selected node.
pub struct DockerArgs {
    /// Target node name (defaults to `local`).
    #[arg(short, long)]
    pub node: Option<String>,
    /// Arguments forwarded to `docker` on the node, e.g. `ps -a`.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

pub struct DockerCommand;

#[async_trait]
impl CommandTrait for DockerCommand {
    type Args = DockerArgs;

    async fn run(args: DockerArgs, ctx: CommandContext) -> anyhow::Result<()> {
        if args.args.is_empty() {
            return Err(anyhow!(
                "no docker arguments provided (e.g. `ins docker --node local ps -a`)"
            ));
        }

        let nodes = load_all_nodes(&nodes_file(&ctx.home)).await?;
        let node = resolve_node(&nodes, args.node.as_deref())?;

        match node {
            NodeRecord::Local() => run_local(&args.args).await,
            NodeRecord::Remote(remote) => run_remote(&remote, &args.args).await,
        }
    }
}

fn resolve_node(nodes: &[NodeRecord], requested: Option<&str>) -> anyhow::Result<NodeRecord> {
    let name = requested.unwrap_or("local");
    nodes
        .iter()
        .find(|record| match record {
            NodeRecord::Local() => name == "local",
            NodeRecord::Remote(remote) => remote.name == name,
        })
        .cloned()
        .ok_or_else(|| anyhow!("node '{}' not found", name))
}

async fn run_local(args: &[String]) -> anyhow::Result<()> {
    let status = Command::new("docker")
        .args(args)
        .status()
        .await
        .with_context(|| format!("run 'docker {}' locally", args.join(" ")))?;
    if !status.success() {
        return Err(anyhow!(
            "docker {} exited with status {:?}",
            args.join(" "),
            status.code()
        ));
    }
    Ok(())
}

async fn run_remote(remote: &RemoteNodeRecord, args: &[String]) -> anyhow::Result<()> {
    let mut parts = vec!["docker".to_string()];
    for arg in args {
        parts.push(shell_quote(arg));
    }
    let command = parts.join(" ");

    let remote_file = build_remote_file(remote);
    let output = remote_file
        .tty_exec(&command)
        .await
        .with_context(|| format!("run 'docker {}' on node '{}'", args.join(" "), remote.name))?;
    if output.exit_status != 0 {
        return Err(anyhow!(
            "docker {} on node '{}' exited with status {}",
            args.join(" "),
            remote.name,
            output.exit_status
        ));
    }
    Ok(())
}

fn build_remote_file(remote: &RemoteNodeRecord) -> RemoteFile {
    let remote_file = RemoteFile::new(
        remote.ip.clone(),
        remote.port,
        remote.user.clone(),
        remote.password.clone(),
    );
    if let Some(key_path) = &remote.key_path {
        remote_file.with_key_path(key_path.clone())
    } else {
        remote_file
    }
}

#[cfg(test)]
#[path = "docker_test.rs"]
mod docker_test;
