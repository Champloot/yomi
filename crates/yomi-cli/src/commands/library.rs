//! `yomi library` — локальная библиотека. Этап M2.

use super::Ctx;
use crate::cli::LibraryCommand;
use anyhow::{bail, Context, Result};
use yomi_db::model::{ScannedChapter, ScannedManga};
use yomi_db::Store;

pub async fn run(ctx: &Ctx, cmd: &LibraryCommand) -> Result<()> {
    match cmd {
        LibraryCommand::Scan { paths } => scan(ctx, paths),
        LibraryCommand::List { filter } => list(filter.as_deref()),
        LibraryCommand::Resume => resume(),
    }
}

fn open_store() -> Result<Store> {
    let path = yomi_core::paths::database_file()?;
    Store::open(&path)
        .with_context(|| format!("открытие базы {}", path.display()))
        .map_err(Into::into)
}

/// Переводит находку сканера в запись библиотеки.
///
/// Два почти одинаковых типа существуют намеренно: `yomi-viewer` не
/// должен зависеть от хранилища, а `yomi-db` — от разбора архивов.
fn to_db(found: yomi_viewer::scan::ScannedManga) -> ScannedManga {
    ScannedManga {
        external_id: found.path.to_string_lossy().to_string(),
        title: found.title,
        authors: found.authors,
        genres: found.genres,
        status: "unknown".to_string(),
        year: found.year,
        description: found.description,
        chapters: found
            .chapters
            .into_iter()
            .map(|c| ScannedChapter {
                external_id: c.path.to_string_lossy().to_string(),
                number: c.number,
                volume: c.volume,
                title: c.title,
                language: c.language,
                scanlator: c.scanlator,
                page_count: c.page_count,
            })
            .collect(),
    }
}

fn scan(ctx: &Ctx, paths: &[std::path::PathBuf]) -> Result<()> {
    // Аргументы командной строки важнее конфига.
    let targets = if paths.is_empty() {
        ctx.config.library.paths.clone()
    } else {
        paths.to_vec()
    };

    if targets.is_empty() {
        bail!(
            "не указано, что сканировать: передайте каталог аргументом \
             или заполните library.paths в конфиге"
        );
    }

    let mut store = open_store()?;
    let mut total_manga = 0usize;
    let mut total_chapters = 0usize;

    for target in &targets {
        if !target.exists() {
            tracing::warn!(path = %target.display(), "каталог не существует, пропускаю");
            continue;
        }
        tracing::info!(path = %target.display(), "сканирую");

        for found in yomi_viewer::scan::scan_library(target) {
            let chapters = found.chapters.len();
            let record = to_db(found);
            let title = record.title.clone();
            store
                .upsert_scanned(&record)
                .with_context(|| format!("запись тайтла «{title}»"))?;
            println!("{title} — глав: {chapters}");
            total_manga += 1;
            total_chapters += chapters;
        }
    }

    if total_manga == 0 {
        println!("Ничего не найдено. Ожидается раскладка: КАТАЛОГ/Тайтл/Том 1.cbz");
        return Ok(());
    }
    println!("\nВ библиотеке: тайтлов {total_manga}, глав {total_chapters}");
    Ok(())
}

fn list(filter: Option<&str>) -> Result<()> {
    let store = open_store()?;
    let items = store.list_manga(filter)?;

    if items.is_empty() {
        if store.manga_count()? == 0 {
            println!("Библиотека пуста. Заполните её командой `yomi library scan КАТАЛОГ`.");
        } else {
            println!("По фильтру ничего не найдено.");
        }
        return Ok(());
    }

    for manga in &items {
        let chapters = store.chapters_of(manga.id)?;
        // Считаем прочитанное, чтобы список показывал прогресс,
        // а не просто перечислял тайтлы.
        let mut completed = 0usize;
        for chapter in &chapters {
            if store
                .progress_of(chapter.id)?
                .map(|p| p.completed)
                .unwrap_or(false)
            {
                completed += 1;
            }
        }
        let authors = if manga.authors.is_empty() {
            "—".to_string()
        } else {
            manga.authors.join(", ")
        };
        println!(
            "{:<4} {:<40} {}/{} глав  {}",
            manga.id,
            manga.title,
            completed,
            chapters.len(),
            authors
        );
    }
    println!("\nВсего тайтлов: {}", items.len());
    Ok(())
}

/// `yomi library resume` — показать, где остановились.
///
/// Команда намеренно не запускает читалку сама, а печатает готовую
/// строку запуска: так видно, что именно откроется, и её можно
/// поправить или подставить в скрипт.
fn resume() -> Result<()> {
    let store = open_store()?;

    let Some((chapter, progress)) = store.last_unfinished()? else {
        println!("Незавершённых глав нет. Начните читать: `yomi read ПУТЬ`");
        return Ok(());
    };

    let manga = store.manga_by_id(chapter.manga_id)?;
    let total = progress
        .total_pages
        .map(|t| t.to_string())
        .unwrap_or_else(|| "?".to_string());

    println!("{} — {}", manga.title, chapter.label());
    println!(
        "Остановились на странице {} из {}",
        progress.page + 1,
        total
    );
    println!();
    println!("yomi read '{}'", chapter.external_id);
    Ok(())
}
