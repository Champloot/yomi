//! `yomi library` — локальная библиотека. Этап M2.

use super::{not_implemented, Ctx};
use crate::cli::LibraryCommand;
use anyhow::Result;

pub async fn run(ctx: &Ctx, cmd: &LibraryCommand) -> Result<()> {
    match cmd {
        LibraryCommand::Scan { paths } => {
            // Аргументы командной строки важнее конфига.
            let targets = if paths.is_empty() {
                ctx.config.library.paths.clone()
            } else {
                paths.clone()
            };

            if targets.is_empty() {
                anyhow::bail!(
                    "не указано, что сканировать: передайте каталог аргументом \
                     или заполните library.paths в конфиге"
                );
            }
            for t in &targets {
                tracing::info!(path = %t.display(), "каталог поставлен в очередь сканирования");
            }
            Err(not_implemented("сканирование библиотеки", "M2"))
        }
        LibraryCommand::List { filter } => {
            tracing::debug!(?filter, "запрошен список библиотеки");
            Err(not_implemented("список библиотеки", "M2"))
        }
    }
}
