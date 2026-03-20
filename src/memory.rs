//! # Memory — Agent 的记忆系统
//!
//! ## 为什么 Agent 需要 Memory？
//!
//! LLM 是无状态的——每次调用都是独立的。
//! 要让 Agent 有"连续对话"能力，必须手动维护对话历史。
//!
//! ## Memory 的几种类型
//!
//! 1. **Short-term (工作记忆)**: 当前对话上下文
//!    - 存在 `messages` 数组里
//!    - 包含完整的对话历史
//!    - 受限于 LLM 的 context window
//!
//! 2. **Long-term (长期记忆)**: 跨会话持久化
//!    - 存在文件/数据库
//!    - 包含用户偏好、历史交互
//!    - 可以被检索
//!
//! 3. **Episodic (情景记忆)**: 按事件分段
//!    - "用户上次问过关于 X 的问题"
//!    - "Agent 曾经这样解决过 Y 问题"
//!
//! ## Context Window 的限制
//!
//! LLM 有最大 token 数限制（如 GPT-4o = 128k tokens）：
//!
//! ```text
//! ┌────────────┬────────────┬────────────┐
//! │ System     │ History    │ Available  │
//! │ Prompt     │ Messages   │ for Input  │
//! │ (~4k tok)  │ (~80k tok) │ (~44k tok) │
//! └────────────┴────────────┴────────────┘
//! ```
//!
//! 当历史太长时，需要：
//! - **Summarization（摘要）**: 把旧消息压缩
//! - **Retrieval（检索）**: 只取相关内容

use crate::llm::LlmMessage;

const MAX_MESSAGES: usize = 50; // 简单截断策略

/// 对话记忆（短期记忆）
pub struct ConversationMemory {
    messages: Vec<LlmMessage>,
}

impl ConversationMemory {
    pub fn new() -> Self {
        Self { messages: Vec::new() }
    }

    /// 获取所有消息（用于发给 LLM）
    pub fn get_messages(&self) -> Vec<LlmMessage> {
        self.messages.clone()
    }

    /// 添加用户消息
    pub fn add_user_message(&mut self, content: &str) {
        self.messages.push(LlmMessage {
            role: "user".to_string(),
            content: content.to_string(),
            name: None,
            tool_call_id: None,
        });

        // 简单截断：超过 MAX_MESSAGES 条时，合并前面的消息
        self.trim_if_needed();
    }

    /// 添加助手消息
    pub fn add_assistant_message(&mut self, content: &str) {
        self.messages.push(LlmMessage {
            role: "assistant".to_string(),
            content: content.to_string(),
            name: None,
            tool_call_id: None,
        });
        self.trim_if_needed();
    }

    /// 添加工具执行结果
    pub fn add_tool_result(&mut self, tool_call_id: &str, result: &str) {
        self.messages.push(LlmMessage {
            role: "tool".to_string(),
            content: result.to_string(),
            name: None,
            tool_call_id: Some(tool_call_id.to_string()),
        });
    }

    /// 添加系统消息
    pub fn add_system_message(&mut self, content: &str) {
        self.messages.insert(0, LlmMessage {
            role: "system".to_string(),
            content: content.to_string(),
            name: None,
            tool_call_id: None,
        });
    }

    /// 清空记忆
    pub fn clear(&mut self) {
        // 保留第一条 system message
        let system = self.messages.iter().find(|m| m.role == "system").cloned();
        self.messages.clear();
        if let Some(sys) = system {
            self.messages.push(sys);
        }
    }

    /// 截断逻辑
    fn trim_if_needed(&mut self) {
        if self.messages.len() > MAX_MESSAGES {
            // 保留第一条（system）和最后 MAX_MESSAGES 条
            let system_msg = self.messages[0..1].to_vec();
            let mut rest = self.messages[1..].to_vec();
            let keep = rest.split_off(rest.len().saturating_sub(MAX_MESSAGES - 1));
            self.messages = system_msg;
            self.messages.extend(keep);
        }
    }
}

impl Default for ConversationMemory {
    fn default() -> Self {
        Self::new()
    }
}
