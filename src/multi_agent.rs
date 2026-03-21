//  # Multi-Agent Architecture — 多专业 Agent 协作
// 
//  Anthropic 论文提出的开放问题：单一通用 Agent vs 多专业 Agent，谁更强？
// 
//  本实现给出一个具体答案：**多专业 Agent 协作**。
// 
//  ## 架构设计
// 
//  ```text
//                     ┌─────────────────┐
//                     │  Orchestrator   │  ← 理解 feature，决定谁来做什么
//                     │  (协调者)        │
//                     └──────┬──────────┘
//                            │ 分配任务
//            ┌───────────────┼───────────────┐
//            ▼               ▼               ▼
//     ┌──────────┐   ┌──────────┐   ┌──────────┐
//     │  Coder   │   │  Tester  │   │ Reviewer │
//     │ (写代码) │   │ (写测试) │   │ (审查)   │
//     └─────┬────┘   └─────┬────┘   └─────┬────┘
//          │              │              │
//          └──────────────┬┴──────────────┘
//                          │ 共享状态
//                    ┌─────▼─────┐
//                    │ Shared    │  ← SPEC.md + Progress + Git
//                    │ State     │    所有 Agent 读写同一套文件
//                    └───────────┘
// ```
//
// ## 工作流程
//
// Orchestrator 拿到一个 feature 后：
// 1. 分析 feature 的类型（UI / 逻辑 / 数据 / 集成）
// 2. 把任务分配给最合适的 Agent
// 3. Coder 实现 → Tester 验证 → Reviewer 把关
// 4. 三方都通过 → 更新 SPEC.md + commit

use crate::llm::{LlmClient, LlmResponse};
use crate::project::{Feature, FeatureList, ProgressLog, ProgressEntry};
use crate::tools::ToolRegistry;
use crate::memory::ConversationMemory;
use std::path::Path;
use std::process::Command;

const MAX_ITERATIONS_PER_PHASE: usize = 15;

// ─────────────────────────────────────────────
// Agent Role — 角色枚举
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentRole {
    Coder,
    Tester,
    Reviewer,
}

impl AgentRole {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Coder => "Coder",
            Self::Tester => "Tester",
            Self::Reviewer => "Reviewer",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Coder => "👨‍💻",
            Self::Tester => "🧪",
            Self::Reviewer => "🔍",
        }
    }

    pub fn system_prompt(&self) -> &'static str {
        match self {
            Self::Coder => {
                r#"你是一个 Rust 开发专家，负责根据 Feature 描述实现功能代码。

你的职责：
1. 严格按照 Feature 描述的 steps 实现功能
2. 代码要整洁，有适当的错误处理
3. 不要做测试（那是 Tester 的工作）
4. 不要做代码审查（那是 Reviewer 的工作）
5. 完成后在 claude-progress.txt 中记录你做了什么

工具：bash（用于文件操作）、calculator 等。
代码编辑方式：通过 bash 使用 echo/tee/cat 重定向写入文件。"#
            }
            Self::Tester => {
                r#"你是一个专业的 QA 工程师，负责验证功能是否正确实现。

你的职责：
1. 理解 Feature 描述和预期行为
2. 设计测试用例（happy path + 边界情况）
3. 编写并运行测试代码
4. 对照 Feature 逐条验证每个 step 是否完成
5. 如果发现问题，明确指出哪些 step 未通过

你的判断是权威的：如果 Tester 说功能有问题，功能就没完成。
工具：bash（运行测试）、calculator（辅助）、web_search（查文档）。

判断标准：
- 功能完全符合描述 → "PASS"
- 有小问题但不影响核心功能 → "MINOR ISSUES"
- 功能有问题或缺失 → "FAIL""#
            }
            Self::Reviewer => {
                r#"你是一个资深软件架构师，负责代码质量把关。

你的职责：
1. 审查 Coder 写的代码质量
2. 检查：代码风格、错误处理、边界情况、安全性
3. 检查：是否有多余代码或技术债务
4. 提出改进建议（如果有必要的话）
5. 如果代码质量过关 → 批准；如果有问题 → 打回 Coder 修改

注意：
- 你不是来重写代码的，是来把关的
- 小问题可以接受，大问题必须修复
- 如果需要 Coder 修改，通过 summary 明确说明

判断标准：
- 代码质量良好 → "APPROVED"
- 需要小修改 → "REQUEST CHANGES: ..."
- 有严重问题 → "REJECTED: ...""#
            }
        }
    }
}

// ─────────────────────────────────────────────
// Specialized Agent — 专业 Agent
// ─────────────────────────────────────────────

