//! `yomi download` — загрузка глав. Этап M3.

use super::Ctx;
use crate::cli::DownloadArgs;
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use yomi_core::model::{Chapter, PageLocation};
use yomi_download::package::{detect_extension, PagePayload};
use yomi_download::{naming, select, write_cbz};

pub async fn run(ctx: &Ctx, args: &DownloadArgs) -> Result<()> {
    let source = ctx
        .registry
        .get(&args.source)
        .with_context(|| format!("источник «{}»; см. `yomi sources list`", args.source))?;

    let output = match &args.output {
        Some(path) => path.clone(),
        None => ctx.config.download_dir()?,
    };

    let manga = source.manga(&args.manga_id).await?;
    let all_chapters = source.chapters(&args.manga_id).await?;

    if all_chapters.is_empty() {
        bail!("у тайтла «{}» нет доступных глав", manga.title);
    }

    let chosen = select::select(&all_chapters, &args.chapters);
    if chosen.is_empty() {
        bail!(
            "под «{}» не подошла ни одна глава. Доступно {} глав; \
             попробуйте all, 5 или 1-10",
            args.chapters,
            all_chapters.len()
        );
    }

    println!("{}: выбрано глав {}", manga.title, chosen.len());

    let mut downloaded = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for chapter in chosen {
        let relative = naming::format_path(
            &ctx.config.download.filename_template,
            &manga.title,
            chapter,
        );
        let target: PathBuf = output.join(relative);

        // Уже скачанное не трогаем: команду часто запускают повторно,
        // чтобы дозабрать новые главы.
        if target.exists() {
            tracing::debug!(path = %target.display(), "глава уже скачана");
            skipped += 1;
            continue;
        }

        match download_chapter(ctx, source.clone(), &manga.title, chapter, &target).await {
            Ok(pages) => {
                println!("  {} — страниц {}", label(chapter), pages);
                downloaded += 1;
            }
            Err(e) => {
                // Сбой одной главы не должен прерывать пакетную загрузку:
                // проще дозабрать одну потом, чем начинать сначала.
                tracing::warn!(chapter = %label(chapter), error = %e, "глава не скачана");
                println!("  {} — не удалось: {e}", label(chapter));
                failed += 1;
            }
        }
    }

    println!("\nСкачано глав: {downloaded}, пропущено уже имеющихся: {skipped}");
    if failed > 0 {
        println!("Не удалось скачать: {failed}");
    }
    println!("Каталог: {}", output.display());

    // Частичный успех остаётся успехом: при пакетной загрузке сбой
    // одной главы обычен, её можно дозабрать повторным запуском.
    // А вот когда не скачалось ничего, сообщать об успехе нельзя —
    // скрипт решит, что данные на месте.
    if downloaded == 0 && failed > 0 {
        bail!("не удалось скачать ни одной главы из {failed}");
    }
    Ok(())
}

fn label(chapter: &Chapter) -> String {
    match (chapter.volume, chapter.number) {
        (Some(v), Some(n)) => format!("т.{v} гл.{n}"),
        (None, Some(n)) => format!("гл.{n}"),
        _ => chapter.title.clone().unwrap_or_else(|| chapter.id.clone()),
    }
}

async fn download_chapter(
    ctx: &Ctx,
    source: std::sync::Arc<dyn yomi_core::source::Source>,
    manga_title: &str,
    chapter: &Chapter,
    target: &std::path::Path,
) -> Result<usize> {
    let pages = source.pages(&chapter.id).await?;
    if pages.is_empty() {
        bail!("страниц нет");
    }

    // Ограничиваем одновременные загрузки: источники ограничивают
    // частоту запросов, и агрессивный клиент получит бан по адресу.
    let concurrency = ctx.config.download.concurrency.max(1) as usize;
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));

    let mut tasks = Vec::with_capacity(pages.len());
    for page in pages {
        let PageLocation::Url(url) = page.location else {
            bail!("источник вернул страницу не в виде ссылки");
        };
        let permit = semaphore.clone();
        let source = source.clone();
        tasks.push(tokio::spawn(async move {
            let _guard = permit.acquire().await;
            let bytes = source.fetch_page(&url).await?;
            Ok::<_, anyhow::Error>(PagePayload {
                index: page.index,
                extension: detect_extension(&bytes).to_string(),
                bytes,
            })
        }));
    }

    let mut payloads = Vec::with_capacity(tasks.len());
    for task in tasks {
        payloads.push(task.await.context("задача загрузки прервана")??);
    }

    let count = payloads.len();
    write_cbz(target, manga_title, chapter, payloads)?;
    Ok(count)
}
