use serde::{Deserialize, Serialize};
use crate::service_handler::ServiceManager;
use crate::logger;

fn expand_services(input: Vec<String>) -> Vec<String> {
    input.iter()
        .flat_map(|s| s.split(','))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 定义给 AI 看的参数结构
#[derive(Deserialize, Serialize)]
pub struct ServiceToolArgs {
    /// 操作: "list" 查询服务列表, "open" 启动服务, "stop" 停止服务
    pub action: String,
    /// 服务名，支持逗号分隔（list 操作时作为过滤关键字，可省略）
    pub services: Option<String>,
    /// 是否修改启动类型为 自动(open) 或 禁用(stop)，仅 open/stop 时有效
    pub permanent: Option<bool>,
    /// 是否设为手动模式，仅 open/stop 时有效
    pub manual: Option<bool>,
    /// list 操作时：true 只显示运行中的服务，false 显示全部，默认 true
    pub only_running: Option<bool>,
}

pub struct McpHandler;

impl McpHandler {
    /// 处理 AI 的工具调用请求
    pub fn handle_call(args: ServiceToolArgs, manager: &ServiceManager) -> String {
        match args.action.to_lowercase().as_str() {
            "list" => {
                let filter = args.services.filter(|s| !s.trim().is_empty());
                let only_running = args.only_running.unwrap_or(true);
                let query_type = if only_running { "MCP_LIST_RUNNING" } else { "MCP_LIST_ALL" };
                match manager.query_services(filter.clone(), only_running) {
                    Ok(output) => {
                        let filter_str = filter.as_deref().unwrap_or("ALL");
                        logger::log_action(&format!("Filter: {}", filter_str), query_type, "SUCCESS");
                        output
                    }
                    Err(e) => {
                        logger::log_action("list", query_type, &format!("FAILED: {}", e));
                        format!("❌ 查询失败: {}", e)
                    }
                }
            }
            "open" | "stop" => {
                let is_start = args.action.to_lowercase() == "open";
                let services_str = match args.services {
                    Some(ref s) if !s.trim().is_empty() => s.clone(),
                    _ => return "❌ open/stop 操作需要提供 services 参数".to_string(),
                };
                let svcs = expand_services(vec![services_str]);
                let permanent = args.permanent.unwrap_or(false);
                let manual = args.manual.unwrap_or(false);

                let action_label = if is_start { "MCP_OPEN" } else { "MCP_STOP" };
                let mode_suffix = if manual {
                    " (MANUAL)"
                } else if permanent {
                    if is_start { " (AUTO)" } else { " (DISABLED)" }
                } else {
                    ""
                };

                let mut output = String::new();
                output.push_str(&format!("🚀 MCP 正在执行批量 {} 操作...\n", if is_start { "开启" } else { "停止" }));

                for svc in svcs {
                    match manager.set_state(&svc, is_start, permanent, manual) {
                        Ok(full_name) => {
                            output.push_str(&format!("✅ 成功: {}\n", full_name));
                            logger::log_action(&full_name, &format!("{}{}", action_label, mode_suffix), "SUCCESS");
                        }
                        Err(e) => {
                            if e.contains("[PROTECTED]") {
                                output.push_str(&format!("🛡️  忽略: {}\n", e));
                                logger::log_action(&svc, action_label, "IGNORED (SYSTEM PROTECTED)");
                            } else {
                                output.push_str(&format!("❌ 失败: {}: {}\n", svc, e));
                                logger::log_action(&svc, action_label, &format!("FAILED: {}", e));
                            }
                        }
                    }
                }
                output
            }
            _ => format!("❌ 未知操作: {}，支持的操作: list / open / stop", args.action),
        }
    }
}
