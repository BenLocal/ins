use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use rand::RngCore;
use tokio::sync::RwLock;

use crate::config::InsConfig;
use crate::execution_output::ExecutionOutput;
use crate::pipeline::PipelineMode;
use crate::store::duck::InstalledServiceRecord;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum JobState {
    Running,
    Done(Result<(), String>),
}

#[allow(dead_code)]
pub struct Job {
    pub id: String,
    pub mode: PipelineMode,
    pub service: InstalledServiceRecord,
    pub output: ExecutionOutput,
    pub state: Arc<RwLock<JobState>>,
    pub started_at: DateTime<Utc>,
}

#[derive(Default)]
pub struct JobRegistry {
    jobs: RwLock<VecDeque<Arc<Job>>>,
}

#[allow(dead_code)]
impl JobRegistry {
    pub async fn get(&self, id: &str) -> Option<Arc<Job>> {
        self.jobs.read().await.iter().find(|j| j.id == id).cloned()
    }

    pub async fn spawn(
        self: &Arc<Self>,
        mode: PipelineMode,
        service: InstalledServiceRecord,
        home: PathBuf,
        config: Arc<InsConfig>,
    ) -> Arc<Job> {
        let id = next_job_id();
        let job = Arc::new(Job {
            id,
            mode,
            service: service.clone(),
            output: ExecutionOutput::streaming(),
            state: Arc::new(RwLock::new(JobState::Running)),
            started_at: Utc::now(),
        });
        self.push(job.clone()).await;

        let task = job.clone();
        tokio::spawn(async move {
            let title = match mode {
                PipelineMode::Check => "Starting check...",
                PipelineMode::Deploy => "Starting deployment...",
            };
            let result = async {
                let prepared = crate::pipeline::prepare_installed_service_deployment(
                    &home,
                    &config,
                    None,
                    &task.service,
                )
                .await?;
                crate::pipeline::execute_pipeline_with_output(
                    &home,
                    prepared,
                    title,
                    mode,
                    task.output.clone(),
                )
                .await
            }
            .await;
            let mut state = task.state.write().await;
            match &result {
                Ok(()) => {
                    *state = JobState::Done(Ok(()));
                    task.output.line("[ins:done] ok");
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    *state = JobState::Done(Err(msg.clone()));
                    task.output.line(format!("[ins:done] err: {msg}"));
                }
            }
        });
        job
    }

    async fn push(&self, job: Arc<Job>) {
        let mut q = self.jobs.write().await;
        q.push_back(job);
        while q.len() > 20 {
            let evict_idx = q
                .iter()
                .position(|j| {
                    j.state
                        .try_read()
                        .map(|s| matches!(*s, JobState::Done(_)))
                        .unwrap_or(false)
                })
                .unwrap_or(0);
            q.remove(evict_idx);
        }
    }
}

pub(crate) fn next_job_id() -> String {
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 4];
    rng.fill_bytes(&mut bytes);
    let suffix: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let now = Utc::now().format("%Y%m%d-%H%M%S");
    format!("{now}-{suffix}")
}

#[cfg(test)]
#[path = "jobs_test.rs"]
mod jobs_test;
