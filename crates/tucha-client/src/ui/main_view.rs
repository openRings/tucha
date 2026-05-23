use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph},
};
use tucha_proto::UserInfo;

use crate::app::{AppState, Focus};
use super::widgets::{to_dbfs, vu_color, vu_meter};

pub fn render(f: &mut Frame, state: &AppState) {
    let area = f.area();

    // ─── Корневой layout ─────────────────────────────────────────────────
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // statusbar
            Constraint::Min(0),     // основная область
            Constraint::Length(1),  // hints
        ])
        .split(area);

    render_statusbar(f, state, rows[0]);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(22), // левая панель
            Constraint::Min(0),     // правая (комната)
        ])
        .split(rows[1]);

    render_left_panel(f, state, cols[0]);
    render_right_panel(f, state, cols[1]);
    render_hints(f, rows[2]);
}

// ─── Статус-бар ──────────────────────────────────────────────────────────────

fn render_statusbar(f: &mut Frame, state: &AppState, area: Rect) {
    let status = match &state.conn_status {
        crate::app::ConnStatus::Connected => {
            let server = &state.server_input;
            let name = &state.username_input;
            format!(" tucha  ●  {server}  │  {name} ")
        }
        _ => " tucha  ○  не подключён ".into(),
    };

    let mute_str = if state.is_muted { " 🔇 MUTED" } else { "" };
    let deaf_str = if state.is_deafened { " 🎧 DEAF" } else { "" };

    let line = Line::from(vec![
        Span::styled(status, Style::default().fg(Color::Cyan)),
        Span::styled(mute_str, Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::styled(deaf_str, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    ]);

    let bar = Paragraph::new(line)
        .style(Style::default().bg(Color::DarkGray));
    f.render_widget(bar, area);
}

// ─── Левая панель: комнаты + участники ───────────────────────────────────────

fn render_left_panel(f: &mut Frame, state: &AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(55),
            Constraint::Percentage(45),
        ])
        .split(area);

    render_rooms(f, state, chunks[0]);
    render_users(f, state, chunks[1]);
}

fn render_rooms(f: &mut Frame, state: &AppState, area: Rect) {
    let focused = state.focus == Focus::Rooms;
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(" Комнаты ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style);

    let items: Vec<ListItem> = state.rooms.iter().enumerate().map(|(i, room)| {
        let is_current = state.current_room == Some(room.id);
        let is_selected = i == state.room_list_idx && focused;
        let count = room.users.len();

        let prefix = if is_current { "▶ " } else { "  " };
        let label = format!("{prefix}#{:<12} {count}", room.name);

        let style = if is_current {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::default().fg(Color::White).bg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Gray)
        };

        ListItem::new(label).style(style)
    }).collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn render_users(f: &mut Frame, state: &AppState, area: Rect) {
    let focused = state.focus == Focus::Users;
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(" Участники ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style);

    let users_in_all_rooms: Vec<&UserInfo> = state.rooms.iter()
        .flat_map(|r| r.users.iter())
        .collect();

    let items: Vec<ListItem> = users_in_all_rooms.iter().enumerate().map(|(i, user)| {
        let is_me = state.my_user_id == Some(user.id);
        let is_selected = i == state.user_list_idx && focused;
        let speaking = state.speaking.get(&user.id).copied().unwrap_or(false);

        let icon = if user.muted { "🔇" } else if speaking { "🎙" } else { "👤" };
        let suffix = if is_me { " ★" } else { "" };
        let label = format!("{icon} {}{suffix}", user.username);

        let style = if is_me {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::default().fg(Color::White).bg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Gray)
        };

        ListItem::new(label).style(style)
    }).collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

// ─── Правая панель: участники комнаты с VU-метрами + аудио-устройства ────────

fn render_right_panel(f: &mut Frame, state: &AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(5),
        ])
        .split(area);

    render_room_view(f, state, chunks[0]);
    render_audio_devices_bar(f, state, chunks[1]);
}

fn render_room_view(f: &mut Frame, state: &AppState, area: Rect) {
    let title = match state.current_room_info() {
        Some(r) => format!(" #{} — {} участников ", r.name, r.users.len()),
        None => " Не в комнате ".into(),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if state.current_room.is_none() {
        let hint = Paragraph::new("\n  Выберите комнату в списке слева и нажмите [Enter]")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(hint, inner);
        return;
    }

    // Список пользователей с VU-метрами
    let meter_width = (inner.width.saturating_sub(28)) as u16;

    let mut lines: Vec<Line> = Vec::new();

    // Сначала я сам
    if let Some(my_id) = state.my_user_id {
        let level = state.my_input_level;
        let speaking = !state.is_muted && level > 0.01;
        lines.push(user_line(
            "вы ★",
            my_id,
            level,
            state.is_muted,
            speaking,
            meter_width,
            state,
        ));
    }

    // Потом остальные
    for user in state.current_users() {
        if state.my_user_id == Some(user.id) { continue; }
        let level = state.volume_levels.get(&user.id).copied().unwrap_or(0.0);
        let speaking = state.speaking.get(&user.id).copied().unwrap_or(false);
        lines.push(user_line(
            &user.username,
            user.id,
            level,
            user.muted,
            speaking,
            meter_width,
            state,
        ));
    }

    let p = Paragraph::new(lines);
    f.render_widget(p, inner);
}

fn user_line<'a>(
    name: &str,
    _user_id: tucha_proto::UserId,
    level: f32,
    muted: bool,
    speaking: bool,
    meter_width: u16,
    _state: &AppState,
) -> Line<'a> {
    let icon = if muted { "🔇" } else if speaking { "🎙" } else { "·" };
    let name_col = format!(" {icon} {:<16}", name);
    let db = to_dbfs(level);
    let db_str = if db < -59.0 {
        format!("{:>6}", "-∞ dB")
    } else {
        format!("{:>6.1} dB", db)
    };
    let meter = vu_meter(level, meter_width.max(8));
    let color = vu_color(level);

    Line::from(vec![
        Span::styled(name_col, if speaking {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        }),
        Span::styled(meter, Style::default().fg(color)),
        Span::styled(format!(" {db_str}"), Style::default().fg(Color::DarkGray)),
    ])
}

fn render_audio_devices_bar(f: &mut Frame, state: &AppState, area: Rect) {
    let block = Block::default()
        .title(" Аудиоустройства  [o] настроить ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let mic = state.input_devices.get(state.selected_input)
        .map(|s| s.as_str())
        .unwrap_or("—");
    let spk = state.output_devices.get(state.selected_output)
        .map(|s| s.as_str())
        .unwrap_or("—");

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    let mic_line = Paragraph::new(format!(" 🎙 Микрофон:  {mic}"))
        .style(Style::default().fg(Color::Gray));
    let spk_line = Paragraph::new(format!(" 🔊 Динамик:   {spk}"))
        .style(Style::default().fg(Color::Gray));

    f.render_widget(mic_line, chunks[0]);
    f.render_widget(spk_line, chunks[1]);
}

// ─── Подсказки клавиш ────────────────────────────────────────────────────────

fn render_hints(f: &mut Frame, area: Rect) {
    let hints = "[j/k] навигация  [Enter] войти  [m] mute  [d] deafen  [o] устройства  [n] новая комната  [q] выйти";
    let p = Paragraph::new(hints)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    f.render_widget(p, area);
}