/// 专业 Agent（Coder / Tester / Reviewer 之一）
pub struct SpecializedAgent {
    role: AgentRole,
    llm: LlmClient,
    memory: ConversationMemory,
    tools: ToolRegistry,
    project_path: String,
    feature: Feature,
    feature_idx: usize,
}

impl SpecializedAgent {
    pub fn new(
        role: AgentRole,
        api_key: String,
        model: String,
        tools: ToolRegistry,
        project_path: &str,
        feature: Feature,
        feature_idx: usize,
    ) -> Self {
        let memory = ConversationMemory::new();
        Self {
            role,
            llm: LlmClient::new(api_key, model),
            memory,
            tools,
            project_path: project_path.to_string(),
            feature,
            feature_idx,
        }
    }

    /// 执行本 Agent 的任务
    pub async fn run(&mut self) -> Result<PhaseResult, AgentError> {
        println!("\n{} {} Agent 开始工作...", self.role.emoji(), self.role.name());

        self.memory.add_system_message(self.role.system_prompt());
        self.memory.add_user_message(&self.build_prompt());

        for iteration in 0..MAX_ITERATIONS_PER_PHASE {
            println!("  🔄 {}/{}", iteration + 1, MAX_ITERATIONS_PER_PHASE);

            let llm_response = self.llm.chat(
                &self.memory.get_messages(),
                &self.tools.list(),
            ).await?;

            match llm_response {
                LlmResponse::Text(text) => {
                    self.memory.add_assistant_message(&text);

                    // 解析 Agent 的判断结果
                    let result = self.parse_phase_result(&text);
                    if let Some(r) = result {
                        println!("{} {}: {}", self.role.emoji(), self.role.name(), r.verdict);
                        return Ok(r);
                    }

                    println!("  💬 {}", &text[..text.len().min(120)]);
                }
                LlmResponse::ToolCalls(calls) => {
                    self.handle_tool_calls(calls).await?;
                }
                LlmResponse::Done(text) => {
                    self.memory.add_assistant_message(&text);
                    let result = self.parse_phase_result(&text)
                        .unwrap_or(PhaseResult {
                            verdict: text.trim().chars().take(100).collect(),
                            summary: text.clone(),
                            pass: false,
                        });
                    return Ok(result);
                }
            }
        }

        Err(AgentError::MaxIterationsReached)
    }

    fn build_prompt(&self) -> String {
        format!(
            r#"请完成以下任务：

Feature #{}: {}
分类: {}
描述: {}

步骤:
{}

工作目录: {}

请开始执行，完成后报告你的结论（PASS/FAIL/APPROVED 等）。"#,
            self.feature_idx,
            self.feature.description,
            self.feature.category,
            self.feature.description,
            self.feature.steps.iter()
                .enumerate()
                .map(|(i, s)| format!("  {}. {}", i + 1, s))
                .collect::<Vec<_>>()
                .join("\n"),
            self.project_path
        )
    }

    /// 解析 Agent 的输出，提取判断结果
    fn parse_phase_result(&self, text: &str) -> Option<PhaseResult> {
        let text_upper = text.to_uppercase();

        let (verdict, pass) = if text_upper.contains("PASS") || text_upper.contains("APPROVED") {
            ("PASS / APPROVED".to_string(), true)
        } else if text_upper.contains("MINOR ISSUES") {
            ("MINOR ISSUES".to_string(), true)
        } else if text_upper.contains("FAIL") || text_upper.contains("REJECTED") {
            ("FAIL / REJECTED".to_string(), false)
        } else {
            return None;
        };

        Some(PhaseResult {
            verdict,
            summary: text.to_string(),
            pass,
        })
    }

    async fn handle_tool_calls(&mut self, calls: Vec<crate::llm::ToolCall>) -> Result<(), AgentError> {
        for call in calls {
            println!("    🔧 {}", call.name);
            match self.tools.execute(&call.name, &call.arguments) {
                Ok(result) => {
                    self.memory.add_tool_result(&call.id, &result);
                }
                Err(e) => {
                    let err = format!("工具执行失败: {}", e);
                    self.memory.add_tool_result(&call.id, &err);
                }
            }
        }
        Ok(())
    }
}

/// 阶段执行结果
#[derive(Debug, Clone)]
pub struct PhaseResult {
    /// 判断结果（PASS / FAIL / APPROVED / REQUEST CHANGES / REJECTED）
    pub verdict: String,
    /// Agent 的完整输出摘要
    pub summary: String,
    /// 是否通过
    pub pass: bool,
}

// ─────────────────────────────────────────────
// Orchestrator — 多 Agent 协调者
// ─────────────────────────────────────────────

