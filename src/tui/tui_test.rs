use std::{
    env,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use ratatui::{Terminal, backend::TestBackend};
use tokio::fs;

use crate::{
    app::types::{AppRecord, ScriptHook},
    node::types::{NodeRecord, RemoteNodeRecord},
    pipeline::PipelineMode,
    provider::DeploymentTarget,
    store::duck::save_deployment_record,
    tui::{
        state::{ActiveSection, NodeFormInput, NodeFormMode, OverlayState, TuiState},
        ui::render,
    },
};

#[tokio::test]
async fn tui_state_loads_nodes_apps_and_services() -> anyhow::Result<()> {
    let home = unique_test_dir("tui-state-load");
    let app_dir = home.join("app").join("demo");

    fs::create_dir_all(&app_dir).await?;
    fs::write(
        app_dir.join("qa.yaml"),
        r#"
name: demo
description: sample app
values: []
"#
        .trim_start(),
    )
    .await?;
    fs::write(app_dir.join("README.md"), "demo").await?;

    save_deployment_record(
        &home,
        &NodeRecord::Remote(RemoteNodeRecord {
            name: "node-a".into(),
            ip: "10.0.0.1".into(),
            port: 22,
            user: "root".into(),
            password: "secret".into(),
            key_path: None,
        }),
        PathBuf::from("/tmp/workspace").as_path(),
        &DeploymentTarget::new(
            AppRecord {
                name: "demo".into(),
                version: None,
                description: Some("sample app".into()),
                author_name: None,
                author_email: None,
                dependencies: vec![],
                before: ScriptHook::default(),
                after: ScriptHook::default(),
                volumes: vec![],
                all_volume: false,
                files: None,
                values: vec![],
            },
            "demo-web".into(),
        ),
        "name: demo\nvalues: []\n",
    )
    .await?;

    let mut state = TuiState::load(
        home.clone(),
        std::sync::Arc::new(crate::config::InsConfig::default()),
    )
    .await?;
    state
        .apply_node_form(NodeFormInput {
            mode: NodeFormMode::Add,
            name: "node-a".into(),
            ip: "10.0.0.1".into(),
            port: 22,
            user: "root".into(),
            password: "secret".into(),
            key_path: None,
        })
        .await?;

    let snapshot = state.snapshot();
    assert_eq!(snapshot.nodes.len(), 2);
    assert_eq!(snapshot.apps.len(), 1);
    assert_eq!(snapshot.services.len(), 1);
    assert!(snapshot.detail_text.contains("node-a"));

    fs::remove_dir_all(&home).await?;
    Ok(())
}

#[tokio::test]
async fn tui_state_opens_service_action_confirmation() -> anyhow::Result<()> {
    let home = unique_test_dir("tui-service-confirm");
    let app_dir = home.join("app").join("demo");

    fs::create_dir_all(&app_dir).await?;
    fs::write(
        app_dir.join("qa.yaml"),
        r#"
name: demo
values: []
"#
        .trim_start(),
    )
    .await?;

    save_deployment_record(
        &home,
        &NodeRecord::Remote(RemoteNodeRecord {
            name: "node-a".into(),
            ip: "10.0.0.1".into(),
            port: 22,
            user: "root".into(),
            password: "secret".into(),
            key_path: None,
        }),
        PathBuf::from("/tmp/workspace").as_path(),
        &DeploymentTarget::new(
            AppRecord {
                name: "demo".into(),
                version: None,
                description: None,
                author_name: None,
                author_email: None,
                dependencies: vec![],
                before: ScriptHook::default(),
                after: ScriptHook::default(),
                volumes: vec![],
                all_volume: false,
                files: None,
                values: vec![],
            },
            "demo-web".into(),
        ),
        "name: demo\nvalues: []\n",
    )
    .await?;

    let mut state = TuiState::load(
        home.clone(),
        std::sync::Arc::new(crate::config::InsConfig::default()),
    )
    .await?;
    state.next_section();
    state.next_section();
    state.open_service_action_confirmation(PipelineMode::Check)?;

    assert!(matches!(
        state.overlay(),
        Some(OverlayState::ServiceActionConfirm(action))
            if action.service.service == "demo-web" && matches!(action.mode, PipelineMode::Check)
    ));

    fs::remove_dir_all(&home).await?;
    Ok(())
}

#[tokio::test]
async fn tui_state_adds_and_updates_nodes() -> anyhow::Result<()> {
    let home = unique_test_dir("tui-state-node-mutations");
    let mut state = TuiState::load(
        home.clone(),
        std::sync::Arc::new(crate::config::InsConfig::default()),
    )
    .await?;

    state
        .apply_node_form(NodeFormInput {
            mode: NodeFormMode::Add,
            name: "node-a".into(),
            ip: "10.0.0.1".into(),
            port: 22,
            user: "root".into(),
            password: "secret".into(),
            key_path: None,
        })
        .await?;
    state
        .apply_node_form(NodeFormInput {
            mode: NodeFormMode::Edit,
            name: "node-a".into(),
            ip: "10.0.0.2".into(),
            port: 2222,
            user: "admin".into(),
            password: "".into(),
            key_path: Some("~/.ssh/id_rsa".into()),
        })
        .await?;

    let snapshot = state.snapshot();
    match &snapshot.nodes[1] {
        NodeRecord::Remote(node) => {
            assert_eq!(node.ip, "10.0.0.2");
            assert_eq!(node.port, 2222);
            assert_eq!(node.user, "admin");
            assert_eq!(node.key_path.as_deref(), Some("~/.ssh/id_rsa"));
        }
        NodeRecord::Local() => panic!("expected remote node"),
    }

    fs::remove_dir_all(&home).await?;
    Ok(())
}

#[test]
fn tui_state_switches_sections_and_tracks_node_form_mode() {
    let mut state = TuiState::default();
    assert_eq!(state.active_section(), ActiveSection::Nodes);

    state.next_section();
    assert_eq!(state.active_section(), ActiveSection::Apps);

    state.open_add_node_form();
    assert_eq!(state.node_form_mode(), Some(NodeFormMode::Add));

    state.cancel_overlay();
    assert_eq!(state.node_form_mode(), None);
}

#[tokio::test]
async fn tui_state_deletes_remote_node_with_confirmation() -> anyhow::Result<()> {
    let home = unique_test_dir("tui-state-node-delete");
    let mut state = TuiState::load(
        home.clone(),
        std::sync::Arc::new(crate::config::InsConfig::default()),
    )
    .await?;

    state
        .apply_node_form(NodeFormInput {
            mode: NodeFormMode::Add,
            name: "node-a".into(),
            ip: "10.0.0.1".into(),
            port: 22,
            user: "root".into(),
            password: "secret".into(),
            key_path: None,
        })
        .await?;
    state.open_delete_node_confirmation()?;
    assert!(matches!(
        state.overlay(),
        Some(OverlayState::DeleteConfirm(crate::tui::state::DeleteTarget::Node { name })) if name == "node-a"
    ));

    state.submit_active_overlay().await?;

    let snapshot = state.snapshot();
    assert_eq!(snapshot.nodes.len(), 1);
    assert!(snapshot.detail_text.contains("local"));

    fs::remove_dir_all(&home).await?;
    Ok(())
}

#[test]
fn tui_state_rejects_deleting_local_node() {
    let mut state = TuiState::default();
    let err = state
        .open_delete_node_confirmation()
        .expect_err("local node delete should fail");
    assert!(err.to_string().contains("local node cannot be deleted"));
}

#[tokio::test]
async fn tui_state_enters_app_file_manager_and_shows_qa_detail() -> anyhow::Result<()> {
    let home = unique_test_dir("tui-app-file-manager");
    let app_dir = home.join("app").join("demo");

    fs::create_dir_all(&app_dir).await?;
    fs::write(
        app_dir.join("qa.yaml"),
        r#"
name: demo
description: sample app
values: []
"#
        .trim_start(),
    )
    .await?;
    fs::write(app_dir.join("README.md"), "hello from app").await?;

    let mut state = TuiState::load(
        home.clone(),
        std::sync::Arc::new(crate::config::InsConfig::default()),
    )
    .await?;
    state.next_section();
    state.inspect_selected();

    assert!(state.is_app_file_manager_active());
    assert!(state.detail_text().contains("hello from app"));

    fs::remove_dir_all(&home).await?;
    Ok(())
}

#[tokio::test]
async fn tui_state_creates_edits_and_deletes_app_text_file() -> anyhow::Result<()> {
    let home = unique_test_dir("tui-app-file-ops");
    let app_dir = home.join("app").join("demo");

    fs::create_dir_all(&app_dir).await?;
    fs::write(
        app_dir.join("qa.yaml"),
        r#"
name: demo
values: []
"#
        .trim_start(),
    )
    .await?;

    let mut state = TuiState::load(
        home.clone(),
        std::sync::Arc::new(crate::config::InsConfig::default()),
    )
    .await?;
    state.next_section();
    state.inspect_selected();

    state.create_app_file("notes.txt".into(), false).await?;
    state
        .save_app_text_file("notes.txt".into(), "hello world".into())
        .await?;
    state.select_next();
    assert!(state.detail_text().contains("hello world"));

    state.open_delete_app_file_confirmation()?;
    state.submit_active_overlay().await?;

    assert!(!tokio::fs::try_exists(app_dir.join("notes.txt")).await?);

    fs::remove_dir_all(&home).await?;
    Ok(())
}

#[tokio::test]
async fn tui_state_can_edit_qa_yaml_when_no_files_exist() -> anyhow::Result<()> {
    let home = unique_test_dir("tui-app-edit-qa");
    let app_dir = home.join("app").join("demo");

    fs::create_dir_all(&app_dir).await?;
    fs::write(
        app_dir.join("qa.yaml"),
        r#"
name: demo
description: original
values: []
"#
        .trim_start(),
    )
    .await?;

    let mut state = TuiState::load(
        home.clone(),
        std::sync::Arc::new(crate::config::InsConfig::default()),
    )
    .await?;
    state.next_section();
    state.inspect_selected();

    assert!(state.can_edit());
    assert!(!state.can_delete());

    state.open_edit_app_text_editor().await?;
    let editor = state.app_text_editor().expect("qa editor should open");
    assert_eq!(editor.relative_path, "qa.yaml");
    assert!(editor.content.contains("description: original"));

    fs::remove_dir_all(&home).await?;
    Ok(())
}

#[test]
fn tui_ui_renders_tabs_and_help_footer() {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let state = TuiState::default();

    terminal
        .draw(|frame| render(frame, &state))
        .expect("draw should succeed");

    let buffer = terminal.backend().buffer().clone();
    let content = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(content.contains("Nodes"));
    assert!(content.contains("Apps"));
    assert!(content.contains("Services"));
    assert!(content.contains("Details"));
    assert!(content.contains("Help"));
    assert!(content.contains("Tab next"));
    assert!(content.contains("Shift+Tab prev"));
    assert!(content.contains("Enter inspect"));
    assert!(content.contains("a add"));
    assert!(content.contains("e edit"));
    assert!(content.contains("d delete"));
    assert!(content.contains("q quit"));
}

#[test]
fn tui_state_opens_quit_confirmation() {
    let mut state = TuiState::default();
    state.open_quit_confirmation();
    assert!(matches!(
        state.overlay(),
        Some(crate::tui::state::OverlayState::QuitConfirm)
    ));
}

#[test]
fn tui_ui_renders_app_footer_actions() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut state = TuiState::default();
    state.next_section();

    terminal
        .draw(|frame| render(frame, &state))
        .expect("draw should succeed");

    let buffer = terminal.backend().buffer().clone();
    let content = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(content.contains("o open"));
}

#[test]
fn tui_ui_renders_service_footer_actions() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut state = TuiState::default();
    state.next_section();
    state.next_section();

    terminal
        .draw(|frame| render(frame, &state))
        .expect("draw should succeed");

    let buffer = terminal.backend().buffer().clone();
    let content = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(content.contains("c check"));
    assert!(content.contains("d deploy"));
}

#[test]
fn tui_ui_renders_service_action_result_overlay() {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut state = TuiState::default();
    state.open_service_action_result(
        PipelineMode::Deploy,
        crate::store::duck::InstalledServiceRecord {
            service: "demo-web".into(),
            app_name: "demo".into(),
            node_name: "node-a".into(),
            workspace: "/srv/demo".into(),
            created_at_ms: 1,
        },
        "Deploy completed for service 'demo-web'".into(),
        true,
    );

    terminal
        .draw(|frame| render(frame, &state))
        .expect("draw should succeed");

    let buffer = terminal.backend().buffer().clone();
    let content = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(content.contains("Deploy Result"));
    assert!(content.contains("Deploy Succeeded"));
    assert!(content.contains("demo-web"));
    assert!(content.contains("/srv/demo"));
}

fn unique_test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("ins-{name}-{}-{nanos}", std::process::id()))
}
