//! `yomi marks` — отметки начала глав внутри файла.
//!
//! Ручная разметка надёжнее любой автоматики: человек размечает один раз
//! и точно. Отмечать удобнее прямо при чтении клавишей `m`, а эта
//! команда нужна для остального — посмотреть результат, исправить
//! опечатку, заполнить границы автоматикой перед ручной правкой.

use crate::cli::MarksCommand;
use anyhow::{bail, Context, Result};
use std::path::Path;
use yomi_db::Store;
use yomi_viewer::archive::PageSource;
use yomi_viewer::structure;

pub async fn run(cmd: &MarksCommand) -> Result<()> {
    match cmd {
        MarksCommand::List { path } => list(path),
        MarksCommand::Add { path, page, title } => add(path, *page, title.as_deref()),
        MarksCommand::Remove { path, page } => remove(path, *page),
        MarksCommand::Detect { path, deep, force } => detect(path, *deep, *force),
        MarksCommand::Clear { path } => clear(path),
    }
}

/// Находит файл в библиотеке.
///
/// Отметки привязаны к записи библиотеки: хранить их для произвольного
/// пути было бы негде, а молча ничего не делать — хуже, чем объяснить.
fn locate(path: &Path) -> Result<(Store, i64)> {
    let db_path = yomi_core::paths::database_file()?;
    if !db_path.exists() {
        bail!("библиотека пуста: сначала `yomi library scan КАТАЛОГ`");
    }

    let store = Store::open(&db_path).with_context(|| format!("открытие {}", db_path.display()))?;

    // Путь должен совпадать с записанным при сканировании.
    let canonical = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let found = store
        .chapter_by_path(&canonical)?
        .or(store.chapter_by_path(&path.to_string_lossy())?);

    match found {
        Some(chapter) => Ok((store, chapter.id)),
        None => bail!(
            "файла нет в библиотеке: {}\n\
             Добавьте его каталог: `yomi library scan КАТАЛОГ`",
            path.display()
        ),
    }
}

fn total_pages(path: &Path) -> Option<usize> {
    PageSource::open(path).ok().map(|s| s.page_count())
}

fn list(path: &Path) -> Result<()> {
    let (store, chapter_id) = locate(path)?;
    let marks = store.marks_of(chapter_id)?;

    if marks.is_empty() {
        println!(
            "Отметок нет.\n\
             Расставьте при чтении клавишей `m` или попробуйте\n\
             `yomi marks detect {} --deep`",
            path.display()
        );
        return Ok(());
    }

    let total = total_pages(path);
    for (index, mark) in marks.iter().enumerate() {
        // Конец главы — страница перед следующей отметкой.
        let end = marks
            .get(index + 1)
            .map(|next| next.page as usize)
            .or(total)
            .unwrap_or(mark.page as usize + 1);
        println!(
            "{:>4}  {:<28} стр. {}–{}",
            index + 1,
            mark.label(index),
            mark.page + 1,
            end
        );
    }
    println!("\nВсего глав: {}", marks.len());
    Ok(())
}

/// Переводит номер страницы из человеческого в внутренний.
fn to_index(page: u32) -> Result<u32> {
    if page == 0 {
        bail!("страницы нумеруются с единицы");
    }
    Ok(page - 1)
}

fn add(path: &Path, page: u32, title: Option<&str>) -> Result<()> {
    let index = to_index(page)?;
    let (store, chapter_id) = locate(path)?;

    if let Some(total) = total_pages(path) {
        if index as usize >= total {
            bail!("в файле {total} страниц, страницы {page} нет");
        }
    }

    store.add_mark(chapter_id, index, title)?;
    println!("Отмечено начало главы на странице {page}");
    Ok(())
}

fn remove(path: &Path, page: u32) -> Result<()> {
    let index = to_index(page)?;
    let (store, chapter_id) = locate(path)?;

    if store.remove_mark(chapter_id, index)? {
        println!("Отметка со страницы {page} снята");
    } else {
        println!("На странице {page} отметки не было");
    }
    Ok(())
}

fn clear(path: &Path) -> Result<()> {
    let (store, chapter_id) = locate(path)?;
    let removed = store.clear_marks(chapter_id)?;
    println!("Снято отметок: {removed}");
    Ok(())
}

fn detect(path: &Path, deep: bool, force: bool) -> Result<()> {
    let (store, chapter_id) = locate(path)?;

    let existing = store.marks_of(chapter_id)?;
    if !existing.is_empty() && !force {
        bail!(
            "уже есть отметок: {}. Используйте --force, чтобы заменить их",
            existing.len()
        );
    }

    let source = PageSource::open(path)?;
    let entries = source.entry_names();
    let info = yomi_viewer::scan::read_comicinfo(path);

    let split = match structure::detect_chapters(&entries, info.as_ref()) {
        Some(split) => Some(split),
        None if deep => {
            println!("Разметки нет, читаю размеры страниц...");
            structure::detect_by_page_shape(&source.page_shapes())
        }
        None => None,
    };

    let Some(split) = split else {
        bail!(
            "границы глав определить не удалось.\n\
             Попробуйте --deep, если главы начинаются с разворота,\n\
             либо расставьте отметки вручную клавишей `m` при чтении"
        );
    };

    if force {
        store.clear_marks(chapter_id)?;
    }

    for chapter in &split.chapters {
        store.add_mark(chapter_id, chapter.start_page, Some(&chapter.title))?;
    }

    println!(
        "Расставлено отметок: {} (источник: {})",
        split.chapters.len(),
        split.source.label_ru()
    );
    if split.source == structure::SplitSource::PageShape {
        println!("Это догадка по содержимому — проверьте границы и поправьте при чтении.");
    }
    Ok(())
}
