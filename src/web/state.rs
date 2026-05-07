use std::path::PathBuf;
use std::sync::Arc;

use crate::config::InsConfig;
use crate::web::jobs::JobRegistry;

#[derive(Clone)]
#[allow(dead_code)]
pub struct AppState {
    pub home: Arc<PathBuf>,
    pub config: Arc<InsConfig>,
    pub jobs: Arc<JobRegistry>,
    pub token: Option<Arc<String>>,
    pub templates: Arc<minijinja::Environment<'static>>,
}

impl AppState {
    #[allow(dead_code)]
    pub fn app_home(&self) -> PathBuf {
        match self.config.app_home_override() {
            Some(path) => {
                let p = PathBuf::from(path);
                if p.is_absolute() {
                    p
                } else {
                    self.home.join(p)
                }
            }
            None => self.home.join("app"),
        }
    }
}
