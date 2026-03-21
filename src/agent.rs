//! # Agent Loop — Agent 的核心大脑
//!
//! ## 什么是 Agent Loop？
//!
//! 传统的 LLM 调用是"一问一答"：
//! ```text
//! User → LLM → Response → 结束
//! ```
//!
//! Agent 的核心是**循环**：
//! ```text
//! User Input → Think → Act → Observe → Think → Act → ... → Final Response
//! ```
//!
//!                          ┌─────────────┐
//!   User Input ─────────────→│   THINK     │←────────────────┐
//!                            │  (LLM call) │                  │
//!                            └──────┬──────┘                  │
//!                                   │                         │
//!                           ┌───────▼───────┐                 │
//!                           │  有工具调用吗？ │─否──→ Final ───┘
//!                           └───────┬───────┘
//!                                   │是
//!                          ┌────────▼────────┐
//!                          │   ACT          │
//!                          │ (执行工具)      │
//!                          └────────┬────────┘
//!                                   │
//!                          ┌────────▼────────┐
//!                          │   OBSERVE      │
//!                          │ (观察结果)      │────────┐
//!                          └────────────────┘        │
//!                                                     │
//!                                    (回到 THINK) ────┘
//! ```
//!
//! 这个循环叫做 **ReAct**: Reason + Act

use crate::llm::{LlmClient, LlmResponse};
use crate::tools::ToolRegistry;
use crate::memory::ConversationMemory;

const MAX_ITERATIONS: usize = 10; // 防止无限循环

/// Agent 主结构
pub struct Agent {
    /// LLM 客户端
    llm: LlmClient,
    /// 工具注册表
    tools: ToolRegistry,
    /// 对话记忆
    memory: ConversationMemory,
}

impl Agent {
    pub fn new(api_key: String, model: String, tools: ToolRegistry, memory: ConversationMemory) -> Self {
        Self {
            llm: LlmClient::new(api_key, model),
            tools,
            memory,
        }
    }

    /// 运行 Agent — 核心循环
    pub async fn run(&mut self, user_input: &str) -> Result<String, AgentError> {
        // Step 1: 把用户输入加入记忆
        self.memory.add_user_message(user_input);

        // 打印当前 token 预算状态
        self.memory.print_budget_status();

        // Agent Loop: 最多循环 MAX_ITERATIONS 次
        for iteration in 0..MAX_ITERATIONS {
            println!("{}", format!("\n  🔄 迭代 {}/{}", iteration + 1, MAX_ITERATIONS));

            // Step 2: THINK — 让 LLM 决定下一步
            // 自动使用 Extended Thinking（Anthropic 建议：复杂任务开启）
            let use_thinking = iteration > 2; // 超过 2 轮后开启 thinking
            let llm_response = self.llm.chat_with_thinking(
                &self.memory.get_messages(),
                &self.tools.list(),
                use_thinking,
            ).await?;

            // 再次打印 token 预算（工具调用后）
            self.memory.print_budget_status();

            // Step 3: 检查 LLM 返回的内容
            match llm_response {
                LlmResponse::Text(text) => {
                    // 没有更多工具调用，直接返回文本响应
                    self.memory.add_assistant_message(&text);
                    return Ok(text);
                }
                LlmResponse::ToolCalls(calls) => {
                    // 有工具调用！
                    let mut all_results = Vec::new();

                    for call in calls {
                        println!("{}", format!("  🔧 执行工具: {}()", call.name));

                        // Step 4: ACT — 执行工具
                        match self.tools.execute(&call.name, &call.arguments) {
                            Ok(result) => {
                                println!("{}", format!("  ✅ 结果: {}", &result[..result.len().min(100)]));
                                all_results.push((call.id.clone(), result));
                            }
                            Err(e) => {
                                let error_msg = format!("工具执行失败: {}", e);
                                println!("{}", format!("  ❌ {}", error_msg));
                                all_results.push((call.id.clone(), error_msg));
                            }
                        }
                    }

                    // Step 5: OBSERVE — 把工具结果加回记忆
                    for (tool_call_id, result) in all_results {
                        self.memory.add_tool_result(&tool_call_id, &result);
                    }
                }
                LlmResponse::Done(text) => {
                    self.memory.add_assistant_message(&text);
                    return Ok(text);
                }
            }
        }

        Err(AgentError::MaxIterationsReached)
    }

    pub fn clear_memory(&mut self) {
        self.memory.clear();
    }
}

#[derive(Debug)]
pub enum AgentError {
    LlmError(String),
    ToolError(String),
    MaxIterationsReached,
    NoApiKey,
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LlmError(s) => write!(f, "LLM 错误: {}", s),
            Self::ToolError(s) => write!(f, "工具错误: {}", s),
            Self::MaxIterationsReached => write!(f, "达到最大迭代次数（{}次），停止循环", MAX_ITERATIONS),
            Self::NoApiKey => write!(f, "未设置 API Key"),
        }
    }
}

impl std::error::Error for AgentError {}

impl From<crate::llm::LlmError> for AgentError {
    fn from(e: crate::llm::LlmError) -> Self {
        Self::LlmError(e.0)
    }
}
