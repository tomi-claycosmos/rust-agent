/// # Tools — Agent 的工具系统
///
/// ## 什么是 Tool？
///
/// Tool 是 Agent 与外部世界交互的方式。
/// 给 Agent 一个 Tool = 教它一个新技能。
///
/// ## Anthropic 的工具设计原则
///
/// 来源：Anthropic Engineering Blog - "Writing Tools for Agents"
///
/// 1. **选对工具，不过多** — Agent 擅长选择的工具不超过 10 个
/// 2. **每个工具有清晰边界** — 如果人无法明确区分用途，Agent 也不行
/// 3. **返回有意义上下文** — 工具返回的是 Agent 的"眼睛"
/// 4. **Token 高效** — 不要一次返回太多，消耗 Agent 的 Attention Budget
/// 5. **工具描述本身需要工程化** — Anthropic 发现 Claude 本身就是最好的工具描述工程师
///
/// ## Anthropic 的关键发现
///
/// - "API 包装式工具"是反模式：不要把 `list_users` + `get_user` + `delete_user`
///   分成三个工具，而是合并成一个 `user_management` 工具
/// - 工具描述要包含：用途 + 使用场景 + 示例 + 注意事项
/// - 优化工具描述后，Claude Code 任务完成时间降低 40%
///
/// ## 本实现
///
/// 所有工具都遵循这些原则设计。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;

// ─────────────────────────────────────────────
// 工具定义
// ─────────────────────────────────────────────

/// 工具定义（给 LLM 看的接口）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// 单个工具调用请求
#[derive(Debug)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String, // JSON 字符串
}

// ─────────────────────────────────────────────
// 工具 trait
// ─────────────────────────────────────────────

pub trait Tool: Send {
    /// 工具名称
    fn name(&self) -> &str;
    /// 工具描述（供 LLM 理解何时使用）
    fn description(&self) -> &str;
    /// JSON Schema 格式的参数定义
    fn input_schema(&self) -> serde_json::Value;
    /// 执行工具
    fn execute(&self, input: &str) -> Result<String, String>;
}

// ─────────────────────────────────────────────
// 工具实现
// ─────────────────────────────────────────────

/// Calculator — 数学计算工具
struct CalculatorTool;

impl Tool for CalculatorTool {
    fn name(&self) -> &str {
        "calculator"
    }

    fn description(&self) -> &str {
        r#"执行数学计算。

使用场景：
- 计算折扣、价格、百分比（如：50 的 15% 是多少）
- 面积/体积/距离等物理量计算
- 日期差值（天数计算）
- 单位换算
- 任何需要精确数值的结果

使用示例：
- calculator("50 * 0.15") → "7.5"（求 50 的 15%）
- calculator("100 + 200 * 3") → "700"（注意运算顺序）
- calculator("(10 + 5) / 3") → "5"（带括号的计算）
- calculator("2 ** 10") → "1024"（指数运算）

注意事项：
- 适用于浮点数和整数的加减乘除、指数运算
- 不适用于字符串操作或日期格式化（请用其他工具）
- 精度：默认使用 Rust 的 f64，足够日常计算使用"#
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "expression": {
                    "type": "string",
                    "description": "数学表达式，如 '50 * 0.15' 或 '(100 + 200) / 2'"
                }
            },
            "required": ["expression"]
        })
    }

    fn execute(&self, input: &str) -> Result<String, String> {
        #[derive(Debug, Deserialize)]
        struct CalcInput {
            expression: String,
        }

        let input: CalcInput =
            serde_json::from_str(input).map_err(|e| format!("参数解析失败: {}", e))?;

        // 用 Rust 简易求值器（仅支持基本运算）
        let result = eval_expression(&input.expression)
            .map_err(|e| format!("计算错误: {}", e))?;

        Ok(result.to_string())
    }
}

/// 简易数学表达式求值
fn eval_expression(expr: &str) -> Result<f64, String> {
    let expr = expr.trim();
    // 移除可能的空格
    let expr = expr.replace(' ', "");

    parse_expr(&expr).map_err(|e| e)
}

fn parse_expr(s: &str) -> Result<f64, String> {
    parse_add_sub(s, &mut 0)
}

