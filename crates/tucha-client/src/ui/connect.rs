use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use crate::app::{AppState, ConnStatus, ConnectField};
use super::widgets::centered_rect;

pub fn render(f: &mut Frame, state: &AppState) {
    let area = f.area();

    // Фон
    let bg = Block::default().style(Style::default().bg(Color::Black));
    f.render_widget(bg, area);

    let popup = centered_rect(50, 40, area);

    let block = Block::default()
        .title("  tucha — подключение  ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(block, popup);

    let inner = popup;
    let inner = Rect {
        x: inner.x + 2,
        y: inner.y + 2,
        width: inner.width.saturating_sub(4),
        height: inner.height.saturating_sub(4),
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // заголовок сервер
            Constraint::Length(3), // поле сервер
            Constraint::Length(1), // заголовок юзернейм
            Constraint::Length(3), // поле юзернейм
            Constraint::Length(1), // отступ
            Constraint::Length(3), // кнопка
            Constraint::Min(0),
        ])
        .split(inner);

    // ─── Поле сервера ────────────────────────────────────────────────────
    let server_focused = state.connect_field == ConnectField::Server;
    let server_style = if server_focused {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let server_label = Paragraph::new("Адрес сервера")
        .style(Style::default().fg(Color::Gray));
    f.render_widget(server_label, chunks[0]);

    let server_block = Block::default()
        .borders(Borders::ALL)
        .border_type(if server_focused { BorderType::Thick } else { BorderType::Plain })
        .border_style(server_style);
    let server_inner = server_block.inner(chunks[1]);
    f.render_widget(server_block, chunks[1]);
    let server_text = Paragraph::new(state.server_input.as_str())
        .style(Style::default().fg(Color::White));
    f.render_widget(server_text, server_inner);

    // ─── Поле имени ──────────────────────────────────────────────────────
    let name_focused = state.connect_field == ConnectField::Username;
    let name_style = if name_focused {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let name_label = Paragraph::new("Имя пользователя")
        .style(Style::default().fg(Color::Gray));
    f.render_widget(name_label, chunks[2]);

    let name_block = Block::default()
        .borders(Borders::ALL)
        .border_type(if name_focused { BorderType::Thick } else { BorderType::Plain })
        .border_style(name_style);
    let name_inner = name_block.inner(chunks[3]);
    f.render_widget(name_block, chunks[3]);
    let name_text = Paragraph::new(state.username_input.as_str())
        .style(Style::default().fg(Color::White));
    f.render_widget(name_text, name_inner);

    // ─── Кнопка / статус ─────────────────────────────────────────────────
    let btn_text = match &state.conn_status {
        ConnStatus::Disconnected => " Подключиться  [Enter] ",
        ConnStatus::Connecting   => " Подключение... ",
        ConnStatus::Connected    => " Подключено ✓ ",
        ConnStatus::Error(_)     => " Ошибка — повторить [Enter] ",
    };
    let btn_color = match &state.conn_status {
        ConnStatus::Error(_)  => Color::Red,
        ConnStatus::Connected => Color::Green,
        _                     => Color::Cyan,
    };
    let btn = Paragraph::new(btn_text)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(btn_color)),
        )
        .style(Style::default().fg(btn_color).add_modifier(Modifier::BOLD));
    f.render_widget(btn, chunks[5]);

    // ─── Подсказки клавиш ────────────────────────────────────────────────
    let hints = Paragraph::new("[Tab] сменить поле   [Enter] подключиться   [Esc] выйти")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::DarkGray));
    let hints_area = ratatui::layout::Rect {
        x: area.x,
        y: area.y + area.height - 2,
        width: area.width,
        height: 1,
    };
    f.render_widget(hints, hints_area);
}
