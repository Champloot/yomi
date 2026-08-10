//! `yomi manga ID` — сколько глав у тайтла реально доступно для скачивания.
//!
//! Появилась из практического вопроса: часть тайтлов на MangaDex отдаётся
//! ссылкой на сторонний сайт (лицензированные, ссылка на mangaplus и
//! подобные) — такие главы видны в списке, но не скачиваются. Команда
//! отвечает на вопрос «а сколько там реально можно взять», не запуская
//! саму загрузку.

use super::Ctx;
use crate::cli::MangaArgs;
use anyhow::{Context, Result};
use std::collections::BTreeMap;

pub async fn run(ctx: &Ctx, args: &MangaArgs) -> Result<()> {
    let source = ctx
        .registry
        .get(&args.source)
        .with_context(|| format!("источник «{}»; см. `yomi sources list`", args.source))?;

    let manga = source.manga(&args.manga_id).await?;
    let chapters = source.chapters(&args.manga_id).await?;

    println!("{}", manga.title);
    if !manga.authors.is_empty() {
        println!("Автор: {}", manga.authors.join(", "));
    }
    println!("Статус: {}", manga.status.label_ru());
    println!();

    if chapters.is_empty() {
        println!("Глав на выбранных языках не найдено.");
        return Ok(());
    }

    let downloadable = chapters.iter().filter(|c| c.is_downloadable()).count();
    let external = chapters.len() - downloadable;

    println!("Всего глав:      {}", chapters.len());
    println!("Можно скачать:   {downloadable}");
    if external > 0 {
        println!("Только внешние:  {external} (ссылка на сторонний сайт, не скачивается)");
    }

    // Разбивка по языкам — часто именно она объясняет, почему «глав нет»:
    // тайтл есть, но не переведён на язык из content_languages.
    let mut by_language: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for chapter in &chapters {
        let entry = by_language
            .entry(chapter.language.as_str())
            .or_insert((0, 0));
        entry.0 += 1;
        if chapter.is_downloadable() {
            entry.1 += 1;
        }
    }
    println!("\nПо языкам:");
    for (lang, (total, ok)) in by_language {
        println!("  {lang:<6} {ok}/{total} доступно");
    }

    Ok(())
}
