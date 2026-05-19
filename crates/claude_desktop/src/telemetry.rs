//! Claude Desktop 代理统计与日志缓冲。
//!
//! 与 `codex_app_transfer_proxy::telemetry` 对称(Codex CLI 代理那一份),但作为
//! claude_desktop crate 内的独立 singleton,日志文件落 `~/.codex-app-transfer/logs/
//! claude-desktop-proxy-YYYY-MM-DD.log`,跟 Codex CLI 代理的 `proxy-…log` 分开。
//!
//! 选择"copy + adapt"而不是"在 codex_app_transfer_proxy 里加 prefix 参数"的原因:
//! claude_desktop crate 不依赖 codex_app_transfer_proxy(架构上两个客户端通路彼此
//! 独立),不想为了一个 telemetry singleton 引入跨 crate 运行期耦合。重复代码约
//! 200 行,可接受。

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

use chrono::{DateTime, Local};
use codex_app_transfer_registry::config_dir;
use serde::Serialize;

const LOG_FILE_PREFIX: &str = "claude-desktop-proxy";

#[derive(Debug, Clone, Serialize)]
pub struct ProxyStatsSnapshot {
    pub total: u64,
    pub success: u64,
    pub failed: u64,
    pub today: u64,
}

#[derive(Debug)]
struct ProxyStatsState {
    total: u64,
    success: u64,
    failed: u64,
    today: u64,
    date: String,
}

impl Default for ProxyStatsState {
    fn default() -> Self {
        Self {
            total: 0,
            success: 0,
            failed: 0,
            today: 0,
            date: Local::now().format("%Y-%m-%d").to_string(),
        }
    }
}

#[derive(Debug, Default)]
pub struct ProxyStats {
    inner: Mutex<ProxyStatsState>,
}

impl ProxyStats {
    pub fn record(&self, success: bool) {
        let today = Local::now().format("%Y-%m-%d").to_string();
        let mut inner = self.inner.lock().unwrap();
        inner.total += 1;
        if inner.date != today {
            inner.today = 0;
            inner.date = today;
        }
        inner.today += 1;
        if success {
            inner.success += 1;
        } else {
            inner.failed += 1;
        }
    }

