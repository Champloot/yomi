//! `yomi build` — собрать том из отдельных файлов-глав.
//!
//! Решает задачу, ради которой всё затевалось: у пользователя лежат
//! скачанные по отдельности главы, а нужен один том с размеченными
//! границами. Границы при этом берутся из числа страниц каждой главы,
//! то есть **без пролистывания тома** — читателю не приходится
//! натыкаться на спойлеры, разыскивая начало главы.

use crate::cli::BuildArgs;
use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use yomi_download::package::{detect_extension, PackMeta, PagePayload};
use yomi_download::parse::{self, Confidence};
use yomi_download::write_cbz;
use yomi_viewer::archive::PageSource;

/// Подпись главы для закладки.
///
/// Одна на оба пути — сборку тома и дописывание главы: раньше они
/// формировали подпись по-разному, и дописанная глава теряла название.
fn chapter_label(number: Option<f32>, title: Option<&str>, fallback: &str) -> String {
    match (number, title) {
        (Some(n), Some(t)) => format!("Глава {n} — {t}"),
        (Some(n), None) => format!("Глава {n}"),
        (None, Some(t)) => t.to_string(),
        (None, None) => fallback.to_string(),
    }
}

/// Одна глава, готовая к укладке в том.
struct Chapter {
    path: PathBuf,
    number: Option<f32>,
    volume: Option<u16>,
    title: Option<String>,
    pages: usize,
}

pub async fn run(args: &BuildArgs) -> Result<()> {
    match &args.add {
        Some(chapter) => add_to_volume(&args.path, chapter, args),
        None => build_volume(&args.path, args),
    }
}

fn is_archive(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_lowercase().as_str(), "cbz" | "zip"))
        .unwrap_or(false)
}

/// Собирает список файлов-глав из каталога, в естественном порядке имён.
fn collect_chapters(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("чтение каталога {}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_archive(p))
        .collect();

    if files.is_empty() {
        bail!("в каталоге {} нет файлов CBZ", dir.display());
    }

    files.sort_by(|a, b| {
        let (a, b) = (
            a.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            b.file_name().and_then(|n| n.to_str()).unwrap_or(""),
        );
        natural_compare(a, b)
    });
    Ok(files)
}

/// Естественное сравнение имён: «2» раньше «10».
fn natural_compare(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |s: &str| -> Vec<Result<u64, String>> {
        let mut out = Vec::new();
        let mut chars = s.chars().peekable();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                let mut digits = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() {
                        digits.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                out.push(digits.parse::<u64>().map_err(|_| digits));
            } else {
                let mut text = String::new();
                while let Some(&t) = chars.peek() {
                    if !t.is_ascii_digit() {
                        text.push(t);
                        chars.next();
                    } else {
                        break;
                    }
                }
                out.push(Err(text));
            }
        }
        out
    };
    split(a).cmp(&split(b))
}

/// Спрашивает у пользователя строку. Пустой ответ — значение по умолчанию.
fn ask(prompt: &str) -> Result<String> {
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(answer.trim().to_string())
}

fn build_volume(dir: &Path, args: &BuildArgs) -> Result<()> {
    if !dir.is_dir() {
        bail!("нужен каталог с файлами глав: {}", dir.display());
    }

    let files = collect_chapters(dir)?;
    let stems: Vec<String> = files
        .iter()
        .map(|p| {
            p.file_stem()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        })
        .collect();

    let analysis = parse::analyze(&stems);
    let detected_series = args.series.clone().or_else(|| parse::common_title(&stems));

    // Считаем страницы: это и есть источник границ глав.
    let mut chapters = Vec::with_capacity(files.len());
    for (index, path) in files.iter().enumerate() {
        let source =
            PageSource::open(path).with_context(|| format!("открытие {}", path.display()))?;
        let fields = analysis.fields.get(index).cloned().unwrap_or_default();

        chapters.push(Chapter {
            path: path.clone(),
            number: fields.chapter,
            volume: fields.volume,
            title: fields.title,
            pages: source.page_count(),
        });
    }

    // Нумерация подряд, если попросили или если номера не распознались.
    if let Some(start) = args.start_chapter {
        for (offset, chapter) in chapters.iter_mut().enumerate() {
            chapter.number = Some(start + offset as f32);
        }
    }

    let volume = args
        .volume
        .or_else(|| chapters.iter().find_map(|c| c.volume));

    print_summary(
        &chapters,
        detected_series.as_deref(),
        volume,
        analysis.confidence,
    );

    // Спрашиваем только то, что осталось неясным.
    let (series, volume) = if args.yes {
        (
            detected_series.unwrap_or_else(|| "Без названия".to_string()),
            volume,
        )
    } else {
        confirm(detected_series, volume, &mut chapters)?
    };

    let target = match &args.output {
        Some(path) => path.clone(),
        None => {
            // Латиница в имени файла намеренно: кириллица в путях
            // переживает не всякую синхронизацию, архиватор и файловую
            // систему, а «Vol» понимают и Komga с Kavita.
            let name = match volume {
                Some(v) => format!("{series}_Vol_{v:02}.cbz"),
                None => format!("{series}.cbz"),
            };
            dir.parent().unwrap_or(dir).join(name)
        }
    };

    if target.exists() && !args.force {
        bail!(
            "файл уже существует: {}\nИспользуйте --force для перезаписи.",
            target.display()
        );
    }

    write_volume(&target, &series, volume, &chapters)?;
    Ok(())
}

