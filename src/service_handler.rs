use windows::Win32::System::Services::*;
use windows::Win32::Security::SC_HANDLE; 
use windows::core::PCWSTR;

pub struct ServiceManager {
    pub sc_handle: SC_HANDLE, 
}

// 定义核心系统服务白名单（禁止停止或禁用）
struct ProtectedService {
    name: &'static str,
    desc: &'static str,
}

const PROTECTED_SERVICES: &[ProtectedService] = &[
    // --- 核心基础设施 (系统地基，动了立刻蓝屏或无法登录) ---
    ProtectedService { name: "RpcSs", desc: "远程过程调用 (RPC) - 系统最核心通信机制" },
    ProtectedService { name: "DcomLaunch", desc: "DCOM 服务进程启动器 - 桌面环境与组件启动核心" },
    ProtectedService { name: "RpcEptMapper", desc: "RPC 终结点映射器 - 协助不同程序间定位通信接口" },
    ProtectedService { name: "SamSs", desc: "安全帐户管理器 - 负责用户登录和账户权限验证" },
    ProtectedService { name: "LSM", desc: "本地会话管理器 - 管理用户登录会话和终端连接" },
    ProtectedService { name: "ProfSvc", desc: "用户配置文件服务 - 负责加载和管理用户桌面配置" },

    // --- 网络命脉 (动了无法上网) ---
    ProtectedService { name: "Dhcp", desc: "DHCP Client - 自动获取 IP 地址" },
    ProtectedService { name: "Dnscache", desc: "DNS Client - 解析网址域名 (如 www.baidu.com)" },
    ProtectedService { name: "nsi", desc: "网络存储接口服务 - 几乎所有网络连接的底层支撑" },
    ProtectedService { name: "WlanSvc", desc: "WLAN AutoConfig - 笔记本无线网络连接管理" },

    // --- 硬件与交互 (动了硬件失效或界面卡死) ---
    ProtectedService { name: "PlugPlay", desc: "即插即用 - 识别和安装 U 盘、鼠标等硬件设备" },
    ProtectedService { name: "Winmgmt", desc: "WMI - 系统的“大管家”，几乎所有管理工具和 UI 都靠它拿硬件数据" },
    ProtectedService { name: "DispBrokerDesktopSvc", desc: "显示策略服务 - 管理分辨率、多显示器布局" },
    ProtectedService { name: "Power", desc: "电源服务 - 处理关机、休眠和 CPU 电源管理" },

    // --- 安全与日志 (动了系统无法自我审计或防护) ---
    ProtectedService { name: "EventLog", desc: "Windows 事件日志 - 记录系统错误和运行状态" },
    ProtectedService { name: "mpssvc", desc: "Windows Defender 防火墙 - 阻止未经授权的网络访问" },
    ProtectedService { name: "WinDefend", desc: "Microsoft Defender 防护服务 - 系统自带杀毒核心" },
    ProtectedService { name: "SecurityHealthService", desc: "Windows 安全中心 - 监控系统整体安全健康" },
];

