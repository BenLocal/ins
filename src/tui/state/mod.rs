mod apps;
mod nodes;
mod services;

use anyhow::Context;

pub use apps::{
    AppCreateField, AppCreateFormState, AppCreateKind, AppTextEditorState, AppViewState,
};
pub use nodes::node_detail;
pub use nodes::{NodeFormField, NodeFormInput, NodeFormMode, NodeFormState};
pub use services::service_detail;

use apps::{app_file_label, app_label, load_app_details};
use nodes::node_label;
use services::service_label;

use crate::pipeline::PipelineMode;
use crate::{app::types::AppRecord, node::types::NodeRecord, store::duck::InstalledServiceRecord};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveSection {
    Nodes,
    Apps,
    Services,
}

impl ActiveSection {
    fn next(self) -> Self {
        match self {
            Self::Nodes => Self::Apps,
            Self::Apps => Self::Services,
            Self::Services => Self::Nodes,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Nodes => Self::Services,
            Self::Apps => Self::Nodes,
            Self::Services => Self::Apps,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Nodes => "Nodes",
            Self::Apps => "Apps",
            Self::Services => "Services",
        }
    }
}

#[derive(Clone, Debug)]
pub enum DeleteTarget {
    Node {
        name: String,
    },
    AppFile {
        app_name: String,
        relative_path: String,
    },
}

#[derive(Clone, Debug)]
pub enum OverlayState {
    NodeForm(NodeFormState),
    DeleteConfirm(DeleteTarget),
    QuitConfirm,
    AppCreateForm(AppCreateFormState),
    AppTextEditor(AppTextEditorState),
    ServiceActionConfirm(ServiceActionState),
    ServiceActionResult(ServiceActionResultState),
}

#[derive(Clone, Debug)]
pub struct ServiceActionState {
    pub mode: PipelineMode,
    pub service: InstalledServiceRecord,
}

#[derive(Clone, Debug)]
pub struct ServiceActionResultState {
    pub mode: PipelineMode,
    pub service: InstalledServiceRecord,
    pub message: String,
    pub succeeded: bool,
    pub scroll: u16,
}

#[cfg(test)]
#[derive(Clone, Debug)]
pub struct TuiSnapshot {
    pub nodes: Vec<NodeRecord>,
    pub apps: Vec<AppRecord>,
    pub services: Vec<InstalledServiceRecord>,
    pub detail_text: String,
}

#[derive(Clone, Debug)]
pub struct TuiState {
    pub(crate) home: std::path::PathBuf,
    pub(crate) config: std::sync::Arc<crate::config::InsConfig>,
    pub(crate) active_section: ActiveSection,
    pub(crate) nodes: Vec<NodeRecord>,
    pub(crate) node_details: Vec<String>,
    pub(crate) node_index: usize,
    pub(crate) apps: Vec<AppRecord>,
    pub(crate) app_details: Vec<String>,
    pub(crate) app_index: usize,
    pub(crate) app_view: AppViewState,
    pub(crate) services: Vec<InstalledServiceRecord>,
    pub(crate) service_details: Vec<String>,
    pub(crate) service_index: usize,
    pub(crate) overlay: Option<OverlayState>,
    pub(crate) status: Option<String>,
}

impl Default for TuiState {
    fn default() -> Self {
        let mut state = Self {
            home: std::path::PathBuf::from("."),
            config: std::sync::Arc::new(crate::config::InsConfig::default()),
            active_section: ActiveSection::Nodes,
            nodes: vec![NodeRecord::Local()],
            node_details: vec![node_detail(&NodeRecord::Local())],
            node_index: 0,
            apps: Vec::new(),
            app_details: Vec::new(),
            app_index: 0,
            app_view: AppViewState::AppList,
            services: Vec::new(),
            service_details: Vec::new(),
            service_index: 0,
            overlay: None,
            status: None,
        };
        state.sync_selection();
        state
    }
}