fn print_summary(
    chapters: &[Chapter],
    series: Option<&str>,
    volume: Option<u16>,
    confidence: Confidence,
) {
    println!("Найдено файлов: {}\n", chapters.len());
    for chapter in chapters {
        let number = chapter
            .number
            .map(|n| format!("гл. {n}"))
            .unwrap_or_else(|| "гл. ?".to_string());
        println!(
            "  {:<10} {:>4} стр.  {}",
            number,
            chapter.pages,
            chapter.title.as_deref().unwrap_or("")
        );
    }

    println!();
    println!("Тайтл: {}", series.unwrap_or("не определён"));
    match volume {
        Some(v) => println!("Том:   {v}"),
        None => println!("Том:   не определён"),
    }

    // Уверенность показываем только когда есть повод усомниться.
    match confidence {
        Confidence::ByMagnitude => {
            println!("\nТом и глава определены по величине чисел — проверьте.");
        }
        Confidence::None => {
            println!("\nНомера глав из имён не читаются: укажите --start-chapter.");
        }
        _ => {}
    }
}

fn confirm(
    series: Option<String>,
    volume: Option<u16>,
    chapters: &mut [Chapter],
) -> Result<(String, Option<u16>)> {
    println!();

    let series = match series {
        Some(name) => {
            let answer = ask(&format!("Название тайтла [{name}]: "))?;
            if answer.is_empty() {
                name
            } else {
                answer
            }
        }
        None => {
            let answer = ask("Название тайтла: ")?;
            if answer.is_empty() {
                bail!("без названия тайтла том не собрать");
            }
            answer
        }
    };

    let volume = match volume {
        Some(v) => {
            let answer = ask(&format!("Номер тома [{v}]: "))?;
            if answer.is_empty() {
                Some(v)
            } else {
                answer.parse().ok()
            }
        }
        None => {
            let answer = ask("Номер тома [Enter — пропустить]: ")?;
            answer.parse().ok()
        }
    };

    if chapters.iter().any(|c| c.number.is_none()) {
        let answer = ask("Номера глав не определены. Первая глава [Enter — пропустить]: ")?;
        if let Ok(start) = answer.parse::<f32>() {
            for (offset, chapter) in chapters.iter_mut().enumerate() {
                chapter.number = Some(start + offset as f32);
            }
        }
    }

    let confirmed = ask("Собрать том? [Y/n]: ")?;
    if matches!(confirmed.to_lowercase().as_str(), "n" | "н" | "no" | "нет") {
        bail!("отменено");
    }

    Ok((series, volume))
}

/// Складывает главы в один архив, попутно запоминая границы.
fn write_volume(
    target: &Path,
    series: &str,
    volume: Option<u16>,
    chapters: &[Chapter],
) -> Result<()> {
    let mut pages: Vec<PagePayload> = Vec::new();
    let mut bookmarks: Vec<(u32, String)> = Vec::new();
    let mut index = 0u32;

    for chapter in chapters {
        let source = PageSource::open(&chapter.path)?;
        let fallback = format!("Глава {}", bookmarks.len() + 1);
        let label = chapter_label(chapter.number, chapter.title.as_deref(), &fallback);
        bookmarks.push((index, label));

        for page in 0..source.page_count() {
            let bytes = source.read_page(page)?;
            pages.push(PagePayload {
                index,
                extension: detect_extension(&bytes).to_string(),
                bytes,
            });
            index += 1;
        }
    }

    let meta = PackMeta {
        series: series.to_string(),
        title: None,
        number: None,
        volume,
        language: None,
        scanlator: None,
        origin: Some(format!("собрано yomi из {} глав", chapters.len())),
    };

    write_cbz_with_bookmarks(target, &meta, pages, &bookmarks)?;

    println!("\nГотово: {}", target.display());
    println!("Глав: {}, страниц: {}", chapters.len(), index);
    println!("Границы глав записаны закладками — их поймут и Komga, и Kavita, и Mihon.");
    Ok(())
}

/// Пишет CBZ и дополняет `ComicInfo.xml` закладками глав.
fn write_cbz_with_bookmarks(
    target: &Path,
    meta: &PackMeta,
    pages: Vec<PagePayload>,
    bookmarks: &[(u32, String)],
) -> Result<()> {
    write_cbz(target, meta, pages)?;
    yomi_download::package::add_bookmarks(target, bookmarks)?;
    Ok(())
}

/// Глава внутри уже собранного тома: где начинается и чем подписана.
struct ExistingChapter {
    start: usize,
    end: usize,
    label: String,
    number: Option<f32>,
}