impl ServiceManager {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let handle = OpenSCManagerW(None, None, SC_MANAGER_ALL_ACCESS)
                .map_err(|e| format!("无法连接服务管理器: {}", e))?;
            Ok(Self { sc_handle: handle })
        }
    }

    pub fn list_services(&self, filter: Option<String>, only_running: bool) -> Result<(), String> {
        unsafe {
            let mut bytes_needed = 0;
            let mut services_returned = 0;
            let mut resume_handle = 0;

            let _ = EnumServicesStatusExW(
                self.sc_handle,
                SC_ENUM_PROCESS_INFO,
                ENUM_SERVICE_TYPE(0x30),
                SERVICE_STATE_ALL,
                None,
                &mut bytes_needed,
                &mut services_returned,
                Some(&mut resume_handle),
                None,
            );

            let mut buffer = vec![0u8; bytes_needed as usize];
            EnumServicesStatusExW(
                self.sc_handle,
                SC_ENUM_PROCESS_INFO,
                ENUM_SERVICE_TYPE(0x30),
                SERVICE_STATE_ALL,
                Some(&mut buffer),
                &mut bytes_needed,
                &mut services_returned,
                Some(&mut resume_handle),
                None,
            ).map_err(|e| format!("枚举服务失败: {}", e))?;

            let services = std::slice::from_raw_parts(
                buffer.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW,
                services_returned as usize,
            );

            // 再次调整表头，增加“启动类型”列
            println!("{:<30} | {:<40} | {:<10} | {:<8}", "服务名称", "显示名称", "状态", "启动类型");
            println!("{}", "-".repeat(105));

            let filter_key = filter.clone().map(|f| f.to_lowercase());
            let mut found_services = Vec::new();

            for s in services {

                // 核心逻辑：如果是 list 命令且服务不是运行中，直接跳过
                let is_running = s.ServiceStatusProcess.dwCurrentState == SERVICE_RUNNING;
                if only_running && !is_running {
                    continue;
                }

                let name = s.lpServiceName.to_string().unwrap_or_default();
                let display_name = s.lpDisplayName.to_string().unwrap_or_default();
                
                // 1. 基本过滤逻辑
                if let Some(ref key) = filter_key {
                    if !name.to_lowercase().contains(key) && !display_name.to_lowercase().contains(key) {
                        continue;
                    }
                }

                // 2. 查询启动类型 (dwStartType)
                let mut start_type_str = "未知";
                let name_wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
                
                // 打开服务查询配置，权限只需 SERVICE_QUERY_CONFIG
                if let Ok(s_handle) = OpenServiceW(self.sc_handle, PCWSTR(name_wide.as_ptr()), SERVICE_QUERY_CONFIG) {
                    let mut config_bytes_needed = 0;
                    let _ = QueryServiceConfigW(s_handle, None, 0, &mut config_bytes_needed);
                    
                    let mut config_buffer = vec![0u8; config_bytes_needed as usize];
                    if QueryServiceConfigW(
                        s_handle,
                        Some(config_buffer.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW),
                        config_bytes_needed,
                        &mut config_bytes_needed
                    ).is_ok() {
                        let config = &*(config_buffer.as_ptr() as *const QUERY_SERVICE_CONFIGW);
                        start_type_str = match config.dwStartType {
                            SERVICE_AUTO_START => "自动",
                            SERVICE_DEMAND_START => "手动",
                            SERVICE_DISABLED => "禁用",
                            SERVICE_BOOT_START | SERVICE_SYSTEM_START => "系统",
                            _ => "其他",
                        };
                    }
                }

                let state = if is_running { 
                    "运行中" 
                } else { 
                    "已停止" 
                };
                
                // 格式化输出：加入启动类型
                println!("{:<30} | {:<40} | {:<10} | {:<8}", name, display_name, state, start_type_str);
                
                found_services.push(format!("{} ({}/{})", name, state, start_type_str));
            }

            // 记录查询日志
            // 1. 使用 clone() 提取字符串内容，不影响原 filter 变量
            // 记录查询日志
            let filter_str = filter.clone().unwrap_or_else(|| "ALL".to_string());

            // 直接拼接所有发现的服务，不设上限
            let summary = if found_services.is_empty() {
                "No matches found".to_string()
            } else {
                format!("Found: {}", found_services.join(", "))
            };

            let query_type = if only_running { "LIST_RUNNING" } else { "LIST_ALL" };

            crate::logger::log_action(
                &format!("Filter: {}", filter_str), 
                query_type, 
                &summary  // 这里直接传入拼接好的完整字符串
            );
        }
        Ok(())
    }

    /// MCP 专用：查询服务列表，返回字符串而非直接打印
    pub fn query_services(&self, filter: Option<String>, only_running: bool) -> Result<String, String> {
        unsafe {
            let mut bytes_needed = 0;
            let mut services_returned = 0;
            let mut resume_handle = 0;

            let _ = EnumServicesStatusExW(
                self.sc_handle,
                SC_ENUM_PROCESS_INFO,
                ENUM_SERVICE_TYPE(0x30),
                SERVICE_STATE_ALL,
                None,
                &mut bytes_needed,
                &mut services_returned,
                Some(&mut resume_handle),
                None,
            );

            let mut buffer = vec![0u8; bytes_needed as usize];
            EnumServicesStatusExW(
                self.sc_handle,
                SC_ENUM_PROCESS_INFO,
                ENUM_SERVICE_TYPE(0x30),
                SERVICE_STATE_ALL,
                Some(&mut buffer),
                &mut bytes_needed,
                &mut services_returned,
                Some(&mut resume_handle),
                None,
            ).map_err(|e| format!("枚举服务失败: {}", e))?;

            let services = std::slice::from_raw_parts(
                buffer.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW,
                services_returned as usize,
            );

            let filter_key = filter.clone().map(|f| f.to_lowercase());
            let mut output = String::new();
            let header = format!("{:<30} | {:<40} | {:<10} | {:<8}\n", "服务名称", "显示名称", "状态", "启动类型");
            output.push_str(&header);
            output.push_str(&"-".repeat(105));
            output.push('\n');

            let mut count = 0usize;

            for s in services {
                let is_running = s.ServiceStatusProcess.dwCurrentState == SERVICE_RUNNING;
                if only_running && !is_running {
                    continue;
                }

                let name = s.lpServiceName.to_string().unwrap_or_default();
                let display_name = s.lpDisplayName.to_string().unwrap_or_default();

                if let Some(ref key) = filter_key {
                    if !name.to_lowercase().contains(key) && !display_name.to_lowercase().contains(key) {
                        continue;
                    }
                }

                let mut start_type_str = "未知";
                let name_wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
                if let Ok(s_handle) = OpenServiceW(self.sc_handle, PCWSTR(name_wide.as_ptr()), SERVICE_QUERY_CONFIG) {
                    let mut config_bytes_needed = 0;
                    let _ = QueryServiceConfigW(s_handle, None, 0, &mut config_bytes_needed);
                    let mut config_buffer = vec![0u8; config_bytes_needed as usize];
                    if QueryServiceConfigW(
                        s_handle,
                        Some(config_buffer.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW),
                        config_bytes_needed,
                        &mut config_bytes_needed,
                    ).is_ok() {
                        let config = &*(config_buffer.as_ptr() as *const QUERY_SERVICE_CONFIGW);
                        start_type_str = match config.dwStartType {
                            SERVICE_AUTO_START => "自动",
                            SERVICE_DEMAND_START => "手动",
                            SERVICE_DISABLED => "禁用",
                            SERVICE_BOOT_START | SERVICE_SYSTEM_START => "系统",
                            _ => "其他",
                        };
                    }
                }

                let state = if is_running { "运行中" } else { "已停止" };
                output.push_str(&format!("{:<30} | {:<40} | {:<10} | {:<8}\n", name, display_name, state, start_type_str));
                count += 1;
            }

            if count == 0 {
                output.push_str("（未找到匹配的服务）\n");
            } else {
                output.push_str(&format!("\n共 {} 个服务\n", count));
            }

            Ok(output)
        }
    }

    pub fn set_state(&self, name: &str, start: bool, permanent: bool, manual: bool) -> Result<String, String> {

        // --- 保险检查：如果是停止操作且在白名单中，直接拒绝 ---
        if !start {
            if let Some(protected) = PROTECTED_SERVICES.iter().find(|s| s.name.eq_ignore_ascii_case(name)) {
                return Err(format!(
                    "[PROTECTED] 拦截操作：{} ({}) 是系统核心组件，禁止停止。", 
                    protected.name, 
                    protected.desc
                ));
            }
        }

        unsafe {
            let name_wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            
            // --- 直接通过枚举获取显示名称 (参考 list_services 的逻辑) ---
            let mut display_name = name.to_string(); // 默认用 ID
            let mut bytes_needed = 0;
            let mut services_returned = 0;
            let mut resume_handle = 0;
    
            // 这里的逻辑和你 list 方法一模一样，只是增加了针对 name 的查询
            let _ = EnumServicesStatusExW(
                self.sc_handle, SC_ENUM_PROCESS_INFO, ENUM_SERVICE_TYPE(0x30),
                SERVICE_STATE_ALL, None, &mut bytes_needed, &mut services_returned,
                Some(&mut resume_handle), None,
            );
    
            let mut buffer = vec![0u8; bytes_needed as usize];
            if EnumServicesStatusExW(
                self.sc_handle, SC_ENUM_PROCESS_INFO, ENUM_SERVICE_TYPE(0x30),
                SERVICE_STATE_ALL, Some(&mut buffer), &mut bytes_needed, &mut services_returned,
                Some(&mut resume_handle), None,
            ).is_ok() {
                let services = std::slice::from_raw_parts(
                    buffer.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW,
                    services_returned as usize,
                );
                // 找到当前正在操作的这个服务，提取它的显示名称
                if let Some(s) = services.iter().find(|s| s.lpServiceName.to_string().unwrap_or_default() == name) {
                    display_name = s.lpDisplayName.to_string().unwrap_or_default();
                }
            }
            // -------------------------------------------------------
    
            let s_handle = OpenServiceW(
                self.sc_handle, 
                PCWSTR(name_wide.as_ptr()), 
                SERVICE_ALL_ACCESS 
            ).map_err(|_| format!("服务 '{}' 不存在", name))?;
    
            if permanent || manual {
                let start_type = if manual {
                    SERVICE_DEMAND_START
                } else {
                    if start { SERVICE_AUTO_START } else { SERVICE_DISABLED }
                };
                
                ChangeServiceConfigW(
                    s_handle, 
                    ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE), 
                    start_type, 
                    SERVICE_ERROR(SERVICE_NO_CHANGE), 
                    None, None, None, None, None, None, None
                ).map_err(|e| format!("配置修改失败: {}", e))?;
            }
    
            if start {
                StartServiceW(s_handle, None).map_err(|e| format!("启动失败: {}", e))?;
            } else {
                let mut status = Default::default();
                let _ = ControlService(s_handle, SERVICE_CONTROL_STOP, &mut status);
            }
    
            // 返回 "服务名称 (服务显示名称)"
            Ok(format!("{} ({})", name, display_name))
        }
    }
}