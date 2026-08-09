//! `yomi search` — поиск в источнике.
//!
//! На M0 работает поверх демо-источника: цепочка «аргументы → запрос →
//! источник → вывод» уже настоящая, меняется только реализация источника.

use super::Ctx;
use crate::cli::SearchArgs;
use anyhow::{Context, Result};
use yomi_core::model::SearchQuery;

pub async fn run(ctx: &Ctx, args: &SearchArgs) -> Result<()> {
    let source = ctx
        .registry
        .get(&args.source)
        .with_context(|| format!("источник «{}»; см. `yomi sources list`", args.source))?;

    let caps = source.capabilities();
    if !args.author.is_empty() && !caps.filter_by_author {
        tracing::warn!(
            source = %source.id(),
            "источник не умеет фильтровать по автору, фильтр проигнорирован"
        );
    }

    let query = SearchQuery {
        text: args.query.clone(),
        include_genres: args.genre.clone(),
        exclude_genres: args.exclude_genre.clone(),
        authors: args.author.clone(),
        statuses: Vec::new(),
        languages: ctx.config.general.content_languages.clone(),
        sort: args.sort.into(),
        page: args.page,
        per_page: args.limit,
    };

    let result = source.search(&query).await?;

    if result.items.is_empty() {
        println!("Ничего не найдено.");
        return Ok(());
    }

    for manga in &result.items {
        let authors = if manga.authors.is_empty() {
            "—".to_string()
        } else {
            manga.authors.join(", ")
        };
        println!(
            "{:<6} {}  [{}]  {}  {}",
            manga.id,
            manga.title,
            manga.status.label_ru(),
            authors,
            manga.genres.join(", ")
        );
    }

    if let Some(total) = result.total {
        println!("\nНайдено: {total}, страница {}", result.page);
    }
    Ok(())
}