/// Orchestrator — 多 Agent 系统的协调者
///
/// 负责：
/// 1. 分析 Feature 类型和复杂度
/// 2. 选择参与的专业 Agent
/// 3. 管理执行流程（串行或并行）
/// 4. 汇总结果，决定是否完成
pub struct Orchestrator {
    llm: LlmClient,
    project_path: String,
    spec_path: String,
    progress_path: String,
}

impl Orchestrator {
    pub fn new(api_key: String, model: String, project_path: &str) -> Self {
        Self {
            llm: LlmClient::new(api_key, model),
            project_path: project_path.to_string(),
            spec_path: Path::new(project_path).join("SPEC.md").to_string_lossy().to_string(),
            progress_path: Path::new(project_path).join("claude-progress.txt").to_string_lossy().to_string(),
        }
    }

    /// 执行一个 Feature：Orchestrator 协调多个专业 Agent 完成
    pub async fn execute_feature(
        &self,
        api_key: String,
        model: String,
        feature_idx: usize,
        feature: Feature,
    ) -> Result<FeatureOutcome, OrchestratorError> {
        println!("\n{} Orchestrator: 协调 Feature #{}", "🎯", feature_idx);
        println!("  Feature: {}", feature.description);
        println!("  分类: {}", feature.category);

        // 决定哪些 Agent 参与
        let participants = self.select_agents(&feature);

        println!("  参与 Agent: {}", participants.iter()
            .map(|r| format!("{}({})", r.emoji(), r.name()))
            .collect::<Vec<_>>()
            .join(", "));

        let mut outcomes = Vec::new();
        let mut current_feature = feature.clone();

        // 串行执行每个参与 Agent
        for role in &participants {
            let mut agent = SpecializedAgent::new(
                *role,
                api_key.clone(),
                model.clone(),
                ToolRegistry::new(),
                &self.project_path,
                current_feature.clone(),
                feature_idx,
            );

            match agent.run().await {
                Ok(result) => {
                    outcomes.push((role, result.clone()));

                    // 根据结果决定下一步
                    match role {
                        AgentRole::Coder => {
                            if !result.pass {
                                // Coder 失败，重试（最多 2 次）
                                println!("  ⚠️ Coder 失败，报告: {}", result.verdict);
                            }
                        }
                        AgentRole::Tester => {
                            if !result.pass {
                                // Tester 发现问题 → 打回 Coder
                                println!("  🔄 Tester 发现问题，打回 Coder 修复");
                                // 重新调用 Coder（带 Tester 的反馈）
                                let mut coder = SpecializedAgent::new(
                                    AgentRole::Coder,
                                    api_key.clone(),
                                    model.clone(),
                                    ToolRegistry::new(),
                                    &self.project_path,
                                    current_feature.clone(),
                                    feature_idx,
                                );
                                // 如果 Coder 再次失败，继续往下走（防止死循环）
                                if let Ok(retry_result) = coder.run().await {
                                    outcomes.push((&AgentRole::Coder, retry_result));
                                }
                            }
                        }
                        AgentRole::Reviewer => {
                            if result.verdict.contains("REQUEST CHANGES") {
                                println!("  🔄 Reviewer 要求修改");
                            } else if result.verdict.contains("REJECTED") {
                                println!("  ⛔ Reviewer 拒绝，重试");
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("  ❌ {} Agent 错误: {}", role.name(), e);
                    outcomes.push((role, PhaseResult {
                        verdict: format!("ERROR: {}", e),
                        summary: e.to_string(),
                        pass: false,
                    }));
                }
            }
        }

        // 汇总结果
        let all_passed = outcomes.iter().all(|(_, r)| r.pass);
        let summary = outcomes.iter()
            .map(|(r, o)| format!("{}: {}", r.name(), o.verdict))
            .collect::<Vec<_>>()
            .join(" → ");

        Ok(FeatureOutcome {
            feature_idx,
            feature_description: feature.description.clone(),
            all_passed,
            agent_results: outcomes.into_iter().map(|(r, o)| (r.clone(), o)).collect(),
            summary,
        })
    }

    /// 根据 Feature 类型选择参与的 Agent
    fn select_agents(&self, feature: &Feature) -> Vec<AgentRole> {
        let cat = feature.category.to_lowercase();

        if cat.contains("ui") || cat.contains("frontend") || cat.contains("界面") {
            // UI 类：需要 Coder + Tester（UI 难以自动化测试，手动为主）
            vec![AgentRole::Coder, AgentRole::Tester]
        } else if cat.contains("data") || cat.contains("storage") || cat.contains("数据") {
            // 数据类：Coder + Tester + Reviewer（数据完整性要求高）
            vec![AgentRole::Coder, AgentRole::Reviewer, AgentRole::Tester]
        } else if cat.contains("api") || cat.contains("integration") || cat.contains("集成") {
            // 集成类：三个都需要
            vec![AgentRole::Coder, AgentRole::Tester, AgentRole::Reviewer]
        } else if cat.contains("error") || cat.contains("boundary") || cat.contains("边界") {
            // 错误处理类：Reviewer 特别重要
            vec![AgentRole::Coder, AgentRole::Reviewer, AgentRole::Tester]
        } else {
            // 默认：核心功能 = Coder + Tester + Reviewer
            vec![AgentRole::Coder, AgentRole::Tester, AgentRole::Reviewer]
        }
    }
}

// ─────────────────────────────────────────────
// Shared State — 共享状态（供所有 Agent 读写）
// ─────────────────────────────────────────────

/// 共享状态管理器
/// 封装了对 SPEC.md 和 claude-progress.txt 的读写操作
pub struct SharedState {
    project_path: String,
}

impl SharedState {
    pub fn new(project_path: &str) -> Self {
        Self {
            project_path: project_path.to_string(),
        }
    }

    pub fn mark_feature_done(&self, feature_idx: usize) -> Result<(), String> {
        let spec_path = Path::new(&self.project_path).join("SPEC.md");
        let mut features = FeatureList::load_from_file(&spec_path)
            .map_err(|e| format!("加载 SPEC.md 失败: {}", e))?;
        features.mark_feature_passed(feature_idx);
        features.save_to_file(&spec_path)
            .map_err(|e| format!("保存 SPEC.md 失败: {}", e))?;
        Ok(())
    }

    pub fn log_progress(&self, entry: ProgressEntry) -> Result<(), String> {
        let progress_path = Path::new(&self.project_path).join("claude-progress.txt");
        let mut log = ProgressLog::load_from_file(&progress_path)
            .unwrap_or_else(|_| ProgressLog::new());
        log.add_entry(entry);
        log.save_to_file(&progress_path)
            .map_err(|e| format!("保存进度文件失败: {}", e))?;
        Ok(())
    }

    pub fn git_commit(&self, feature_idx: usize, feature_desc: &str) -> Result<(), String> {
        let output = Command::new("git")
            .args(["add", "-A"])
            .current_dir(&self.project_path)
            .output()
            .map_err(|e| format!("git add 失败: {}", e))?;

        if !output.status.success() {
            return Ok(());
        }

        let msg = format!(
            "feat(#{}): {}\n\n- Feature #{} completed via multi-agent\n- Roles: Coder + Tester + Reviewer",
            feature_idx,
            feature_desc.chars().take(50).collect::<String>(),
            feature_idx
        );

        let output = Command::new("git")
            .args(["commit", "-m", &msg])
            .current_dir(&self.project_path)
            .output()
            .map_err(|e| format!("git commit 失败: {}", e))?;

        if output.status.success() {
            println!("  ✅ git commit: Feature #{}", feature_idx);
        }

        Ok(())
    }
}

// ─────────────────────────────────────────────
// 结果类型
// ─────────────────────────────────────────────

/// 单个 Feature 的执行结果
#[derive(Debug, Clone)]
pub struct FeatureOutcome {
    pub feature_idx: usize,
    pub feature_description: String,
    pub all_passed: bool,
    pub agent_results: Vec<(AgentRole, PhaseResult)>,
    pub summary: String,
}

// ─────────────────────────────────────────────
// 错误类型
// ─────────────────────────────────────────────

#[derive(Debug)]
pub enum AgentError {
    LlmError(String),
    MaxIterationsReached,
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LlmError(s) => write!(f, "LLM 错误: {}", s),
            Self::MaxIterationsReached => {
                write!(f, "达到最大迭代次数（{}次）", MAX_ITERATIONS_PER_PHASE)
            }
        }
    }
}

impl std::error::Error for AgentError {}

impl From<crate::llm::LlmError> for AgentError {
    fn from(e: crate::llm::LlmError) -> Self {
        Self::LlmError(e.0)
    }
}

#[derive(Debug)]
pub enum OrchestratorError {
    SpecLoadFailed(String),
    ProgressSaveFailed(String),
    FeatureFailed(String),
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpecLoadFailed(s) => write!(f, "加载 SPEC.md 失败: {}", s),
            Self::ProgressSaveFailed(s) => write!(f, "保存进度文件失败: {}", s),
            Self::FeatureFailed(s) => write!(f, "Feature 执行失败: {}", s),
        }
    }
}

impl std::error::Error for OrchestratorError {}
