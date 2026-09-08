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
    // 右上角：自主模式标志
    if state.auto_mode {
        let badge = Paragraph::new("🤖 自主模式")
            .style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(ratatui::layout::Alignment::Right);
        f.render_widget(badge, area);
    }
    let progress = match (state.stream_started, &state.progress) {
        (Some(t0), Some(p)) if p.contains("中…") => {
            format!("{p} {:.0}s", t0.elapsed().as_secs_f32())
        }
        (_, Some(p)) => p.clone(),
        (Some(_), None) => "LLM 响应中…".into(),
        _ => "就绪".into(),
    };
    let title = format!(
        " STS2 Agent · R{} │ in={} out={} │ ${:.4} │ {}",
        state.current_turn, state.total_input, state.total_output, state.total_cost, progress,
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
    use ratatui::text::{Line, Span, Text};

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" 对话 ")
        .border_style(Color::DarkGray);
    let dim = Style::default().fg(Color::DarkGray);

    // 组装带样式的行：思考过程（流式+历史）用浅色
    let mut lines: Vec<Line> = Vec::new();

    // 历史对话
    for msg in &state.chat {
        let (prefix, styled) = match msg.role {
            MsgRole::User => ("你", false),
            MsgRole::Agent => ("Agent", false),
            MsgRole::System => ("系统", false),
            MsgRole::Thinking => ("思考", true),
        };
        let head = if styled {
            Span::styled(format!("[{prefix}]"), dim)
        } else {
            Span::raw(format!("[{prefix}]"))
        };
        lines.push(Line::from(vec![head]));
        for line in msg.text.lines() {
            let l = format!("  {line}");
            lines.push(if styled {
                Line::from(Span::styled(l, dim))
            } else {
                Line::from(Span::raw(l))
            });
        }
        lines.push(Line::from(Span::raw("")));
    }

    // 流式思考（时序在回答之前，浅色实时滚动）
    if state.show_thinking && !state.reasoning_text.is_empty() {
        lines.push(Line::from(Span::styled("(思考中)", dim)));
        for line in state.reasoning_text.lines() {
            lines.push(Line::from(Span::styled(format!("  {line}"), dim)));
        }
    }

    // 流式回答
    if !state.streaming_text.is_empty() {
        lines.push(Line::from(Span::raw("[Agent]")));
        for line in state.streaming_text.lines() {
            lines.push(Line::from(Span::raw(format!("  {line}"))));
        }
    }

    // Pending
    if let Some(action) = &state.pending_action {
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::raw(format!("⏳ 待确认: {action}"))));
        lines.push(Line::from(Span::raw(
            "  输入「执行」确认 / 输入其他文字与 Agent 沟通".to_string(),
        )));
    }

    // 手动换行（保持每行样式；行数准确使 scroll offset 精确）
    let visible = area.height.saturating_sub(2) as usize;
    let max_width = area.width.saturating_sub(2) as usize;
    let wrapped: Vec<Line> = lines
        .into_iter()
        .flat_map(|l| {
            let styled = l.spans.first().map(|s| s.style).unwrap_or_default();
            let raw: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
            wrap_line(&raw, max_width).into_iter().map(move |w| {
                if styled.fg.is_some() {
                    Line::from(Span::styled(w, styled))
                } else {
                    Line::from(Span::raw(w))
                }
            })
        })
        .collect();

    let text = Text::from(wrapped);
    let content_lines = text.lines.len() as u16;
    let max_scroll = content_lines.saturating_sub(visible as u16);
    let scroll_offset = max_scroll.saturating_sub(state.chat_scroll);

    let p = Paragraph::new(text).scroll((scroll_offset, 0)).block(block);
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
