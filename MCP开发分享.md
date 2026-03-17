# MCP 技术文档

> 以 win-service-control-mcp 项目为例，介绍 MCP 协议原理、交互流程及快速搭建方法。

---

## 目录

1. [MCP 是什么](#1-mcp-是什么)
2. [MCP 交互流程（以本项目为例）](#2-mcp-交互流程以本项目为例)
3. [如何快速搭建自己的 MCP 服务](#3-如何快速搭建自己的-mcp-服务)
4. [用 Rust 开发 MCP 工具的优势](#4-用-rust-开发-mcp-工具的优势)

---

## 1. MCP 是什么

MCP（Model Context Protocol，模型上下文协议）是 Anthropic 提出的一种开放协议，用于标准化 AI 模型与外部工具/数据源之间的通信方式。

通俗来说，MCP 解决的问题是：**让 AI 能够调用你自己写的程序来完成任务**。

### 1.1 为什么需要 MCP

在没有 MCP 之前，如果想让 AI 操作本地文件、数据库或系统功能，需要为每个 AI 平台单独开发插件，格式各不相同。MCP 的出现统一了这套规范，任何支持 MCP 的 AI 客户端（如 Cursor、Claude Desktop、OpenClaw）都可以直接使用符合规范的 MCP Server。

### 1.2 核心概念

| 概念 | 说明 |
|---|---|
| **MCP Client** | AI 所在的客户端程序，如 Cursor、Claude Desktop、OpenClaw |
| **MCP Server** | 你开发的工具服务程序，对外暴露可供 AI 调用的工具 |
| **Tool（工具）** | MCP Server 对外声明的一个可调用功能，附带参数 Schema |
| **stdio 传输** | 通过标准输入/输出（stdin/stdout）通信，最简单的传输方式 |

### 1.3 技术本质

MCP 的底层实现非常简单，本质就是：

```
JSON-RPC 2.0  over  stdio（标准输入输出）
```

客户端和服务端之间通过 stdin/stdout 互发 **单行 JSON** 消息，每条消息一行，用换行符 `\n` 分隔。没有任何复杂的框架，只需要能读写 JSON 即可实现。

---

## 2. MCP 交互流程（以本项目为例）

本项目（win-service-control-mcp）是一个 Windows 服务和进程管理工具，通过 MCP 协议让 Cursor 的 AI 能够直接管理系统服务和进程。

### 2.1 程序启动

Cursor 在 `mcp.json` 中配置好路径后，每次需要使用工具时会自动启动 MCP Server 进程：

```json
{
  "mcpServers": {
    "win-service-control": {
      "command": "D:\\path\\to\\wsm.exe",
      "args": ["mcp"]
    }
  }
}
```

`wsm.exe mcp` 启动后，`main()` 进入 `Commands::Mcp` 分支，调用 `run_loop()`，程序阻塞在 stdin 等待输入，直到 Cursor 关闭连接。

### 2.2 完整交互时序

```
Cursor (MCP Client)                    wsm.exe (MCP Server)
       |                                       |
       |  ① 启动进程，建立 stdin/stdout 管道    |
       |-------------------------------------> |  进入 run_loop() 循环
       |                                       |
       |  ② initialize 请求（握手）            |
       |  {"jsonrpc":"2.0",                    |
       |   "method":"initialize",              |
       |   "params":{                          |
       |     "protocolVersion":"2024-11-05",   |
       |     "clientInfo":{"name":"Cursor"},   |
       |     "capabilities":{}                 |
       |   }, "id":1}                          |
       |-------------------------------------> |
       |                                       |
       |  ③ initialize 响应（声明服务器能力）   |
       |  {"jsonrpc":"2.0","id":1,             |
       |   "result":{                          |
       |     "protocolVersion":"2024-11-05",   |
       |     "capabilities":{"tools":{}},      |
       |     "serverInfo":{                    |
       |       "name":"win-service-control",   |
       |       "version":"0.1.0"}              |
       |   }}                                  |
       | <------------------------------------|
       |                                       |
       |  ④ initialized 通知（无需回复）        |
       |  {"jsonrpc":"2.0",                    |
       |   "method":"notifications/initialized"|
       |   }                                   |
       |-------------------------------------> |  直接忽略，不回复
       |                                       |
       |  ⑤ tools/list 请求（获取工具列表）    |
       |  {"jsonrpc":"2.0",                    |
       |   "method":"tools/list","id":2}       |
       |-------------------------------------> |
       |                                       |
       |  ⑥ tools/list 响应（返回工具定义）    |
       |  {"jsonrpc":"2.0","id":2,             |
       |   "result":{"tools":[                 |
       |     {"name":"manage_services",...},   |
       |     {"name":"manage_processes",...}   |
       |   ]}}                                 |
       | <------------------------------------|
       |                                       |
       |        === 进入正常工作状态 ===         |
       |                                       |
       |  ⑦ tools/call 请求（AI 决定调用工具） |
       |  {"jsonrpc":"2.0",                    |
       |   "method":"tools/call",              |
       |   "params":{                          |
       |     "name":"manage_services",         |
       |     "arguments":{                     |
       |       "action":"list",                |
       |       "only_running":true             |
       |     }                                 |
       |   }, "id":3}                          |
       |-------------------------------------> |
       |                                       |
       |  ⑧ tools/call 响应（返回执行结果）    |
       |  {"jsonrpc":"2.0","id":3,             |
       |   "result":{                          |
       |     "content":[{                      |
       |       "type":"text",                  |
       |       "text":"服务名称 | 显示名称..."  |
       |     }]                                |
       |   }}                                  |
       | <------------------------------------|
       |                                       |
       |  （后续工具调用重复 ⑦⑧ 步骤）         |
```

### 2.3 消息类型说明

MCP 消息分三种类型，判断规则简单：

| 类型 | 特征 | 是否需要回复 |
|---|---|---|
| **Request（请求）** | 有 `id` 字段 + 有 `method` 字段 | 必须回复，带相同 `id` |
| **Notification（通知）** | 无 `id` 字段 + 有 `method` 字段 | 绝对不能回复 |
| **Response（响应）** | 有 `id` 字段 + 有 `result`/`error` 字段 | Server 通常不会收到，若收到说明客户端有 bug，应忽略或返回错误 |

本项目的判断逻辑（`mcp_server.rs`）：

```rust
// 读取消息类型标识
let id = request.get("id").cloned().unwrap_or(Value::Null);
let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");

// 未知方法且无 id → 通知，直接忽略
if id == Value::Null {
    return None; // 返回 None 表示不需要发送响应
}
```

### 2.4 工具 Schema 的作用

`tools/list` 返回的每个工具都包含 `inputSchema`，这是 AI 理解如何调用工具的关键：

```json
{
  "name": "manage_services",
  "description": "管理 Windows 服务，支持查询列表、启动和停止服务",
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
        "description": "服务名，支持逗号分隔"
      }
    },
    "required": ["action"]
  }
}
```

- `name`：工具的唯一标识符，AI 选定工具后在 `tools/call` 请求中用它指定调用哪个工具，Server 也靠它做路由分发。必须唯一且在版本内保持稳定，是机器用的字段
- `description`：AI 根据这个决定什么时候调用这个工具，写得越清晰 AI 调用越准确，是给 AI 读的字段
- `properties`：每个参数的类型、枚举值、说明
- `required`：必填参数列表

**description 写得越清晰，AI 调用越准确**，这是 MCP Server 开发中最重要的细节。

### 2.5 本项目架构分层

```
main.rs                  ← CLI 入口，命令行解析，启动 MCP 模式
  │
  ├── mcp_server.rs      ← MCP 传输层：负责 JSON 收发和方法路由
  │                         不包含任何业务逻辑
  │
  ├── mcp_adapter.rs     ← 服务管理工具的参数解析和业务适配
  ├── mcp_adapter_process.rs  ← 进程管理工具的参数解析和业务适配
  │
  ├── service_handler.rs ← 服务管理核心逻辑（Windows SCM API）
  ├── process_handler.rs ← 进程管理核心逻辑（Windows TlHelp32 API）
  │
  └── logger.rs          ← 操作日志记录
```

这种分层的好处是：`mcp_server.rs` 只负责协议，不关心业务；业务代码同时服务于 CLI 和 MCP 两种调用方式，不重复。

---

## 3. 如何快速搭建自己的 MCP 服务

### 3.1 核心原则

搭建 MCP Server 不需要任何专用框架，只需要：

1. 能读写标准输入输出（stdin/stdout）
2. 能解析和生成 JSON
3. 实现 4 个必要的方法处理器

任何语言都可以（Rust、Python、Node.js、Go 等），以下以 Rust 为例。

### 3.2 最小可用模板（Rust）

**Cargo.toml：**

```toml
[package]
name = "my-mcp-server"
version = "0.1.0"
edition = "2021"

[dependencies]
serde_json = "1.0"
```

**src/main.rs：**

```rust
use std::io::{self, BufRead, Write};
use serde_json::{json, Value};

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    // stdin.lock().lines() 是一个阻塞迭代器，每次 .next() 都会阻塞等待 stdin 出现一行新内容，没有输入时线程处于挂起状态，CPU 占用为 0%
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }

        // 解析 JSON
        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let params = request.get("params").cloned();

        // 路由处理
        let response = handle(method, params, id);

        // 发送响应（通知类无需回复）
        if let Some(resp) = response {
            // 获取stdout独占锁，这是标准库惯用方式，如果是单线程没必要，
            let mut out = stdout.lock();
            writeln!(out, "{}", serde_json::to_string(&resp).unwrap()).unwrap();
        }
    }
}

fn handle(method: &str, params: Option<Value>, id: Value) -> Option<Value> {
    match method {
        // 1. 握手
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "my-server", "version": "0.1.0" }
            }
        })),

        // 2. 初始化通知，忽略
        "notifications/initialized" => None,

        // 3. 返回工具列表
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": [{
                    "name": "my_tool",
                    "description": "工具描述，告诉 AI 这个工具能做什么",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "input": {
                                "type": "string",
                                "description": "参数描述"
                            }
                        },
                        "required": ["input"]
                    }
                }]
            }
        })),

        // 4. 执行工具调用
        "tools/call" => {
            let params = params.unwrap_or(Value::Null);
            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);

            let result = match tool_name {
                "my_tool" => {
                    let input = args.get("input").and_then(|v| v.as_str()).unwrap_or("");
                    format!("收到输入: {}", input) // 替换为你的业务逻辑
                }
                _ => format!("未知工具: {}", tool_name),
            };

            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{ "type": "text", "text": result }]
                }
            }))
        }

        // 未知方法
        _ => if id == Value::Null { None } else {
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": "Method not found" }
            }))
        }
    }
}
```

### 3.3 在 Cursor 中配置

编译后，在 `~/.cursor/mcp.json` 中添加：

```json
{
  "mcpServers": {
    "my-server": {
      "command": "/path/to/my-mcp-server",
      "args": []
    }
  }
}
```

###  在 openclaw 中配置
全局安装 `mcporter`，然后然后把上面那个配置发给ai让它通过 `mcporter` 自己调用

```bash
pnpm install mcporter -g
```


### 3.4 开发注意事项

**① 响应必须单行**

```rust
// ✅ 正确：单行紧凑 JSON
serde_json::to_string(&resp).unwrap()

// ❌ 错误：多行格式化 JSON，客户端只读第一行会解析失败
serde_json::to_string_pretty(&resp).unwrap()
```

**② 通知不能回复**

```rust
// notifications/initialized、exit 等没有 id 的消息
// 必须返回 None，不能发送任何响应
"notifications/initialized" => None,
```

**③ description 决定 AI 的调用准确性**

工具和参数的 `description` 字段直接影响 AI 是否能正确理解和调用工具。写得越具体越好，包括：
- 工具的使用场景
- 参数的格式要求（如"支持逗号分隔"）
- 可选值的含义（`enum` 每个值的说明）

**④ 权限问题**

如果工具需要管理员权限（如本项目的 Windows 服务操作），需要以管理员身份启动 Cursor，否则工具调用会返回权限错误。

**⑤ stderr 用于调试**

MCP 使用 stdout 传输协议数据，调试日志请输出到 stderr，避免污染协议通道：

```rust
eprintln!("[DEBUG] 收到方法: {}", method); // 输出到 stderr，不影响协议
```

### 3.5 常见错误排查

| 现象 | 原因 | 解决方案 |
|---|---|---|
| Cursor 显示红色错误 | 进程启动失败或立即崩溃 | 检查 exe 路径是否正确，手动运行确认没有崩溃 |
| 连接黄色 loading 超时 | initialize 握手无响应 | 确认 `initialize` 方法有正确回复，检查响应格式 |
| 工具不出现在列表 | `tools/list` 响应格式错误 | 检查 JSON 结构，特别是 `tools` 数组和 `inputSchema` |
| 调用工具无响应 | `tools/call` 路由缺失或崩溃 | 检查工具名匹配是否正确，业务代码是否 panic |
| 参数解析失败 | `arguments` 字段结构不匹配 | 检查 Schema 定义与实际解析结构是否一致 |

---

## 4. 用 Rust 开发 MCP 工具的优势

MCP Server 可以用任何语言实现，但 Rust 在某些场景下具有明显优势，本项目即是一个典型案例。

### 4.1 单一可执行文件，无运行时依赖

Rust 编译产物是独立的原生可执行文件（`.exe`），不依赖任何运行时环境。

| 语言 | 部署要求 |
|---|---|
| Python | 目标机器必须安装 Python + 依赖包 |
| Node.js | 目标机器必须安装 Node.js + npm 依赖 |
| Java | 目标机器必须安装 JRE |
| **Rust** | **只需一个 exe 文件，双击或命令行直接运行** |

对于 MCP Server 这种需要分发和部署的场景，Rust 的单文件分发极为方便——复制一个 exe 到任意 Windows 机器即可使用，无需安装任何环境。

### 4.2 直接调用系统底层 API

本项目需要直接操作 Windows 服务管理器（SCM）和进程快照 API，这类系统级功能在 Rust 中通过 `windows` crate 可以直接调用原生 Win32 API，无任何性能损耗，也无需借助中间层。

```rust
// 直接调用 Windows API，和 C/C++ 同等能力
let handle = OpenSCManagerW(None, None, SC_MANAGER_ALL_ACCESS)?;
let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
```

相比之下，Python 或 Node.js 调用系统 API 需要通过 FFI 或第三方封装库，存在额外的调用开销和兼容性风险。

### 4.3 启动速度极快

MCP Server 每次被 Cursor 调用时都需要快速完成 initialize 握手。Rust 程序没有虚拟机启动、JIT 编译或垃圾回收等开销，启动时间通常在几毫秒以内，握手响应几乎是即时的。

### 4.4 内存占用极低

本项目在 MCP 模式下，程序阻塞等待 stdin 输入时：

- 内存占用约 **2-4 MB**
- CPU 占用 **0%**

相比 Node.js（基础内存约 30-50 MB）或 Python（基础内存约 20-40 MB）的 MCP Server，Rust 的资源占用几乎可以忽略不计。对于需要长期驻留后台的 MCP Server 来说，这一点尤为重要。

### 4.5 编译期类型安全，减少运行时错误

MCP 调用是运行时的网络交互，参数解析出错会导致工具调用失败。Rust 的强类型系统配合 `serde` 可以在编译期保证参数结构的正确性：

```rust
// 参数结构体，serde 自动处理反序列化和类型校验
#[derive(Deserialize)]
pub struct ServiceToolArgs {
    pub action: String,
    pub services: Option<String>,
    pub permanent: Option<bool>,
}

// 解析失败会返回明确的错误信息，而不是运行时崩溃
let args: ServiceToolArgs = serde_json::from_value(args_value)
    .map_err(|e| format!("参数解析失败: {}", e))?;
```

### 4.6 适用场景总结

Rust 特别适合以下类型的 MCP Server 开发：

- **系统管理类工具**：需要调用 OS API（文件系统、进程、注册表、网络接口等）
- **需要分发给他人使用**：单文件部署，无环境依赖
- **性能敏感型工具**：大量数据处理、高频调用
- **长期驻留的后台服务**：对内存和 CPU 占用有严格要求

如果是快速原型验证或脚本类工具，Python 或 Node.js 的开发效率可能更高；如果对性能、部署便利性和系统级能力有要求，Rust 是更好的选择。

