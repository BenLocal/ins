use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};

use crate::tui::state::{
    ActiveSection, AppCreateField, DeleteTarget, NodeFormField, OverlayState, TuiState,
};

pub fn render(frame: &mut Frame, state: &TuiState) {
    if let Some(overlay) = state.overlay() {
        match overlay {
            OverlayState::ServiceActionConfirm(action) => {
                frame.render_widget(Clear, frame.area());
                render_service_action_confirm(frame, frame.area(), action);
                return;
            }
            OverlayState::ServiceActionResult(result) => {
                frame.render_widget(Clear, frame.area());
                render_service_action_result(frame, frame.area(), result);
                return;
            }
            _ => {}
        }
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(4),
        ])
        .split(frame.area());

    render_tabs(frame, chunks[0], state.active_section());
    render_body(frame, chunks[1], state);
    render_footer(frame, chunks[2], state);

    if let Some(overlay) = state.overlay() {
        match overlay {
            OverlayState::NodeForm(_) => {
                render_node_form(frame, centered_rect(70, 70, frame.area()), state)
            }
            OverlayState::DeleteConfirm(target) => {
                render_delete_confirm(frame, centered_rect(60, 30, frame.area()), target)
            }
            OverlayState::QuitConfirm => {
                render_quit_confirm(frame, centered_rect(50, 30, frame.area()))
            }
            OverlayState::AppCreateForm(_) => {
                render_app_create_form(frame, centered_rect(70, 40, frame.area()), state)
            }
            OverlayState::AppTextEditor(_) => {
                render_app_text_editor(frame, centered_rect(80, 70, frame.area()), state)
            }
            OverlayState::ServiceActionConfirm(_) | OverlayState::ServiceActionResult(_) => {}
        }
    }
}

fn render_tabs(frame: &mut Frame, area: Rect, active: ActiveSection) {
    let tabs = Tabs::new(["Nodes", "Apps", "Services"])
        .block(Block::default().borders(Borders::ALL).title("ins tui"))
        .highlight_style(Style::default().bg(ratatui::style::Color::Blue).white())
        .select(match active {
            ActiveSection::Nodes => 0,
            ActiveSection::Apps => 1,
            ActiveSection::Services => 2,
        });
    frame.render_widget(tabs, area);
}

fn render_body(frame: &mut Frame, area: Rect, state: &TuiState) {
    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    let items = state
        .list_items()
        .into_iter()
        .map(ListItem::new)
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(state.active_section().title()),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    let mut list_state = ListState::default();
    list_state.select(state.selected_index());
    frame.render_stateful_widget(list, panels[0], &mut list_state);

    let detail = Paragraph::new(Text::from(state.detail_text()))
        .block(Block::default().borders(Borders::ALL).title("Details"))
        .scroll((
            if state.active_section() == ActiveSection::Apps {
                state.app_detail_scroll().unwrap_or(0)
            } else {
                0
            },
            0,
        ))
        .wrap(Wrap { trim: false });
    frame.render_widget(detail, panels[1]);
}

fn render_footer(frame: &mut Frame, area: Rect, state: &TuiState) {
    let status = state.status_text();
    let lines = match state.active_section() {
        ActiveSection::Nodes => vec![
            Line::from("Tab next | Shift+Tab prev | j/k or Up/Down move | Enter inspect"),
            Line::from("a add node | e edit node | d delete node | q quit"),
        ],
        ActiveSection::Apps => vec![
            Line::from("Tab next | Shift+Tab prev | j/k or Up/Down move | Enter open/inspect"),
            Line::from("Esc back | a add file/dir | e edit text file | o open in $EDITOR"),
            Line::from("d delete | PgUp/PgDn scroll | q quit"),
        ],
        ActiveSection::Services => vec![
            Line::from("Tab next | Shift+Tab prev | j/k or Up/Down move | Enter inspect"),
            Line::from("c check | d deploy | q quit"),
        ],
    };
    let footer = Paragraph::new(if let Some(status) = status {
        Text::from(vec![
            Line::from(status),
            Line::from(shortcuts_hint(state.active_section())),
        ])
    } else {
        Text::from(lines)
    })
    .block(Block::default().borders(Borders::ALL).title("Help"))
    .wrap(Wrap { trim: false });
    frame.render_widget(footer, area);
}

fn render_node_form(frame: &mut Frame, area: Rect, state: &TuiState) {
    let Some(form) = state.node_form() else {
        return;
    };

    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(match form.mode {
            crate::tui::state::NodeFormMode::Add => "Add Node",
            crate::tui::state::NodeFormMode::Edit => "Edit Node",
        });
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(2),
        ])
        .split(inner);

    let fields = [
        (NodeFormField::Name, form.name.as_str()),
        (NodeFormField::Ip, form.ip.as_str()),
        (NodeFormField::Port, form.port.as_str()),
        (NodeFormField::User, form.user.as_str()),
        (NodeFormField::Password, form.password.as_str()),
        (NodeFormField::KeyPath, form.key_path.as_str()),
    ];

    for (index, (field, value)) in fields.into_iter().enumerate() {
        let label = if field == form.active_field {
            format!("> {}: {}", field.label(), value)
        } else {
            format!("  {}: {}", field.label(), value)
        };
        let paragraph = Paragraph::new(label).block(Block::default().borders(Borders::BOTTOM));
        frame.render_widget(paragraph, rows[index]);
    }

    let help = Paragraph::new("Type to edit | Tab/Shift-Tab move | Enter save | Esc cancel")
        .italic()
        .wrap(Wrap { trim: true });
    frame.render_widget(help, rows[6]);
}

