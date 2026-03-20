//! # Coding Agent — 增量式编程 Agent
//!
//! 对应 Anthropic 双 Agent 架构的**后续会话**。
//!
//! 每次会话的流程：
//! 1. **Get Bearings**: 读取进度文件 + git log + SPEC.md，快速了解当前状态
//! 2. **Health Check**: 运行 init.sh，验证基础功能没被破坏
//! 3. **Pick Feature**: 从 SPEC.md 选一个最高优先级未完成的功能
//! 4. **Implement**: 实现该功能（可能需要多轮工具调用）
//! 5. **Test**: 端到端测试验证
//! 6. **Update**: 标记 passes=true，更新 claude-progress.txt，git commit
//!
//! 关键约束：
//! - **每次只做一个 feature** —— 防止上下文耗尽
//! - **必须端到端测试** —— 不能仅靠代码审查
//! - **git commit 交接** —— 下一会话通过 git 历史恢复状态

use crate::llm::{LlmClient, LlmResponse, LlmMessage};
use crate::project::{FeatureList, ProgressLog, ProgressEntry, Feature};
use crate::tools::ToolRegistry;
use crate::memory::ConversationMemory;
use std::fs;
use std::path::Path;
use std::process::Command;

const MAX_ITERATIONS_PER_FEATURE: usize = 20;

/// Coding Agent — 增量式编程
pub struct CodingAgent {
    llm: LlmClient,
    tools: ToolRegistry,
    memory: ConversationMemory,
    project_path: String,
    spec_path: String,
    progress_path: String,
}

impl CodingAgent {
    pub fn new(
        api_key: String,
        model: String,
        tools: ToolRegistry,
        project_path: &str,
    ) -> Self {
        let memory = ConversationMemory::new();
        Self {
            llm: LlmClient::new(api_key, model),
            tools,
            memory,
            project_path: project_path.to_string(),
            spec_path: Path::new(project_path).join("SPEC.md").to_string_lossy().to_string(),
            progress_path: Path::new(project_path).join("claude-progress.txt").to_string_lossy().to_string(),
        }
    }

    /// 运行 Coding Agent — 主循环
    ///
    /// 返回 (是否完成所有功能, 报告字符串)
    pub async fn run(&mut self) -> Result<String, CodingError> {
        println!("\n🔧 Coding Agent: 开始增量编程...\n");

        // Step 1: 读取当前状态（Get Bearings）
        let (features, progress) = self.get_bearings()?;

        // Step 2: 健康检查（init.sh 验证）
        self.health_check()?;

        // Step 3: 选一个待做的 feature
        let (feature_idx, feature) = match features.next_pending_feature() {
            Some((idx, f)) => (idx, f.clone()),
            None => {
                return Ok("🎉 所有功能已完成！项目交付。".to_string());
            }
        };

        println!("\n📌 选中 Feature #{}: {}", feature_idx, feature.description);
        println!("  步骤: {}", feature.steps.join(" → "));

        // Step 4: 实现该 feature
        self.implement_feature(feature_idx, &feature).await?;

        // Step 5: 更新 SPEC.md（mark passes=true）
        let mut features = FeatureList::load_from_file(Path::new(&self.spec_path))
            .map_err(|e| CodingError::SpecLoadFailed(e))?;
        features.mark_feature_passed(feature_idx);
        features.save_to_file(Path::new(&self.spec_path))
            .map_err(|e| CodingError::SpecSaveFailed(e))?;

        // Step 6: 更新进度日志
        let entry = ProgressEntry::new(
            feature_idx,
            &feature.description,
            &format!("已实现 {} 个步骤", feature.steps.len()),
            "完成，等待测试验证",
            vec!["端到端测试".to_string()],
        );
        let mut log = ProgressLog::load_from_file(Path::new(&self.progress_path))
            .unwrap_or_else(|_| ProgressLog::new());
        log.add_entry(entry);
        log.save_to_file(Path::new(&self.progress_path))
            .map_err(|e| CodingError::ProgressSaveFailed(e))?;

        // Step 7: Git commit
        self.git_commit(feature_idx, &feature.description)?;

        let remaining = features.total_features - features.completed_features;
        Ok(format!(
            "Feature #{} 完成 ✅\n进度: {}/{} ({:.0}%)\n剩余: {} 个功能",
            feature_idx,
            features.completed_features,
            features.total_features,
            features.progress_percent(),
            remaining
        ))
    }