impl TuiState {
    pub async fn load(
        home: std::path::PathBuf,
        config: std::sync::Arc<crate::config::InsConfig>,
    ) -> anyhow::Result<Self> {
        let app_home = match config.app_home_override() {
            Some(path) => std::path::PathBuf::from(path),
            None => home.join("app"),
        };
        tokio::fs::create_dir_all(&app_home)
            .await
            .with_context(|| format!("create app home {}", app_home.display()))?;

        let nodes =
            crate::cli::node::list_node_records(&crate::cli::node::nodes_file(&home)).await?;
        let apps = crate::cli::app::list_app_records(&app_home, config.defaults_env()).await?;
        let services = crate::cli::service::list_service_records(&home).await?;
        let app_details = load_app_details(&app_home, &apps).await?;

        let mut state = Self {
            home,
            config,
            active_section: ActiveSection::Nodes,
            nodes: nodes.clone(),
            node_details: nodes.iter().map(node_detail).collect(),
            node_index: 0,
            apps,
            app_details,
            app_index: 0,
            app_view: AppViewState::AppList,
            services: services.clone(),
            service_details: services.iter().map(service_detail).collect(),
            service_index: 0,
            overlay: None,
            status: None,
        };
        state.sync_selection();
        Ok(state)
    }

    #[cfg(test)]
    pub fn snapshot(&self) -> TuiSnapshot {
        TuiSnapshot {
            nodes: self.nodes.clone(),
            apps: self.apps.clone(),
            services: self.services.clone(),
            detail_text: self.detail_text(),
        }
    }

    pub fn active_section(&self) -> ActiveSection {
        self.active_section
    }

    pub fn overlay(&self) -> Option<&OverlayState> {
        self.overlay.as_ref()
    }

    pub fn open_quit_confirmation(&mut self) {
        self.overlay = Some(OverlayState::QuitConfirm);
        self.status = Some("Confirm quit".into());
    }

    pub fn open_service_action_confirmation(&mut self, mode: PipelineMode) -> anyhow::Result<()> {
        let service = self
            .selected_service()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no service selected"))?;
        self.overlay = Some(OverlayState::ServiceActionConfirm(ServiceActionState {
            mode,
            service: service.clone(),
        }));
        self.status = Some(format!(
            "Confirm {} for '{}'",
            service_action_label(mode),
            service.service
        ));
        Ok(())
    }

    pub fn open_service_action_result(
        &mut self,
        mode: PipelineMode,
        service: InstalledServiceRecord,
        message: String,
        succeeded: bool,
    ) {
        self.overlay = Some(OverlayState::ServiceActionResult(
            ServiceActionResultState {
                mode,
                service: service.clone(),
                message,
                succeeded,
                scroll: 0,
            },
        ));
        self.status = Some(format!(
            "{} {} for '{}'",
            service_action_label(mode),
            if succeeded { "completed" } else { "failed" },
            service.service
        ));
    }

    pub fn pending_service_action(&self) -> Option<ServiceActionState> {
        match self.overlay.as_ref() {
            Some(OverlayState::ServiceActionConfirm(action)) => Some(action.clone()),
            _ => None,
        }
    }

    pub fn scroll_service_action_result_down(&mut self) {
        if let Some(OverlayState::ServiceActionResult(result)) = self.overlay.as_mut() {
            result.scroll = result.scroll.saturating_add(1);
        }
    }

    pub fn scroll_service_action_result_up(&mut self) {
        if let Some(OverlayState::ServiceActionResult(result)) = self.overlay.as_mut() {
            result.scroll = result.scroll.saturating_sub(1);
        }
    }