fn shortcuts_hint(section: ActiveSection) -> &'static str {
    match section {
        ActiveSection::Nodes => {
            "Tab next | Shift+Tab prev | j/k move | a add node | e edit node | d delete node | q quit"
        }
        ActiveSection::Apps => {
            "Tab next | Shift+Tab prev | j/k move | Enter open | Esc back | a add | e edit | o open | d delete | PgUp/PgDn scroll | q quit"
        }
        ActiveSection::Services => {
            "Tab next | Shift+Tab prev | j/k move | Enter inspect | c check | d deploy | q quit"
        }
    }
}

fn render_delete_confirm(frame: &mut Frame, area: Rect, target: &DeleteTarget) {
    frame.render_widget(Clear, area);
    let message = match target {
        DeleteTarget::Node { name } => format!("Delete node '{name}'?"),
        DeleteTarget::AppFile {
            app_name,
            relative_path,
        } => format!("Delete '{relative_path}' from app '{app_name}'?"),
    };
    let paragraph = Paragraph::new(Text::from(vec![
        Line::from(message),
        Line::from("Enter confirm | Esc cancel"),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Delete"))
    .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn render_quit_confirm(frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);
    let paragraph = Paragraph::new(Text::from(vec![
        Line::from("Exit ins tui?"),
        Line::from("Enter confirm | Esc cancel"),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Quit"))
    .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn render_service_action_confirm(
    frame: &mut Frame,
    area: Rect,
    action: &crate::tui::state::ServiceActionState,
) {
    let verb = match action.mode {
        crate::pipeline::PipelineMode::Check => "Check",
        crate::pipeline::PipelineMode::Deploy => "Deploy",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("{verb} Confirm"));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = vec![
        Line::from(format!(
            "Run {verb} for service '{}'? ",
            action.service.service
        )),
        Line::from(""),
        Line::from(format!("namespace: {}", action.service.namespace)),
        Line::from(format!("app: {}", action.service.app_name)),
        Line::from(format!("node: {}", action.service.node_name)),
        Line::from(format!("workspace: {}", action.service.workspace)),
        Line::from(""),
        Line::from("Enter confirm | Esc cancel"),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn render_service_action_result(
    frame: &mut Frame,
    area: Rect,
    result: &crate::tui::state::ServiceActionResultState,
) {
    let verb = match result.mode {
        crate::pipeline::PipelineMode::Check => "Check",
        crate::pipeline::PipelineMode::Deploy => "Deploy",
    };
    let status = if result.succeeded {
        "Succeeded"
    } else {
        "Failed"
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("{verb} Result"));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![
        Line::from(format!("{verb} {status}")),
        Line::from(format!("service: {}", result.service.service)),
        Line::from(format!("namespace: {}", result.service.namespace)),
        Line::from(format!("app: {}", result.service.app_name)),
        Line::from(format!("node: {}", result.service.node_name)),
        Line::from(format!("workspace: {}", result.service.workspace)),
        Line::from(""),
    ];
    lines.extend(Text::from(result.message.clone()).lines);
    lines.push(Line::from(""));
    lines.push(Line::from(
        "Up/Down or PgUp/PgDn scroll | Enter or Esc close",
    ));

    let paragraph = Paragraph::new(Text::from(lines))
        .scroll((result.scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

fn render_app_create_form(frame: &mut Frame, area: Rect, state: &TuiState) {
    let Some(form) = state.app_create_form() else {
        return;
    };
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Create In {}", form.app_name));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(2),
        ])
        .split(inner);

    let kind_label = if form.active_field == AppCreateField::Kind {
        format!("> {}: {}", AppCreateField::Kind.label(), form.kind.label())
    } else {
        format!("  {}: {}", AppCreateField::Kind.label(), form.kind.label())
    };
    let path_label = if form.active_field == AppCreateField::Path {
        format!("> {}: {}", AppCreateField::Path.label(), form.path)
    } else {
        format!("  {}: {}", AppCreateField::Path.label(), form.path)
    };
    frame.render_widget(Paragraph::new(kind_label), rows[0]);
    frame.render_widget(Paragraph::new(path_label), rows[1]);
    frame.render_widget(
        Paragraph::new("Tab move | f/d or Space set kind | Enter create | Esc cancel"),
        rows[2],
    );
}

fn render_app_text_editor(frame: &mut Frame, area: Rect, state: &TuiState) {
    let Some(editor) = state.app_text_editor() else {
        return;
    };
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Edit {}", editor.relative_path));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(2)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(editor.content.clone()).wrap(Wrap { trim: false }),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new("Type to edit | Enter newline | Ctrl+S save | Esc cancel"),
        rows[1],
    );

    let (cursor_x, cursor_y) = editor_cursor_position(rows[0], &editor.content);
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn editor_cursor_position(area: Rect, content: &str) -> (u16, u16) {
    let mut x = area.x;
    let mut y = area.y;
    let lines = content.split('\n').collect::<Vec<_>>();

    for (index, line) in lines.iter().enumerate() {
        let width = line.chars().count() as u16;
        x = area.x.saturating_add(width);
        if y >= area.y.saturating_add(area.height.saturating_sub(1)) {
            x = x.min(area.x.saturating_add(area.width.saturating_sub(1)));
            return (x, y);
        }
        if index + 1 < lines.len() {
            y = y.saturating_add(1);
        }
    }

    (
        x.min(area.x.saturating_add(area.width.saturating_sub(1))),
        y.min(area.y.saturating_add(area.height.saturating_sub(1))),
    )
}

fn centered_rect(horizontal: u16, vertical: u16, area: Rect) -> Rect {
    let vertical_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - vertical) / 2),
            Constraint::Percentage(vertical),
            Constraint::Percentage((100 - vertical) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - horizontal) / 2),
            Constraint::Percentage(horizontal),
            Constraint::Percentage((100 - horizontal) / 2),
        ])
        .split(vertical_layout[1])[1]
}
