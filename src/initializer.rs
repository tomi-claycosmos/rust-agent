// # Initializer Agent — 首个会话初始化
//
// 负责根据用户需求建立完整的项目脚手架：
// 1. 创建 SPEC.md（feature list，JSON 格式）
// 2. 创建 init.sh（开发环境启动脚本）
// 3. 创建 claude-progress.txt（进度日志）
// 4. 初始化 git 仓库并做初始 commit
//
// 这是 Anthropic 双 Agent 架构的**第一个会话**：
// ```
// 用户需求 → Initializer Agent → 项目脚手架 → Coding Agent（循环）→ 最终交付
// ```

use crate::llm::{LlmClient, LlmResponse};
use crate::project::FeatureList;
use crate::tools::ToolRegistry;
use crate::memory::ConversationMemory;
use std::fs;
use std::path::Path;
use std::process::Command;

const MAX_ITERATIONS: usize = 15;

/// Initializer Agent — 建立项目脚手架
pub struct InitializerAgent {
    llm: LlmClient,
    tools: ToolRegistry,
    memory: ConversationMemory,
    project_path: String,
    project_name: String,
    user_prompt: String,
}

impl InitializerAgent {
    pub fn new(
        api_key: String,
        model: String,
        tools: ToolRegistry,
        project_path: &str,
        project_name: &str,
        user_prompt: &str,
    ) -> Self {
        let memory = ConversationMemory::new();
        Self {
            llm: LlmClient::new(api_key, model),
            tools,
            memory,
            project_path: project_path.to_string(),
            project_name: project_name.to_string(),
            user_prompt: user_prompt.to_string(),
        }
    }

    /// 执行初始化 — 主流程
    pub async fn run(&mut self) -> Result<String, InitError> {
        println!("\n🚀 Initializer Agent: 开始初始化项目...\n");

        // Step 1: 分析用户需求，生成 feature list
        let features = self.generate_feature_list().await?;

        // Step 2: 创建项目目录结构
        self.create_project_structure()?;

        // Step 3: 写入 SPEC.md
        let spec_path = Path::new(&self.project_path).join("SPEC.md");
        self.write_spec_md(&spec_path, &features)?;

        // Step 4: 写入 init.sh
        let init_sh_path = Path::new(&self.project_path).join("init.sh");
        self.write_init_sh(&init_sh_path)?;

        // Step 5: 写入 claude-progress.txt（初始状态）
        let progress_path = Path::new(&self.project_path).join("claude-progress.txt");
        self.write_initial_progress(&progress_path)?;

        // Step 6: 初始化 git 仓库
        self.init_git_repo()?;

        println!("\n✅ 项目脚手架初始化完成！\n");
        println!("  📄 SPEC.md — {} 个功能需求", features.features.len());
        println!("  📜 init.sh — 开发环境启动脚本");
        println!("  📊 claude-progress.txt — 进度日志");
        println!("  📚 git 仓库 — 初始 commit");

        Ok(format!(
            "项目 '{}' 初始化完成。{} 个功能需求已写入 SPEC.md，可交由 Coding Agent 执行。",
            self.project_name,
            features.features.len()
        ))
    }

