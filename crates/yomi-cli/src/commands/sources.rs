//! `yomi sources list` — единственная команда, полностью работающая на M0.

use super::Ctx;
use crate::cli::SourcesCommand;
use anyhow::Result;

pub async fn run(ctx: &Ctx, cmd: &SourcesCommand) -> Result<()> {
    match cmd {
        SourcesCommand::List => list(ctx),
    }
}

fn list(ctx: &Ctx) -> Result<()> {
    if ctx.registry.is_empty() {
        println!("Источники не зарегистрированы.");
        return Ok(());
    }

    if ctx.json {
        // Ручная сборка JSON: на M0 не тащим serde_json ради двух полей.
        let entries: Vec<String> = ctx
            .registry
            .list()
            .iter()
            .map(|s| {
                let c = s.capabilities();
                format!(
                    r#"{{"id":"{}","name":"{}","text_search":{},"genres":{},"auth":{},"fragile":{}}}"#,
                    s.id(),
                    s.name(),
                    c.text_search,
                    c.filter_by_genre,
                    c.requires_auth,
                    c.fragile
                )
            })
            .collect();
        println!("[{}]", entries.join(","));
        return Ok(());
    }

    println!("{:<12} {:<20} ВОЗМОЖНОСТИ", "ID", "НАЗВАНИЕ");
    for source in ctx.registry.list() {
        let c = source.capabilities();
        let mut features = Vec::new();
        if c.text_search {
            features.push("поиск");
        }
        if c.filter_by_genre {
            features.push("жанры");
        }
        if c.filter_by_author {
            features.push("автор");
        }
        if c.requires_auth {
            features.push("нужен вход");
        }
        if c.fragile {
            features.push("нестабилен");
        }
        println!(
            "{:<12} {:<20} {}",
            source.id().as_str(),
            source.name(),
            features.join(", ")
        );
    }
    Ok(())
}