    pub fn status_text(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn selected_service(&self) -> Option<&InstalledServiceRecord> {
        self.services.get(self.service_index)
    }

    pub fn next_section(&mut self) {
        self.active_section = self.active_section.next();
        self.sync_selection();
    }

    pub fn previous_section(&mut self) {
        self.active_section = self.active_section.previous();
        self.sync_selection();
    }

    pub fn can_add(&self) -> bool {
        match self.active_section {
            ActiveSection::Nodes => true,
            ActiveSection::Apps => self.is_app_file_manager_active(),
            ActiveSection::Services => false,
        }
    }

    pub fn can_edit(&self) -> bool {
        match self.active_section {
            ActiveSection::Nodes => true,
            ActiveSection::Apps => {
                self.is_app_file_manager_active()
                    && self
                        .selected_app_file()
                        .is_none_or(|file| !file.is_dir && file.is_text)
            }
            ActiveSection::Services => false,
        }
    }

    pub fn can_delete(&self) -> bool {
        match self.active_section {
            ActiveSection::Nodes => true,
            ActiveSection::Apps => self.selected_app_file().is_some(),
            ActiveSection::Services => false,
        }
    }

    pub fn select_next(&mut self) {
        match self.active_section {
            ActiveSection::Nodes => {
                self.node_index = next_index(self.node_index, self.nodes.len());
            }
            ActiveSection::Apps => match &mut self.app_view {
                AppViewState::AppList => {
                    self.app_index = next_index(self.app_index, self.apps.len())
                }
                AppViewState::FileManager(manager) => {
                    manager.selected_file_index =
                        next_index(manager.selected_file_index, manager.files.len())
                }
            },
            ActiveSection::Services => {
                self.service_index = next_index(self.service_index, self.services.len());
            }
        }
        self.sync_selection();
    }

    pub fn select_previous(&mut self) {
        match self.active_section {
            ActiveSection::Nodes => {
                self.node_index = previous_index(self.node_index, self.nodes.len());
            }
            ActiveSection::Apps => match &mut self.app_view {
                AppViewState::AppList => {
                    self.app_index = previous_index(self.app_index, self.apps.len())
                }
                AppViewState::FileManager(manager) => {
                    manager.selected_file_index =
                        previous_index(manager.selected_file_index, manager.files.len())
                }
            },
            ActiveSection::Services => {
                self.service_index = previous_index(self.service_index, self.services.len());
            }
        }
        self.sync_selection();
    }

    pub fn inspect_selected(&mut self) {
        match self.active_section {
            ActiveSection::Apps => {
                if !self.is_app_file_manager_active() {
                    if let Err(error) = self.enter_selected_app_file_manager_sync() {
                        self.status = Some(error.to_string());
                    }
                }
            }
            _ => self.status = Some(format!("Inspecting {}", self.active_section.title())),
        }
    }

    pub fn handle_escape(&mut self) {
        if self.overlay.is_some() {
            self.cancel_overlay();
        } else if self.active_section == ActiveSection::Apps && self.is_app_file_manager_active() {
            self.app_view = AppViewState::AppList;
            self.status = Some("Back to app list".into());
        }
    }

    pub fn cancel_overlay(&mut self) {
        self.overlay = None;
        self.status = Some("Cancelled".into());
    }

    pub fn next_overlay_field(&mut self) {
        match self.overlay.as_mut() {
            Some(OverlayState::NodeForm(form)) => form.next_field(),
            Some(OverlayState::AppCreateForm(form)) => {
                let index = AppCreateField::ALL
                    .iter()
                    .position(|field| *field == form.active_field)
                    .unwrap_or(0);
                form.active_field = AppCreateField::ALL[(index + 1) % AppCreateField::ALL.len()];
            }
            _ => {}
        }
    }

    pub fn previous_overlay_field(&mut self) {
        match self.overlay.as_mut() {
            Some(OverlayState::NodeForm(form)) => form.previous_field(),
            Some(OverlayState::AppCreateForm(form)) => {
                let index = AppCreateField::ALL
                    .iter()
                    .position(|field| *field == form.active_field)
                    .unwrap_or(0);
                form.active_field = AppCreateField::ALL
                    [(index + AppCreateField::ALL.len() - 1) % AppCreateField::ALL.len()];
            }
            _ => {}
        }
    }

    pub fn push_overlay_char(&mut self, c: char) {
        match self.overlay.as_mut() {
            Some(OverlayState::NodeForm(form)) => form.active_value_mut().push(c),
            Some(OverlayState::AppCreateForm(form)) => match form.active_field {
                AppCreateField::Kind => match c {
                    'f' | 'F' => form.kind = AppCreateKind::File,
                    'd' | 'D' => form.kind = AppCreateKind::Directory,
                    ' ' => form.kind = form.kind.toggle(),
                    _ => {}
                },
                AppCreateField::Path => form.path.push(c),
            },
            Some(OverlayState::AppTextEditor(editor)) => editor.content.push(c),
            _ => {}
        }
    }

    pub fn backspace_overlay_value(&mut self) {
        match self.overlay.as_mut() {
            Some(OverlayState::NodeForm(form)) => {
                form.active_value_mut().pop();
            }
            Some(OverlayState::AppCreateForm(form))
                if form.active_field == AppCreateField::Path =>
            {
                form.path.pop();
            }
            Some(OverlayState::AppTextEditor(editor)) => {
                editor.content.pop();
            }
            _ => {}
        }
    }

    pub fn insert_overlay_newline(&mut self) {
        if let Some(OverlayState::AppTextEditor(editor)) = self.overlay.as_mut() {
            editor.content.push('\n');
        }
    }

    pub fn set_status(&mut self, status: String) {
        self.status = Some(status);
    }

    pub fn list_items(&self) -> Vec<String> {
        match self.active_section {
            ActiveSection::Nodes => self.nodes.iter().map(node_label).collect(),
            ActiveSection::Apps => match &self.app_view {
                AppViewState::AppList => self.apps.iter().map(app_label).collect(),
                AppViewState::FileManager(manager) => {
                    manager.files.iter().map(app_file_label).collect()
                }
            },
            ActiveSection::Services => self.services.iter().map(service_label).collect(),
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        let len = match self.active_section {
            ActiveSection::Nodes => self.nodes.len(),
            ActiveSection::Apps => match &self.app_view {
                AppViewState::AppList => self.apps.len(),
                AppViewState::FileManager(manager) => manager.files.len(),
            },
            ActiveSection::Services => self.services.len(),
        };

        if len == 0 {
            None
        } else {
            Some(match self.active_section {
                ActiveSection::Nodes => self.node_index,
                ActiveSection::Apps => match &self.app_view {
                    AppViewState::AppList => self.app_index,
                    AppViewState::FileManager(manager) => manager.selected_file_index,
                },
                ActiveSection::Services => self.service_index,
            })
        }
    }

    pub fn detail_text(&self) -> String {
        match self.active_section {
            ActiveSection::Nodes => self
                .node_details
                .get(self.node_index)
                .cloned()
                .unwrap_or_else(|| "No nodes found.".into()),
            ActiveSection::Apps => match &self.app_view {
                AppViewState::AppList => self
                    .app_details
                    .get(self.app_index)
                    .cloned()
                    .unwrap_or_else(|| "No apps found.".into()),
                AppViewState::FileManager(manager) => apps::manager_file_detail(manager),
            },
            ActiveSection::Services => self
                .service_details
                .get(self.service_index)
                .cloned()
                .unwrap_or_else(|| "No services found.".into()),
        }
    }

    pub async fn submit_active_overlay(&mut self) -> anyhow::Result<()> {
        let Some(overlay) = self.overlay.clone() else {
            return Ok(());
        };

        match overlay {
            OverlayState::NodeForm(form) => {
                self.apply_node_form(Self::build_node_input_from_form(form)?)
                    .await
            }
            OverlayState::DeleteConfirm(DeleteTarget::Node { name }) => {
                self.delete_node(name).await
            }
            OverlayState::DeleteConfirm(DeleteTarget::AppFile {
                app_name,
                relative_path,
            }) => self.delete_app_file(&app_name, &relative_path).await,
            OverlayState::AppCreateForm(form) => {
                self.create_app_file_for(
                    &form.app_name,
                    form.path.trim().into(),
                    form.kind == AppCreateKind::Directory,
                )
                .await
            }
            OverlayState::AppTextEditor(editor) => {
                self.save_app_text_file_for(
                    &editor.app_name,
                    editor.relative_path.clone(),
                    editor.content,
                )
                .await
            }
            OverlayState::ServiceActionConfirm(_) | OverlayState::ServiceActionResult(_) => Ok(()),
            OverlayState::QuitConfirm => Ok(()),
        }
    }

    pub fn sync_selection(&mut self) {
        self.node_index = clamp_index(self.node_index, self.nodes.len());
        self.app_index = clamp_index(self.app_index, self.apps.len());
        self.service_index = clamp_index(self.service_index, self.services.len());
        if let AppViewState::FileManager(manager) = &mut self.app_view {
            manager.selected_file_index =
                clamp_index(manager.selected_file_index, manager.files.len());
        }
    }

    pub async fn reload_services(&mut self) -> anyhow::Result<()> {
        self.services = crate::cli::service::list_service_records(&self.home).await?;
        self.service_details = self.services.iter().map(service_detail).collect();
        self.service_index = clamp_index(self.service_index, self.services.len());
        Ok(())
    }
}

fn service_action_label(mode: PipelineMode) -> &'static str {
    match mode {
        PipelineMode::Check => "check",
        PipelineMode::Deploy => "deploy",
    }
}

pub(crate) fn next_index(current: usize, len: usize) -> usize {
    if len == 0 { 0 } else { (current + 1) % len }
}

pub(crate) fn previous_index(current: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        (current + len - 1) % len
    }
}

pub(crate) fn clamp_index(index: usize, len: usize) -> usize {
    if len == 0 { 0 } else { index.min(len - 1) }
}
