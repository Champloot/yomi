//! `yomi library` — что отслеживается и на чём остановились.

use super::Ctx;
use crate::cli::{LibraryArgs, LibraryCommand};
use anyhow::{bail, Context, Result};
use yomi_db::model::{ScannedChapter, ScannedManga};
use yomi_db::Store;

pub async fn run(ctx: &Ctx, args: &LibraryArgs) -> Result<()> {
    match (&args.command, &args.title) {
        (Some(LibraryCommand::Scan { paths }), _) => scan(ctx, paths),
        (Some(LibraryCommand::Clean { yes }), _) => clean(*yes),
        (None, Some(title)) => show(title),
        (None, None) => overview(),
    }
}

fn open_store() -> Result<Store> {
    let path = yomi_core::paths::database_file()?;
    Store::open(&path)
        .with_context(|| format!("открытие базы {}", path.display()))
        .map_err(Into::into)
}

/// Приводит путь к канонической форме.
///
/// Записи библиотеки опознаются по пути, поэтому форма записи важна:
/// если сканировать относительным путём, а читать абсолютным, файл
/// не найдётся и прогресс потеряется.
pub fn canonical(path: &std::path::Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn to_db(found: yomi_viewer::scan::ScannedManga) -> ScannedManga {
    ScannedManga {
        external_id: canonical(&found.path),
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
                external_id: canonical(&c.path),
                number: c.number,
                volume: c.volume,
                title: c.title,
                language: c.language,
                scanlator: c.scanlator,
                page_count: c.page_count,
                kind: match c.kind {
                    yomi_viewer::structure::FileKind::Chapter => "chapter",
                    yomi_viewer::structure::FileKind::Volume => "volume",
                    yomi_viewer::structure::FileKind::Single => "single",
                    yomi_viewer::structure::FileKind::Unknown => "unknown",
                }
                .to_string(),
            })
            .collect(),
    }
}

fn scan(ctx: &Ctx, paths: &[std::path::PathBuf]) -> Result<()> {
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
    let (mut total_manga, mut total_chapters) = (0usize, 0usize);

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
            println!("{title} — файлов: {chapters}");
            total_manga += 1;
            total_chapters += chapters;
        }
    }

    if total_manga == 0 {
        println!("Ничего не найдено. Ожидается раскладка: КАТАЛОГ/Тайтл/Том 1.cbz");
        return Ok(());
    }
    println!("\nВ библиотеке: тайтлов {total_manga}, файлов {total_chapters}");
    Ok(())
}

/// `yomi library` — что отслеживается.
///
/// Показывает не перечень файлов, а состояние: тайтл, диапазон томов и
/// на чём читатель остановился. Перечислять файлы здесь бессмысленно —
/// для этого есть показ по названию.
fn overview() -> Result<()> {
    let store = open_store()?;
    let items = store.list_manga(None)?;

    if items.is_empty() {
        println!("Библиотека пуста. Заполните её командой `yomi library scan КАТАЛОГ`.");
        return Ok(());
    }

    let mut missing = 0usize;

    for manga in &items {
        let chapters = store.chapters_of(manga.id)?;
        missing += chapters.iter().filter(|c| !c.path().exists()).count();

        let volumes = match store.volume_range(manga.id)? {
            Some((first, last)) if first == last => format!("том {first}"),
            Some((first, last)) => format!("тома {first}–{last}"),
            None => format!("файлов {}", chapters.len()),
        };

        let position = match store.resume_target(manga.id)? {
            Some((chapter, Some(progress))) => {
                format!(
                    "остановились: {} стр. {}",
                    chapter.label(),
                    progress.page + 1
                )
            }
            Some((chapter, None)) => format!("дальше: {}", chapter.label()),
            None => "прочитано".to_string(),
        };

        println!("{:<34} {:<16} {}", manga.title, volumes, position);
    }

    println!("\nВсего тайтлов: {}", items.len());
    println!("Подробности: yomi library НАЗВАНИЕ");

    // Не чистим сами: пропавший файл может быть на отключённом диске,
    // а удаление записи унесло бы прогресс. Просто предупреждаем.
    if missing > 0 {
        println!("\nФайлов не найдено на диске: {missing}. Проверить: yomi library clean");
    }
    Ok(())
}

/// `yomi library НАЗВАНИЕ` — тома тайтла, свежие сверху.
fn show(title: &str) -> Result<()> {
    let store = open_store()?;
    let found = store.list_manga(Some(title))?;

    let manga = match found.len() {
        0 => bail!("в библиотеке нет тайтла «{title}»"),
        1 => &found[0],
        _ => {
            println!("Под «{title}» подходит несколько тайтлов:");
            for m in &found {
                println!("  {}", m.title);
            }
            bail!("уточните название");
        }
    };

    let mut chapters = store.chapters_of(manga.id)?;
    // Свежее сверху: к последнему тому обращаются чаще, чем к первому.
    chapters.reverse();

    let resume = store.resume_target(manga.id)?.map(|(c, _)| c.id);

    println!("{}", manga.title);
    if !manga.authors.is_empty() {
        println!("{}", manga.authors.join(", "));
    }
    println!();

    let mut separator_drawn = false;
    for chapter in &chapters {
        // Черта отделяет то, на чём остановились, от прочитанного ниже.
        if !separator_drawn && Some(chapter.id) == resume {
            println!("{}", "─".repeat(46));
            separator_drawn = true;
        }

        let mark = match store.progress_of(chapter.id)? {
            Some(p) if p.completed => "✓".to_string(),
            Some(p) => format!("{}", p.page + 1),
            None => "·".to_string(),
        };
        let here = if Some(chapter.id) == resume {
            " ←"
        } else {
            ""
        };

        println!(
            "{mark:>5}  {:<26} {:>4} стр.{here}",
            chapter.label(),
            chapter.page_count.unwrap_or(0)
        );
    }

    println!("\nПродолжить: yomi read \"{}\"", manga.title);
    Ok(())
}

/// `yomi library clean` — убрать записи о пропавших файлах.
fn clean(confirmed: bool) -> Result<()> {
    let mut store = open_store()?;

    let missing: Vec<_> = store
        .all_chapters()?
        .into_iter()
        .filter(|c| !c.path().exists())
        .collect();

    if missing.is_empty() {
        println!("Все файлы на месте, чистить нечего.");
        return Ok(());
    }

    for chapter in &missing {
        println!("пропал: {}", chapter.external_id);
    }

    if !confirmed {
        println!(
            "\nНайдено записей о пропавших файлах: {}.\n\
             Проверьте, что диск с коллекцией подключён — если файлы \n\
             просто недоступны, удалять их из библиотеки не нужно: \n\
             вместе с ними пропадёт и прогресс чтения.\n\n\
             Удалить: yomi library clean --yes",
            missing.len()
        );
        return Ok(());
    }

    let ids: Vec<i64> = missing.iter().map(|c| c.id).collect();
    let removed = store.delete_chapters(&ids)?;
    let titles = store.delete_empty_manga()?;
    println!("\nУдалено глав: {removed}, опустевших тайтлов: {titles}");
    Ok(())
}
