//! TUI 应用状态。

use tokio_util::sync::CancellationToken;

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
    /// 是否自动执行 ACTION 行（true=用户指令时执行，false=仅建议）。
    pub execute_actions: bool,
    /// 自主模式：用户说了"你自己打"等，Agent 连续操作直到用户喊停。
    pub auto_mode: bool,
    /// 当前任务描述（用户指令的原文），每次分析时提醒 LLM 目标。
    pub task: Option<String>,
    /// 当前 LLM 流的 cancel token，打断时用。
    pub current_cancel: CancellationToken,
    /// 当前会话 ID（用于读取 session 笔记）。
    pub session_id: String,
    /// 上一次已知的状态 JSON（用于检测用户手动操作）。
    pub last_state_json: String,
    /// LLM 正在分析的状态 JSON（决策开始时的快照）。
    /// StreamDone 时与当前 last_state_json 比较，不一致则作废。
    pub decision_state_json: String,
    /// 上次状态轮询时间（Instant 的简单替代：SystemTime）。
    pub last_poll: std::time::Instant,
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
    #[allow(dead_code)]
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
            execute_actions: false,
            auto_mode: false,
            task: None,
            current_cancel: CancellationToken::new(),
            session_id: String::new(),
            last_state_json: String::new(),
            decision_state_json: String::new(),
            last_poll: std::time::Instant::now(),
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

/// K5: 从 GameState 提取关键词（转发到 sts2_agent::knowledge）。
pub fn extract_keywords_external(gs: &GameState) -> Vec<String> {
    sts2_agent::knowledge::extract_keywords(gs)
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

    // 卡牌奖励选择
    if let Some(cr) = &gs.card_reward {
        if !cr.cards.is_empty() {
            lines.push(String::new());
            lines.push("可选卡牌:".into());
            for card in &cr.cards {
                lines.push(format!(
                    "  [{}] {} {}费 {}",
                    card.index.unwrap_or(0),
                    card.name,
                    card.cost,
                    card.description.as_deref().unwrap_or("")
                ));
            }
            lines.push(format!("  可跳过: {}", cr.can_skip.unwrap_or(false)));
        }
    }

    // 卡牌选择叠层（transform/upgrade/remove/choose）
    if let Some(cs) = &gs.card_select {
        lines.push(String::new());
        if let Some(p) = &cs.prompt {
            lines.push(format!("卡牌选择: {p}"));
        } else {
            lines.push("卡牌选择:".into());
        }
        if let Some(st) = &cs.screen_type {
            lines.push(format!("  类型: {st}"));
        }
        for card in &cs.cards {
            lines.push(format!(
                "  [{}] {} {}费 {}",
                card.index.unwrap_or(0),
                card.name,
                card.cost,
                card.description.as_deref().unwrap_or("")
            ));
        }
        lines.push(format!(
            "  可确认: {} | 可取消: {} | 预览中: {}",
            cs.can_confirm.unwrap_or(false),
            cs.can_cancel.unwrap_or(false),
            cs.preview_showing.unwrap_or(false)
        ));
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
        StateType::CardReward => "选牌".into(),
        StateType::CardSelect => "卡牌选择".into(),
        StateType::RestSite => "休息点".into(),
        StateType::Shop | StateType::FakeMerchant => "商店".into(),
        StateType::Event => "事件".into(),
        StateType::Treasure => "宝箱".into(),
        _ => format!("{:?}", gs.state_type),
    }
}
