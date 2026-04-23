use super::absolute_workspace;
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
