use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::anyhow;
use tokio::fs;
use tokio::task::JoinSet;

use crate::app::types::AppRecord;
use crate::execution_output::ExecutionOutput;
use crate::file::FileTrait;
use crate::file::local::LocalFile;
use crate::file::remote::RemoteFile;
use crate::node::types::NodeRecord;
use crate::provider::DeploymentTarget;
use crate::store::duck::save_deployment_record;
use crate::volume::compose::inject_compose_volumes;
use crate::volume::list::{load_volumes, volumes_file};
use crate::volume::types::{ResolvedVolume, VolumeRecord};

use super::labels::{is_docker_compose_file, maybe_inject_compose_labels};
use super::progress::{CopyAppProgress, CopyProgressSlot};
use super::target::app_qa_file;
use super::template::{
    build_target_template_values, is_template_file, render_template, rendered_template_name,
};
use super::{COPY_CONCURRENCY, PreparedDeployment, node_name};

/// Everything a copy task needs that is constant across files within one target.
#[derive(Clone)]
struct CopyContext {
    app: AppRecord,
    template_values: serde_json::Value,
    node: NodeRecord,
    volumes_config: Vec<VolumeRecord>,
    output: ExecutionOutput,
}

#[allow(dead_code)]
pub async fn copy_prepared_apps_to_workspace(
    home: &Path,
    prepared: &PreparedDeployment,
) -> anyhow::Result<Vec<ResolvedVolume>> {
    let output = ExecutionOutput::stdout();
    copy_prepared_apps_to_workspace_with_output(home, prepared, &output).await
}

pub async fn copy_prepared_apps_to_workspace_with_output(
    home: &Path,
    prepared: &PreparedDeployment,
    output: &ExecutionOutput,
) -> anyhow::Result<Vec<ResolvedVolume>> {
    let volumes_config = load_volumes(&volumes_file(home)).await?;
    copy_apps_to_workspace_with_output(
        home,
        &prepared.targets,
        &prepared.app_home,
        &prepared.workspace,
        &prepared.node,
        &volumes_config,
        &prepared.node_info,
        output,
    )
    .await
}

