use std::{fs, io::Write};
use chrono::Local;

pub fn log_action(service: &str, action: &str, status: &str) {
    let now = Local::now().format("%Y-%m-%d %H:%M:%S");
    let log_msg = format!("[{}] [{}] {} | {}\n", now, action, service, status);
    
    // 获取 exe 同级目录
    let mut log_path = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
    log_path.pop(); 
    log_path.push("service_history.log");

    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path) 
    {
        let _ = file.write_all(log_msg.as_bytes());
    }
}