use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

/// Горизонтальный VU-метр (0.0 – 1.0)
pub fn vu_meter(level: f32, width: u16) -> String {
    let filled = ((level.clamp(0.0, 1.0) * width as f32) as usize).min(width as usize);
    let empty = width as usize - filled;
    let bar: String = "█".repeat(filled) + &"░".repeat(empty);

    // Цвет по уровню
    bar
}

/// Цвет VU по уровню
pub fn vu_color(level: f32) -> Color {
    if level > 0.8 { Color::Red }
    else if level > 0.5 { Color::Yellow }
    else { Color::Green }
}

/// dBFS из линейного уровня
pub fn to_dbfs(level: f32) -> f32 {
    if level < 1e-6 { return -60.0; }
    20.0 * level.log10()
}

/// Всплывающее уведомление внизу экрана
pub fn render_notification(f: &mut Frame, msg: &str) {
    let area = f.area();
    let width = (msg.len() as u16 + 4).min(area.width);
    let x = area.width.saturating_sub(width + 2);
    let y = area.height.saturating_sub(3);
    let notif_area = Rect { x, y, width, height: 3 };

    let p = Paragraph::new(msg)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .style(Style::default().bg(Color::DarkGray)),
        )
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Center);

    f.render_widget(Clear, notif_area);
    f.render_widget(p, notif_area);
}

/// Центрированный прямоугольник
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
