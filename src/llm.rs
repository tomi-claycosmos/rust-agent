//! # LLM Integration — 如何与语言模型对话
//!
//! ## OpenAI API 格式 vs Anthropic API 格式
//!
//! 不同 LLM 的 API 格式略有不同。这里展示核心概念：
//!
//! ### OpenAI Chat Completions
//! ```json
//! {
//!   "model": "gpt-4o",
//!   "messages": [
//!     {"role": "system", "content": "你是一个有帮助的助手"},
//!     {"role": "user", "content": "你好"}
//!   ],
//!   "tools": [
//!     {
//!       "type": "function",
//!       "function": {
//!         "name": "calculator",
//!         "description": "执行数学计算",
//!         "parameters": {"type": "object", "properties": {...}}
//!       }
//!     }
//!   ]
//! }
//! ```
//!
//! ### Anthropic Messages API
//! ```json
//! {
//!   "model": "claude-sonnet-4-20250514",
//!   "messages": [...],
//!   "tools": [...]
//! }
//! ```
//!
//! ## Tool Calling 的工作原理
//!
//! 1. 你在 "messages" 里告诉 LLM："你有两个工具可用"
//! 2. LLM 决定需要调用工具时，返回 `tool_calls` 字段
//! 3. 你执行工具，把结果作为 `tool` 类型的消息加回对话
//! 4. LLM 继续思考，决定下一步
//!    （回到 agent.rs 的循环）

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ─────────────────────────────────────────────
// 公开类型
// ─────────────────────────────────────────────

/// 单条对话消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: String,        // "system" | "user" | "assistant" | "tool"
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>, // tool 结果时需要
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>, // tool 结果时需要
}

/// LLM 返回的响应类型
pub enum LlmResponse {
    /// 纯文本响应（没有工具调用）
    Text(String),
    /// 工具调用请求（需要执行工具）
    ToolCalls(Vec<ToolCall>),
    /// 完成（带最终文本）
    Done(String),
}

/// 单个工具调用
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String, // JSON 字符串
}

// ─────────────────────────────────────────────
// LLM Client
// ─────────────────────────────────────────────

pub struct LlmClient {
    http: Client,
    api_key: String,
    model: String,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
}

impl LlmClient {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            http: Client::new(),
            api_key,
            model,
        }
    }

    /// 发送对话请求到 LLM
    pub async fn chat(&self, messages: &[LlmMessage], tools: &[ToolDef]) -> Result<LlmResponse, LlmError> {
        // 自动检测使用哪个 API（按模型名判断）
        if self.model.contains("claude") || self.model.contains("anthropic") {
            self.chat_anthropic(messages, tools).await
        } else {
            self.chat_openai(messages, tools).await
        }
    }

    /// OpenAI Chat Completions API
    async fn chat_openai(&self, messages: &[LlmMessage], tools: &[ToolDef]) -> Result<LlmResponse, LlmError> {
        #[derive(Serialize)]
        struct Request<'a> {
            model: &'a str,
            messages: Vec<&'a LlmMessage>,
            tools: Vec<Value>,
            #[serde(rename = "tool_choice")]
            tool_choice: Value,
        }

        let tool_defs: Vec<Value> = tools.iter().map(|t| t.into()).collect();

        let req_body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "tools": tool_defs,
            "tool_choice": "auto",
        });

        let resp = self.http
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req_body)
            .send()
            .await
            .map_err(|e| LlmError(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError(format!("API 错误 ({}): {}", status, body)));
        }

        let output: OpenAiResponse = resp.json().await.map_err(|e| LlmError(e.to_string()))?;

        let choice = output.choices.first()
            .ok_or_else(|| LlmError("没有返回 choices".to_string()))?;

        // 检查是否有 tool_calls
        if let Some(tool_calls) = &choice.message.tool_calls {
            let calls: Vec<ToolCall> = tool_calls.iter().map(|tc| ToolCall {
                id: tc.id.clone(),
                name: tc.function.name.clone(),
                arguments: tc.function.arguments.clone(),
            }).collect();
            return Ok(LlmResponse::ToolCalls(calls));
        }

        // 纯文本响应
        Ok(LlmResponse::Text(choice.message.content.clone().unwrap_or_default()))
    }

    /// Anthropic Messages API
    async fn chat_anthropic(&self, messages: &[LlmMessage], tools: &[ToolDef]) -> Result<LlmResponse, LlmError> {
        // Anthropic API 把 system 单独拿出来
        let system_msg = messages.iter().find(|m| m.role == "system");
        let chat_msgs: Vec<&LlmMessage> = messages.iter().filter(|m| m.role != "system").collect();

        #[derive(Serialize)]
        struct Request<'a> {
            model: &'a str,
            messages: Vec<&'a LlmMessage>,
            system: Option<&'a str>,
            tools: Vec<Value>,
            #[serde(rename = "max_tokens")]
            max_tokens: u32,
        }

        let req_body = serde_json::json!({
            "model": self.model,
            "messages": chat_msgs,
            "system": system_msg.map(|m| m.content.as_str()),
            "tools": tools,
            "max_tokens": 4096,
        });

        let resp = self.http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&req_body)
            .send()
            .await
            .map_err(|e| LlmError(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError(format!("API 错误 ({}): {}", status, body)));
        }

        #[derive(Deserialize)]
        #[serde(tag = "type")]
        enum ContentBlock {
            #[serde(rename = "text")]
            Text { text: String },
            #[serde(rename = "tool_use")]
            ToolUse { id: String, name: String, input: Value },
        }

        #[derive(Deserialize)]
        struct AnthropicResponse {
            content: Vec<ContentBlock>,
        }

        let output: AnthropicResponse = resp.json().await.map_err(|e| LlmError(e.to_string()))?;

        let mut tool_calls = Vec::new();
        let mut final_text = String::new();

        for block in output.content {
            match block {
                ContentBlock::Text { text } => final_text.push_str(&text),
                ContentBlock::ToolUse { id, name, input } => {
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments: serde_json::to_string(&input).unwrap_or_default(),
                    });
                }
            }
        }

        if !tool_calls.is_empty() {
            Ok(LlmResponse::ToolCalls(tool_calls))
        } else {
            Ok(LlmResponse::Text(final_text))
        }
    }
}

// ─────────────────────────────────────────────
// API 响应结构
// ─────────────────────────────────────────────

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Deserialize)]
struct OpenAiMessage {
    content: Option<String>,
    #[serde(rename = "tool_calls")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Deserialize)]
struct OpenAiToolCall {
    id: String,
    function: OpenAiFunction,
}

#[derive(Deserialize)]
struct OpenAiFunction {
    name: String,
    arguments: String,
}

// ─────────────────────────────────────────────
// 工具定义（用于发给 LLM）
// ─────────────────────────────────────────────

use crate::tools::ToolDef;

impl From<&ToolDef> for Value {
    fn from(t: &ToolDef) -> Self {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": t.name,
                "description": t.description,
                "parameters": t.parameters,
            }
        })
    }
}

// ─────────────────────────────────────────────
// 错误类型
// ─────────────────────────────────────────────

pub struct LlmError(pub String);

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Debug for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LlmError({})", self.0)
    }
}

impl std::error::Error for LlmError {}
