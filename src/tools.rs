//! # Tool System — 工具注册与执行
//!
//! ## 什么是 Tool Calling？
//!
//! Tool Calling 让 Agent 可以执行外部操作，而不仅仅是"说话"。
//!
//! ## 工具调用的完整流程
//! ```text
//! Agent: "帮我查北京天气"
//!                           ┌──────────────────┐
//!                           │  LLM 决定调用     │
//!                           │  get_weather()    │
//!                           └────────┬─────────┘
//!                                    │
//!                                    ▼
//!                           ┌──────────────────┐
//!                           │  工具执行器       │
//!                           │  (ToolRegistry)   │
//!                           └────────┬─────────┘
//!                                    │
//!            ┌───────────────────────┼───────────────────────┐
//!            ▼                       ▼                       ▼
//!     ┌─────────────┐        ┌─────────────┐        ┌─────────────┐
//!     │ get_weather │        │  calculator │        │  web_search │
//!     │  (HTTP API) │        │  (计算)     │        │  (HTTP API) │
//!     └─────────────┘        └─────────────┘        └─────────────┘
//! ```

use serde_json::{Value, Map};
use serde::Serialize;
use std::collections::HashMap;

// ─────────────────────────────────────────────
// 工具定义（发给 LLM，告诉她有哪些工具可用）
// ─────────────────────────────────────────────

/// 工具定义（用于注册给 LLM）
#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// 工具接口（每种工具都实现这个 trait）
trait Tool: Send + Sync {
    fn execute(&self, args: &Map<String, Value>) -> Result<String, ToolError>;
    fn definition(&self) -> ToolDef;
}

/// 工具注册表
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
    tool_defs: Vec<ToolDef>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            tools: HashMap::new(),
            tool_defs: Vec::new(),
        };

        // 注册内置工具
        registry.register(CalculatorTool);
        registry.register(WeatherTool);
        registry.register(WebSearchTool);
        registry.register(TimeTool);
        registry.register(BashTool);

        registry
    }

    /// 注册工具
    fn register<T: Tool + 'static>(&mut self, tool: T) {
        let def = tool.definition();
        self.tool_defs.push(def.clone());
        self.tools.insert(def.name.clone(), Box::new(tool));
    }

    /// 列出所有已注册的工具
    pub fn list(&self) -> Vec<ToolDef> {
        self.tool_defs.clone()
    }

    /// 执行工具
    pub fn execute(&self, name: &str, arguments_json: &str) -> Result<String, ToolError> {
        let tool = self.tools.get(name)
            .ok_or_else(|| ToolError(format!("未知工具: {}", name)))?;

        let args: Value = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError(format!("参数 JSON 解析失败: {}", e)))?;

        let args_map = args.as_object()
            .ok_or_else(|| ToolError("参数必须是 JSON 对象".to_string()))?;

        tool.execute(args_map)
    }
}

// ─────────────────────────────────────────────
// 内置工具实现
// ─────────────────────────────────────────────

/// 计算器工具
struct CalculatorTool;
impl Tool for CalculatorTool {
    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "calculator".to_string(),
            description: "执行数学计算。支持 +, -, *, /, ^ 运算符和括号优先级。例如: 2+2, 3*4, (10+5)/3".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "expression": {
                        "type": "string",
                        "description": "数学表达式，例如 '2 + 2' 或 'sqrt(16) + 3'"
                    }
                },
                "required": ["expression"]
            }),
        }
    }

    fn execute(&self, args: &Map<String, Value>) -> Result<String, ToolError> {
        let expr = args.get("expression")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError("缺少 expression 参数".to_string()))?;

        let result = eval_expression(expr)
            .map_err(|e| ToolError(format!("计算错误: {}", e)))?;

        Ok(format!("{}", result))
    }
}

/// 天气查询工具
struct WeatherTool;
impl Tool for WeatherTool {
    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "get_weather".to_string(),
            description: "获取指定城市的当前天气。返回温度、天气状况等信息。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "city": {
                        "type": "string",
                        "description": "城市名称，例如 '北京' 或 'Shanghai'"
                    }
                },
                "required": ["city"]
            }),
        }
    }

    fn execute(&self, args: &Map<String, Value>) -> Result<String, ToolError> {
        let city = args.get("city")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError("缺少 city 参数".to_string()))?;

        // 模拟天气数据
        let conditions = ["晴天 ☀️", "多云 ⛅", "小雨 🌧️", "雪天 ❄️", "大风 💨"];
        let temps = [15, 18, 22, 25, 28, 12, 8, 30];

        let idx = city.len() % conditions.len();
        let condition = conditions[idx];
        let temp = temps[city.len() % temps.len()];

        Ok(format!("{} 今天的天气：{}，温度 {}°C", city, condition, temp))
    }
}

/// 网页搜索工具
struct WebSearchTool;
impl Tool for WebSearchTool {
    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "web_search".to_string(),
            description: "搜索互联网获取信息。返回搜索结果摘要。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "搜索关键词"
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "最多返回多少条结果",
                        "default": 3
                    }
                },
                "required": ["query"]
            }),
        }
    }

    fn execute(&self, args: &Map<String, Value>) -> Result<String, ToolError> {
        let query = args.get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError("缺少 query 参数".to_string()))?;

        let max = args.get("max_results")
            .and_then(|v| v.as_i64())
            .unwrap_or(3) as usize;

        Ok(format!(
            "搜索 '{}' 的结果（{}条）：\n  1. [结果 A] - 相关描述...\n  2. [结果 B] - 相关描述...\n  3. [结果 C] - 相关描述...",
            query, max
        ))
    }
}

