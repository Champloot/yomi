//! Сканирование каталога с мангой.
//!
//! Ожидаемая раскладка — та, что складывается сама собой у большинства:
//!
//! ```text
//! ~/Манга/
//! ├── Усогуй/            <- тайтл
//! │   ├── Том 1.cbz      <- глава
//! │   └── Том 2.cbz
//! └── Другой тайтл/
//!     └── Глава 1/       <- глава может быть и каталогом с картинками
//! ```
//!
//! Отдельно поддержан случай «CBZ прямо в корне сканируемого каталога»:
//! тогда тайтл берётся из имени файла. Люди хранят мангу по-разному, и
//! отказываться сканировать из-за неподходящей раскладки — плохой обмен.

use crate::archive::PageSource;
use crate::comicinfo::{self, ComicInfo};
use crate::natural_sort;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Найденная глава.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedChapter {
    pub path: PathBuf,
    pub number: Option<f32>,
    pub volume: Option<u16>,
    pub title: Option<String>,
    pub language: String,
    pub scanlator: Option<String>,
    pub page_count: Option<u32>,
}

/// Найденный тайтл.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedManga {
    pub path: PathBuf,
    pub title: String,
    pub authors: Vec<String>,
    pub genres: Vec<String>,
    pub year: Option<u16>,
    pub description: Option<String>,
    pub chapters: Vec<ScannedChapter>,
}

fn is_cbz(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_lowercase().as_str(), "cbz" | "zip"))
        .unwrap_or(false)
}

/// Достаёт `ComicInfo.xml` из CBZ, если он там есть.
fn read_comicinfo(cbz: &Path) -> Option<ComicInfo> {
    let file = std::fs::File::open(cbz).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    // Имя встречается в разном регистре — ищем без учёта регистра.
    let name = archive
        .file_names()
        .find(|n| n.eq_ignore_ascii_case("ComicInfo.xml"))
        .map(str::to_string)?;

    let mut entry = archive.by_name(&name).ok()?;
    let mut xml = String::new();
    entry.read_to_string(&mut xml).ok()?;
    Some(comicinfo::parse(&xml))
}

/// Вытаскивает номер тома и главы из имени файла.
///
/// Работает по частым шаблонам: `Том 3`, `v03`, `Vol.3`, `гл 12`, `ch12`,
/// `_VOL-32`. Если ничего не нашлось, номер останется `None` — это не
/// ошибка, порядок тогда определится естественной сортировкой имён.
fn guess_numbers(file_name: &str) -> (Option<u16>, Option<f32>) {
    let lower = file_name.to_lowercase();

    let find_after = |markers: &[&str]| -> Option<f32> {
        for marker in markers {
            let mut from = 0usize;
            while let Some(pos) = lower[from..].find(marker) {
                let start = from + pos + marker.len();
                let rest = lower[start..].trim_start_matches([' ', '.', '-', '_', '#']);
                let digits: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                let digits = digits.trim_end_matches('.');
                if let Ok(v) = digits.parse::<f32>() {
                    return Some(v);
                }
                from = start;
            }
        }
        None
    };

    let volume = find_after(&["том", "vol", "v"]).map(|v| v as u16);
    let chapter = find_after(&["глава", "гл", "chapter", "ch"]);
    (volume, chapter)
}

fn chapter_from_path(path: &Path) -> ScannedChapter {
    let file_name = path
        .file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let info = if is_cbz(path) {
        read_comicinfo(path)
    } else {
        None
    };
    let (guessed_volume, guessed_number) = guess_numbers(&file_name);

    // Число страниц считаем сами: значение из ComicInfo часто врёт,
    // а открыть архив мы всё равно можем.
    let page_count = PageSource::open(path).ok().map(|s| s.page_count() as u32);

    match info {
        Some(info) => ScannedChapter {
            path: path.to_path_buf(),
            // Метаданные из файла достовернее догадок по имени.
            number: info.number.or(guessed_number),
            volume: info.volume.or(guessed_volume),
            title: info.title,
            language: info.language.unwrap_or_else(|| "ru".to_string()),
            scanlator: info.scanlator,
            page_count: page_count.or(info.page_count),
        },
        None => ScannedChapter {
            path: path.to_path_buf(),
            number: guessed_number,
            volume: guessed_volume,
            title: None,
            language: "ru".to_string(),
            scanlator: None,
            page_count,
        },
    }
}

