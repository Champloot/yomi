//! Реализация команд.
//!
//! Каждая команда — отдельный модуль с функцией `run`. Правило: команда
//! отвечает только за «взять данные из ядра и напечатать». Как только
//! в команде появляется логика — она переезжает в `yomi-core`.

mod config_cmd;
mod info;
mod library;
mod marks;
mod pack;
mod read;

use crate::cli::{Cli, Command};
use anyhow::Result;

/// Контекст выполнения: то, что нужно почти каждой команде.
pub struct Ctx {
    pub config: yomi_core::config::Config,
    pub json: bool,
}

pub async fn dispatch(cli: &Cli) -> Result<()> {
    let config = yomi_core::config::Config::load(cli.config.as_deref())?;
    let ctx = Ctx {
        config,
        json: cli.json,
    };

    match &cli.command {
        Command::Read(args) => read::run(&ctx, args).await,
        Command::Info(args) => info::run(args).await,
        Command::Pack(args) => pack::run(args).await,
        Command::Library(cmd) => library::run(&ctx, cmd).await,
        Command::Marks(cmd) => marks::run(cmd).await,
        Command::Config(cmd) => config_cmd::run(&ctx, cmd, cli.config.as_deref()).await,
    }
}
