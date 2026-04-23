pub mod state;
pub mod ui;

use std::{env, io, path::Path, path::PathBuf, process::Command, time::Duration};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::execution_output::ExecutionOutput;
use crate::pipeline::{
    PipelineMode, execute_pipeline_with_output, prepare_installed_service_deployment,
};
use crate::tui::state::TuiState;

pub async fn run(
    home: PathBuf,
    config: std::sync::Arc<crate::config::InsConfig>,
) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_event_loop(&mut terminal, TuiState::load(home, config).await).await;
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    initial_state: anyhow::Result<TuiState>,
) -> anyhow::Result<()> {
    let mut state = initial_state?;

    loop {
        terminal.draw(|frame| ui::render(frame, &state))?;

        if !event::poll(Duration::from_millis(100))? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        if state.overlay().is_some() {
            match key.code {
                KeyCode::Esc => state.cancel_overlay(),
                KeyCode::Down
                    if matches!(
                        state.overlay(),
                        Some(crate::tui::state::OverlayState::ServiceActionResult(_))
                    ) =>
                {
                    state.scroll_service_action_result_down()
                }
                KeyCode::Up
                    if matches!(
                        state.overlay(),
                        Some(crate::tui::state::OverlayState::ServiceActionResult(_))
                    ) =>
                {
                    state.scroll_service_action_result_up()
                }
                KeyCode::PageDown
                    if matches!(
                        state.overlay(),
                        Some(crate::tui::state::OverlayState::ServiceActionResult(_))
                    ) =>
                {
                    for _ in 0..10 {
                        state.scroll_service_action_result_down();
                    }
                }
                KeyCode::PageUp
                    if matches!(
                        state.overlay(),
                        Some(crate::tui::state::OverlayState::ServiceActionResult(_))
                    ) =>
                {
                    for _ in 0..10 {
                        state.scroll_service_action_result_up();
                    }
                }
                KeyCode::Tab | KeyCode::Down if state.app_text_editor().is_none() => {
                    state.next_overlay_field()
                }
                KeyCode::BackTab | KeyCode::Up if state.app_text_editor().is_none() => {
                    state.previous_overlay_field()
                }
                KeyCode::Enter => {
                    if state.app_text_editor().is_some() {
                        state.insert_overlay_newline();
                    } else if state.pending_service_action().is_some() {
                        if let Err(error) = run_confirmed_service_action(terminal, &mut state).await
                        {
                            state.set_status(error.to_string());
                        }
                    } else if matches!(
                        state.overlay(),
                        Some(crate::tui::state::OverlayState::ServiceActionResult(_))
                    ) {
                        state.cancel_overlay();
                    } else if matches!(
                        state.overlay(),
                        Some(crate::tui::state::OverlayState::QuitConfirm)
                    ) {
                        return Ok(());
                    } else if let Err(error) = state.submit_active_overlay().await {
                        state.set_status(error.to_string());
                    }
                }
                KeyCode::Backspace => state.backspace_overlay_value(),
                KeyCode::Char('s') if key.modifiers == KeyModifiers::CONTROL => {
                    if let Err(error) = state.submit_active_overlay().await {
                        state.set_status(error.to_string());
                    }
                }
                KeyCode::Char(c)
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
                {
                    state.push_overlay_char(c);
                }
                _ => {}
            }
            continue;
        }

        match key.code {
            KeyCode::Char('q') => state.open_quit_confirmation(),
            KeyCode::Esc => state.handle_escape(),
            KeyCode::Tab => state.next_section(),
            KeyCode::BackTab => state.previous_section(),
            KeyCode::Down | KeyCode::Char('j') => state.select_next(),
            KeyCode::Up | KeyCode::Char('k') => state.select_previous(),
            KeyCode::PageDown
                if state.active_section() == crate::tui::state::ActiveSection::Apps =>
            {
                state.scroll_app_detail_down()
            }
            KeyCode::PageUp if state.active_section() == crate::tui::state::ActiveSection::Apps => {
                state.scroll_app_detail_up()
            }
            KeyCode::Enter => state.inspect_selected(),
            KeyCode::Char('a') if state.can_add() => match state.active_section() {
                crate::tui::state::ActiveSection::Nodes => state.open_add_node_form(),
                crate::tui::state::ActiveSection::Apps => {
                    if let Err(error) = state.open_create_app_file_form() {
                        state.set_status(error.to_string());
                    }
                }
                crate::tui::state::ActiveSection::Services => {}
            },
            KeyCode::Char('e') if state.can_edit() => match state.active_section() {
                crate::tui::state::ActiveSection::Nodes => {
                    if let Err(error) = state.open_edit_node_form() {
                        state.set_status(error.to_string());
                    }
                }
                crate::tui::state::ActiveSection::Apps => {
                    if let Err(error) = state.open_edit_app_text_editor().await {
                        state.set_status(error.to_string());
                    }
                }
                crate::tui::state::ActiveSection::Services => {}
            },
            KeyCode::Char('d') if state.can_delete() => match state.active_section() {
                crate::tui::state::ActiveSection::Nodes => {
                    if let Err(error) = state.open_delete_node_confirmation() {
                        state.set_status(error.to_string());
                    }
                }
                crate::tui::state::ActiveSection::Apps => {
                    if let Err(error) = state.open_delete_app_file_confirmation() {
                        state.set_status(error.to_string());
                    }
                }
                crate::tui::state::ActiveSection::Services => {}
            },
            KeyCode::Char('c')
                if state.active_section() == crate::tui::state::ActiveSection::Services =>
            {
                if let Err(error) = state.open_service_action_confirmation(PipelineMode::Check) {
                    state.set_status(error.to_string());
                }
            }
            KeyCode::Char('d')
                if state.active_section() == crate::tui::state::ActiveSection::Services =>
            {
                if let Err(error) = state.open_service_action_confirmation(PipelineMode::Deploy) {
                    state.set_status(error.to_string());
                }
            }
            KeyCode::Char('o')
                if state.active_section() == crate::tui::state::ActiveSection::Apps
                    && state.can_open_external_editor() =>
            {
                if let Some(path) = state.external_editor_target() {
                    if let Err(error) =
                        open_in_external_editor(terminal, path.as_path(), &mut state).await
                    {
                        state.set_status(error.to_string());
                    }
                }
            }
            _ => {}
        }
    }
}