    /// 通过 LLM 生成结构化的 feature list
    async fn generate_feature_list(&mut self) -> Result<FeatureList, InitError> {
        let system_prompt = r#"你是一个经验丰富的软件架构师。用户会提供一个项目需求，你需要生成一个结构化的功能列表（Feature List）。

要求：
1. 生成的 feature 必须具体、可测试、有验收标准
2. 每个 feature 必须有明确的 steps（实现步骤）
3. 所有 feature 一律标记 "passes": false
4. 覆盖完整的用户功能（不只是技术实现），包括：核心功能、边界情况、错误处理、UI/UX、数据持久化等
5. 输出格式：纯 JSON，不要有任何解释文字

JSON 结构：
{
  "project_name": "项目名称",
  "prompt": "原始需求（保留）",
  "features": [
    {
      "category": "功能分类（如：core, ui, error-handling, data）",
      "description": "功能描述",
      "steps": ["步骤1", "步骤2", "..."],
      "passes": false
    }
  ],
  "totalFeatures": <数字>,
  "completedFeatures": 0
}"#;

        self.memory.add_system_message(system_prompt);
        self.memory.add_user_message(&format!(
            "请为以下项目需求生成完整的 Feature List（JSON 格式）：\n\n{}",
            self.user_prompt
        ));

        let llm_response = self.llm.chat(&self.memory.get_messages(), &[]).await?;

        let text = match llm_response {
            LlmResponse::Text(t) | LlmResponse::Done(t) => t,
            LlmResponse::ToolCalls(_) => {
                return Err(InitError::LlmUnexpectedToolCall);
            }
        };

        // 从 LLM 输出中提取 JSON（可能包含 markdown 代码块）
        let json_str = extract_json(&text);
        let mut feature_list: FeatureList = serde_json::from_str(json_str)
            .map_err(|e| InitError::FeatureListParseFailed(format!("{}: {}", e, json_str)))?;

        feature_list.total_features = feature_list.features.len();

        println!("\n📋 生成 {} 个功能需求", feature_list.features.len());

        Ok(feature_list)
    }

    /// 创建基本项目目录结构
    fn create_project_structure(&self) -> Result<(), InitError> {
        let base = Path::new(&self.project_path);
        fs::create_dir_all(base)
            .map_err(|e| InitError::IoError(format!("无法创建项目目录: {}", e)))?;

        // 尝试创建常见子目录（忽略错误）
        let subdirs = ["src", "tests", "docs", "scripts", "data"];
        for dir in &subdirs {
            let _ = fs::create_dir_all(base.join(dir));
        }

        println!("  📁 项目目录: {}", self.project_path);
        Ok(())
    }

    /// 写入 SPEC.md
    fn write_spec_md(&self, path: &Path, features: &FeatureList) -> Result<(), InitError> {
        let json = serde_json::to_string_pretty(features)
            .map_err(|e| InitError::IoError(format!("序列化失败: {}", e)))?;
        let content = format!(
            "# {} — 功能规格说明书\n\n> ⚠️ 本文件由 Initializer Agent 自动生成。\n> Coding Agent 只能修改 `passes` 字段，**禁止删除或修改功能描述**。\n\n## 原始需求\n\n{}\n\n## 功能列表（JSON）\n\n```json\n{}\n```\n",
            self.project_name,
            self.user_prompt,
            json
        );
        fs::write(path, content)
            .map_err(|e| InitError::IoError(format!("写入 SPEC.md 失败: {}", e)))?;
        println!("  ✅ SPEC.md");
        Ok(())
    }

