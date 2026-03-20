//! Rust Agent — 学习项目
//! 
//! 本项目从零实现一个简化但完整的 Agent，理解以下核心概念：
//! 
//! 1. **Agent Loop（Agent 循环）** — Agent 是如何"思考→行动→观察→再思考"的
//! 2. **Tool Calling（工具调用）** — Agent 如何调用外部工具
//! 3. **LLM Integration** — 如何与语言模型交互
//! 4. **Memory（记忆）** — 如何保存对话历史
//! 5. **Streaming（流式输出）** — 如何实时看到模型的思考过程
//! 
//! 运行方法:
//!   cp .env.example .env  # 填入 API key
//!   cargo run

mod agent;
mod llm;
mod tools;
mod memory;

use std::io::{self, Write as IoWrite};
use colored::Colorize;
use crate::agent::Agent;
use crate::tools::ToolRegistry;
use crate::memory::ConversationMemory;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "\n🤖 Rust Agent 学习项目\n".cyan());
    println!("输入你的问题，按 Enter 发送，输入 'quit' 退出。\n");

    // 初始化各组件
    dotenvy::dotenv().ok(); // 从 .env 加载 API key

    let api_key = std::env::var("OPENAI_API_KEY")
        .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
        .expect("请设置 OPENAI_API_KEY 或 ANTHROPIC_API_KEY 环境变量");

    let model = std::env::var("MODEL").unwrap_or_else(|_| "gpt-4o".to_string());

    let tools = ToolRegistry::new();
    let memory = ConversationMemory::new();
    let mut agent = Agent::new(api_key, model, tools, memory);

    // 交互式循环
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

        // 运行 Agent（核心循环）
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
