use std::path::Path;

use anyhow::Context;
use async_trait::async_trait;

use crate::cli::{CommandContext, CommandTrait};
use crate::node::list::load_all_nodes;
use crate::node::types::NodeRecord;
use crate::output::print_structured_list;
use crate::volume::list::{load_volumes, volumes_file};

#[derive(clap::Args, Clone, Debug)]
/// Manage per-node Docker volume backings.
pub struct VolumeArgs {
    #[command(subcommand)]
    pub command: VolumeSubcommand,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum VolumeSubcommand {
    /// Add a volume backing for a node.
    Add(VolumeAddArgs),
    /// Update an existing volume backing.
    Set(VolumeSetArgs),
    /// Delete a volume backing.
    Delete(VolumeDeleteArgs),
    /// List configured volumes.
    List(VolumeListArgs),
}

#[derive(clap::Args, Clone, Debug, Default)]
pub struct VolumeListArgs {}

pub struct VolumeCommand;

#[async_trait]
impl CommandTrait for VolumeCommand {
    type Args = VolumeArgs;

    async fn run(args: VolumeArgs, ctx: CommandContext) -> anyhow::Result<()> {
        let path = volumes_file(&ctx.home);
        match args.command {
            VolumeSubcommand::Add(add_args) => add_volume(&ctx.home, &path, add_args).await,
            VolumeSubcommand::Set(set_args) => set_volume(&ctx.home, &path, set_args).await,
            VolumeSubcommand::Delete(delete_args) => delete_volume_cmd(&path, delete_args).await,
            VolumeSubcommand::List(_) => list_volumes(&path, ctx.output).await,
        }
    }
}

async fn list_volumes(path: &Path, output: crate::OutputFormat) -> anyhow::Result<()> {
    let volumes = load_volumes(path).await?;
    print_structured_list(&volumes, output, "no volumes found")
}

#[derive(clap::Args, Clone, Debug)]
pub struct VolumeAddArgs {
    #[command(subcommand)]
    pub kind: VolumeTypeArgs,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum VolumeTypeArgs {
    /// Local filesystem bind mount on the node.
    Filesystem(FilesystemVolumeArgs),
    /// SMB/CIFS remote share mounted on the node.
    Cifs(CifsVolumeArgs),
}

#[derive(clap::Args, Clone, Debug)]
pub struct FilesystemVolumeArgs {
    /// Logical volume name referenced from compose files.
    #[arg(short, long)]
    pub name: String,
    /// Target node name.
    #[arg(long)]
    pub node: String,
    /// Absolute path on the node to bind-mount.
    #[arg(long)]
    pub path: String,
}

#[derive(clap::Args, Clone, Debug)]
pub struct CifsVolumeArgs {
    /// Logical volume name referenced from compose files.
    #[arg(short, long)]
    pub name: String,
    /// Target node name.
    #[arg(long)]
    pub node: String,
    /// SMB share in the form //server/share.
    #[arg(long)]
    pub server: String,
    /// SMB username.
    #[arg(long)]
    pub username: String,
    /// SMB password (stored in plaintext, matching the nodes.json convention).
    #[arg(long)]
    pub password: String,
}

async fn add_volume(home: &Path, path: &Path, args: VolumeAddArgs) -> anyhow::Result<()> {
    match args.kind {
        VolumeTypeArgs::Filesystem(fs) => {
            validate_name(&fs.name)?;
            validate_node(home, &fs.node).await?;
            validate_filesystem_path(&fs.path)?;
            crate::volume::list::add_filesystem(path, &fs.name, &fs.node, &fs.path).await?;
            println!("volume add filesystem");
            Ok(())
        }
        VolumeTypeArgs::Cifs(cifs) => {
            validate_name(&cifs.name)?;
            validate_node(home, &cifs.node).await?;
            validate_cifs_server(&cifs.server)?;
            crate::volume::list::add_cifs(
                path,
                &cifs.name,
                &cifs.node,
                &cifs.server,
                &cifs.username,
                &cifs.password,
            )
            .await?;
            println!("volume add cifs");
            Ok(())
        }
    }
}

fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        anyhow::bail!("volume name cannot be empty");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        anyhow::bail!(
            "volume name '{}' contains invalid characters (allowed: a-z A-Z 0-9 _ -)",
            name
        );
    }
    Ok(())
}

async fn validate_node(home: &Path, node: &str) -> anyhow::Result<()> {
    let nodes = load_all_nodes(&crate::cli::node::nodes_file(home))
        .await
        .with_context(|| format!("validate node '{}'", node))?;
    let exists = nodes.iter().any(|record| match record {
        NodeRecord::Local() => node == "local",
        NodeRecord::Remote(remote) => remote.name == node,
    });
    if !exists {
        anyhow::bail!("node '{}' not found", node);
    }
    Ok(())
}

fn validate_filesystem_path(path: &str) -> anyhow::Result<()> {
    if path.is_empty() {
        anyhow::bail!("filesystem path cannot be empty");
    }
    if !path.starts_with('/') {
        anyhow::bail!("filesystem path must be absolute (got '{}')", path);
    }
    Ok(())
}

fn validate_cifs_server(server: &str) -> anyhow::Result<()> {
    if !server.starts_with("//") {
        anyhow::bail!("cifs server must start with '//' (got '{}')", server);
    }
    Ok(())
}

#[derive(clap::Args, Clone, Debug)]
pub struct VolumeSetArgs {
    #[command(subcommand)]
    pub kind: VolumeTypeArgs,
}

#[derive(clap::Args, Clone, Debug)]
pub struct VolumeDeleteArgs {
    /// Logical volume name.
    #[arg(short, long)]
    pub name: String,
    /// Target node name.
    #[arg(long)]
    pub node: String,
}

async fn set_volume(home: &Path, path: &Path, args: VolumeSetArgs) -> anyhow::Result<()> {
    match args.kind {
        VolumeTypeArgs::Filesystem(fs) => {
            validate_name(&fs.name)?;
            validate_node(home, &fs.node).await?;
            validate_filesystem_path(&fs.path)?;
            crate::volume::list::set_filesystem(path, &fs.name, &fs.node, &fs.path).await?;
            println!("volume set filesystem");
            Ok(())
        }
        VolumeTypeArgs::Cifs(cifs) => {
            validate_name(&cifs.name)?;
            validate_node(home, &cifs.node).await?;
            validate_cifs_server(&cifs.server)?;
            crate::volume::list::set_cifs(
                path,
                &cifs.name,
                &cifs.node,
                &cifs.server,
                &cifs.username,
                &cifs.password,
            )
            .await?;
            println!("volume set cifs");
            Ok(())
        }
    }
}

async fn delete_volume_cmd(path: &Path, args: VolumeDeleteArgs) -> anyhow::Result<()> {
    validate_name(&args.name)?;
    crate::volume::list::delete_volume(path, &args.name, &args.node).await?;
    println!("volume delete");
    Ok(())
}