/// Содержит ли каталог картинки напрямую — то есть является ли он главой.
fn looks_like_chapter_dir(dir: &Path) -> bool {
    PageSource::open_directory(dir).is_ok()
}

/// Собирает тайтл из каталога.
fn manga_from_dir(dir: &Path) -> Option<ScannedManga> {
    let mut chapter_paths: Vec<PathBuf> = Vec::new();

    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if looks_like_chapter_dir(&path) {
                chapter_paths.push(path);
            }
        } else if is_cbz(&path) {
            chapter_paths.push(path);
        }
    }

    if chapter_paths.is_empty() {
        return None;
    }

    natural_sort::sort(&mut chapter_paths, |p| {
        p.file_name().and_then(|n| n.to_str()).unwrap_or("")
    });

    let chapters: Vec<ScannedChapter> =
        chapter_paths.iter().map(|p| chapter_from_path(p)).collect();

    // Метаданные тайтла берём из первой главы, где они есть: название
    // серии, авторы и жанры повторяются во всех томах.
    let series_info = chapter_paths
        .iter()
        .filter(|p| is_cbz(p))
        .find_map(|p| read_comicinfo(p));

    let dir_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| dir.display().to_string());

    let (title, authors, genres, year, description) = match series_info {
        Some(info) => {
            let mut authors = info.writers;
            for artist in info.pencillers {
                if !authors.contains(&artist) {
                    authors.push(artist);
                }
            }
            (
                info.series.unwrap_or(dir_name),
                authors,
                info.genres,
                info.year,
                info.summary,
            )
        }
        None => (dir_name, Vec::new(), Vec::new(), None, None),
    };

    Some(ScannedManga {
        path: dir.to_path_buf(),
        title,
        authors,
        genres,
        year,
        description,
        chapters,
    })
}

