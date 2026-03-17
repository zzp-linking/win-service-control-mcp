use tokio::io::{stdin, stdout, AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde_json::{json, Value};

use crate::mcp_adapter::{McpHandler, ServiceToolArgs};
use crate::mcp_adapter_process::{ProcessMcpHandler, ProcessToolArgs};
use crate::service_handler::ServiceManager;
use crate::process_handler::ProcessManager;

pub async fn run_loop(manager: ServiceManager, proc_manager: ProcessManager) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdin = BufReader::new(stdin());
    let mut stdout = stdout();
    let mut line = String::new();

    loop {
        line.clear();
        let n = stdin.read_line(&mut line).await?;
        if n == 0 {
            break; // EOF，客户端断开
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            
            continue;
        }

        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let params = request.get("params").cloned();

        let response = handle_request(method, params, id, &manager, &proc_manager).await;

        if let Some(resp) = response {
            let mut resp_str = serde_json::to_string(&resp)?;
            resp_str.push('\n');
            stdout.write_all(resp_str.as_bytes()).await?;
            stdout.flush().await?;
        }
    }

    Ok(())
}

async fn handle_request(
    method: &str,
    params: Option<Value>,
    id: Value,
    manager: &ServiceManager,
    proc_manager: &ProcessManager,
) -> Option<Value> {
    match method {
        // Cursor 发来的初始化握手
        "initialize" => {
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "win-service-control",
                        "version": "0.1.0"
                    }
                }
            }))
        }

        // 初始化完成通知，无需回复
        "notifications/initialized" => None,

        // 工具列表查询
        "tools/list" => {
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [
                    {
                        "name": "manage_processes",
                        "description": "查询和终止 Windows 进程，支持按进程名或 PID 批量操作",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "action": {
                                    "type": "string",
                                    "enum": ["list", "kill"],
                                    "description": "操作类型: list 查询进程列表, kill 终止进程"
                                },
                                "processes": {
                                    "type": "string",
                                    "description": "进程名或 PID，支持逗号分隔，例如 \"notepad.exe, 1234\"。list 时作为关键字过滤可省略，kill 时必填"
                                }
                            },
                            "required": ["action"]
                        }
                    },
                    {
                        "name": "manage_services",
                        "description": "管理 Windows 服务，支持查询列表、启动和停止服务，可批量操作，支持修改启动类型",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "action": {
                                    "type": "string",
                                    "enum": ["list", "open", "stop"],
                                    "description": "操作类型: list 查询服务列表, open 启动服务, stop 停止服务"
                                },
                                "services": {
                                    "type": "string",
                                    "description": "服务名，支持逗号分隔，例如 \"Spooler, XLServicePlatform\"。list 操作时作为关键字过滤，可省略"
                                },
                                "only_running": {
                                    "type": "boolean",
                                    "description": "list 操作时：true 只显示运行中的服务（默认），false 显示全部服务"
                                },
                                "permanent": {
                                    "type": "boolean",
                                    "description": "open/stop 时是否同时修改启动类型（open 设为自动，stop 设为禁用）"
                                },
                                "manual": {
                                    "type": "boolean",
                                    "description": "open/stop 时是否将启动类型设为手动"
                                }
                            },
                            "required": ["action"]
                        }
                    }
                    ]
                }
            }))
        }

        // 工具调用
        "tools/call" => {
            let params = params.unwrap_or(Value::Null);
            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");

            match tool_name {
                "manage_processes" => {
                    let args_value = params.get("arguments").cloned().unwrap_or(Value::Null);
                    match serde_json::from_value::<ProcessToolArgs>(args_value) {
                        Ok(args) => {
                            let result_text = ProcessMcpHandler::handle_call(args, proc_manager);
                            Some(json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "content": [{ "type": "text", "text": result_text }]
                                }
                            }))
                        }
                        Err(e) => Some(error_response(id, -32602, &format!("参数解析失败: {}", e))),
                    }
                }
                "manage_services" => {
                    let args_value = params.get("arguments").cloned().unwrap_or(Value::Null);
                    match serde_json::from_value::<ServiceToolArgs>(args_value) {
                        Ok(args) => {
                            let result_text = McpHandler::handle_call(args, manager);
                            Some(json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "content": [{ "type": "text", "text": result_text }]
                                }
                            }))
                        }
                        Err(e) => Some(error_response(id, -32602, &format!("参数解析失败: {}", e))),
                    }
                }
                _ => Some(error_response(id, -32601, &format!("未知工具: {}", tool_name))),
            }
        }

        // 未知方法
        _ => {
            // 通知类消息（无 id）不需要回复
            if id == Value::Null {
                None
            } else {
                Some(error_response(id, -32601, &format!("未知方法: {}", method)))
            }
        }
    }
}

fn error_response(id: Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}
