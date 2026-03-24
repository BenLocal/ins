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
mod tests {
    use super::{VersionInfo, compile_time_value, release_tag};

    #[test]
    fn release_tag_prefixes_version_with_v() {
        assert_eq!(release_tag("0.1.0"), "v0.1.0");
    }

    #[test]
    fn compile_time_value_ignores_empty_strings() {
        assert_eq!(compile_time_value(Some("   ")), None);
        assert_eq!(compile_time_value(Some("abc")), Some("abc".into()));
    }

    #[test]
    fn render_includes_version_tag_and_git_details() {
        let info = VersionInfo {
            version: "0.1.0".into(),
            release_tag: "v0.1.0".into(),
            git_tag: Some("v0.1.0".into()),
            git_commit: Some("abcdef123456".into()),
            dirty: Some("clean".into()),
        };

        assert_eq!(
            info.render(),
            "version: 0.1.0\n\
tag: v0.1.0\n\
git tag: v0.1.0\n\
git commit: abcdef123456\n\
worktree: clean\n"
        );
    }
}
