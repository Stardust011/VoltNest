//! 独立运行入口：`cargo run -p voltnest-floating`。
//!
//! 只是把 `SessionMode::Demo` 包一层；宿主托管模式由主程序带 `--floating-host` 拉起，
//! 走的是同一个 [`voltnest_floating::run_session`]。

fn main() {
    init_stderr_logger();
    let options = voltnest_floating::SessionOptions {
        mode: voltnest_floating::SessionMode::Demo,
    };
    if let Err(err) = voltnest_floating::run_session(options) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

/// 极简 stderr 日志器，遵循项目 `log` 门面约定。
fn init_stderr_logger() {
    struct StderrLogger;

    impl log::Log for StderrLogger {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }

        fn log(&self, record: &log::Record) {
            use std::io::Write;
            let _ = writeln!(std::io::stderr(), "[{}] {}", record.level(), record.args());
        }

        fn flush(&self) {
            use std::io::Write;
            let _ = std::io::stderr().flush();
        }
    }

    let _ = log::set_boxed_logger(Box::new(StderrLogger))
        .map(|()| log::set_max_level(log::LevelFilter::Info));
}
