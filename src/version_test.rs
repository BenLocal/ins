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
