//! Реализация команд.
//!
//! Каждая команда — отдельный модуль с функцией `run`. Правило: команда
//! отвечает только за «взять данные из ядра и напечатать». Как только
//! в команде появляется логика — она переезжает в `yomi-core`.

mod config_cmd;
mod download;
mod library;
mod read;
mod search;
mod sources;

use crate::cli::{Cli, Command};
use anyhow::Result;

/// Контекст выполнения: то, что нужно почти каждой команде.
pub struct Ctx {
    pub config: yomi_core::config::Config,
    pub registry: yomi_core::source::Registry,
    pub json: bool,
}

pub async fn dispatch(cli: &Cli) -> Result<()> {
    let config = yomi_core::config::Config::load(cli.config.as_deref())?;
    let ctx = Ctx {
        config,
        registry: yomi_core::source::default_registry(),
        json: cli.json,
    };

    match &cli.command {
        Command::Read(args) => read::run(&ctx, args).await,
        Command::Library(cmd) => library::run(&ctx, cmd).await,
        Command::Search(args) => search::run(&ctx, args).await,
        Command::Download(args) => download::run(&ctx, args).await,
        Command::Sources(cmd) => sources::run(&ctx, cmd).await,
        Command::Config(cmd) => config_cmd::run(&ctx, cmd, cli.config.as_deref()).await,
    }
}

/// Заглушка для нереализованных возможностей.
///
/// Возвращает ошибку с кодом 6, а не молча ничего не делает: скрипты
/// должны отличать «не умею» от «сделал».
pub fn not_implemented(what: &'static str, milestone: &str) -> anyhow::Error {
    anyhow::Error::new(yomi_core::Error::NotImplemented(what)).context(format!(
        "запланировано на этап {milestone}, см. docs/ROADMAP.md"
    ))
}
