use log::{self, LevelFilter, Log, SetLoggerError};
/// Really simple logger
use std::str::FromStr;

struct Logger;

impl Log for Logger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true // log::set_max_level() is sufficient
    }

    fn log(&self, record: &log::Record) {
        let lvl = record.level().to_string();
        let tgt = if record.target().is_empty() {
            record.module_path().unwrap_or_default()
        } else {
            record.target()
        };

        eprintln!("{lvl:<5} [{tgt}] {}", record.args());
    }

    fn flush(&self) {}
}

pub fn setup() -> Result<(), SetLoggerError> {
    let lvl = std::env::var("RUST_LOG")
        .ok()
        .as_deref()
        .map(LevelFilter::from_str)
        .and_then(Result::ok)
        .unwrap_or(LevelFilter::Warn);

    log::set_max_level(lvl);
    log::set_logger(&Logger {})
}
