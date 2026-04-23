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
#[path = "tui_test.rs"]
mod tui_test;
