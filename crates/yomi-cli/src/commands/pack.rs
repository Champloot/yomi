//! `yomi pack` — собрать CBZ из каталога с картинками.
//!
//! Частый случай: пользователь достал сканы откуда-то, разложил их по
//! порядку и хочет получить один файл, который читается и здесь, и в
//! Komga, Kavita, Mihon. Раньше упаковка была доступна только как часть
//! загрузки из сети; теперь это самостоятельная операция.

use crate::cli::PackArgs;
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use yomi_download::package::{detect_extension, PackMeta, PagePayload};
use yomi_download::write_cbz;
use yomi_viewer::archive::PageSource;

pub async fn run(args: &PackArgs) -> Result<()> {
    if !args.path.is_dir() {
        bail!("нужен каталог с изображениями: {}", args.path.display());
    }

    // Порядок страниц берём у того же кода, что читает каталоги при
    // чтении: page2.jpg встанет перед page10.jpg, а служебные файлы
    // отсеются. Иначе порядок в архиве разошёлся бы с порядком в читалке.
    let source = PageSource::open_directory(&args.path)
        .with_context(|| format!("чтение каталога {}", args.path.display()))?;

    let dir_name = args
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "без_названия".to_string());

    let target: PathBuf = match &args.output {
        Some(path) => path.clone(),
        None => args.path.with_extension("cbz"),
    };

    if target.exists() && !args.force {
        bail!(
            "файл уже существует: {}\nИспользуйте --force для перезаписи.",
            target.display()
        );
    }

    let meta = PackMeta {
        series: args.series.clone().unwrap_or_else(|| dir_name.clone()),
        title: args.title.clone(),
        number: args.chapter,
        volume: args.volume,
        language: None,
        scanlator: None,
        origin: None,
    };

    let total = source.page_count();
    println!("Собираю {} страниц из {}", total, args.path.display());

    let mut pages = Vec::with_capacity(total);
    for index in 0..total {
        let bytes = source
            .read_page(index)
            .with_context(|| format!("чтение страницы {}", index + 1))?;
        pages.push(PagePayload {
            index: index as u32,
            extension: detect_extension(&bytes).to_string(),
            bytes,
        });
    }

    write_cbz(&target, &meta, pages)?;
    println!("Готово: {}", target.display());
    Ok(())
}