/// Восстанавливает состав тома из закладок `ComicInfo.xml`.
fn existing_chapters(
    info: Option<&yomi_viewer::comicinfo::ComicInfo>,
    total: usize,
) -> Vec<ExistingChapter> {
    let Some(info) = info else {
        return Vec::new();
    };

    info.bookmarks
        .iter()
        .enumerate()
        .map(|(index, mark)| {
            let end = info
                .bookmarks
                .get(index + 1)
                .map(|next| next.page as usize)
                .unwrap_or(total);
            ExistingChapter {
                start: mark.page as usize,
                end,
                label: mark.title.clone(),
                number: parse::number_from_label(&mark.title),
            }
        })
        .collect()
}

fn add_to_volume(volume_path: &Path, chapter_path: &Path, args: &BuildArgs) -> Result<()> {
    if !volume_path.is_file() {
        bail!("нужен собранный том: {}", volume_path.display());
    }

    let existing = PageSource::open(volume_path)?;
    let addition = PageSource::open(chapter_path)?;
    let previous = yomi_viewer::scan::read_comicinfo(volume_path);

    let volume_number = args
        .volume
        .or_else(|| previous.as_ref().and_then(|i| i.volume));

    // Файл разбираем заново, зная номер тома: соседи по папке могли
    // называться иначе, а этот файл мог прийти откуда угодно.
    let stem = chapter_path
        .file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let fields = parse::analyze_one(&stem, volume_number);

    // Глава из другого тома — не запрет, но почти всегда оплошность.
    if let (Some(current), Some(incoming)) = (volume_number, fields.volume) {
        if current != incoming && !args.yes {
            println!(
                "Внимание: том собран как {current}, а добавляемая глава помечена томом {incoming}."
            );
            let answer = ask("Всё равно добавить? [y/N]: ")?;
            if !matches!(answer.to_lowercase().as_str(), "y" | "yes" | "д" | "да") {
                bail!("отменено");
            }
        }
    }

    let chapters = existing_chapters(previous.as_ref(), existing.page_count());
    let label = chapter_label(fields.chapter, fields.title.as_deref(), &stem);

    // Место вставки — по номеру главы, а не в конец: глава 365,
    // добавленная к тому с главами 364 и 366, обязана встать между ними.
    let position = match fields.chapter {
        Some(number) => chapters
            .iter()
            .position(|c| c.number.map(|existing| existing > number).unwrap_or(false))
            .unwrap_or(chapters.len()),
        None => chapters.len(),
    };

    if chapters.is_empty() {
        println!(
            "В томе нет разметки глав — дописываю в конец ({} + {} страниц)",
            existing.page_count(),
            addition.page_count()
        );
    } else if position == chapters.len() {
        println!("Глава встанет в конец тома");
    } else {
        println!("Глава встанет перед «{}»", chapters[position].label);
    }

    // Собираем страницы в новом порядке. Первыми идут страницы до
    // первой закладки: это обложка тома, к главам она не относится.
    let leading = chapters.first().map(|c| c.start).unwrap_or(0);
    let mut pages: Vec<PagePayload> = Vec::new();
    let mut bookmarks: Vec<(u32, String)> = Vec::new();
    let mut index = 0u32;

    let push_page = |bytes: Vec<u8>, pages: &mut Vec<PagePayload>, index: &mut u32| {
        pages.push(PagePayload {
            index: *index,
            extension: detect_extension(&bytes).to_string(),
            bytes,
        });
        *index += 1;
    };

    for page in 0..leading {
        push_page(existing.read_page(page)?, &mut pages, &mut index);
    }

    let write_addition = |pages: &mut Vec<PagePayload>,
                          bookmarks: &mut Vec<(u32, String)>,
                          index: &mut u32|
     -> Result<()> {
        bookmarks.push((*index, label.clone()));
        for page in 0..addition.page_count() {
            let bytes = addition.read_page(page)?;
            pages.push(PagePayload {
                index: *index,
                extension: detect_extension(&bytes).to_string(),
                bytes,
            });
            *index += 1;
        }
        Ok(())
    };

    for (order, chapter) in chapters.iter().enumerate() {
        if order == position {
            write_addition(&mut pages, &mut bookmarks, &mut index)?;
        }
        bookmarks.push((index, chapter.label.clone()));
        for page in chapter.start..chapter.end {
            push_page(existing.read_page(page)?, &mut pages, &mut index);
        }
    }

    if position >= chapters.len() {
        if chapters.is_empty() {
            // Тома без разметки: дописываем страницы как есть.
            for page in leading..existing.page_count() {
                push_page(existing.read_page(page)?, &mut pages, &mut index);
            }
        }
        write_addition(&mut pages, &mut bookmarks, &mut index)?;
    }

    let meta = PackMeta {
        series: args
            .series
            .clone()
            .or_else(|| previous.as_ref().and_then(|i| i.series.clone()))
            .unwrap_or_else(|| "Без названия".to_string()),
        volume: volume_number,
        origin: Some("собрано yomi".to_string()),
        ..Default::default()
    };

    let target = args
        .output
        .clone()
        .unwrap_or_else(|| volume_path.to_path_buf());
    write_cbz_with_bookmarks(&target, &meta, pages, &bookmarks)?;

    println!("Готово: {} — теперь {} страниц", target.display(), index);
    Ok(())
}