/// Обходит каталог библиотеки и возвращает найденные тайтлы.
///
/// Вложенность — один уровень: `корень/Тайтл/главы`. Рекурсия вглубь
/// намеренно не делается, иначе на больших коллекциях сканирование
/// расползается по всей файловой системе.
pub fn scan_library(root: &Path) -> Vec<ScannedManga> {
    let mut found = Vec::new();

    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(path = %root.display(), error = %e, "каталог недоступен");
            return found;
        }
    };

    let mut loose_cbz: Vec<PathBuf> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(manga) = manga_from_dir(&path) {
                found.push(manga);
            }
        } else if is_cbz(&path) {
            loose_cbz.push(path);
        }
    }

    // CBZ, лежащие прямо в корне: каждый становится отдельным тайтлом.
    for cbz in loose_cbz {
        let name = cbz
            .file_stem()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let info = read_comicinfo(&cbz);
        let chapter = chapter_from_path(&cbz);
        found.push(ScannedManga {
            path: cbz.clone(),
            title: info.as_ref().and_then(|i| i.series.clone()).unwrap_or(name),
            authors: info.as_ref().map(|i| i.writers.clone()).unwrap_or_default(),
            genres: info.as_ref().map(|i| i.genres.clone()).unwrap_or_default(),
            year: info.as_ref().and_then(|i| i.year),
            description: info.and_then(|i| i.summary),
            chapters: vec![chapter],
        });
    }

    natural_sort::sort(&mut found, |m| m.title.as_str());
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn png_bytes() -> Vec<u8> {
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::new(4, 4));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    fn make_cbz(path: &Path, pages: usize, comicinfo: Option<&str>) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::FileOptions::default();
        for i in 0..pages {
            zip.start_file(format!("{i:03}.png"), opts).unwrap();
            zip.write_all(&png_bytes()).unwrap();
        }
        if let Some(xml) = comicinfo {
            zip.start_file("ComicInfo.xml", opts).unwrap();
            zip.write_all(xml.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn guesses_volume_and_chapter_from_common_patterns() {
        assert_eq!(guess_numbers("Usogui_VOL-32").0, Some(32));
        assert_eq!(guess_numbers("Том 3").0, Some(3));
        assert_eq!(guess_numbers("v05").0, Some(5));
        assert_eq!(guess_numbers("глава 12").1, Some(12.0));
        assert_eq!(guess_numbers("chapter 7.5").1, Some(7.5));
    }

    #[test]
    fn unparseable_names_yield_no_numbers() {
        assert_eq!(guess_numbers("просто имя"), (None, None));
    }

    #[test]
    fn scans_title_directory_with_cbz_chapters() {
        let root = tempfile::tempdir().unwrap();
        let title_dir = root.path().join("Усогуй");
        std::fs::create_dir(&title_dir).unwrap();
        make_cbz(&title_dir.join("Том 1.cbz"), 3, None);
        make_cbz(&title_dir.join("Том 2.cbz"), 4, None);

        let found = scan_library(root.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Усогуй");
        assert_eq!(found[0].chapters.len(), 2);
        assert_eq!(found[0].chapters[0].volume, Some(1));
        assert_eq!(found[0].chapters[0].page_count, Some(3));
    }

    #[test]
    fn comicinfo_overrides_guesses_from_file_name() {
        let root = tempfile::tempdir().unwrap();
        let title_dir = root.path().join("Папка с непонятным именем");
        std::fs::create_dir(&title_dir).unwrap();
        make_cbz(
            &title_dir.join("файл.cbz"),
            2,
            Some("<ComicInfo><Series>Настоящее название</Series><Volume>7</Volume><Genre>драма</Genre><Writer>Автор</Writer></ComicInfo>"),
        );

        let found = scan_library(root.path());
        assert_eq!(found[0].title, "Настоящее название");
        assert_eq!(found[0].genres, vec!["драма"]);
        assert_eq!(found[0].authors, vec!["Автор"]);
        assert_eq!(found[0].chapters[0].volume, Some(7));
    }

    #[test]
    fn directory_of_images_counts_as_a_chapter() {
        let root = tempfile::tempdir().unwrap();
        let title_dir = root.path().join("Тайтл");
        let chapter_dir = title_dir.join("Глава 1");
        std::fs::create_dir_all(&chapter_dir).unwrap();
        std::fs::write(chapter_dir.join("001.png"), png_bytes()).unwrap();
        std::fs::write(chapter_dir.join("002.png"), png_bytes()).unwrap();

        let found = scan_library(root.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].chapters.len(), 1);
        assert_eq!(found[0].chapters[0].page_count, Some(2));
    }

    #[test]
    fn loose_cbz_in_root_becomes_its_own_title() {
        let root = tempfile::tempdir().unwrap();
        make_cbz(&root.path().join("Одиночный том.cbz"), 2, None);

        let found = scan_library(root.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Одиночный том");
    }

    #[test]
    fn empty_and_junk_directories_are_skipped() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("пустая")).unwrap();
        let junk = root.path().join("мусор");
        std::fs::create_dir(&junk).unwrap();
        std::fs::write(junk.join("заметки.txt"), b"x").unwrap();

        assert!(scan_library(root.path()).is_empty());
    }

    #[test]
    fn missing_root_does_not_panic() {
        assert!(scan_library(Path::new("/такого/пути/нет")).is_empty());
    }

    #[test]
    fn results_are_sorted_naturally_by_title() {
        let root = tempfile::tempdir().unwrap();
        for name in ["Тайтл 10", "Тайтл 2"] {
            let dir = root.path().join(name);
            std::fs::create_dir(&dir).unwrap();
            make_cbz(&dir.join("1.cbz"), 1, None);
        }
        let found = scan_library(root.path());
        assert_eq!(found[0].title, "Тайтл 2");
        assert_eq!(found[1].title, "Тайтл 10");
    }
}
