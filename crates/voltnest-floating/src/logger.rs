//! 极简 stderr 日志器，遵循项目 `log` 门面约定。

use std::io::Write;

struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let _ = writeln!(std::io::stderr(), "[{}] {}", record.level(), record.args());
    }

    fn flush(&self) {
        let _ = std::io::stderr().flush();
    }
}

/// 安装 stderr 日志器；重复安装会静默忽略。
pub fn init() {
    let _ = log::set_boxed_logger(Box::new(StderrLogger))
        .map(|()| log::set_max_level(log::LevelFilter::Info));
}
