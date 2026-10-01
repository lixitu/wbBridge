//! `wbbridge-core` 独立可执行入口 —— 编排逻辑在 `wbbridge_core::orchestration`。
//!
//! 与嵌入 Tauri 壳的形态共用同一份编排代码；本入口只负责 CLI 约定与「关停即退出进程」。
use std::process::ExitCode;

use wbbridge_core::orchestration::{run, StartOptions, HELP};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.first().map(String::as_str) {
        Some("--help" | "-h") => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
        Some("--version" | "-V") => {
            println!("{} {}", wbbridge_core::PACKAGE_NAME, wbbridge_core::VERSION);
            ExitCode::SUCCESS
        }
        Some(unknown) => {
            eprintln!("未知参数：{unknown}");
            eprintln!("{HELP}");
            ExitCode::from(2)
        }
        None => {
            let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("启动异步运行时失败：{error}");
                    return ExitCode::from(1);
                }
            };
            ExitCode::from(runtime.block_on(run(StartOptions {
                handle_signals: true,
                ..Default::default()
            })))
        }
    }
}
