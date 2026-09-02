//! TUI 应用状态。

use sts2_core::{GameState, StateType};

/// 对话消息。
#[derive(Debug, Clone)]
pub struct ChatMsg {
    pub role: MsgRole,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MsgRole {
    User,
    Agent,
    System,
}

/// UI 状态。
pub struct AppState {
    pub game_state: GameState,
    pub chat: Vec<ChatMsg>,
    pub streaming_text: String,
    pub reasoning_text: String,
    pub current_turn: u32,
    pub total_input: u64,
    pub total_output: u64,
    pub total_cost: f64,
    pub progress: Option<String>,
    pub show_thinking: bool,
    #[allow(dead_code)]
    pub zh: bool,
    pub finished: bool,
    pub pending_action: Option<String>,
    pub input: String,
    pub cursor: usize,
    /// 对话面板滚动偏移（0=最底部，递增=向上滚）。
    pub chat_scroll: u16,
}

/// 运行模式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Idle,
    FetchingState,
    Streaming,
    Executing,
    PendingConfirm,
}

impl AppState {
    pub fn new(zh: bool) -> Self {
        Self {
            game_state: GameState::default(),
            chat: Vec::new(),
            streaming_text: String::new(),
            reasoning_text: String::new(),
            current_turn: 0,
            total_input: 0,
            total_output: 0,
            total_cost: 0.0,
            progress: None,
            show_thinking: false,
            zh,
            finished: false,
            pending_action: None,
            input: String::new(),
            cursor: 0,
            chat_scroll: 0,
        }
    }

    #[allow(dead_code)]
    pub fn toggle_thinking(&mut self) {
        self.show_thinking = !self.show_thinking;
    }

    pub fn status_color(&self) -> ratatui::style::Color {
        use ratatui::style::Color;
        if self.finished {
            Color::Yellow
        } else if self.progress.is_some() {
            Color::Green
        } else {
            Color::Cyan
        }
    }

    pub fn input_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            // 找到前一个字符边界
            let s = &self.input[..self.cursor];
            if let Some(prev) = s.chars().last() {
                self.cursor -= prev.len_utf8();
                self.input.remove(self.cursor);
            }
        }
    }

    pub fn submit(&mut self) -> String {
        let text = self.input.clone();
        self.input.clear();
        self.cursor = 0;
        text
    }

    /// 推入对话消息并自动重置滚动。
    pub fn push_chat(&mut self, role: MsgRole, text: String) {
        self.chat.push(ChatMsg { role, text });
        self.chat_scroll = 0;
    }
}

/// 计算字符串的显示宽度（ASCII=1，CJK=2，其他=1）。
pub fn display_width(s: &str) -> u16 {
    s.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum()
}

pub fn state_lines(gs: &GameState) -> Vec<String> {
    let mut lines = Vec::new();
    let p = match gs.player.as_ref() {
        Some(p) => p,
        None => return vec!["无角色信息".into()],
    };

    lines.push(format!("职业: {}", p.character));
    lines.push(format!("HP: {}/{}", p.hp, p.max_hp));
    if p.block > 0 {
        lines.push(format!("格挡: {}", p.block));
    }
    if let Some(e) = p.energy {
        lines.push(format!("能量: {}/{}", e, p.max_energy.unwrap_or(3)));
    }
    lines.push(format!("金币: {}", p.gold));

    if let Some(hand) = &p.hand {
        if !hand.is_empty() {
            lines.push(String::new());
            lines.push("手牌:".into());
            for card in hand {
                let playable = if card.can_play.unwrap_or(true) {
                    ""
                } else {
                    " [不可出]"
                };
                lines.push(format!(
                    "  [{}] {} {}费{}",
                    card.index.unwrap_or(0),
                    card.name,
                    card.cost,
                    playable
                ));
            }
        }
    }

    if let Some(b) = &gs.battle {
        if !b.enemies.is_empty() {
            lines.push(String::new());
            lines.push("敌人:".into());
            for e in &b.enemies {
                let intent = e
                    .intents
                    .first()
                    .map(|i| format!("{} {}", i.kind, i.label.as_deref().unwrap_or("")))
                    .unwrap_or_default();
                lines.push(format!(
                    "  {} {}/{} HP | {}",
                    e.name, e.hp, e.max_hp, intent
                ));
            }
        }
    }

    if let Some(m) = &gs.map {
        if !m.next_options.is_empty() {
            lines.push(String::new());
            lines.push("路径:".into());
            for opt in &m.next_options {
                lines.push(format!(
                    "  [{}] {}",
                    opt.index.unwrap_or(0),
                    opt.kind.as_deref().unwrap_or("?")
                ));
            }
        }
    }

    if let Some(r) = &gs.rewards {
        if !r.items.is_empty() {
            lines.push(String::new());
            lines.push("奖励:".into());
            for item in &r.items {
                lines.push(format!(
                    "  [{}] {}",
                    item.index.unwrap_or(0),
                    item.description.as_deref().unwrap_or("")
                ));
            }
        }
    }

    let _ = gs.state_type;
    lines
}

pub fn state_summary(gs: &GameState) -> String {
    match gs.state_type {
        StateType::Map => "地图".into(),
        StateType::Monster | StateType::Elite | StateType::Boss => {
            let b = gs.battle.as_ref();
            let p = gs.player.as_ref();
            match (b, p) {
                (Some(b), Some(p)) => format!(
                    "战斗 R{} | {}/{} HP {} 能量 | {}",
                    b.round.unwrap_or(0),
                    p.hp,
                    p.max_hp,
                    p.energy.unwrap_or(0),
                    b.enemies.first().map(|e| e.name.as_str()).unwrap_or("?")
                ),
                _ => "战斗".into(),
            }
        }
        StateType::Rewards => "奖励".into(),
        StateType::RestSite => "休息点".into(),
        StateType::Shop | StateType::FakeMerchant => "商店".into(),
        StateType::Event => "事件".into(),
        StateType::Treasure => "宝箱".into(),
        _ => format!("{:?}", gs.state_type),
    }
}