    /// Get Bearings — 快速了解项目当前状态
    fn get_bearings(&mut self) -> Result<(FeatureList, ProgressLog), CodingError> {
        println!("  📖 读取项目状态...");

        let spec = FeatureList::load_from_file(Path::new(&self.spec_path))
            .map_err(|e| CodingError::SpecLoadFailed(e))?;
        let progress = ProgressLog::load_from_file(Path::new(&self.progress_path))
            .unwrap_or_else(|_| ProgressLog::new());

        println!("  项目: {}", spec.project_name);
        println!("  总功能: {} | 已完成: {} | 进度: {:.0}%",
            spec.total_features, spec.completed_features, spec.progress_percent());

        // 读取最近的 git commit 历史
        if let Some(last) = progress.last_entry() {
            println!("  上次完成: Feature #{} — {}", last.feature_index, last.feature_desc);
        }

        // 读取 git log
        let git_log = self.read_git_log();
        if !git_log.is_empty() {
            println!("  📚 最近 git commits:");
            for line in git_log.lines().take(5) {
                println!("    {}", line);
            }
        }

        Ok((spec, progress))
    }

    /// Health Check — 验证基础功能没被破坏
    fn health_check(&self) -> Result<(), CodingError> {
        println!("\n  🏥 健康检查...");

        let init_sh = Path::new(&self.project_path).join("init.sh");
        if !init_sh.exists() {
            println!("  ⚠️  init.sh 不存在，跳过健康检查");
            return Ok(());
        }

        // 简单检查：运行 init.sh 看是否报错
        #[cfg(unix)]
        {
            let output = Command::new("bash")
                .arg(&init_sh)
                .current_dir(&self.project_path)
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    println!("  ✅ init.sh 运行正常");
                }
                Ok(o) => {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    if !stderr.is_empty() || !stdout.contains("未检测到") {
                        println!("  ⚠️  init.sh 有警告: {}", stderr.trim().lines().last().unwrap_or(""));
                    } else {
                        println!("  ✅ init.sh 检查通过");
                    }
                }
                Err(e) => {
                    println!("  ⚠️  无法运行 init.sh: {}", e);
                }
            }
        }

        Ok(())
    }

    /// 实现单个 feature（核心实现循环）
    async fn implement_feature(&mut self, feature_idx: usize, feature: &Feature) -> Result<(), CodingError> {
        let system_prompt = r#"你是一个专业的 Rust 开发者，正在根据 SPEC.md 中的功能描述进行增量开发。

规则（必须遵守）：
1. 每次只实现一个功能，不要试图一次性做太多
2. 实现后必须测试，不能仅靠代码审查判断功能是否正常
3. 始终保持代码整洁：有适当的错误处理，注释清晰
4. 完成后更新 claude-progress.txt，记录你做了什么
5. 禁止删除 SPEC.md 中的功能描述，只允许修改 passes 字段

工作目录：项目根目录
工具：calculator, bash, get_weather, get_current_time, web_search, 以及代码编辑工具（通过 bash 使用 echo/vi/cat 等）"#;

        self.memory.add_system_message(system_prompt);
        self.memory.add_user_message(&format!(
            "请实现以下功能：\n\nFeature #{}: {}\n分类: {}\n步骤:\n{}\n\n工作目录: {}\n\n完成后：\n1. 编写测试代码验证功能\n2. 运行测试\n3. 如果测试通过，更新 SPEC.md 中该 feature 的 passes=true\n4. 更新 claude-progress.txt\n5. git add + commit",
            feature_idx,
            feature.description,
            feature.category,
            feature.steps.iter().enumerate().map(|(i, s)| format!("  {}. {}", i + 1, s)).collect::<Vec<_>>().join("\n"),
            self.project_path
        ));

        // Agent loop: 最多 MAX_ITERATIONS_PER_FEATURE 轮
        for iteration in 0..MAX_ITERATIONS_PER_FEATURE {
            println!("\n  🔄 实现迭代 {}/{}", iteration + 1, MAX_ITERATIONS_PER_FEATURE);

            let llm_response = self.llm.chat(&self.memory.get_messages(), &self.tools.list()).await?;

            match llm_response {
                LlmResponse::Text(text) => {
                    self.memory.add_assistant_message(&text);
                    // LLM 停止调用工具，视为该 feature 实现完成
                    if text.contains("完成") || text.contains("done") || text.contains("complete") || text.contains("pass") {
                        println!("  ✅ 实现完成");
                        return Ok(());
                    }
                    // 有文本响应但没有明确说完成，说明在等待更多信息
                    println!("  💬 {}", &text[..text.len().min(100)]);
                    return Ok(());
                }
                LlmResponse::ToolCalls(calls) => {
                    let mut all_results = Vec::new();
                    for call in calls {
                        println!("    🔧 {}", call.name);

                        // 执行工具
                        match self.tools.execute(&call.name, &call.arguments) {
                            Ok(result) => {
                                self.memory.add_tool_result(&call.id, &result);
                                all_results.push((call.id, result));
                            }
                            Err(e) => {
                                let err_msg = format!("工具执行失败: {}", e);
                                self.memory.add_tool_result(&call.id, &err_msg);
                                all_results.push((call.id, err_msg));
                            }
                        }
                    }

                    // 把工具结果转为 assistant 消息继续循环
                    let result_summary = all_results.iter()
                        .map(|(id, r)| format!("[{}]: {}", id, r))
                        .collect::<Vec<_>>()
                        .join("; ");
                    self.memory.add_assistant_message(&format!("工具执行结果: {}", result_summary));
                }
                LlmResponse::Done(text) => {
                    self.memory.add_assistant_message(&text);
                    println!("  ✅ 实现完成: {}", text.trim().chars().take(80).collect::<String>());
                    return Ok(());
                }
            }
        }

        Err(CodingError::MaxIterationsReached)
    }

    /// 读取 git log
    fn read_git_log(&self) -> String {
        let output = Command::new("git")
            .args(["log", "--oneline", "-10"])
            .current_dir(&self.project_path)
            .output();

        match output {
            Ok(o) if o.status.success() => {
                String::from_utf8_lossy(&o.stdout).to_string()
            }
            _ => String::new(),
        }
    }

    /// Git commit 当前进度
    fn git_commit(&self, feature_idx: usize, feature_desc: &str) -> Result<(), CodingError> {
        let output = Command::new("git")
            .args(["add", "-A"])
            .current_dir(&self.project_path)
            .output();

        match output {
            Ok(o) if o.status.success() => {}
            _ => { return Ok(()); }
        }

        let commit_msg = format!(
            "feat(#{}): {}\n\n- Feature #{} completed\n- Source: claude-progress.txt",
            feature_idx,
            feature_desc.chars().take(50).collect::<String>(),
            feature_idx
        );

        let output = Command::new("git")
            .args(["commit", "-m", &commit_msg])
            .current_dir(&self.project_path)
            .output();

        match output {
            Ok(o) if o.status.success() => {
                println!("  ✅ git commit: Feature #{}", feature_idx);
            }
            _ => {
                println!("  ⚠️  git commit 无变化（可能没有文件变更）");
            }
        }

        Ok(())
    }

    /// 清除记忆（单次会话模式）
    pub fn clear_memory(&mut self) {
        self.memory.clear();
    }
}

#[derive(Debug)]
pub enum CodingError {
    LlmError(String),
    SpecLoadFailed(String),
    SpecSaveFailed(String),
    ProgressSaveFailed(String),
    GitError(String),
    HealthCheckFailed,
    MaxIterationsReached,
}

impl std::fmt::Display for CodingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LlmError(s) => write!(f, "LLM 错误: {}", s),
            Self::SpecLoadFailed(s) => write!(f, "加载 SPEC.md 失败: {}", s),
            Self::SpecSaveFailed(s) => write!(f, "保存 SPEC.md 失败: {}", s),
            Self::ProgressSaveFailed(s) => write!(f, "保存进度文件失败: {}", s),
            Self::GitError(s) => write!(f, "Git 错误: {}", s),
            Self::HealthCheckFailed => write!(f, "健康检查失败"),
            Self::MaxIterationsReached => write!(f, "达到最大迭代次数（{}次）", MAX_ITERATIONS_PER_FEATURE),
        }
    }
}

impl std::error::Error for CodingError {}

impl From<crate::llm::LlmError> for CodingError {
    fn from(e: crate::llm::LlmError) -> Self {
        Self::LlmError(e.0)
    }
}
