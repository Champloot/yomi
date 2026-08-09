//! Что нам подсунули: одна глава, целый том или что-то ещё — и можно ли
//! разбить том на главы.
//!
//! # Про разбиение
//!
//! Надёжно разбить том на главы можно **только по разметке**, которая уже
//! есть в файле. Таких источников три, в порядке достоверности:
//!
//! 1. закладки `Bookmark` в `ComicInfo.xml` — штатный механизм, его
//!    проставляют Komga, Kavita и ComicRack;
//! 2. вложенные каталоги внутри архива (`Глава 01/001.jpg`);
//! 3. шаблоны в именах файлов (`c001-p012.jpg`, `ch3_05.png`).
//!
//! Угадывать границы по количеству страниц — например, резать каждые
//! двадцать — здесь сознательно не делается. Реальные главы бывают от
//! двенадцати до полусотни страниц, к тому же в томе есть обложки,
//! оглавления и послесловия. Такое деление ошибалось бы почти всегда,
//! но выглядело бы уверенно, а это хуже честного «разметки нет».

use crate::comicinfo::ComicInfo;

/// Что представляет собой файл.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// Одна глава.
    Chapter,
    /// Том — несколько глав под одной обложкой.
    Volume,
    /// Одиночное изображение.
    Single,
    /// Разобраться не удалось.
    Unknown,
}

impl FileKind {
    pub fn label_ru(&self) -> &'static str {
        match self {
            Self::Chapter => "глава",
            Self::Volume => "том",
            Self::Single => "изображение",
            Self::Unknown => "неизвестно",
        }
    }
}

/// Выше этого числа страниц файл почти наверняка том.
///
/// Ориентир: типичная глава манги — 15–45 страниц, том — 160–220.
/// Порог намеренно взят с запасом: ошибиться, назвав том главой, менее
/// обидно, чем наоборот, а при явной разметке в имени файла или
/// метаданных счётчик страниц вообще не спрашивают.
const VOLUME_PAGE_THRESHOLD: u32 = 70;

/// Определяет тип файла по трём сигналам, от достоверного к косвенному.
pub fn detect_kind(file_name: &str, page_count: u32, info: Option<&ComicInfo>) -> FileKind {
    if page_count <= 1 {
        return FileKind::Single;
    }

    let lower = file_name.to_lowercase();

    // 1. Явное указание в имени файла — самый честный сигнал: его
    //    поставил человек, который знает, что внутри.
    let says_chapter = ["глава", "гл.", "гл_", "гл ", "chapter", "ch.", "ch_"]
        .iter()
        .any(|m| lower.contains(m));
    let says_volume = ["том", "vol", "v.", "тома"]
        .iter()
        .any(|m| lower.contains(m));

    // Если сказано и то и другое («Том 3, главы 20-28»), это том.
    if says_volume {
        return FileKind::Volume;
    }
    if says_chapter {
        return FileKind::Chapter;
    }

    // 2. Метаданные: заполненный Volume без номера главы означает том.
    if let Some(info) = info {
        if !info.bookmarks.is_empty() {
            // Внутри размечено несколько глав — значит, это том.
            if info.bookmarks.len() > 1 {
                return FileKind::Volume;
            }
        }
        match (info.volume, info.number) {
            (Some(_), None) => return FileKind::Volume,
            (_, Some(_)) => return FileKind::Chapter,
            _ => {}
        }
    }

    // 3. Последний довод — объём.
    if page_count >= VOLUME_PAGE_THRESHOLD {
        FileKind::Volume
    } else {
        FileKind::Chapter
    }
}

/// Полный разбор файла: тип и найденное разбиение.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analysis {
    pub kind: FileKind,
    /// `None`, если разметки глав внутри нет.
    pub split: Option<Split>,
}

/// Разбирает файл целиком.
///
/// Предпочтительная точка входа: в отличие от [`detect_kind`], учитывает
/// найденное разбиение. Файл, внутри которого размечено несколько глав,
/// является томом независимо от имени и числа страниц — это факт о
/// содержимом, а не догадка.
pub fn analyze(file_name: &str, entries: &[String], info: Option<&ComicInfo>) -> Analysis {
    let mut kind = detect_kind(file_name, entries.len() as u32, info);
    let split = detect_chapters(entries, info);

    if let Some(split) = &split {
        if split.chapters.len() > 1 && kind != FileKind::Single {
            kind = FileKind::Volume;
        }
    }

    Analysis { kind, split }
}

/// Глава, найденная внутри тома.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InnerChapter {
    pub title: String,
    /// Номер первой страницы главы, с нуля.
    pub start_page: u32,
    pub page_count: u32,
}

/// Откуда взялось разбиение — это стоит показать пользователю, чтобы он
/// понимал, насколько результату можно доверять.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitSource {
    /// Закладки в `ComicInfo.xml`.
    Bookmarks,
    /// Вложенные каталоги внутри архива.
    Directories,
    /// Шаблон в именах файлов.
    FileNames,
}

