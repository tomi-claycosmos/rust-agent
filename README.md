# 🤖 Rust Agent — 学习项目

从零实现一个完整 Agent，参考 Anthropic 的**双 Agent 长运行架构**，实现 Initializer + Coding Agent 双阶段工作流。

## 🎯 学习路径

**第一阶段：单 Agent（交互模式）**

| 概念 | 代码位置 | 核心知识点 |
|------|---------|-----------|
| Agent Loop | `src/agent.rs` | ReAct 循环：Think → Act → Observe |
| LLM Integration | `src/llm.rs` | OpenAI / Anthropic API 格式、Tool Calling 协议 |
| Tool System | `src/tools.rs` | 工具注册、参数解析、安全执行 |
| Memory | `src/memory.rs` | 对话历史管理、Context Window 限制 |

**第二阶段：双 Agent（Anthropic 长运行方案）**

| 组件 | 代码位置 | 核心知识点 |
|------|---------|-----------|
| Initializer Agent | `src/initializer.rs` | 分析需求 → 生成 feature list JSON |
| Coding Agent | `src/coding_agent.rs` | 单 feature 增量实现，git 交接 |
| Project Scaffold | `src/project.rs` | SPEC.md、claude-progress.txt、init.sh |
| Dual-Agent Mode | `src/main.rs` | CLI 入口，支持 init/continue 子命令 |

---

## 🔧 运行方法

### 交互式对话（单 Agent）

```bash
cp .env.example .env
vim .env   # 填入 OPENAI_API_KEY 或 ANTHROPIC_API_KEY
cargo run
```

### 双 Agent 模式（长运行项目）

```bash
# Step 1: Initializer Agent — 根据需求建立项目脚手架
cargo run -- init \
  --project ./my-todo \
  --name "Todo App" \
  --prompt "一个简单的 Todo 应用，支持添加、完成、删除任务，数据存储在本地 JSON 文件"

# Step 2: Coding Agent — 每次迭代实现一个 feature
cargo run -- continue --project ./my-todo

# Coding Agent 完成后，继续下一个：
cargo run -- continue --project ./my-todo

# 限制最大迭代次数（如最多做 5 个 feature）：
cargo run -- continue --project ./my-todo --max 5
```

---

## 🏗️ 双 Agent 架构详解

```
用户需求
   │
   ▼
┌─────────────────────────────┐
│   Initializer Agent          │  ←─ 分析需求，生成 feature list
│   (一次性执行)               │
│                             │
│  • 生成 SPEC.md (JSON)       │
│  • 生成 init.sh             │
│  • 生成 claude-progress.txt  │
│  • 初始化 git 仓库          │
└────────────┬────────────────┘
             │
             ▼
┌─────────────────────────────┐
│   Coding Agent (循环)        │  ←─ 每次实现一个 feature
│                             │
│  每次会话：                 │
│  1. Get Bearings            │     读进度 + git log + SPEC.md
│  2. Health Check            │     运行 init.sh，验证基础功能
│  3. Pick Feature            │     从 SPEC.md 选未完成的 feature
│  4. Implement               │     增量实现 + 端到端测试
│  5. Update                  │     标记 passes=true，commit
└────────────┬────────────────┘
             │
             ▼  (重复直到所有 feature 完成)
          🎉 项目交付
```

### 核心文件说明

| 文件 | 创建者 | 用途 |
|------|--------|------|
| `SPEC.md` | Initializer | 结构化功能列表（JSON），Coding Agent 只改 passes 字段 |
| `claude-progress.txt` | Initializer + Coding | 进度日志，每完成一个 feature 追加条目 |
| `init.sh` | Initializer | 一键启动开发环境 |
| git history | Coding Agent | 每次 commit 记录变更，下一会话通过 git 恢复状态 |

### 为什么需要这个架构？

传统 Agent 的问题（Anthropic 论文原文）：

1. **Agent 试图一口气完成所有功能** → 上下文在实现中途耗尽，留下半成品
2. **Agent 过早宣告任务完成** → 实现部分功能后宣布"搞定了"
3. **会话间状态丢失** → 下一 Agent 从零开始，浪费大量时间重新理解项目

解决方案：
- 强制**单 feature 增量**（不怕中途停止）
- **git commit 交接**（下一 Agent 恢复状态）
- **feature list 约束**（passes=false 是外部约束，不能随意删除）

---

## 📁 项目结构

```
rust-agent/
├── Cargo.toml              # 依赖配置
├── .env.example            # 环境变量模板
├── src/
│   ├── main.rs             # 入口：交互模式 + 双 Agent CLI
│   ├── agent.rs            # 单 Agent（交互模式）
│   ├── llm.rs              # LLM 客户端
│   ├── tools.rs            # 工具注册表
│   ├── memory.rs           # 对话历史管理
│   ├── project.rs          # SPEC.md / 进度文件结构
│   ├── initializer.rs      # Initializer Agent
│   └── coding_agent.rs     # Coding Agent
└── README.md
```

## 多 Agent 架构

### 单 Agent vs 多 Agent

rust-agent 支持两种 Coding Agent 模式：

**单 Agent 模式（默认）**：一个 Agent 完成所有工作，简单高效。

**多 Agent 模式（`--multi-agent`）**：
```
                    ┌─────────────────┐
                    │  Orchestrator   │  ← 分析 feature，决定谁来做什么
                    │  (协调者)        │
                    └──────┬──────────┘
                           │ 分配任务
           ┌───────────────┼───────────────┐
           ▼               ▼               ▼
    ┌──────────┐   ┌──────────┐   ┌──────────┐
    │  Coder   │   │  Tester  │   │ Reviewer │
    │ (写代码) │   │ (写测试) │   │ (审查)   │
    └─────┬────┘   └─────┬────┘   └─────┬────┘
          │              │              │
          └──────────────┬┴──────────────┘
                         │
                   ┌─────▼─────┐
                   │ Shared    │  ← SPEC.md + Progress + Git
                   │ State     │    所有 Agent 读写同一套文件
                   └───────────┘
```

Orchestrator 根据 Feature 类型自动选择参与 Agent：
- UI/前端 → Coder + Tester
- 数据/存储 → Coder + Reviewer + Tester
- API/集成 → 三者全部参与
- 默认 → Coder + Tester + Reviewer

```bash
# 使用多 Agent 模式
cargo run -- continue --project ./my-todo --multi-agent
```

---

## 🧪 扩展练习

### 1. 添加持久化记忆
```rust
// 在 memory.rs 中添加：
// - 保存到文件 (serde_json)
// - 加载历史对话
// - 按时间戳检索
```

### 2. 实现 RAG 检索（进阶）
```rust
// 新增 src/retrieval.rs：
// - embedding 生成 (使用 LLM 的 embedding API)
// - 向量相似度搜索
// - 把相关记忆注入 context
```

### 3. 添加 MCP 工具协议（高级）
```rust
// 参考 OpenClaw 的 MCP SDK 实现：
// - 客户端：连接 MCP 服务器
// - 协议：JSON-RPC 2.0 over stdio
// - 动态发现可用工具
```

## 🔗 参考资料

- [Anthropic: Effective Harnesses for Long-Running Agents](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents) ← **核心参考**
- [OpenAI Function Calling 文档](https://platform.openai.com/docs/guides/function-calling)
- [Anthropic Tool Use 文档](https://docs.anthropic.com/en/docs/build-with-claude/tool-use)
- [ReAct 论文](https://arxiv.org/abs/2210.03629) — Agent Loop 的理论基础
