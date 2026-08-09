//! `yomi info` — что внутри файла.
//!
//! Отвечает на два вопроса, которые иначе приходится выяснять открыв
//! файл: это том или отдельная глава, и размечены ли внутри тома главы.

use crate::cli::InfoArgs;
use anyhow::{bail, Result};
use yomi_viewer::archive::PageSource;
use yomi_viewer::structure::{self, FileKind};

pub async fn run(args: &InfoArgs) -> Result<()> {
    if !args.path.exists() {
        bail!("путь не существует: {}", args.path.display());
    }

    let source = PageSource::open(&args.path)?;
    let pages = source.page_count();
    let entries = source.entry_names();

    let file_name = args
        .path
        .file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let info = yomi_viewer::scan::read_comicinfo(&args.path);
    let analysis = structure::analyze(&file_name, &entries, info.as_ref());
    let kind = analysis.kind;

    println!("{}", args.path.display());
    println!("Тип:     {}", kind.label_ru());
    println!("Страниц: {pages}");

    if let Some(meta) = &info {
        if let Some(series) = &meta.series {
            println!("Серия:   {series}");
        }
        if let Some(volume) = meta.volume {
            println!("Том:     {volume}");
        }
        if !meta.writers.is_empty() {
            println!("Автор:   {}", meta.writers.join(", "));
        }
    } else {
        println!("Метаданных ComicInfo.xml внутри нет");
    }

    match analysis.split {
        Some(split) => {
            println!(
                "\nРазбиение на главы найдено ({}):",
                split.source.label_ru()
            );
            for chapter in &split.chapters {
                println!(
                    "  {:<28} стр. {}–{} ({})",
                    chapter.title,
                    chapter.start_page + 1,
                    chapter.start_page + chapter.page_count,
                    chapter.page_count
                );
            }
        }
        None if kind == FileKind::Volume => {
            // Важно объяснить, почему не разбили: иначе выглядит как
            // недоработка, хотя это отсутствие данных в самом файле.
            println!(
                "\nРазметки глав внутри нет: файл не содержит ни закладок\n\
                 ComicInfo.xml, ни вложенных каталогов, ни номеров глав в\n\
                 именах страниц. Определить границы по одному лишь числу\n\
                 страниц нельзя — главы бывают и по 12, и по 50 страниц."
            );
        }
        None => {}
    }

    Ok(())
}