impl SplitSource {
    pub fn label_ru(&self) -> &'static str {
        match self {
            Self::Bookmarks => "закладки ComicInfo.xml",
            Self::Directories => "каталоги внутри архива",
            Self::FileNames => "имена файлов",
        }
    }
}

/// Результат попытки разбить том на главы.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split {
    pub source: SplitSource,
    pub chapters: Vec<InnerChapter>,
}

/// Пытается разбить том на главы по имеющейся разметке.
///
/// `entries` — имена страниц в порядке чтения. Возвращает `None`, если
/// разметки нет: это честный ответ, а не повод угадывать.
pub fn detect_chapters(entries: &[String], info: Option<&ComicInfo>) -> Option<Split> {
    let total = entries.len() as u32;
    if total == 0 {
        return None;
    }

    if let Some(split) = from_bookmarks(info, total) {
        return Some(split);
    }
    if let Some(split) = from_directories(entries) {
        return Some(split);
    }
    from_file_names(entries)
}

fn from_bookmarks(info: Option<&ComicInfo>, total: u32) -> Option<Split> {
    let info = info?;
    if info.bookmarks.len() < 2 {
        return None;
    }

    let mut chapters = Vec::new();
    for (i, mark) in info.bookmarks.iter().enumerate() {
        let next = info.bookmarks.get(i + 1).map(|b| b.page).unwrap_or(total);
        chapters.push(InnerChapter {
            title: mark.title.clone(),
            start_page: mark.page,
            page_count: next.saturating_sub(mark.page),
        });
    }
    Some(Split {
        source: SplitSource::Bookmarks,
        chapters,
    })
}

/// Каталог верхнего уровня внутри архива, если он есть.
fn top_directory(entry: &str) -> Option<&str> {
    let (dir, _) = entry.split_once('/')?;
    if dir.is_empty() {
        None
    } else {
        Some(dir)
    }
}

fn from_directories(entries: &[String]) -> Option<Split> {
    // Все страницы должны лежать по каталогам, иначе это не разбиение.
    let dirs: Vec<&str> = entries.iter().filter_map(|e| top_directory(e)).collect();
    if dirs.len() != entries.len() {
        return None;
    }

    let mut chapters: Vec<InnerChapter> = Vec::new();
    for (index, dir) in dirs.iter().enumerate() {
        match chapters.last_mut() {
            Some(last) if last.title == *dir => last.page_count += 1,
            _ => chapters.push(InnerChapter {
                title: dir.to_string(),
                start_page: index as u32,
                page_count: 1,
            }),
        }
    }

    if chapters.len() < 2 {
        return None;
    }
    Some(Split {
        source: SplitSource::Directories,
        chapters,
    })
}

/// Номер главы из имени вида `c001-p012.jpg`, `ch3_05.png`, `012_005.jpg`.
fn chapter_marker(entry: &str) -> Option<u32> {
    let name = entry.rsplit('/').next().unwrap_or(entry).to_lowercase();

    for marker in ["ch", "c"] {
        // Маркер должен стоять в начале имени, иначе легко принять
        // за него случайную букву в середине названия.
        if let Some(rest) = name.strip_prefix(marker) {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                return digits.parse().ok();
            }
        }
    }
    None
}

