# 🤖 Rust Agent — 学习项目

从零实现一个简化但完整的 Agent，理解 Agent 的核心概念。

## 🎯 学习目标

完成本项目后，你将理解：

| 概念 | 代码位置 | 核心知识点 |
|------|---------|-----------|
| Agent Loop | `src/agent.rs` | ReAct 循环：Think → Act → Observe |
| LLM Integration | `src/llm.rs` | OpenAI / Anthropic API 格式、Tool Calling 协议 |
| Tool System | `src/tools.rs` | 工具注册、参数解析、安全执行 |
| Memory | `src/memory.rs` | 对话历史管理、Context Window 限制 |

## 📁 项目结构

```
rust-agent/
├── Cargo.toml       # 项目依赖配置
├── .env.example     # 环境变量模板
├── src/
│   ├── main.rs      # 入口：交互式循环 + 参数初始化
│   ├── agent.rs     # ⭐ Agent Loop：核心循环逻辑
│   ├── llm.rs       # LLM 客户端：OpenAI + Anthropic API
│   ├── tools.rs     # 工具系统：5个内置工具
│   └── memory.rs    # 记忆系统：对话历史管理
└── README.md        # 本文件
```

## 🔧 运行方法

```bash
# 1. 复制环境变量配置
cp .env.example .env

# 2. 编辑 .env，填入你的 API key
vim .env

# 3. 运行
cargo run
```

## 🧪 示例对话

```
🤖 Rust Agent 学习项目

输入你的问题，按 Enter 发送，输入 'quit' 退出。

👤 你: 北京今天天气怎么样？

  🔄 迭代 1/10
  🔧 执行工具: get_weather(city="北京")
  ✅ 结果: 北京今天的天气：多云，温度 18°C
  🔄 迭代 2/10

🤖 Agent: 北京今天天气多云，气温约 18°C，适合外出。

👤 你: 帮我计算 158 + 347 等于多少？

  🔄 迭代 1/10
  🔧 执行工具: calculator(expression="158+347")
  ✅ 结果: 505
  🔄 迭代 2/10

🤖 Agent: 158 + 347 = 505 ✓

👤 你: 帮我执行 ls 命令

  🔄 迭代 1/10
  🔧 执行工具: bash(command="ls")
  ✅ 结果: Cargo.toml
         README.md
         src/

🤖 Agent: 当前目录下有：Cargo.toml、README.md 和 src/ 目录。
```

## 🏗️ 核心架构图

```
┌─────────────────────────────────────────────────────────┐
│                      main.rs                           │
│                  (交互式输入循环)                       │
└──────────────────────┬────────────────────────────────┘
                       │ user_input
                       ▼
┌─────────────────────────────────────────────────────────┐
│                      Agent                              │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐        │
│  │  LLM     │───→│  THINK   │───→│  有工具？ │        │
│  │  Client  │←───│  (LLM)   │    └────┬─────┘        │
│  └──────────┘    └──────────┘         │是            │
│                              ┌──────────▼──────────┐   │
│                              │        ACT          │   │
│                              │  (ToolRegistry)     │   │
│                              └──────────┬──────────┘   │
│                                         │             │
│                              ┌──────────▼──────────┐  │
│                              │      OBSERVE        │  │
│                              │   (tool result)     │──┘
│                              └──────────────────────┘
│                                                         │
│  ┌──────────────────────────────────────────────────┐  │
│  │  ConversationMemory (对话历史)                    │  │
│  │  [system] [user] [assistant] [tool] [assistant]  │  │
│  └──────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│                  ToolRegistry                           │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐   │
│  │ calculator  │  │ get_weather  │  │   bash     │   │
│  │  (数学计算)  │  │   (查天气)   │  │ (执行命令)  │   │
│  └─────────────┘  └─────────────┘  └─────────────┘   │
│  ┌─────────────┐                                       │
│  │ web_search │  ... 可自由扩展更多工具                │
│  └─────────────┘                                       │
└─────────────────────────────────────────────────────────┘
```

## 📚 扩展练习

完成基础版本后，可以尝试以下扩展：

### 1. 添加持久化记忆（简单）
```rust
// 在 memory.rs 中添加：
// - 保存到文件 (serde_json)
// - 加载历史对话
// - 按时间戳检索
```

### 2. 添加 Streaming 输出（中等）
```rust
// 在 llm.rs 中添加：
// - OpenAI SSE 流式 API
// - 实时显示 LLM 的思考过程
// - tokio_stream 处理流式响应
```

### 3. 实现 RAG 检索（进阶）
```rust
// 新增 src/retrieval.rs：
// - embedding 生成 (使用 LLM 的 embedding API)
// - 向量相似度搜索
// - 把相关记忆注入 context
```

### 4. 添加 MCP 工具协议（高级）
```rust
// 参考 OpenClaw 的 MCP SDK 实现：
// - 客户端：连接 MCP 服务器
// - 协议：JSON-RPC 2.0 over stdio
// - 动态发现可用工具
```

## 🔗 参考资料

- [OpenAI Function Calling 文档](https://platform.openai.com/docs/guides/function-calling)
- [Anthropic Tool Use 文档](https://docs.anthropic.com/en/docs/build-with-claude/tool-use)
- [ReAct 论文](https://arxiv.org/abs/2210.03629) — Agent Loop 的理论基础
- [LangChain Agents](https://python.langchain.com/docs/concepts/agents/) — 详细的 Agent 设计模式
