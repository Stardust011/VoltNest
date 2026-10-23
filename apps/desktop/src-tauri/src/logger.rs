//! 极简文件日志（`%APPDATA%\VoltNest\logs\voltnest.log`）。不引入 `tauri-plugin-log`：其目录 `%APPDATA%\<identifier>\logs` 会与本应用的数据根分叉、多写一个目录。只需「出问题时拿到时间线」，三十行足够。时间戳一律走 `clock::now_utc_ms()`（全仓唯一时间源）。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

/// 超过该大小就在启动时截断：这是诊断日志，不是审计日志。
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// 日志文件名。
pub const LOG_FILE: &str = "voltnest.log";

struct FileLogger {
    file: Mutex<Option<File>>,
    level: log::LevelFilter,
}

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let Ok(mut guard) = self.file.lock() else {
            return;
        };
        let Some(file) = guard.as_mut() else {
            return;
        };
        let at = voltnest_provider::clock::now_utc_ms();
        let _ = writeln!(
            file,
            "{at} {:<5} {} — {}",
            record.level(),
            record.target(),
            record.args()
        );
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.file.lock()
            && let Some(file) = guard.as_mut()
        {
            let _ = file.flush();
        }
    }
}

/// 初始化全局日志。目录不存在时静默降级为「不写日志」，绝不阻塞启动。
pub fn init(logs_dir: &Path) {
    let path = logs_dir.join(LOG_FILE);
    let _ = std::fs::create_dir_all(logs_dir);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::remove_file(&path);
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok();

    let logger = FileLogger {
        file: Mutex::new(file),
        level: log::LevelFilter::Info,
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}
