//! # Project Scaffolding — 长运行项目的文件结构定义
//!
//! 参考 Anthropic 的双 Agent 架构，长运行项目需要以下文件：
//! - SPEC.md: 结构化 feature list（JSON格式）
//! - claude-progress.txt: 进度日志
//! - init.sh: 开发环境启动脚本
//! - git: 版本历史记录

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Feature 定义（对应 Anthropic 论文中的 feature list）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    pub category: String,
    pub description: String,
    pub steps: Vec<String>,
    #[serde(default)]
    pub passes: bool,
}

/// Feature List — 完整功能列表
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureList {
    pub project_name: String,
    pub prompt: String,
    pub features: Vec<Feature>,
    #[serde(rename = "totalFeatures")]
    pub total_features: usize,
    #[serde(rename = "completedFeatures")]
    pub completed_features: usize,
}

impl FeatureList {
    pub fn new(project_name: &str, prompt: &str) -> Self {
        Self {
            project_name: project_name.to_string(),
            prompt: prompt.to_string(),
            features: Vec::new(),
            total_features: 0,
            completed_features: 0,
        }
    }

    /// 加载已有的 feature list
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("无法读取 SPEC.md: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("SPEC.md 解析失败: {}", e))
    }

    /// 保存到文件
    pub fn save_to_file(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("序列化失败: {}", e))?;
        fs::write(path, json)
            .map_err(|e| format!("写入失败: {}", e))
    }

    /// 获取下一个未完成的高优先级 feature
    pub fn next_pending_feature(&self) -> Option<(usize, &Feature)> {
        self.features.iter()
            .enumerate()
            .find(|(_, f)| !f.passes)
    }

    /// 标记 feature 为通过
    pub fn mark_feature_passed(&mut self, index: usize) {
        if index < self.features.len() {
            self.features[index].passes = true;
            self.completed_features = self.features.iter().filter(|f| f.passes).count();
        }
    }

    /// 进度百分比
    pub fn progress_percent(&self) -> f64 {
        if self.total_features == 0 { return 0.0; }
        (self.completed_features as f64 / self.total_features as f64) * 100.0
    }
}

/// 进度日志条目
#[derive(Debug, Clone)]
pub struct ProgressEntry {
    pub timestamp: String,
    pub feature_index: usize,
    pub feature_desc: String,
    pub action: String,
    pub outcome: String,
    pub next_steps: Vec<String>,
}

impl ProgressEntry {
    pub fn new(index: usize, feature_desc: &str, action: &str, outcome: &str, next: Vec<String>) -> Self {
        Self {
            timestamp: current_timestamp(),
            feature_index: index,
            feature_desc: feature_desc.to_string(),
            action: action.to_string(),
            outcome: outcome.to_string(),
            next_steps: next,
        }
    }

    pub fn to_file_format(&self) -> String {
        let next = if self.next_steps.is_empty() {
            "  (none)".to_string()
        } else {
            self.next_steps.iter()
                .enumerate()
                .map(|(i, s)| format!("  {}. {}", i + 1, s))
                .collect::<Vec<_>>()
                .join("\n")
        };
        format!(
            "[{}] Feature #{}: {}\n  Action: {}\n  Outcome: {}\n  Next steps:\n{}\n",
            self.timestamp,
            self.feature_index,
            self.feature_desc,
            self.action,
            self.outcome,
            next
        )
    }
}

fn current_timestamp() -> String {
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
    format!("{:04}-{:02}-{:02} {:02}:{:02}", year, month, day, hour, min)
}

/// Progress 日志管理器
pub struct ProgressLog {
    entries: Vec<ProgressEntry>,
}

impl ProgressLog {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn add_entry(&mut self, entry: ProgressEntry) {
        self.entries.push(entry);
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), String> {
        let content: String = self.entries.iter()
            .map(|e| e.to_file_format())
            .collect::<Vec<_>>()
            .join("\n──────────────────────────────\n\n");
        fs::write(path, content)
            .map_err(|e| format!("写入进度文件失败: {}", e))
    }

    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let content = fs::read_to_string(path)
            .map_err(|e| format!("无法读取进度文件: {}", e))?;
        // 简单解析：每个条目以 [timestamp] 开头
        let entries: Vec<ProgressEntry> = content
            .split("\n──────────────────────────────")
            .filter(|s| s.trim().starts_with('['))
            .filter_map(|block| {
                let lines: Vec<&str> = block.trim().split('\n').collect();
                if lines.len() < 5 { return None; }
                let ts_line = lines[0];
                let feat_line = lines.get(1)?;
                let action_line = lines.get(2)?;
                let outcome_line = lines.get(3)?;
                let ts = ts_line.trim().trim_start_matches('[').split(']').next().unwrap_or("").to_string();
                let feat_parts: Vec<&str> = feat_line.splitn(2, ": ").collect();
                let feat_idx: usize = feat_parts.get(1)?.split(':').next()?.trim().trim_start_matches('#').parse().ok()?;
                let feat_desc = feat_parts.get(1)?.split(": ").nth(1).unwrap_or("").to_string();
                let action = action_line.trim().trim_start_matches("  Action:").trim().to_string();
                let outcome = outcome_line.trim().trim_start_matches("  Outcome:").trim().to_string();
                Some(ProgressEntry {
                    timestamp: ts,
                    feature_index: feat_idx,
                    feature_desc: feat_desc,
                    action,
                    outcome,
                    next_steps: vec![],
                })
            })
            .collect();
        Ok(Self { entries })
    }

    pub fn last_entry(&self) -> Option<&ProgressEntry> {
        self.entries.last()
    }

    pub fn total_completed(&self) -> usize {
        self.entries.iter().filter(|e| e.outcome.contains("完成") || e.outcome.contains("pass") || e.outcome.contains("success")).count()
    }
}

impl Default for ProgressLog {
    fn default() -> Self {
        Self::new()
    }
}