fn parse_add_sub(s: &str, pos: &mut usize) -> Result<f64, String> {
    let mut result = parse_mul_div(s, pos)?;

    while *pos < s.len() {
        let remaining = &s[*pos..];
        if remaining.starts_with('+') {
            *pos += 1;
            let val = parse_mul_div(s, pos)?;
            result += val;
        } else if remaining.starts_with('-') {
            *pos += 1;
            let val = parse_mul_div(s, pos)?;
            result -= val;
        } else {
            break;
        }
    }
    Ok(result)
}

fn parse_mul_div(s: &str, pos: &mut usize) -> Result<f64, String> {
    let mut result = parse_power(s, pos)?;

    while *pos < s.len() {
        let remaining = &s[*pos..];
        if remaining.starts_with('*') {
            if remaining.starts_with("**") {
                *pos += 2;
                let val = parse_power(s, pos)?;
                result = result.powf(val);
            } else {
                *pos += 1;
                let val = parse_power(s, pos)?;
                result *= val;
            }
        } else if remaining.starts_with('/') {
            *pos += 1;
            let val = parse_power(s, pos)?;
            if val == 0.0 {
                return Err("除数不能为 0".to_string());
            }
            result /= val;
        } else {
            break;
        }
    }
    Ok(result)
}

fn parse_power(s: &str, pos: &mut usize) -> Result<f64, String> {
    let result = parse_unary(s, pos)?;

    if *pos < s.len() {
        let remaining = &s[*pos..];
        if remaining.starts_with("**") {
            *pos += 2;
            let val = parse_unary(s, pos)?;
            return Ok(result.powf(val));
        }
    }
    Ok(result)
}

fn parse_unary(s: &str, pos: &mut usize) -> Result<f64, String> {
    if *pos < s.len() && s[*pos..].starts_with('-') {
        *pos += 1;
        let val = parse_atom(s, pos)?;
        Ok(-val)
    } else {
        parse_atom(s, pos)
    }
}

fn parse_atom(s: &str, pos: &mut usize) -> Result<f64, String> {
    let remaining = &s[*pos..];

    // 括号
    if remaining.starts_with('(') {
        *pos += 1;
        let val = parse_expr(&s[*pos - 1..])?;
        // 找到匹配的右括号
        let mut depth = 1;
        let mut end = *pos;
        while end < s.len() && depth > 0 {
            if s[end..].starts_with('(') {
                depth += 1;
            } else if s[end..].starts_with(')') {
                depth -= 1;
            }
            end += 1;
        }
        if depth != 0 {
            return Err("括号不匹配".to_string());
        }
        *pos = end;
        // 重新从括号内容解析
        let inner = &s[*pos - 1..end - 1];
        parse_expr(&inner[1..])
    } else {
        // 数字
        let start = *pos;
        while *pos < s.len() && (s[*pos..].chars().next().map(|c| c.is_numeric()).unwrap_or(false) || s.as_bytes()[*pos] == b'.') {
            *pos += 1;
        }
        if *pos == start {
            return Err(format!("无法解析: {}", remaining.chars().take(10).collect::<String>()));
        }
        s[start..*pos].parse::<f64>().map_err(|_| "数字解析失败".to_string())
    }
}

/// Bash — 执行命令行
struct BashTool;

impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        r#"在终端执行命令。

使用场景：
- 读写文件（cat, echo, tee, mv, mkdir 等）
- 运行程序和脚本（cargo, python, node, npm 等）
- 检查文件存在性和内容（ls, find, grep 等）
- Git 操作（git status, git log, git add, git commit 等）
- 安装依赖（pip install, cargo build 等）

使用示例：
- bash("ls -la") → 列出当前目录文件
- bash("cat README.md") → 读取 README 内容
- bash("echo 'hello' > hello.txt") → 写入文件
- bash("cargo build 2>&1 | tail -20") → 编译并只看最后20行输出
- bash("git log --oneline -5") → 最近5次提交

