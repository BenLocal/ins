use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use tokio::fs;

use crate::{
    app::types::AppRecord,
    cli::app::{inspect_app_content, list_app_records},
    tui::state::{OverlayState, TuiState, clamp_index},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppCreateKind {
    File,
    Directory,
}

impl AppCreateKind {
    pub fn toggle(self) -> Self {
        match self {
            Self::File => Self::Directory,
            Self::Directory => Self::File,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppCreateField {
    Kind,
    Path,
}

impl AppCreateField {
    pub const ALL: [Self; 2] = [Self::Kind, Self::Path];

    pub fn label(self) -> &'static str {
        match self {
            Self::Kind => "Kind",
            Self::Path => "Path",
        }
    }
}

#[derive(Clone, Debug)]
pub struct AppCreateFormState {
    pub app_name: String,
    pub active_field: AppCreateField,
    pub kind: AppCreateKind,
    pub path: String,
}

#[derive(Clone, Debug)]
pub struct AppTextEditorState {
    pub app_name: String,
    pub relative_path: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppFileEntryState {
    pub name: String,
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub is_dir: bool,
    pub is_text: bool,
}

#[derive(Clone, Debug)]
pub struct AppFileManagerState {
    pub app_name: String,
    pub qa_detail: String,
    pub files: Vec<AppFileEntryState>,
    pub selected_file_index: usize,
    pub file_scroll: u16,
}

#[derive(Clone, Debug, Default)]
pub enum AppViewState {
    #[default]
    AppList,
    FileManager(AppFileManagerState),
}

pub fn app_label(app: &AppRecord) -> String {
    app.name.clone()
}

pub fn app_file_label(file: &AppFileEntryState) -> String {
    if file.is_dir {
        format!("{}/", file.relative_path)
    } else {
        file.relative_path.clone()
    }
}

pub async fn load_app_details(app_home: &Path, apps: &[AppRecord]) -> anyhow::Result<Vec<String>> {
    let mut details = Vec::with_capacity(apps.len());
    for app in apps {
        details.push(inspect_app_content(app_home, &app.name).await?);
    }
    Ok(details)
}

pub async fn build_app_file_manager(
    app_home: &Path,
    app: &AppRecord,
) -> anyhow::Result<AppFileManagerState> {
    let app_dir = app_home.join(&app.name);
    let qa_detail = inspect_app_content(app_home, &app.name).await?;
    let mut files = Vec::new();

    if let Some(entries) = &app.files {
        let mut sorted_entries = entries.clone();
        sorted_entries.sort_by(|left, right| left.name.cmp(&right.name));
        for entry in sorted_entries {
            let absolute_path = PathBuf::from(&entry.path);
            let relative_path = absolute_path
                .strip_prefix(&app_dir)
                .ok()
                .and_then(|path| path.to_str())
                .unwrap_or(entry.name.as_str())
                .to_string();
            let is_text = if entry.is_dir {
                false
            } else {
                is_text_file(&absolute_path).await.unwrap_or(false)
            };
            files.push(AppFileEntryState {
                name: entry.name,
                relative_path,
                absolute_path,
                is_dir: entry.is_dir,
                is_text,
            });
        }
    }

    Ok(AppFileManagerState {
        app_name: app.name.clone(),
        qa_detail,
        files,
        selected_file_index: 0,
        file_scroll: 0,
    })
}

fn build_app_file_manager_sync(
    app_home: &Path,
    app: &AppRecord,
) -> anyhow::Result<AppFileManagerState> {
    let app_dir = app_home.join(&app.name);
    let qa_detail = std::fs::read_to_string(app_home.join(&app.name).join("qa.yaml"))
        .with_context(|| {
            format!(
                "read app file {}",
                app_home.join(&app.name).join("qa.yaml").display()
            )
        })?;
    let mut files = Vec::new();

    if let Some(entries) = &app.files {
        let mut sorted_entries = entries.clone();
        sorted_entries.sort_by(|left, right| left.name.cmp(&right.name));
        for entry in sorted_entries {
            let absolute_path = PathBuf::from(&entry.path);
            let relative_path = absolute_path
                .strip_prefix(&app_dir)
                .ok()
                .and_then(|path| path.to_str())
                .unwrap_or(entry.name.as_str())
                .to_string();
            let is_text = if entry.is_dir {
                false
            } else {
                std::fs::read(&absolute_path)
                    .ok()
                    .and_then(|bytes| std::str::from_utf8(&bytes).ok().map(|_| true))
                    .unwrap_or(false)
            };
            files.push(AppFileEntryState {
                name: entry.name,
                relative_path,
                absolute_path,
                is_dir: entry.is_dir,
                is_text,
            });
        }
    }

    Ok(AppFileManagerState {
        app_name: app.name.clone(),
        qa_detail,
        files,
        selected_file_index: 0,
        file_scroll: 0,
    })
}

pub fn manager_file_detail(manager: &AppFileManagerState) -> String {
    if let Some(file) = manager.files.get(manager.selected_file_index) {
        let mut text = format!("name: {}\npath: {}\n", file.name, file.relative_path);
        if file.is_dir {
            text.push_str("type: directory");
        } else if file.is_text {
            text.push_str("type: text\n\n");
            match std::fs::read_to_string(&file.absolute_path) {
                Ok(content) => text.push_str(&content),
                Err(_) => text.push_str("<failed to read file>"),
            }
        } else {
            text.push_str("type: binary");
        }
        text
    } else {
        manager.qa_detail.clone()
    }
}

impl TuiState {
    pub fn is_app_file_manager_active(&self) -> bool {
        matches!(self.app_view, AppViewState::FileManager(_))
    }

    pub fn app_create_form(&self) -> Option<&AppCreateFormState> {
        match self.overlay.as_ref() {
            Some(OverlayState::AppCreateForm(form)) => Some(form),
            _ => None,
        }
    }

    pub fn app_text_editor(&self) -> Option<&AppTextEditorState> {
        match self.overlay.as_ref() {
            Some(OverlayState::AppTextEditor(editor)) => Some(editor),
            _ => None,
        }
    }

    pub fn current_app_name(&self) -> anyhow::Result<&str> {
        match &self.app_view {
            AppViewState::AppList => self
                .apps
                .get(self.app_index)
                .map(|app| app.name.as_str())
                .context("no app selected"),
            AppViewState::FileManager(manager) => Ok(manager.app_name.as_str()),
        }
    }

    pub fn selected_app_file(&self) -> Option<&AppFileEntryState> {
        match &self.app_view {
            AppViewState::FileManager(manager) => manager.files.get(manager.selected_file_index),
            AppViewState::AppList => None,
        }
    }

    pub fn open_create_app_file_form(&mut self) -> anyhow::Result<()> {
        let app_name = self.current_app_name()?.to_string();
        self.overlay = Some(OverlayState::AppCreateForm(AppCreateFormState {
            app_name,
            active_field: AppCreateField::Kind,
            kind: AppCreateKind::File,
            path: String::new(),
        }));
        self.status = Some("Creating app file or directory".into());
        Ok(())
    }

    pub async fn open_edit_app_text_editor(&mut self) -> anyhow::Result<()> {
        let app_name = self.current_app_name()?.to_string();
        let (relative_path, absolute_path) = match self.selected_app_file().cloned() {
            Some(file) => {
                if file.is_dir {
                    bail!("directories cannot be edited");
                }
                if !file.is_text {
                    bail!("binary files cannot be edited");
                }
                (file.relative_path, file.absolute_path)
            }
            None => (
                "qa.yaml".into(),
                self.app_file_absolute_path(&app_name, "qa.yaml"),
            ),
        };
        let content = fs::read_to_string(&absolute_path)
            .await
            .with_context(|| format!("read app file {}", absolute_path.display()))?;
        self.overlay = Some(OverlayState::AppTextEditor(AppTextEditorState {
            app_name,
            relative_path,
            content,
        }));
        self.status = Some("Editing app file".into());
        Ok(())
    }

    pub fn open_delete_app_file_confirmation(&mut self) -> anyhow::Result<()> {
        let app_name = self.current_app_name()?.to_string();
        let file = self
            .selected_app_file()
            .cloned()
            .context("no app file selected")?;
        self.overlay = Some(OverlayState::DeleteConfirm(super::DeleteTarget::AppFile {
            app_name,
            relative_path: file.relative_path.clone(),
        }));
        self.status = Some(format!("Confirm delete for '{}'", file.relative_path));
        Ok(())
    }

    #[cfg(test)]
    pub async fn create_app_file(
        &mut self,
        relative_path: String,
        is_dir: bool,
    ) -> anyhow::Result<()> {
        let app_name = self.current_app_name()?.to_string();
        self.create_app_file_for(&app_name, relative_path, is_dir)
            .await
    }

    #[cfg(test)]
    pub async fn save_app_text_file(
        &mut self,
        relative_path: String,
        content: String,
    ) -> anyhow::Result<()> {
        let app_name = self.current_app_name()?.to_string();
        self.save_app_text_file_for(&app_name, relative_path, content)
            .await
    }

    pub async fn create_app_file_for(
        &mut self,
        app_name: &str,
        relative_path: String,
        is_dir: bool,
    ) -> anyhow::Result<()> {
        let relative_path = validate_relative_path(&relative_path)?;
        let kind = if is_dir {
            crate::app::files::FileKind::Directory
        } else {
            crate::app::files::FileKind::Text
        };
        let app_dir = self.home.join("app").join(app_name);
        crate::app::files::create_file(&app_dir, &relative_path, kind).await?;
        self.reload_apps().await?;
        self.reopen_app_file_manager(app_name, Some(&relative_path))
            .await?;
        self.overlay = None;
        self.status = Some(format!(
            "Created {} '{}'",
            if is_dir { "directory" } else { "file" },
            relative_path
        ));
        Ok(())
    }

    pub async fn save_app_text_file_for(
        &mut self,
        app_name: &str,
        relative_path: String,
        content: String,
    ) -> anyhow::Result<()> {
        let relative_path = validate_relative_path(&relative_path)?;
        let app_dir = self.home.join("app").join(app_name);
        crate::app::files::write_file(&app_dir, &relative_path, &content).await?;
        self.reload_apps().await?;
        self.reopen_app_file_manager(app_name, Some(&relative_path))
            .await?;
        self.overlay = None;
        self.status = Some(format!("Saved file '{}'", relative_path));
        Ok(())
    }

    pub async fn delete_app_file(
        &mut self,
        app_name: &str,
        relative_path: &str,
    ) -> anyhow::Result<()> {
        let relative_path = validate_relative_path(relative_path)?;
        let app_dir = self.home.join("app").join(app_name);
        crate::app::files::delete_file(&app_dir, &relative_path).await?;
        self.reload_apps().await?;
        self.reopen_app_file_manager(app_name, None).await?;
        self.overlay = None;
        self.status = Some(format!("Deleted '{}'", relative_path));
        Ok(())
    }

    pub async fn reload_apps(&mut self) -> anyhow::Result<()> {
        let app_home = self.home.join("app");
        self.apps = list_app_records(&app_home, self.config.defaults_env()).await?;
        self.app_details = load_app_details(&app_home, &self.apps).await?;
        self.app_index = clamp_index(self.app_index, self.apps.len());
        Ok(())
    }

    pub async fn reopen_app_file_manager(
        &mut self,
        app_name: &str,
        preferred_path: Option<&str>,
    ) -> anyhow::Result<()> {
        let app_home = self.home.join("app");
        let app = self
            .apps
            .iter()
            .find(|app| app.name == app_name)
            .cloned()
            .with_context(|| format!("app '{}' not found", app_name))?;
        self.app_index = self
            .apps
            .iter()
            .position(|app| app.name == app_name)
            .unwrap_or(0);
        let mut manager = build_app_file_manager(&app_home, &app).await?;
        if let Some(preferred_path) = preferred_path {
            manager.selected_file_index = manager
                .files
                .iter()
                .position(|file| file.relative_path == preferred_path)
                .unwrap_or(0);
        }
        self.app_view = AppViewState::FileManager(manager);
        self.sync_selection();
        Ok(())
    }

    pub fn enter_selected_app_file_manager_sync(&mut self) -> anyhow::Result<()> {
        let app = self
            .apps
            .get(self.app_index)
            .cloned()
            .context("no app selected")?;
        let app_home = self.home.join("app");
        let manager = build_app_file_manager_sync(&app_home, &app)?;
        self.app_view = AppViewState::FileManager(manager);
        self.status = Some(format!("Managing files for '{}'", app.name));
        self.sync_selection();
        Ok(())
    }

    pub fn app_file_absolute_path(&self, app_name: &str, relative_path: &str) -> PathBuf {
        self.home.join("app").join(app_name).join(relative_path)
    }

    pub fn can_open_external_editor(&self) -> bool {
        self.is_app_file_manager_active()
            && self
                .selected_app_file()
                .is_none_or(|file| !file.is_dir && file.is_text)
    }

    pub fn external_editor_target(&self) -> Option<PathBuf> {
        if !self.is_app_file_manager_active() {
            return None;
        }

        match self.selected_app_file() {
            Some(file) => Some(file.absolute_path.clone()),
            None => self
                .current_app_name()
                .ok()
                .map(|app_name| self.app_file_absolute_path(app_name, "qa.yaml")),
        }
    }

    pub fn refresh_current_app_file_manager_sync(&mut self) -> anyhow::Result<()> {
        let app_name = self.current_app_name()?.to_string();
        let preferred_path = self
            .selected_app_file()
            .map(|file| file.relative_path.clone());
        let app_home = self.home.join("app");
        let app = self
            .apps
            .iter()
            .find(|app| app.name == app_name)
            .cloned()
            .with_context(|| format!("app '{}' not found", app_name))?;
        let mut manager = build_app_file_manager_sync(&app_home, &app)?;
        if let Some(preferred_path) = preferred_path {
            manager.selected_file_index = manager
                .files
                .iter()
                .position(|file| file.relative_path == preferred_path)
                .unwrap_or(0);
        }
        self.app_view = AppViewState::FileManager(manager);
        self.sync_selection();
        Ok(())
    }

    pub fn scroll_app_detail_down(&mut self) {
        if let AppViewState::FileManager(manager) = &mut self.app_view {
            manager.file_scroll = manager.file_scroll.saturating_add(1);
        }
    }

    pub fn scroll_app_detail_up(&mut self) {
        if let AppViewState::FileManager(manager) = &mut self.app_view {
            manager.file_scroll = manager.file_scroll.saturating_sub(1);
        }
    }

    pub fn app_detail_scroll(&self) -> Option<u16> {
        match &self.app_view {
            AppViewState::FileManager(manager) => Some(manager.file_scroll),
            AppViewState::AppList => None,
        }
    }
}

fn validate_relative_path(path: &str) -> anyhow::Result<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        bail!("path cannot be empty");
    }
    let candidate = Path::new(trimmed);
    if candidate.is_absolute() || trimmed.contains("..") {
        bail!("invalid relative path '{}'", trimmed);
    }
    Ok(trimmed.into())
}

async fn is_text_file(path: &Path) -> anyhow::Result<bool> {
    let bytes = fs::read(path)
        .await
        .with_context(|| format!("read app file {}", path.display()))?;
    Ok(std::str::from_utf8(&bytes).is_ok())
}
