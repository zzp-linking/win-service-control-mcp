use serde::{Deserialize, Serialize};
use crate::process_handler::ProcessManager;
use crate::logger;

#[derive(Deserialize, Serialize)]
pub struct ProcessToolArgs {
    /// 操作: "list" 查询进程列表, "kill" 终止进程
    pub action: String,
    /// 进程名或 PID，支持逗号分隔。list 时作为过滤关键字（可省略），kill 时为必填
    pub processes: Option<String>,
}

pub struct ProcessMcpHandler;

impl ProcessMcpHandler {
    pub fn handle_call(args: ProcessToolArgs, manager: &ProcessManager) -> String {
        match args.action.to_lowercase().as_str() {
            "list" => {
                let filter = args.processes.as_deref().filter(|s| !s.trim().is_empty());
                match manager.list_processes(filter) {
                    Ok(list) => {
                        let filter_str = filter.unwrap_or("ALL");
                        logger::log_action(
                            &format!("Filter: {}", filter_str),
                            "MCP_PS_LIST",
                            &format!("Found {} processes", list.len()),
                        );
                        manager.format_list(&list)
                    }
                    Err(e) => {
                        logger::log_action("list", "MCP_PS_LIST", &format!("FAILED: {}", e));
                        format!("❌ 查询失败: {}", e)
                    }
                }
            }
            "kill" => {
                let targets_str = match args.processes.as_ref().filter(|s| !s.trim().is_empty()) {
                    Some(s) => s.clone(),
                    None => return "❌ kill 操作需要提供 processes 参数（进程名或 PID）".to_string(),
                };

                let targets: Vec<String> = targets_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();

                let results = manager.kill_processes(targets);
                let mut output = String::new();
                output.push_str("🚀 MCP 正在执行批量终止进程操作...\n");

                for (target, result) in &results {
                    match result {
                        Ok(_) => {
                            output.push_str(&format!("✅ 成功终止: {}\n", target));
                            logger::log_action(target, "MCP_PS_KILL", "SUCCESS");
                        }
                        Err(e) if e.contains("[PROTECTED]") => {
                            output.push_str(&format!("🛡️  忽略: {} — {}\n", target, e));
                            logger::log_action(target, "MCP_PS_KILL", "IGNORED (PROTECTED)");
                        }
                        Err(e) => {
                            output.push_str(&format!("❌ 失败: {} — {}\n", target, e));
                            logger::log_action(target, "MCP_PS_KILL", &format!("FAILED: {}", e));
                        }
                    }
                }
                output
            }
            _ => format!("❌ 未知操作: {}，支持的操作: list / kill", args.action),
        }
    }
}