    pub fn snapshot(&self) -> ProxyStatsSnapshot {
        let inner = self.inner.lock().unwrap();
        ProxyStatsSnapshot {
            total: inner.total,
            success: inner.success,
            failed: inner.failed,
            today: inner.today,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProxyLogEntry {
    pub time: String,
    pub level: String,
    pub message: String,
}

#[derive(Debug)]
pub struct LogBuffer {
    logs: Mutex<Vec<ProxyLogEntry>>,
    max_size: usize,
    file_lock: Mutex<()>,
    log_dir_override: Option<PathBuf>,
}

impl LogBuffer {
    pub fn new(max_size: usize) -> Self {
        Self {
            logs: Mutex::new(Vec::new()),
            max_size,
            file_lock: Mutex::new(()),
            log_dir_override: None,
        }
    }

    #[cfg(test)]
    fn new_in_dir(max_size: usize, log_dir: PathBuf) -> Self {
        Self {
            logs: Mutex::new(Vec::new()),
            max_size,
            file_lock: Mutex::new(()),
            log_dir_override: Some(log_dir),
        }
    }

    pub fn add(&self, level: impl Into<String>, message: impl Into<String>) {
        let now = Local::now();
        let level = level.into();
        let message = message.into();
        {
            let mut logs = self.logs.lock().unwrap();
            logs.push(ProxyLogEntry {
                time: now.format("%H:%M:%S").to_string(),
                level: level.clone(),
                message: message.clone(),
            });
            if logs.len() > self.max_size {
                let keep_from = logs.len() - self.max_size;
                logs.drain(0..keep_from);
            }
        }
        self.append_to_file(now, &level, &message);
    }

    pub fn get_all(&self) -> Vec<ProxyLogEntry> {
        self.logs.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.logs.lock().unwrap().clear();
        self.archive_logs();
    }

    fn append_to_file(&self, now: DateTime<Local>, level: &str, message: &str) {
        let Some(dir) = self.log_dir() else {
            return;
        };
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = dir.join(format!(
            "{LOG_FILE_PREFIX}-{}.log",
            now.format("%Y-%m-%d")
        ));
        let _guard = self.file_lock.lock().unwrap();
        let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
            return;
        };
        let _ = writeln!(
            file,
            "{}\t{}\t{}",
            now.format("%Y-%m-%d %H:%M:%S"),
            level,
            message
        );
    }

    fn archive_logs(&self) {
        let Some(dir) = self.log_dir() else {
            return;
        };
        if !dir.is_dir() {
            return;
        }
        let backup_dir = self.log_backup_dir();
        if fs::create_dir_all(&backup_dir).is_err() {
            return;
        }
        let tag = Local::now().format("%Y%m%d-%H%M%S").to_string();
        let prefix_with_dash = format!("{LOG_FILE_PREFIX}-");
        let _guard = self.file_lock.lock().unwrap();
        let Ok(entries) = fs::read_dir(&dir) else {
            return;
        };
        for entry in entries.flatten() {
            let src = entry.path();
            let Some(name) = src.file_name().and_then(|v| v.to_str()) else {
                continue;
            };
            if !name.starts_with(&prefix_with_dash) || !name.ends_with(".log") || !src.is_file() {
                continue;
            }
            let base = name.trim_end_matches(".log");
            let mut dst = backup_dir.join(format!("{base}_{tag}.log"));
            let mut counter = 1;
            while dst.exists() {
                dst = backup_dir.join(format!("{base}_{tag}_{counter}.log"));
                counter += 1;
            }
            let _ = fs::rename(&src, dst);
        }
    }

    fn log_dir(&self) -> Option<PathBuf> {
        self.log_dir_override
            .clone()
            .or_else(claude_desktop_proxy_log_dir)
    }

    fn log_backup_dir(&self) -> PathBuf {
        self.log_dir()
            .unwrap_or_else(|| PathBuf::from(".codex-app-transfer").join("logs"))
            .join("backup")
    }
}

#[derive(Debug)]
pub struct ProxyTelemetry {
    pub stats: ProxyStats,
    pub logs: LogBuffer,
}

impl Default for ProxyTelemetry {
    fn default() -> Self {
        Self {
            stats: ProxyStats::default(),
            logs: LogBuffer::new(200),
        }
    }
}

static TELEMETRY: OnceLock<ProxyTelemetry> = OnceLock::new();

pub fn claude_desktop_proxy_telemetry() -> &'static ProxyTelemetry {
    TELEMETRY.get_or_init(ProxyTelemetry::default)
}

pub fn claude_desktop_proxy_log_dir() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("logs"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("cas-claude-desktop-{name}-{nanos}"))
    }

    #[test]
    fn stats_records_success_failed_and_today() {
        let stats = ProxyStats::default();
        stats.record(true);
        stats.record(false);
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.total, 2);
        assert_eq!(snapshot.success, 1);
        assert_eq!(snapshot.failed, 1);
        assert_eq!(snapshot.today, 2);
    }

    #[test]
    fn log_buffer_writes_claude_desktop_prefixed_file() {
        let dir = unique_temp_dir("logs-write");
        let buffer = LogBuffer::new_in_dir(2, dir.clone());
        buffer.add("INFO", "first request");
        buffer.add("ERROR", "failed request");
        buffer.add("SUCCESS", "finished request");
        let entries = buffer.get_all();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].level, "ERROR");
        assert_eq!(entries[1].level, "SUCCESS");

        let today = Local::now().format("%Y-%m-%d").to_string();
        let log_path = dir.join(format!("claude-desktop-proxy-{today}.log"));
        let content = fs::read_to_string(log_path).unwrap();
        assert!(content.contains("\tINFO\tfirst request"));
        assert!(content.contains("\tERROR\tfailed request"));
        assert!(content.contains("\tSUCCESS\tfinished request"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn log_buffer_clear_archives_claude_desktop_files_only() {
        let dir = unique_temp_dir("logs-clear");
        let buffer = LogBuffer::new_in_dir(20, dir.clone());

        // Codex 端的 proxy-…log 不应被 archive(防误清)
        fs::create_dir_all(&dir).unwrap();
        let codex_log = dir.join("proxy-foreign.log");
        fs::write(&codex_log, b"belongs to codex telemetry").unwrap();

        buffer.add("INFO", "before clear");
        let today = Local::now().format("%Y-%m-%d").to_string();
        let log_path = dir.join(format!("claude-desktop-proxy-{today}.log"));
        assert!(log_path.exists());

        buffer.clear();

        assert!(buffer.get_all().is_empty());
        assert!(!log_path.exists());
        assert!(codex_log.exists(), "Codex proxy-…log 不应被 Claude clear 误移动");

        let backup_dir = dir.join("backup");
        let archived: Vec<PathBuf> = fs::read_dir(&backup_dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        assert_eq!(archived.len(), 1);
        assert!(archived[0]
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .starts_with(&format!("claude-desktop-proxy-{today}_")));

        let _ = fs::remove_dir_all(dir);
    }
}
