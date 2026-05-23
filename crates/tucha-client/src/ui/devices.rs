use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph},
};

use crate::app::{AppState, DeviceFocus};
use super::widgets::centered_rect;

pub fn render_overlay(f: &mut Frame, state: &AppState) {
    let area = f.area();
    let popup = centered_rect(60, 55, area);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .title("  Аудиоустройства  ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));
    f.render_widget(block.clone(), popup);

    let inner = block.inner(popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // заголовок
            Constraint::Percentage(50),
            Constraint::Length(1),  // разделитель
            Constraint::Percentage(50),
            Constraint::Length(1),  // hints
        ])
        .split(inner);

    // ─── Микрофоны ────────────────────────────────────────────────────────
    let mic_focused = state.device_focus == DeviceFocus::Input;
    let mic_title = if mic_focused { "▶ Микрофон" } else { "  Микрофон" };
    let mic_border = if mic_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let mic_block = Block::default()
        .title(mic_title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(mic_border);

    let mic_items: Vec<ListItem> = state.input_devices.iter().enumerate().map(|(i, d)| {
        let selected = i == state.selected_input;
        let prefix = if selected { "● " } else { "  " };
        let style = if selected && mic_focused {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };
        ListItem::new(format!("{prefix}{d}")).style(style)
    }).collect();

    let mic_list = List::new(mic_items).block(mic_block);
    f.render_widget(mic_list, chunks[1]);

    // ─── Динамики ─────────────────────────────────────────────────────────
    let spk_focused = state.device_focus == DeviceFocus::Output;
    let spk_title = if spk_focused { "▶ Динамик" } else { "  Динамик" };
    let spk_border = if spk_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let spk_block = Block::default()
        .title(spk_title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(spk_border);

    let spk_items: Vec<ListItem> = state.output_devices.iter().enumerate().map(|(i, d)| {
        let selected = i == state.selected_output;
        let prefix = if selected { "● " } else { "  " };
        let style = if selected && spk_focused {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };
        ListItem::new(format!("{prefix}{d}")).style(style)
    }).collect();

    let spk_list = List::new(spk_items).block(spk_block);
    f.render_widget(spk_list, chunks[3]);

    // ─── Hints ────────────────────────────────────────────────────────────
    let hints = Paragraph::new("[j/k] выбор   [Tab] переключить секцию   [Enter] применить   [Esc] закрыть")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    f.render_widget(hints, chunks[4]);
}