async fn run_confirmed_service_action(
    _terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut TuiState,
) -> anyhow::Result<()> {
    let action = state
        .pending_service_action()
        .ok_or_else(|| anyhow::anyhow!("no service action pending"))?;
    let service = action.service;
    let mode = action.mode;
    state.overlay = None;
    state.set_status(format!(
        "Running {} for '{}'",
        match mode {
            PipelineMode::Check => "check",
            PipelineMode::Deploy => "deploy",
        },
        service.service
    ));

    let title = match mode {
        PipelineMode::Check => "Starting check...",
        PipelineMode::Deploy => "Starting deployment...",
    };
    let output = ExecutionOutput::buffered();
    let result = async {
        let prepared =
            prepare_installed_service_deployment(&state.home, &state.config, None, &service)
                .await?;
        execute_pipeline_with_output(&state.home, prepared, title, mode, output.clone()).await
    }
    .await;

    match result {
        Ok(()) => {
            state.reload_services().await?;
            state.open_service_action_result(mode, service.clone(), output.snapshot(), true);
        }
        Err(error) => {
            let mut message = output.snapshot();
            if !message.is_empty() {
                message.push('\n');
            }
            message.push_str(&format!("Error: {error}"));
            state.open_service_action_result(mode, service.clone(), message, false);
        }
    }
    Ok(())
}

async fn open_in_external_editor(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    path: &Path,
    state: &mut TuiState,
) -> anyhow::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    let editor = env::var("EDITOR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "vi".into());
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or("vi");
    let mut command = Command::new(program);
    command.args(parts);
    let status = command.arg(path).status()?;

    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;
    terminal.autoresize()?;
    terminal.hide_cursor()?;

    if !status.success() {
        anyhow::bail!("editor exited with status {status}");
    }

    state.reload_apps().await?;
    state.refresh_current_app_file_manager_sync()?;
    state.set_status(format!("Opened '{}' in $EDITOR", path.display()));
    Ok(())
}

#[cfg(test)]
mod tests {
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
}
