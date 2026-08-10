//! Реализация команд.
//!
//! Каждая команда — отдельный модуль с функцией `run`. Правило: команда
//! отвечает только за «взять данные из ядра и напечатать». Как только
//! в команде появляется логика — она переезжает в `yomi-core`.

mod config_cmd;
mod download;
mod info;
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
    let registry = build_registry(&config);
    let ctx = Ctx {
        config,
        registry,
        json: cli.json,
    };

    match &cli.command {
        Command::Read(args) => read::run(&ctx, args).await,
        Command::Info(args) => info::run(args).await,
        Command::Library(cmd) => library::run(&ctx, cmd).await,
        Command::Search(args) => search::run(&ctx, args).await,
        Command::Download(args) => download::run(&ctx, args).await,
        Command::Sources(cmd) => sources::run(&ctx, cmd).await,
        Command::Config(cmd) => config_cmd::run(&ctx, cmd, cli.config.as_deref()).await,
    }
}

/// Собирает реестр источников.
///
/// Сетевые источники регистрируются здесь, а не в ядре: ядро не должно
/// зависеть от HTTP-клиента. Сбой создания источника не роняет
/// программу — остальные источники и локальное чтение продолжат
/// работать без него.
fn build_registry(config: &yomi_core::config::Config) -> yomi_core::source::Registry {
    let registry = yomi_core::source::default_registry();

    let timeout = std::time::Duration::from_secs(config.network.timeout_secs as u64);
    match yomi_source_mangadex::MangaDexSource::new(
        &config.network.user_agent,
        timeout,
        config.general.content_languages.clone(),
    ) {
        Ok(source) => registry.register(std::sync::Arc::new(source)),
        Err(e) => {
            tracing::warn!(error = %e, "источник MangaDex недоступен");
            registry
        }
    }
}