fn from_file_names(entries: &[String]) -> Option<Split> {
    let markers: Vec<u32> = entries.iter().filter_map(|e| chapter_marker(e)).collect();
    if markers.len() != entries.len() {
        return None;
    }

    let mut chapters: Vec<InnerChapter> = Vec::new();
    let mut last_marker: Option<u32> = None;
    for (index, marker) in markers.iter().enumerate() {
        if Some(*marker) == last_marker {
            if let Some(last) = chapters.last_mut() {
                last.page_count += 1;
            }
        } else {
            chapters.push(InnerChapter {
                title: format!("Глава {marker}"),
                start_page: index as u32,
                page_count: 1,
            });
            last_marker = Some(*marker);
        }
    }

    if chapters.len() < 2 {
        return None;
    }
    Some(Split {
        source: SplitSource::FileNames,
        chapters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comicinfo::Bookmark;

    fn info_with(bookmarks: Vec<(u32, &str)>) -> ComicInfo {
        ComicInfo {
            bookmarks: bookmarks
                .into_iter()
                .map(|(page, title)| Bookmark {
                    page,
                    title: title.into(),
                })
                .collect(),
            ..Default::default()
        }
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn explicit_name_beats_page_count() {
        // 200 страниц, но в имени сказано «глава» — верим человеку.
        assert_eq!(detect_kind("Глава 5", 200, None), FileKind::Chapter);
        // И наоборот: 20 страниц, но сказано «том».
        assert_eq!(detect_kind("Том 1", 20, None), FileKind::Volume);
    }

    #[test]
    fn volume_wins_when_name_mentions_both() {
        assert_eq!(
            detect_kind("Том 3, главы 20-28", 180, None),
            FileKind::Volume
        );
    }

    #[test]
    fn large_page_count_means_volume() {
        assert_eq!(detect_kind("непонятное имя", 195, None), FileKind::Volume);
    }

    #[test]
    fn small_page_count_means_chapter() {
        assert_eq!(detect_kind("непонятное имя", 18, None), FileKind::Chapter);
    }

    #[test]
    fn single_image_is_recognised() {
        assert_eq!(detect_kind("обложка", 1, None), FileKind::Single);
    }

    #[test]
    fn several_bookmarks_mean_volume() {
        let info = info_with(vec![(0, "Глава 1"), (20, "Глава 2")]);
        assert_eq!(detect_kind("файл", 40, Some(&info)), FileKind::Volume);
    }

    #[test]
    fn metadata_volume_without_chapter_number_means_volume() {
        let info = ComicInfo {
            volume: Some(3),
            number: None,
            ..Default::default()
        };
        assert_eq!(detect_kind("файл", 30, Some(&info)), FileKind::Volume);
    }

    #[test]
    fn bookmarks_split_volume_into_chapters() {
        let info = info_with(vec![(0, "Глава 1"), (24, "Глава 2"), (50, "Глава 3")]);
        let split = detect_chapters(&names(&["a"; 70]), Some(&info)).unwrap();
        assert_eq!(split.source, SplitSource::Bookmarks);
        assert_eq!(split.chapters.len(), 3);
        assert_eq!(split.chapters[0].page_count, 24);
        assert_eq!(split.chapters[1].start_page, 24);
        assert_eq!(
            split.chapters[2].page_count, 20,
            "последняя глава — до конца тома"
        );
    }

    #[test]
    fn single_bookmark_is_not_a_split() {
        let info = info_with(vec![(0, "Начало")]);
        assert!(detect_chapters(&names(&["a", "b"]), Some(&info)).is_none());
    }

    #[test]
    fn internal_directories_split_volume() {
        let entries = names(&["Глава 01/001.jpg", "Глава 01/002.jpg", "Глава 02/001.jpg"]);
        let split = detect_chapters(&entries, None).unwrap();
        assert_eq!(split.source, SplitSource::Directories);
        assert_eq!(split.chapters.len(), 2);
        assert_eq!(split.chapters[0].page_count, 2);
        assert_eq!(split.chapters[1].start_page, 2);
    }

    #[test]
    fn flat_archive_has_no_directory_split() {
        assert!(detect_chapters(&names(&["001.jpg", "002.jpg"]), None).is_none());
    }

    #[test]
    fn file_name_patterns_split_volume() {
        let entries = names(&["c001-p001.jpg", "c001-p002.jpg", "c002-p001.jpg"]);
        let split = detect_chapters(&entries, None).unwrap();
        assert_eq!(split.source, SplitSource::FileNames);
        assert_eq!(split.chapters.len(), 2);
        assert_eq!(split.chapters[0].title, "Глава 1");
    }

    #[test]
    fn plain_numbered_pages_are_not_mistaken_for_chapters() {
        // Обычная нумерация страниц не должна выглядеть как разбиение.
        assert!(detect_chapters(&names(&["001.jpg", "002.jpg", "003.jpg"]), None).is_none());
    }

    #[test]
    fn bookmarks_take_priority_over_directories() {
        let info = info_with(vec![(0, "Первая"), (2, "Вторая")]);
        let entries = names(&["Каталог A/1.jpg", "Каталог A/2.jpg", "Каталог B/1.jpg"]);
        let split = detect_chapters(&entries, Some(&info)).unwrap();
        assert_eq!(
            split.source,
            SplitSource::Bookmarks,
            "метаданные достовернее структуры"
        );
    }

    #[test]
    fn analyze_upgrades_to_volume_when_chapters_are_found_inside() {
        // Имя ни о чём не говорит, страниц немного — но внутри три
        // размеченные главы, значит это том.
        let entries = names(&[
            "Глава 01/1.jpg",
            "Глава 01/2.jpg",
            "Глава 02/1.jpg",
            "Глава 02/2.jpg",
            "Глава 03/1.jpg",
        ]);
        let result = analyze("Произвольное имя", &entries, None);
        assert_eq!(result.kind, FileKind::Volume);
        assert_eq!(result.split.unwrap().chapters.len(), 3);
    }

    #[test]
    fn analyze_keeps_chapter_when_no_split_is_found() {
        let result = analyze("Глава 5", &names(&["1.jpg", "2.jpg"]), None);
        assert_eq!(result.kind, FileKind::Chapter);
        assert!(result.split.is_none());
    }

    #[test]
    fn analyze_does_not_turn_a_single_image_into_a_volume() {
        let result = analyze("обложка", &names(&["cover.png"]), None);
        assert_eq!(result.kind, FileKind::Single);
    }

    #[test]
    fn empty_input_yields_nothing() {
        assert!(detect_chapters(&[], None).is_none());
    }
}