#[cfg(test)]
pub async fn copy_apps_to_workspace(
    home: &Path,
    targets: &[DeploymentTarget],
    app_home: &Path,
    workspace: &Path,
    node: &NodeRecord,
) -> anyhow::Result<()> {
    let output = ExecutionOutput::stdout();
    copy_apps_to_workspace_with_output(
        home,
        targets,
        app_home,
        workspace,
        node,
        &[],
        &crate::node::info::NodeInfo::default(),
        &output,
    )
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn copy_apps_to_workspace_with_output(
    home: &Path,
    targets: &[DeploymentTarget],
    app_home: &Path,
    workspace: &Path,
    node: &NodeRecord,
    volumes_config: &[VolumeRecord],
    node_info: &crate::node::info::NodeInfo,
    output: &ExecutionOutput,
) -> anyhow::Result<Vec<ResolvedVolume>> {
    output.line("Saving deployment records...");
    for target in targets {
        let source_dir = app_home.join(&target.app.name);
        let qa_file = app_qa_file(&source_dir);
        let qa_yaml = fs::read_to_string(&qa_file)
            .await
            .map_err(|e| anyhow!("read qa file {}: {}", qa_file.display(), e))?;
        output.line(format!(
            "Save deployment record for app '{}' into service '{}'",
            target.app.name, target.service
        ));
        save_deployment_record(home, node, workspace, target, &qa_yaml).await?;
    }

    target_file_for_node(node).create_dir_all(workspace).await?;

    let mut resolved_volumes: Vec<ResolvedVolume> = Vec::new();

    output.line("Copying app files to workspace...");
    for target in targets {
        let source_dir = app_home.join(&target.app.name);
        let target_dir = workspace.join(&target.service);
        let template_values =
            build_target_template_values(target, node, volumes_config, node_info, output)?;

        let ctx = CopyContext {
            app: target.app.clone(),
            template_values,
            node: node.clone(),
            volumes_config: volumes_config.to_vec(),
            output: output.clone(),
        };

        if let Some(progress) = CopyAppProgress::new(
            &target.app.name,
            &target.service,
            &source_dir,
            &target_dir,
            output.echo_enabled(),
        )
        .await?
        {
            let mut batch =
                copy_dir_recursive(&source_dir, &target_dir, &ctx, Some(progress.clone())).await?;
            resolved_volumes.append(&mut batch);
            progress.finish();
        } else {
            output.line(format!(
                "  Copying app '{}' into service '{}' at {}",
                target.app.name,
                target.service,
                target_dir.display()
            ));
            let mut batch = copy_dir_recursive(&source_dir, &target_dir, &ctx, None).await?;
            resolved_volumes.append(&mut batch);
        }
    }

    Ok(dedupe_volumes(resolved_volumes))
}

fn dedupe_volumes(volumes: Vec<ResolvedVolume>) -> Vec<ResolvedVolume> {
    let mut seen = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    for v in volumes {
        if seen.insert(v.docker_name.clone()) {
            result.push(v);
        }
    }
    result
}

async fn copy_dir_recursive(
    source: &Path,
    target: &Path,
    ctx: &CopyContext,
    progress: Option<Arc<CopyAppProgress>>,
) -> anyhow::Result<Vec<ResolvedVolume>> {
    let jobs = collect_copy_jobs(source, target).await?;
    target_file_for_node(&ctx.node)
        .create_dir_all(target)
        .await?;

    if jobs.is_empty() {
        return Ok(Vec::new());
    }

    let mut join_set = JoinSet::new();
    let mut next_job = 0usize;
    let mut available_slots: Vec<usize> = (0..COPY_CONCURRENCY.min(jobs.len())).rev().collect();
    let mut resolved = Vec::new();

    loop {
        while next_job < jobs.len() && !available_slots.is_empty() {
            let slot = available_slots.pop().expect("slot available");
            let job = jobs[next_job].clone();
            let ctx = ctx.clone();
            let slot_progress = progress.as_ref().map(|progress| progress.slot(slot));
            join_set.spawn(async move {
                let result = copy_file_to_workspace(job, &ctx, slot_progress).await;
                (slot, result)
            });
            next_job += 1;
        }

        let Some(joined) = join_set.join_next().await else {
            break;
        };
        let (slot, result) = joined.map_err(|e| anyhow!("copy task join error: {}", e))?;
        let mut batch = result?;
        resolved.append(&mut batch);
        available_slots.push(slot);
    }

    Ok(resolved)
}

async fn copy_file_to_workspace(
    job: CopyJob,
    ctx: &CopyContext,
    progress: Option<CopyProgressSlot>,
) -> anyhow::Result<Vec<ResolvedVolume>> {
    let source_file = LocalFile;
    let target_file = target_file_for_node(&ctx.node);

    if job.render_as_template {
        if let Some(progress) = progress.as_ref() {
            progress.start_template(&job.target_path);
        } else {
            ctx.output.line(format!(
                "    Rendering template {} -> {}",
                job.source_path.display(),
                job.target_path.display()
            ));
        }
        let source = source_file
            .read(&job.source_path, None)
            .await
            .map_err(|e| anyhow!("read template {}: {}", job.source_path.display(), e))?;
        let rendered = render_template(&source, &ctx.template_values)?;
        let rendered = maybe_inject_compose_labels(
            &job.target_path,
            &rendered,
            &ctx.template_values,
            &ctx.node,
        )?;
        let (rendered, resolved) = maybe_inject_compose_volumes(
            &job.target_path,
            rendered,
            &ctx.app,
            &ctx.node,
            &ctx.volumes_config,
        )?;
        let rendered_size = rendered.len() as u64;
        if let Some(progress) = progress.as_ref() {
            progress.begin_write_phase(rendered_size);
        }
        let progress_write = progress.as_ref().map(|progress| progress.write_progress());
        target_file
            .write(&job.target_path, &rendered, progress_write.as_ref())
            .await
            .map_err(|e| anyhow!("write rendered file {}: {}", job.target_path.display(), e))?;
        if let Some(progress) = progress.as_ref() {
            progress.finish_file();
        }
        return Ok(resolved);
    }

    let source_meta = fs::metadata(&job.source_path)
        .await
        .map_err(|e| anyhow!("metadata source file {}: {}", job.source_path.display(), e))?;
    let source_size = source_meta.len();
    if let Some(progress) = progress.as_ref() {
        progress.start_copy(&job.target_path, source_size);
    } else {
        ctx.output.line(format!(
            "    Copying file {} -> {}",
            job.source_path.display(),
            job.target_path.display()
        ));
    }
    let source_bytes = source_file
        .read_bytes(&job.source_path, None)
        .await
        .map_err(|e| anyhow!("read source file {}: {}", job.source_path.display(), e))?;
    if is_docker_compose_file(&job.target_path) {
        let source = String::from_utf8(source_bytes).map_err(|e| {
            anyhow!(
                "read compose file {} as utf-8: {}",
                job.source_path.display(),
                e
            )
        })?;
        let rendered = maybe_inject_compose_labels(
            &job.target_path,
            &source,
            &ctx.template_values,
            &ctx.node,
        )?;
        let (rendered, resolved) = maybe_inject_compose_volumes(
            &job.target_path,
            rendered,
            &ctx.app,
            &ctx.node,
            &ctx.volumes_config,
        )?;
        let rendered_size = rendered.len() as u64;
        if let Some(progress) = progress.as_ref() {
            progress.begin_write_phase(rendered_size);
        }
        let progress_write = progress.as_ref().map(|progress| progress.write_progress());
        target_file
            .write(&job.target_path, &rendered, progress_write.as_ref())
            .await
            .map_err(|e| anyhow!("write compose file {}: {}", job.target_path.display(), e))?;
        if let Some(progress) = progress.as_ref() {
            progress.finish_file();
        }
        return Ok(resolved);
    }
    if let Some(progress) = progress.as_ref() {
        progress.begin_write_phase(source_size);
    }
    let progress_write = progress.as_ref().map(|progress| progress.write_progress());
    target_file
        .write_bytes(&job.target_path, &source_bytes, progress_write.as_ref())
        .await
        .map_err(|e| {
            anyhow!(
                "copy {} to {}: {}",
                job.source_path.display(),
                job.target_path.display(),
                e
            )
        })?;
    if let Some(progress) = progress.as_ref() {
        progress.finish_file();
    }
    Ok(Vec::new())
}

fn maybe_inject_compose_volumes(
    path: &Path,
    content: String,
    app: &AppRecord,
    node: &NodeRecord,
    volumes_config: &[VolumeRecord],
) -> anyhow::Result<(String, Vec<ResolvedVolume>)> {
    if !is_docker_compose_file(path) {
        return Ok((content, Vec::new()));
    }
    let node_name_str = node_name(node).to_string();
    inject_compose_volumes(&content, app, &node_name_str, volumes_config)
}

#[derive(Clone, Debug)]
struct CopyJob {
    source_path: PathBuf,
    target_path: PathBuf,
    render_as_template: bool,
}

async fn collect_copy_jobs(source: &Path, target: &Path) -> anyhow::Result<Vec<CopyJob>> {
    let mut jobs = Vec::new();
    let mut stack = vec![(source.to_path_buf(), target.to_path_buf())];

    while let Some((current_source, current_target)) = stack.pop() {
        let mut entries = fs::read_dir(&current_source)
            .await
            .map_err(|e| anyhow!("read source dir {}: {}", current_source.display(), e))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| anyhow!("iterate source dir {}: {}", current_source.display(), e))?
        {
            let file_name = entry.file_name();
            let source_path = entry.path();
            let file_type = entry
                .file_type()
                .await
                .map_err(|e| anyhow!("read file type {}: {}", source_path.display(), e))?;

            if file_type.is_dir() {
                let target_path = current_target.join(&file_name);
                stack.push((source_path, target_path));
                continue;
            }

            let file_name = file_name.to_string_lossy().to_string();
            let target_path = if is_template_file(&file_name) {
                current_target.join(rendered_template_name(&file_name))
            } else {
                current_target.join(&file_name)
            };
            jobs.push(CopyJob {
                source_path,
                target_path,
                render_as_template: is_template_file(&file_name),
            });
        }
    }

    Ok(jobs)
}

pub(super) fn target_file_for_node(node: &NodeRecord) -> Box<dyn FileTrait> {
    match node {
        NodeRecord::Local() => Box::new(LocalFile),
        NodeRecord::Remote(remote) => {
            let remote_file = RemoteFile::new(
                remote.ip.clone(),
                remote.port,
                remote.user.clone(),
                remote.password.clone(),
            );
            let remote_file = if let Some(key_path) = &remote.key_path {
                remote_file.with_key_path(key_path.clone())
            } else {
                remote_file
            };
            Box::new(remote_file)
        }
    }
}
