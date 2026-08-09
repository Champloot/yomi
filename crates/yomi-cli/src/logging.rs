//! Настройка логирования.
//!
//! Логи идут в stderr, полезный вывод — в stdout. Это позволяет
//! делать `yomi search ... > file.txt` и не получать мусор в файле.
//! Уровень задаётся флагами -v/-vv или переменной `YOMI_LOG`.

use tracing_subscriber::{fmt, EnvFilter};

pub const ENV_LOG: &str = "YOMI_LOG";

pub fn init(verbose: u8, quiet: bool) {
    let level = if quiet {
        "error"
    } else {
        match verbose {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        }
    };

    // Переменная окружения имеет приоритет над флагами.
    let filter = EnvFilter::try_from_env(ENV_LOG)
        .unwrap_or_else(|_| EnvFilter::new(format!("yomi={level},yomi_core={level}")));

    let _ = fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(verbose >= 2)
        .without_time()
        .try_init();
}