/// 时间查询工具
struct TimeTool;
impl Tool for TimeTool {
    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "get_current_time".to_string(),
            description: "获取当前时间和日期。无需参数。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    fn execute(&self, _args: &Map<String, Value>) -> Result<String, ToolError> {
        Ok(chrono_lite_now())
    }
}

/// Bash 命令工具
struct BashTool;
impl Tool for BashTool {
    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "bash".to_string(),
            description: "在终端执行 bash 命令。仅用于只读操作（ls, pwd, echo, date 等）。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "要执行的 bash 命令"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    fn execute(&self, args: &Map<String, Value>) -> Result<String, ToolError> {
        use std::process::Command;

        let cmd = args.get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError("缺少 command 参数".to_string()))?;

        let allowed = ["ls", "pwd", "echo", "date", "whoami", "df", "free", "uname", "cat", "head", "tail", "wc", "find"];
        let first_word = cmd.split_whitespace().next().unwrap_or("");
        if !allowed.contains(&first_word) {
            return Err(ToolError(format!(
                "命令 '{}' 不在白名单中。允许: {}",
                first_word, allowed.join(", ")
            )));
        }

        let output = Command::new("sh")
            .args(["-c", cmd])
            .output()
            .map_err(|e| ToolError(format!("执行失败: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return Err(ToolError(format!("命令执行失败:\n{}\n{}", stdout, stderr)));
        }

        if stderr.is_empty() {
            Ok(stdout.to_string())
        } else {
            Ok(format!("{}\nstderr: {}", stdout, stderr))
        }
    }
}

// ─────────────────────────────────────────────
// 工具函数
// ─────────────────────────────────────────────

/// 数学表达式求值（支持 + - * / 和括号）
fn eval_expression(expr: &str) -> Result<f64, String> {
    let expr = expr.replace(" ", "").replace("×", "*").replace("÷", "/");

    // 安全检查：只允许数字、运算符、括号、小数点
    if !expr.chars().all(|c| c.is_numeric() || "+-*/.()^ ".contains(c)) {
        return Err("包含不支持的字符".to_string());
    }

    parse_expr(&expr, &mut 0).map(|(v, i)| {
        if i < expr.len() {
            Err("表达式解析不完整".to_string())
        } else {
            Ok(v)
        }
    }).unwrap_or_else(|e| Err(e))
}

fn parse_expr(s: &str, i: &mut usize) -> Result<(f64, usize), String> {
    let (mut left, mut pos) = parse_term(s, i)?;

    while *i < s.len() {
        let c = s[*i..].chars().next().unwrap();
        if c == '+' {
            *i += 1;
            let (right, new_pos) = parse_term(s, i)?;
            left += right;
            pos = new_pos;
        } else if c == '-' {
            *i += 1;
            let (right, new_pos) = parse_term(s, i)?;
            left -= right;
            pos = new_pos;
        } else {
            break;
        }
    }

    Ok((left, pos))
}

fn parse_term(s: &str, i: &mut usize) -> Result<(f64, usize), String> {
    let (mut left, mut pos) = parse_factor(s, i)?;

    while *i < s.len() {
        let c = s[*i..].chars().next().unwrap();
        if c == '*' {
            *i += 1;
            let (right, new_pos) = parse_factor(s, i)?;
            left *= right;
            pos = new_pos;
        } else if c == '/' {
            *i += 1;
            let (right, new_pos) = parse_factor(s, i)?;
            if right == 0.0 {
                return Err("除数不能为0".to_string());
            }
            left /= right;
            pos = new_pos;
        } else {
            break;
        }
    }

    Ok((left, pos))
}

fn parse_factor(s: &str, i: &mut usize) -> Result<(f64, usize), String> {
    // 跳过空白
    while *i < s.len() && s[*i..].starts_with(' ') {
        *i += 1;
    }

    if *i >= s.len() {
        return Err("意外的表达式结尾".to_string());
    }

    let c = s[*i..].chars().next().unwrap();

    if c == '(' {
        *i += 1;
        let (val, pos) = parse_expr(s, i)?;
        if *i < s.len() && s[*i..].starts_with(')') {
            *i += 1;
            Ok((val, *i))
        } else {
            Err("缺少右括号".to_string())
        }
    } else if c == '-' {
        *i += 1;
        let (val, pos) = parse_factor(s, i)?;
        Ok((-val, pos))
    } else {
        parse_number(s, i)
    }
}

fn parse_number(s: &str, i: &mut usize) -> Result<(f64, usize), String> {
    let start = *i;
    while *i < s.len() && (s[*i..].chars().next().unwrap().is_numeric() || s[*i..].starts_with('.')) {
        *i += 1;
    }
    let num_str = &s[start..*i];
    num_str.parse::<f64>()
        .map(|v| (v, *i))
        .map_err(|_| format!("无法解析数字: '{}'", num_str))
}

/// 简化版当前时间
fn chrono_lite_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap();
    let secs = now.as_secs();
    let days = secs / 86400;
    let year = 1970 + days / 365;
    let remaining_days = days % 365;
    let month = remaining_days / 30 + 1;
    let day = remaining_days % 30 + 1;
    let hour = (secs % 86400) / 3600;
    let min = (secs % 3600) / 60;
    let sec = secs % 60;
    format!(
        "{}年{}月{}日 {:02}:{:02}:{:02} (UTC+8)",
        year, month, day,
        (hour + 8) % 24, min, sec
    )
}

// ─────────────────────────────────────────────
// 错误类型
// ─────────────────────────────────────────────

pub struct ToolError(pub String);

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Debug for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ToolError({})", self.0)
    }
}

impl std::error::Error for ToolError {}