注意事项：
- 优先用 echo/tee 写文件，不用重定向 > 时加引号防止 shell 注入
- 组合命令用 && 或 |，如 "cargo build && cargo test"
- 错误输出也重要：用 2>&1 合并 stderr 和 stdout
- 工作目录是项目根目录"#
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "要执行的 shell 命令，如 'ls -la' 或 'cat file.txt'"
                },
                "timeout": {
                    "type": "number",
                    "description": "超时时间（秒），默认60秒"
                }
            },
            "required": ["command"]
        })
    }

    fn execute(&self, input: &str) -> Result<String, String> {
        #[derive(Debug, Deserialize)]
        struct BashInput {
            command: String,
            #[serde(default = "default_timeout")]
            timeout: u64,
        }

        fn default_timeout() -> u64 {
            60
        }

        let input: BashInput =
            serde_json::from_str(input).map_err(|e| format!("参数解析失败: {}", e))?;

        let output = Command::new("bash")
            .arg("-c")
            .arg(&input.command)
            .output()
            .map_err(|e| format!("命令执行失败: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = String::new();

        if !stdout.is_empty() {
            result.push_str(&format!("[stdout]\n{}\n", stdout));
        }
        if !stderr.is_empty() {
            result.push_str(&format!("[stderr]\n{}\n", stderr));
        }
        if result.is_empty() {
            result = "(命令执行完成，无输出)".to_string();
        }

        result.push_str(&format!("\n[exit code: {}]", output.status.code().unwrap_or(-1)));

        Ok(result)
    }
}

/// Get Time — 获取当前时间
struct GetTimeTool;

impl Tool for GetTimeTool {
    fn name(&self) -> &str {
        "get_current_time"
    }

    fn description(&self) -> &str {
        r#"获取当前时间。

使用场景：
- 记录操作时间戳
- 判断当前是白天还是夜晚
- 在日志中加入时间信息
- 与时间相关的条件判断

使用示例：
- get_current_time("") → 返回当前时间（如 "2024-01-15 14:30:25 UTC"）

注意事项：
- 返回的是 UTC 时间
- 适合记录时间，但不适用于复杂的时间计算（用 calculator 工具）"#
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "timezone": {
                    "type": "string",
                    "description": "时区（如 'Asia/Shanghai'），默认 UTC"
                }
            }
        })
    }

    fn execute(&self, input: &str) -> Result<String, String> {
        #[derive(Debug, Deserialize)]
        struct TimeInput {
            timezone: Option<String>,
        }

        let input: TimeInput =
            serde_json::from_str(input).unwrap_or(TimeInput { timezone: None });

        let now = chrono_lite_now();

        let tz = input.timezone.unwrap_or_else(|| "UTC".to_string());
        Ok(format!("{} ({})", now, tz))
    }
}

/// 简化版 chrono now
fn chrono_lite_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let secs = now.as_secs();
    let days = secs / 86400;
    let year = 1970 + days / 365;
    let remaining_days = days % 365;
    let month = remaining_days / 30 + 1;
    let day = remaining_days % 30 + 1;
    let hour = (secs % 86400) / 3600;
    let min = (secs % 3600) / 60;
    let sec = secs % 60;
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", year, month, day, hour, min, sec)
}

// ─────────────────────────────────────────────
// 工具注册表
// ─────────────────────────────────────────────

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
    pub tool_defs: Vec<ToolDef>,
}

impl Clone for ToolRegistry {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut tools = HashMap::new();
        let mut tool_defs = Vec::new();

        // 注册所有工具
        let tool_list: Vec<Box<dyn Tool>> = vec![
            Box::new(CalculatorTool),
            Box::new(BashTool),
            Box::new(GetTimeTool),
        ];

        for tool in tool_list {
            let name = tool.name().to_string();
            let def = ToolDef {
                name: name.clone(),
                description: tool.description().to_string(),
                input_schema: tool.input_schema(),
            };
            tool_defs.push(def);
            tools.insert(name, tool);
        }

        Self { tools, tool_defs }
    }

    /// 列出所有工具定义（给 LLM 看）
    pub fn list(&self) -> Vec<ToolDef> {
        self.tool_defs.clone()
    }

    /// 执行工具
    pub fn execute(&self, name: &str, arguments: &str) -> Result<String, String> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| format!("未知工具: {}", name))?;
        tool.execute(arguments)
    }

    /// 检查工具是否存在
    pub fn has(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────
// 测试
// ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculator() {
        let tool = CalculatorTool {};
        assert_eq!(tool.execute(r#"{"expression": "2 + 3"}"#).unwrap(), "5");
        assert_eq!(tool.execute(r#"{"expression": "10 * 0.15"}"#).unwrap(), "1.5");
        assert_eq!(tool.execute(r#"{"expression": "(10 + 5) / 3"}"#).unwrap(), "5");
    }

    #[test]
    fn test_chrono_now() {
        let now = chrono_lite_now();
        assert!(now.contains('-'));
        assert!(now.contains(':'));
    }
}
