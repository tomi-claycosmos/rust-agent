use crate::llm::LlmMessage;

// ─────────────────────────────────────────────
// Token Budget — Anthropic Context Engineering 启发
// ─────────────────────────────────────────────

/// Token 预算管理器
///
/// Anthropic 发现：token 使用量解释了 ~80% 的性能方差。
/// 监控并管理 token 预算，是 Context Engineering 的核心。
#[derive(Debug, Clone)]
pub struct TokenBudget {
    /// 当前已用 token 数
    pub current: usize,
    /// 上限（由模型决定）
    pub limit: usize,
    /// 系统 prompt 的 token 数（不计入可压缩部分）
    pub system_tokens: usize,
    /// 压缩后保留的最新消息条数
    pub recent_keep: usize,
}

impl TokenBudget {
    /// 新建预算（默认 128k 上下文）
    pub fn new(limit: usize) -> Self {
        Self {
            current: 0,
            limit,
            system_tokens: 0,
            recent_keep: 20,
        }
    }

    /// 设置系统 prompt token 数
    pub fn set_system_tokens(&mut self, tokens: usize) {
        self.system_tokens = tokens;
        self.current += tokens;
    }

    /// 更新当前使用量
    pub fn update(&mut self, messages: &[LlmMessage]) {
        self.current = self.system_tokens;
        for msg in messages {
            self.current += estimate_message_tokens(msg);
        }
    }

    /// 剩余可用 token
    pub fn available(&self) -> usize {
        self.limit.saturating_sub(self.current)
    }

    /// 使用率（0.0 ~ 1.0）
    pub fn usage_ratio(&self) -> f64 {
        self.current as f64 / self.limit as f64
    }

    /// 是否需要压缩
    pub fn needs_compression(&self, threshold: f64) -> bool {
        self.usage_ratio() > threshold
    }

    /// 剩余百分比描述
    pub fn status_string(&self) -> String {
        let pct = self.usage_ratio() * 100.0;
        let avail = self.available();
        format!(
            "{:.1}% used ({}/{} tokens, {} available)",
            pct, self.current, self.limit, avail
        )
    }
}

/// 估算消息的 token 数量
fn estimate_message_tokens(msg: &LlmMessage) -> usize {
    let role_tokens = 4;
    let content_tokens = estimate_text_tokens(&msg.content);
    role_tokens + content_tokens
}

/// 估算文本 token 数量（粗略近似）
fn estimate_text_tokens(text: &str) -> usize {
    let char_count = text.chars().count();
    let english_chars = text.chars().filter(|c| c.is_ascii()).count();
    let chinese_chars = char_count - english_chars;
    (english_chars as f64 * 1.3 / 1.0) as usize + chinese_chars * 2
}

// ─────────────────────────────────────────────
// Conversation Memory — 带 token 预算的记忆
// ─────────────────────────────────────────────

/// 对话历史管理器
///
/// 支持：
/// - 自动 token 计数和预算管理
/// - 超过阈值时自动压缩旧消息
/// - Anthropic Context Engineering 策略：系统 prompt 不压缩，保留最近 N 条
#[derive(Debug, Clone)]
pub struct ConversationMemory {
    messages: Vec<LlmMessage>,
    budget: TokenBudget,
    compression_threshold: f64,
}

