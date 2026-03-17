use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW,
    PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
use windows::Win32::Foundation::CloseHandle;

/// 进程信息
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub parent_pid: u32,
}

/// 不允许强制终止的系统核心进程
const PROTECTED_PROCESSES: &[&str] = &[
    "system", "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe",
    "services.exe", "lsass.exe", "svchost.exe", "dwm.exe",
];

pub struct ProcessManager;

impl ProcessManager {
    pub fn new() -> Self {
        Self
    }

    /// 枚举当前所有进程
    pub fn list_processes(&self, filter: Option<&str>) -> Result<Vec<ProcessInfo>, String> {
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
                .map_err(|e| format!("创建进程快照失败: {}", e))?;

            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };

            let mut processes = Vec::new();

            if Process32FirstW(snapshot, &mut entry).is_ok() {
                loop {
                    let name = String::from_utf16_lossy(
                        &entry.szExeFile[..entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len())]
                    );

                    let matches = match filter {
                        Some(f) => name.to_lowercase().contains(&f.to_lowercase()),
                        None => true,
                    };

                    if matches {
                        processes.push(ProcessInfo {
                            pid: entry.th32ProcessID,
                            name,
                            parent_pid: entry.th32ParentProcessID,
                        });
                    }

                    if Process32NextW(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }

            let _ = CloseHandle(snapshot);
            Ok(processes)
        }
    }

    /// 格式化进程列表为字符串（供 CLI 和 MCP 共用）
    pub fn format_list(&self, processes: &[ProcessInfo]) -> String {
        let mut output = String::new();
        output.push_str(&format!("{:<8} | {:<8} | {}\n", "PID", "父PID", "进程名"));
        output.push_str(&"-".repeat(50));
        output.push('\n');
        for p in processes {
            output.push_str(&format!("{:<8} | {:<8} | {}\n", p.pid, p.parent_pid, p.name));
        }
        output.push_str(&format!("\n共 {} 个进程\n", processes.len()));
        output
    }

    /// 终止指定进程（按名称或 PID）
    pub fn kill_processes(&self, targets: Vec<String>) -> Vec<(String, Result<(), String>)> {
        let mut results = Vec::new();

        // 先枚举所有进程，建立名称→PID 的映射
        let all = match self.list_processes(None) {
            Ok(p) => p,
            Err(e) => {
                results.push(("*".to_string(), Err(format!("枚举进程失败: {}", e))));
                return results;
            }
        };

        for target in targets {
            let target_trimmed = target.trim().to_string();

            // 判断是 PID（纯数字）还是进程名
            let matched_pids: Vec<(u32, String)> = if let Ok(pid) = target_trimmed.parse::<u32>() {
                all.iter()
                    .filter(|p| p.pid == pid)
                    .map(|p| (p.pid, p.name.clone()))
                    .collect()
            } else {
                // 按名称匹配（不区分大小写，支持不带 .exe 后缀）
                let target_lower = target_trimmed.to_lowercase();
                let target_with_exe = if target_lower.ends_with(".exe") {
                    target_lower.clone()
                } else {
                    format!("{}.exe", target_lower)
                };
                all.iter()
                    .filter(|p| {
                        let name_lower = p.name.to_lowercase();
                        name_lower == target_lower || name_lower == target_with_exe
                    })
                    .map(|p| (p.pid, p.name.clone()))
                    .collect()
            };

            if matched_pids.is_empty() {
                results.push((target_trimmed, Err("未找到匹配的进程".to_string())));
                continue;
            }

            for (pid, name) in matched_pids {
                // 检查保护名单
                if PROTECTED_PROCESSES.iter().any(|&p| name.to_lowercase() == p) {
                    results.push((
                        format!("{} (PID:{})", name, pid),
                        Err("[PROTECTED] 系统核心进程，禁止终止".to_string()),
                    ));
                    continue;
                }

                let result = unsafe {
                    match OpenProcess(PROCESS_TERMINATE, false, pid) {
                        Ok(handle) => {
                            let r = TerminateProcess(handle, 1)
                                .map_err(|e| format!("终止失败: {}", e));
                            let _ = CloseHandle(handle);
                            r.map(|_| ())
                        }
                        Err(e) => Err(format!("无法打开进程: {}", e)),
                    }
                };
                results.push((format!("{} (PID:{})", name, pid), result));
            }
        }

        results
    }
}
