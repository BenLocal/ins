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

#[tokio::test]
async fn check_namespace_conflicts_errors_when_service_uses_other_namespace() -> anyhow::Result<()>
{
    use crate::app::types::AppRecord;
    use crate::node::types::NodeRecord;
    use crate::provider::DeploymentTarget;
    use crate::store::duck::save_deployment_record;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home: PathBuf = std::env::temp_dir().join(format!(
        "ins-prep-conflict-{}-{}",
        std::process::id(),
        nanos
    ));
    let workspace = home.join("ws");
    let node = NodeRecord::Local();

    let existing = DeploymentTarget::new(
        AppRecord {
            name: "nginx".into(),
            ..AppRecord::default()
        },
        "web".into(),
    );
    save_deployment_record(
        &home,
        &node,
        &workspace,
        &existing,
        "default",
        "name: nginx\n",
    )
    .await?;

    let new_target = DeploymentTarget::new(
        AppRecord {
            name: "nginx".into(),
            ..AppRecord::default()
        },
        "web".into(),
    );

    let err = super::check_namespace_conflicts(&home, &node, "staging", &[new_target])
        .await
        .expect_err("should conflict");
    let msg = err.to_string();
    assert!(msg.contains("'web'"), "error mentions service: {msg}");
    assert!(msg.contains("default"), "error mentions existing ns: {msg}");
    assert!(
        msg.contains("staging"),
        "error mentions requested ns: {msg}"
    );

    std::fs::remove_dir_all(&home)?;
    Ok(())
}

#[tokio::test]
async fn check_namespace_conflicts_passes_when_same_namespace() -> anyhow::Result<()> {
    use crate::app::types::AppRecord;
    use crate::node::types::NodeRecord;
    use crate::provider::DeploymentTarget;
    use crate::store::duck::save_deployment_record;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home: PathBuf = std::env::temp_dir().join(format!(
        "ins-prep-conflict-pass-{}-{}",
        std::process::id(),
        nanos
    ));
    let workspace = home.join("ws");
    let node = NodeRecord::Local();

    let existing = DeploymentTarget::new(
        AppRecord {
            name: "nginx".into(),
            ..AppRecord::default()
        },
        "web".into(),
    );
    save_deployment_record(
        &home,
        &node,
        &workspace,
        &existing,
        "default",
        "name: nginx\n",
    )
    .await?;

    let new_target = DeploymentTarget::new(
        AppRecord {
            name: "nginx".into(),
            ..AppRecord::default()
        },
        "web".into(),
    );

    super::check_namespace_conflicts(&home, &node, "default", &[new_target])
        .await
        .expect("same-namespace redeploy must pass");

    std::fs::remove_dir_all(&home)?;
    Ok(())
}

#[tokio::test]
async fn check_namespace_conflicts_passes_when_no_existing_record() -> anyhow::Result<()> {
    use crate::app::types::AppRecord;
    use crate::node::types::NodeRecord;
    use crate::provider::DeploymentTarget;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home: PathBuf = std::env::temp_dir().join(format!(
        "ins-prep-conflict-empty-{}-{}",
        std::process::id(),
        nanos
    ));
    let node = NodeRecord::Local();

    let new_target = DeploymentTarget::new(
        AppRecord {
            name: "nginx".into(),
            ..AppRecord::default()
        },
        "web".into(),
    );

    super::check_namespace_conflicts(&home, &node, "staging", &[new_target])
        .await
        .expect("no existing record means no conflict");

    if home.exists() {
        std::fs::remove_dir_all(&home)?;
    }
    Ok(())
}