    /// 写入 init.sh
    fn write_init_sh(&self, path: &Path) -> Result<(), InitError> {
        // 推断项目的启动方式
        let content = format!(
            r#"#!/bin/bash
# init.sh — 项目开发环境启动脚本
# 由 Initializer Agent 生成

set -e
cd "$(dirname "$0")"

echo "🚀 初始化项目: {name}"

# 检测项目类型并启动开发环境
if [ -f "Cargo.toml" ]; then
    echo "📦 Rust 项目检测到，启动 cargo..."
    cargo build 2>&1 | tail -5
elif [ -f "package.json" ]; then
    echo "📦 Node.js 项目检测到，安装依赖..."
    npm install
    echo "🌐 启动开发服务器..."
    npm run dev &
elif [ -f "requirements.txt" ]; then
    echo "🐍 Python 项目检测到，创建虚拟环境..."
    python3 -m venv .venv
    source .venv/bin/activate
    pip install -r requirements.txt
else
    echo "⚠️  未检测到已知项目类型，请手动启动"
fi

echo ""
echo "✅ 项目已启动"
echo "📊 进度日志: claude-progress.txt"
echo "📋 功能列表: SPEC.md"
"#,
            name = self.project_name
        );
        fs::write(path, content)
            .map_err(|e| InitError::IoError(format!("写入 init.sh 失败: {}", e)))?;
        // 设置可执行权限
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(path)
                .map_err(|e| InitError::IoError(format!("无法读取权限: {}", e)))?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms)
                .map_err(|e| InitError::IoError(format!("设置权限失败: {}", e)))?;
        }
        println!("  ✅ init.sh");
        Ok(())
    }

    /// 写入初始进度日志
    fn write_initial_progress(&self, path: &Path) -> Result<(), InitError> {
        let content = format!(
            "# claude-progress.txt — 项目进度日志\n# 由 Initializer Agent 生成\n# 格式：[时间戳] Feature #N: 描述\n# Coding Agent 每次完成后在此追加条目\n\n[INIT] 项目初始化完成\n  Project: {}\n  Total Features: {} 个\n  Status: 等待 Coding Agent 开始\n\n---\n",
            self.project_name,
            self.project_name.len()
        );
        fs::write(path, content)
            .map_err(|e| InitError::IoError(format!("写入进度文件失败: {}", e)))?;
        println!("  ✅ claude-progress.txt");
        Ok(())
    }

    /// 初始化 git 仓库
    fn init_git_repo(&self) -> Result<(), InitError> {
        let base = Path::new(&self.project_path);

        // 检查 git 是否可用
        let git_available = Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if !git_available {
            println!("  ⚠️  git 未安装，跳过版本控制初始化");
            return Ok(());
        }

        // 检查是否已是 git 仓库
        let is_git = base.join(".git").exists();

        if !is_git {
            let init_output = Command::new("git")
                .args(["init"])
                .current_dir(base)
                .output()
                .map_err(|e| InitError::IoError(format!("git init 失败: {}", e)))?;

            if !init_output.status.success() {
                println!("  ⚠️  git init 失败，跳过");
                return Ok(());
            }
        }

        // 初始 commit
        let add_output = Command::new("git")
            .args(["add", "-A"])
            .current_dir(base)
            .output()
            .map_err(|e| InitError::IoError(format!("git add 失败: {}", e)))?;

        let commit_output = Command::new("git")
            .args(["commit", "-m", "init: project scaffold by Initializer Agent\n\n- SPEC.md: feature list\n- init.sh: dev startup\n- claude-progress.txt: progress log"])
            .current_dir(base)
            .output()
            .map_err(|e| InitError::IoError(format!("git commit 失败: {}", e)))?;

        if commit_output.status.success() {
            println!("  ✅ git 仓库初始化完成（初始 commit）");
        } else {
            println!("  ⚠️  git commit 失败，可能没有文件需要提交");
        }

        Ok(())
    }
}

/// 从文本中提取 JSON（支持 markdown 代码块包裹）
fn extract_json(text: &str) -> &str {
    let text = text.trim();
    // 尝试找 ```json ... ``` 包裹
    if let Some(start) = text.find("```json") {
        let rest = &text[start + 7..];
        if let Some(end) = rest.find("```") {
            return rest[..end].trim();
        }
    }
    // 尝试找 ``` ... ```
    if let Some(start) = text.find("```") {
        let rest = &text[start + 3..];
        if let Some(end) = rest.find("```") {
            return rest[..end].trim();
        }
    }
    // 尝试找 { ... }
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return &text[start..=end];
        }
    }
    text
}

#[derive(Debug)]
pub enum InitError {
    LlmError(String),
    LlmUnexpectedToolCall,
    FeatureListParseFailed(String),
    IoError(String),
    GitError(String),
}

impl std::fmt::Display for InitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LlmError(s) => write!(f, "LLM 错误: {}", s),
            Self::LlmUnexpectedToolCall => write!(f, "LLM 意外返回了工具调用"),
            Self::FeatureListParseFailed(s) => write!(f, "Feature List 解析失败: {}", s),
            Self::IoError(s) => write!(f, "IO 错误: {}", s),
            Self::GitError(s) => write!(f, "Git 错误: {}", s),
        }
    }
}

impl std::error::Error for InitError {}

impl From<crate::llm::LlmError> for InitError {
    fn from(e: crate::llm::LlmError) -> Self {
        Self::LlmError(e.0)
    }
}
