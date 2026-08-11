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
        (Some(LibraryCommand::Forget { target }), _) => forget(target),
        (Some(LibraryCommand::Reset { yes }), _) => reset(*yes),
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
            let (manga_id, _) = store
                .upsert_scanned(&record)
                .with_context(|| format!("запись тайтла «{title}»"))?;

            // Закладки из ComicInfo переносим в базу, чтобы поиск главы
            // не открывал каждый файл заново. Ручные отметки при этом
            // не трогаем: правка пользователя важнее разметки файла.
            for entry in store.chapters_of(manga_id)? {
                if !store.marks_of(entry.id)?.is_empty() {
                    continue;
                }
                let Some(info) = yomi_viewer::scan::read_comicinfo(&entry.path()) else {
                    continue;
                };
                for mark in &info.bookmarks {
                    store.add_mark(entry.id, mark.page, Some(&mark.title))?;
                }
            }
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

        // Диапазон глав полезнее числа страниц: по нему видно, где
        // искать нужную главу, а количество страниц ни о чём не говорит.
        println!(
            "{mark:>5}  {:<24} {:<20}{here}",
            chapter.label(),
            chapters_range(&store, chapter)?
        );
    }

    println!("\nПродолжить:    yomi read \"{}\"", manga.title);
    println!(
        "Открыть том:   yomi read \"{}\" --volume НОМЕР",
        manga.title
    );
    println!(
        "Найти главу:   yomi read \"{}\" --chapter НОМЕР",
        manga.title
    );
    Ok(())
}

/// Какие главы лежат в файле — из отметок или из номера самой записи.
fn chapters_range(store: &Store, chapter: &yomi_db::model::LibraryChapter) -> Result<String> {
    // Два источника, как и в читалке: ручные отметки из базы важнее,
    // но у собранного тома разметка живёт закладками внутри файла.
    let mut numbers: Vec<f32> = store
        .marks_of(chapter.id)?
        .iter()
        .filter_map(|m| m.title.as_deref().and_then(chapter_number_from_label))
        .collect();

    if numbers.is_empty() {
        numbers = yomi_viewer::scan::read_comicinfo(&chapter.path())
            .map(|info| {
                info.bookmarks
                    .iter()
                    .filter_map(|b| chapter_number_from_label(&b.title))
                    .collect()
            })
            .unwrap_or_default();
    }

    if numbers.len() >= 2 {
        let first = numbers.iter().cloned().fold(f32::INFINITY, f32::min);
        let last = numbers.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        return Ok(format!("главы {}–{}", trim(first), trim(last)));
    }

    if let Some(number) = chapter.number {
        return Ok(format!("глава {}", trim(number)));
    }

    // Разметки нет — сказать про главы нечего, показываем объём.
    Ok(match chapter.page_count {
        Some(pages) => format!("{pages} стр., глав не размечено"),
        None => "глав не размечено".to_string(),
    })
}

/// Целые номера без дробной части: «359», а не «359.0».
fn trim(number: f32) -> String {
    if number.fract().abs() < f32::EPSILON {
        format!("{}", number as i64)
    } else {
        format!("{number}")
    }
}

fn chapter_number_from_label(label: &str) -> Option<f32> {
    let digits: String = label
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    digits.trim_end_matches('.').parse().ok()
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

/// `yomi library forget` — убрать тайтл или отдельный файл.
///
/// Удаляется только запись, файл на диске остаётся. Вместе с записью
/// уходят прогресс и ручные отметки — восстановить их неоткуда,
/// поэтому команда всегда говорит, что именно потеряется.
fn forget(target: &str) -> Result<()> {
    let mut store = open_store()?;

    // Сначала пробуем как путь: он однозначен, а названия повторяются.
    let path = std::path::Path::new(target);
    if path.exists() || target.contains('/') {
        let canonical = canonical(path);
        if store.delete_chapter_by_path(&canonical)? || store.delete_chapter_by_path(target)? {
            let empty = store.delete_empty_manga()?;
            println!("Запись убрана из библиотеки (файл на диске остался).");
            if empty > 0 {
                println!("Заодно убрано опустевших тайтлов: {empty}");
            }
            return Ok(());
        }
        bail!("в библиотеке нет записи о файле {}", path.display());
    }

    let found = store.list_manga(Some(target))?;
    let manga = match found.len() {
        0 => bail!("в библиотеке нет тайтла «{target}»"),
        1 => found[0].clone(),
        _ => {
            println!("Под «{target}» подходит несколько тайтлов:");
            for m in &found {
                let chapters = store.chapters_of(m.id)?.len();
                println!("  {} — файлов {}", m.title, chapters);
            }
            bail!("уточните название или укажите путь к файлу");
        }
    };

    let chapters = store.chapters_of(manga.id)?;
    store.delete_manga(manga.id)?;
    println!(
        "«{}» убран из библиотеки: записей {} (файлы на диске остались).",
        manga.title,
        chapters.len()
    );
    Ok(())
}

/// `yomi library reset` — очистить библиотеку целиком.
fn reset(confirmed: bool) -> Result<()> {
    let mut store = open_store()?;

    let manga = store.manga_count()?;
    let chapters = store.chapter_count()?;

    if manga == 0 {
        println!("Библиотека и так пуста.");
        return Ok(());
    }

    if !confirmed {
        println!(
            "Будет удалено: тайтлов {manga}, записей о файлах {chapters}.\n\
             Вместе с ними пропадут прогресс чтения и расставленные вручную\n\
             отметки глав — восстановить их будет неоткуда. Файлы на диске\n\
             не пострадают, библиотеку можно собрать заново командой scan.\n\n\
             Очистить: yomi library reset --yes"
        );
        return Ok(());
    }

    let (manga, chapters) = store.clear_all()?;
    println!("Библиотека очищена: тайтлов {manga}, записей {chapters}.");
    println!("Собрать заново: yomi library scan КАТАЛОГ");
    Ok(())
}
