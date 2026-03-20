//! Rust Agent — 学习项目
//!
//! 本项目从零实现一个简化但完整的 Agent，理解以下核心概念：
//!
//! 1. **Agent Loop（Agent 循环）** — Agent 是如何"思考→行动→观察→再思考"的
//! 2. **Tool Calling（工具调用）** — Agent 如何调用外部工具
//! 3. **LLM Integration** — 如何与语言模型交互
//! 4. **Memory（记忆）** — 如何保存对话历史
//! 5. **Dual-Agent 架构** — Initializer + Coding Agent（Anthropic 长运行方案）

mod agent;
mod llm;
mod tools;
mod memory;
mod project;
mod initializer;
mod coding_agent;

use std::io::{self, Write as IoWrite};
use colored::Colorize;
use std::path::PathBuf;
use crate::agent::Agent;
use crate::initializer::InitializerAgent;
use crate::coding_agent::CodingAgent;
use crate::tools::ToolRegistry;
use crate::memory::ConversationMemory;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        run_dual_agent_mode(&args).await?;
    } else {
        run_interactive_mode().await?;
    }

    Ok(())
}

// ─────────────────────────────────────────────
// 交互式对话模式（单 Agent）
// ─────────────────────────────────────────────

async fn run_interactive_mode() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "\n🤖 Rust Agent 学习项目\n".cyan());
    println!("输入你的问题，按 Enter 发送，输入 'quit' 退出。\n");

    let api_key = std::env::var("OPENAI_API_KEY")
        .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
        .expect("请设置 OPENAI_API_KEY 或 ANTHROPIC_API_KEY 环境变量");

    let model = std::env::var("MODEL").unwrap_or_else(|_| "gpt-4o".to_string());

    let tools = ToolRegistry::new();
    let memory = ConversationMemory::new();
    let mut agent = Agent::new(api_key, model, tools, memory);

    loop {
        print!("{}", "\n👤 你: ".blue());
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() || input == "quit" || input == "exit" {
            println!("再见！👋");
            break;
        }

        if input == "clear" {
            agent.clear_memory();
            println!("{}", "🧹 记忆已清除".yellow());
            continue;
        }

        match agent.run(input).await {
            Ok(response) => {
                println!("\n{} {}\n", "🤖 Agent:".green(), response);
            }
            Err(e) => {
                eprintln!("{} {}", "❌ 错误:".red(), e);
            }
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────
// 双 Agent 模式（Anthropic 长运行架构）
// ─────────────────────────────────────────────

async fn run_dual_agent_mode(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("OPENAI_API_KEY")
        .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
        .expect("请设置 OPENAI_API_KEY 或 ANTHROPIC_API_KEY");

    let model = std::env::var("MODEL").unwrap_or_else(|_| "gpt-4o".to_string());

    let subcommand = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match subcommand {
        "init" => {
            let mut project_path: Option<PathBuf> = None;
            let mut name: Option<String> = None;
            let mut prompt: Option<String> = None;

            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--project" if i + 1 < args.len() => {
                        project_path = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    }
                    "--name" if i + 1 < args.len() => {
                        name = Some(args[i + 1].clone());
                        i += 2;
                    }
                    "--prompt" => {
                        prompt = Some(args[i + 1..].join(" "));
                        break;
                    }
                    _ => i += 1,
                }
            }

            let project_path = project_path.ok_or("缺少 --project 参数")?;
            let name = name.ok_or("缺少 --name 参数")?;
            let prompt = prompt.ok_or("缺少 --prompt 参数")?;

            println!("{}", "\n🚀 双 Agent 模式: Initializer".cyan());
            println!("  项目: {} @ {:?}", name, project_path);

            let mut init_agent = InitializerAgent::new(
                api_key, model,
                ToolRegistry::new(),
                &project_path.to_string_lossy(),
                &name,
                &prompt,
            );

            match init_agent.run().await {
                Ok(report) => {
                    println!("\n{}", format!("✅ {}", report).green());
                }
                Err(e) => {
                    eprintln!("{} {}", "❌ 初始化失败:".red(), e);
                    std::process::exit(1);
                }
            }
        }

        "continue" => {
            let mut project_path: Option<PathBuf> = None;
            let mut max = usize::MAX;

            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--project" if i + 1 < args.len() => {
                        project_path = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    }
                    "--max" if i + 1 < args.len() => {
                        max = args[i + 1].parse().unwrap_or(usize::MAX);
                        i += 2;
                    }
                    _ => i += 1,
                }
            }

            let project_path = project_path.ok_or("缺少 --project 参数")?;
            let project_str = project_path.to_string_lossy().to_string();

            println!("{}", "\n🔧 双 Agent 模式: Coding Agent".cyan());
            println!("  项目: {:?}", project_path);

            let mut completed = 0;
            loop {
                if completed >= max {
                    println!("\n达到最大 feature 数量限制（{}），停止。", max);
                    break;
                }

                let mut coding_agent = CodingAgent::new(
                    api_key.clone(),
                    model.clone(),
                    ToolRegistry::new(),
                    &project_str,
                );

                match coding_agent.run().await {
                    Ok(report) => {
                        println!("\n{}", format!("✅ {}", report).green());
                        if report.contains("所有功能已完成") || report.contains("完成") && completed > 0 {
                            break;
                        }
                        completed += 1;
                    }
                    Err(e) => {
                        eprintln!("{} {}", "❌ Coding Agent 错误:".red(), e);
                        break;
                    }
                }
            }

            if completed > 0 {
                println!("\n📊 本轮完成 {} 个 features。", completed);
                println!("💡 再次运行 `cargo run -- continue --project {:?}` 继续。", project_path);
            }
        }

        _ => {
            print_usage();
        }
    }

    Ok(())
}

fn print_usage() {
    println!(r#"
🤖 Rust Agent — 双 Agent 架构（Anthropic 长运行方案）

用法:
  cargo run                                    # 交互式对话模式
  cargo run -- init --project <path> --name <name> --prompt <需求描述>
                                                   # Initializer Agent: 初始化项目
  cargo run -- continue --project <path>        # Coding Agent: 实现下一个 feature
  cargo run -- continue --project <path> --max 5  # Coding Agent: 最多做 5 个 feature

示例:
  # 初始化一个新项目
  cargo run -- init \
    --project ./my-webapp \
    --name "Todo App" \
    --prompt "一个简单的 Todo 应用，支持添加、完成、删除任务，数据存储在本地文件"

  # 继续实现下一个 feature
  cargo run -- continue --project ./my-webapp
"#);
}
