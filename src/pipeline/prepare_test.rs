use super::absolute_workspace;
use crate::app::dependency::DEFAULT_NAMESPACE;
use std::path::Path;

#[test]
fn absolute_workspace_resolves_relative_path_against_cwd() {
    let resolved = absolute_workspace(Path::new("./workspace")).expect("absolute");
    assert!(
        resolved.is_absolute(),
        "expected absolute, got {:?}",
        resolved
    );
    assert!(resolved.ends_with("workspace"));
}

#[test]
fn absolute_workspace_preserves_already_absolute_path() {
    let resolved = absolute_workspace(Path::new("/srv/ins-ws")).expect("absolute");
    assert_eq!(resolved, Path::new("/srv/ins-ws"));
}

#[test]
fn resolve_namespace_defaults_when_unset() {
    let resolved = super::resolve_namespace(None).expect("resolve");
    assert_eq!(resolved, DEFAULT_NAMESPACE);
}

#[test]
fn resolve_namespace_passes_through_valid_input() {
    assert_eq!(
        super::resolve_namespace(Some("staging".into())).unwrap(),
        "staging"
    );
    assert_eq!(
        super::resolve_namespace(Some("ns_1".into())).unwrap(),
        "ns_1"
    );
}

#[test]
fn resolve_namespace_rejects_invalid_chars() {
    super::resolve_namespace(Some("Bad".into())).unwrap_err();
    super::resolve_namespace(Some("with space".into())).unwrap_err();
}
