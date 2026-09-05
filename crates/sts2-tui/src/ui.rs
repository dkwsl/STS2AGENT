//! ratatui 布局：
//! 顶栏(1) | 主体(左状态 + 右对话可滚动) | 底部输入框(全宽)

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::app::{display_width, state_lines, AppState, MsgRole};

pub fn draw(f: &mut Frame, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // 顶栏
            Constraint::Min(8),    // 主体
            Constraint::Length(3), // 输入框
        ])
        .split(f.area());

    draw_top_bar(f, state, chunks[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(28), Constraint::Min(20)])
        .split(chunks[1]);

    draw_state_panel(f, state, body[0]);
    draw_chat_panel(f, state, body[1]);

    draw_input_box(f, state, chunks[2]);
}

fn draw_top_bar(f: &mut Frame, state: &AppState, area: Rect) {
    let title = format!(
        " STS2 Agent · R{} │ in={} out={} │ ${:.4} │ {}",
        state.current_turn,
        state.total_input,
        state.total_output,
        state.total_cost,
        state.progress.as_deref().unwrap_or("就绪"),
    );
    let style = Style::default()
        .fg(state.status_color())
        .add_modifier(Modifier::BOLD);
    f.render_widget(Paragraph::new(title).style(style), area);
}

fn draw_state_panel(f: &mut Frame, state: &AppState, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" 状态 ")
        .border_style(Color::DarkGray);
    let lines = state_lines(&state.game_state);
    let items: Vec<ListItem> = lines.iter().map(|l| ListItem::new(l.as_str())).collect();
    f.render_widget(List::new(items).block(block), area);
}

fn draw_chat_panel(f: &mut Frame, state: &AppState, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" 对话 ")
        .border_style(Color::DarkGray);

    let mut lines: Vec<String> = Vec::new();

    // 历史对话
    for msg in &state.chat {
        let prefix = match msg.role {
            MsgRole::User => "你",
            MsgRole::Agent => "Agent",
            MsgRole::System => "系统",
        };
        lines.push(format!("[{prefix}]"));
        for line in msg.text.lines() {
            lines.push(format!("  {line}"));
        }
        lines.push(String::new());
    }

    // 流式输出
    if !state.streaming_text.is_empty() {
        lines.push("[Agent]".into());
        for line in state.streaming_text.lines() {
            lines.push(format!("  {line}"));
        }
    }

    // 思考
    if state.show_thinking && !state.reasoning_text.is_empty() {
        lines.push(String::new());
        lines.push("(思考)".into());
        for line in state.reasoning_text.lines().take(10) {
            lines.push(format!("  {line}"));
        }
    }

    // Pending
    if let Some(action) = &state.pending_action {
        lines.push(String::new());
        lines.push(format!("⏳ 待确认: {action}"));
        lines.push("  输入「执行」确认 / 输入其他文字与 Agent 沟通".into());
    }

    // 手动换行（不用 Wrap，使行数准确，scroll offset 精确）
    let visible = area.height.saturating_sub(2) as usize;
    let max_width = area.width.saturating_sub(2) as usize;
    let wrapped: Vec<String> = lines.iter().flat_map(|l| wrap_line(l, max_width)).collect();

    let content = wrapped.join("\n");
    let content_lines = content.lines().count() as u16;
    let max_scroll = content_lines.saturating_sub(visible as u16);
    let scroll_offset = max_scroll.saturating_sub(state.chat_scroll);

    let p = Paragraph::new(content)
        .scroll((scroll_offset, 0))
        .block(block);
    f.render_widget(p, area);
}

/// 按显示宽度换行（CJK=2列），不截断内容。
fn wrap_line(line: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![line.to_string()];
    }
    let width = crate::app::display_width(line) as usize;
    if width <= max_width {
        return vec![line.to_string()];
    }
    let mut result = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for c in line.chars() {
        let w = if c.is_ascii() { 1 } else { 2 };
        if current_width + w > max_width && !current.is_empty() {
            result.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(c);
        current_width += w;
    }
    if !current.is_empty() {
        result.push(current);
    }
    if result.is_empty() {
        vec![line.to_string()]
    } else {
        result
    }
}

fn draw_input_box(f: &mut Frame, state: &AppState, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" 输入（回车发送，↑↓ 滚动对话） ")
        .border_style(Color::Cyan);

    // 直接渲染输入文本，不附加 █（避免多字节字符截断 panic）
    let p = Paragraph::new(state.input.as_str())
        .style(Style::default().fg(Color::White))
        .block(block);
    f.render_widget(p, area);

    // 用 set_cursor_position 显示终端光标
    let inner = area;
    let border_offset = 1u16;
    let cursor_x =
        border_offset + display_width(&state.input[..state.cursor.min(state.input.len())]);
    f.set_cursor_position((inner.x + cursor_x, inner.y + border_offset));
}
