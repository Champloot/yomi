//! `yomi config` — работа с конфигурацией. Полностью реализовано на M0.

use super::Ctx;
use crate::cli::ConfigCommand;
use anyhow::{Context, Result};
use std::path::Path;
use yomi_core::paths;

pub async fn run(ctx: &Ctx, cmd: &ConfigCommand, explicit: Option<&Path>) -> Result<()> {
    match cmd {
        ConfigCommand::Init { force } => init(ctx, explicit, *force),
        ConfigCommand::Show => {
            print!("{}", ctx.config.to_toml());
            Ok(())
        }
        ConfigCommand::Path => path(explicit),
    }
}

fn init(ctx: &Ctx, explicit: Option<&Path>, force: bool) -> Result<()> {
    let target = match explicit {
        Some(p) => p.to_path_buf(),
        None => paths::config_file()?,
    };

    let written = ctx
        .config
        .save(&target, force)
        .with_context(|| format!("запись конфигурации в {}", target.display()))?;

    if written {
        println!("Конфигурация создана: {}", target.display());
    } else {
        println!(
            "Файл уже существует: {}\nИспользуйте --force для перезаписи.",
            target.display()
        );
    }
    Ok(())
}

fn path(explicit: Option<&Path>) -> Result<()> {
    let config = match explicit {
        Some(p) => p.to_path_buf(),
        None => paths::config_file()?,
    };
    println!("Конфигурация: {}", config.display());
    println!("Данные:       {}", paths::data_dir()?.display());
    println!("Кэш:          {}", paths::cache_dir()?.display());
    println!("База:         {}", paths::database_file()?.display());
    Ok(())
}
