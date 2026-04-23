#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionInfo {
    pub version: String,
    pub release_tag: String,
    pub git_tag: Option<String>,
    pub git_commit: Option<String>,
    pub dirty: Option<String>,
}

impl VersionInfo {
    pub fn current() -> Self {
        let version = env!("CARGO_PKG_VERSION").to_string();

        Self {
            release_tag: release_tag(&version),
            version,
            git_tag: compile_time_value(option_env!("INS_GIT_TAG")),
            git_commit: compile_time_value(option_env!("INS_GIT_COMMIT")),
            dirty: compile_time_value(option_env!("INS_GIT_DIRTY")),
        }
    }

    pub fn render(&self) -> String {
        let git_tag = self.git_tag.as_deref().unwrap_or("none");
        let git_commit = self.git_commit.as_deref().unwrap_or("unknown");
        let dirty = self.dirty.as_deref().unwrap_or("unknown");

        format!(
            "version: {}\ntag: {}\ngit tag: {}\ngit commit: {}\nworktree: {}\n",
            self.version, self.release_tag, git_tag, git_commit, dirty
        )
    }
}

fn release_tag(version: &str) -> String {
    format!("v{version}")
}

fn compile_time_value(value: Option<&'static str>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[cfg(test)]
#[path = "version_test.rs"]
mod version_test;