impl ConversationMemory {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            budget: TokenBudget::new(128_000),
            compression_threshold: 0.75,
        }
    }

    pub fn with_limit(limit: usize) -> Self {
        Self {
            messages: Vec::new(),
            budget: TokenBudget::new(limit),
            compression_threshold: 0.75,
        }
    }

    pub fn with_threshold(threshold: f64) -> Self {
        Self {
            messages: Vec::new(),
            budget: TokenBudget::new(128_000),
            compression_threshold: threshold,
        }
    }

    // ── 消息操作 ──

    pub fn add_system_message(&mut self, content: &str) {
        self.messages.insert(0, LlmMessage {
            role: "system".to_string(),
            content: content.to_string(),
            name: None,
            tool_call_id: None,
        });
        self.budget.set_system_tokens(estimate_text_tokens(content) + 4);
    }

    pub fn add_user_message(&mut self, content: &str) {
        self.messages.push(LlmMessage {
            role: "user".to_string(),
            content: content.to_string(),
            name: None,
            tool_call_id: None,
        });
        self.try_compress();
    }

    pub fn add_assistant_message(&mut self, content: &str) {
        self.messages.push(LlmMessage {
            role: "assistant".to_string(),
            content: content.to_string(),
            name: None,
            tool_call_id: None,
        });
    }

    pub fn add_tool_result(&mut self, tool_call_id: &str, result: &str) {
        self.messages.push(LlmMessage {
            role: "tool".to_string(),
            content: result.to_string(),
            name: None,
            tool_call_id: Some(tool_call_id.to_string()),
        });
        self.try_compress();
    }

    pub fn get_messages(&self) -> Vec<LlmMessage> {
        self.messages.clone()
    }

    pub fn clear(&mut self) {
        let system_msg = self.messages.first().cloned();
        self.messages.clear();
        if let Some(msg) = system_msg {
            self.messages.push(msg);
            self.budget.update(&self.messages);
        }
    }

    // ── Token 预算管理 ──

    /// 打印当前 token 预算状态（供调试和监控）
    pub fn print_budget_status(&self) {
        eprintln!("  💰 Token Budget: {}", self.budget.status_string());
    }

    /// 获取当前 token 预算信息
    pub fn budget_info(&self) -> &TokenBudget {
        &self.budget
    }

    /// 更新压缩阈值
    pub fn set_compression_threshold(&mut self, threshold: f64) {
        self.compression_threshold = threshold;
    }

    /// 尝试压缩（如果超过阈值）
    fn try_compress(&mut self) {
        self.budget.update(&self.messages);

        if !self.budget.needs_compression(self.compression_threshold) {
            return;
        }

        eprintln!(
            "  ⚠️  Token 预算超限 ({:.0}%)，开始压缩旧消息...",
            self.budget.usage_ratio() * 100.0
        );

        self.compress();
    }

    /// 执行压缩：保留系统 prompt + 最近 N 条
    fn compress(&mut self) {
        let system_messages: Vec<_> = self.messages.iter()
            .filter(|m| m.role == "system")
            .cloned()
            .collect();

        let non_system: Vec<_> = self.messages.iter()
            .filter(|m| m.role != "system")
            .cloned()
            .collect();

        let keep_count = self.budget.recent_keep;
        let to_keep = non_system.len().min(keep_count);
        let kept = if to_keep > 0 {
            non_system[non_system.len() - to_keep..].to_vec()
        } else {
            vec![]
        };

        let dropped_count = non_system.len().saturating_sub(to_keep);
        if dropped_count > 0 {
            let user_count = non_system.iter().filter(|m| m.role == "user").count();
            let assistant_count = non_system.iter().filter(|m| m.role == "assistant").count();
            let summary = LlmMessage {
                role: "assistant".to_string(),
                content: format!(
                    "[系统: 早期 {} 条对话已压缩为摘要]\n\
                    最近 {} 条消息保留了完整内容。\
                    摘要：共 {} 条消息，其中 {} 条用户消息和 {} 条助手消息。",
                    dropped_count,
                    to_keep,
                    non_system.len(),
                    user_count,
                    assistant_count,
                ),
                name: None,
                tool_call_id: None,
            };

            let old_len = self.messages.len();
            self.messages.clear();
            self.messages.extend(system_messages);
            self.messages.push(summary);
            self.messages.extend(kept);
            self.budget.update(&self.messages);

            eprintln!(
                "  ✅ 压缩完成：{} → {} 条消息",
                old_len, self.messages.len()
            );
        }
    }

    pub fn force_compress(&mut self) {
        self.compress();
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

impl Default for ConversationMemory {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────
// 序列化支持（用于持久化记忆）
// ─────────────────────────────────────────────

impl ConversationMemory {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.messages)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let messages: Vec<LlmMessage> = serde_json::from_str(json)?;
        let mut mem = Self::new();
        mem.messages = messages;
        mem.budget.update(&mem.messages);
        Ok(mem)
    }
}
