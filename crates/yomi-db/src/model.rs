//! Типы, которыми оперирует хранилище.
//!
//! Это не копия `yomi_core::model`: там доменные сущности источника,
//! здесь — записи библиотеки с идентификаторами базы и прогрессом.

use std::path::PathBuf;

/// Источник для файлов на диске.
pub const LOCAL_SOURCE: &str = "local";

/// Тайтл в библиотеке.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryManga {
    pub id: i64,
    pub source: String,
    /// Для локальных — путь к каталогу тайтла.
    pub external_id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub genres: Vec<String>,
    pub status: String,
    pub year: Option<u16>,
    pub description: Option<String>,
}

/// Глава в библиотеке.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryChapter {
    pub id: i64,
    pub manga_id: i64,
    /// Для локальных — путь к CBZ или каталогу главы.
    pub external_id: String,
    pub number: Option<f32>,
    pub volume: Option<u16>,
    pub title: Option<String>,
    pub language: String,
    pub scanlator: Option<String>,
    pub page_count: Option<u32>,
    /// «chapter», «volume», «single» или «unknown».
    pub kind: String,
}

impl LibraryChapter {
    pub fn path(&self) -> PathBuf {
        PathBuf::from(&self.external_id)
    }

    /// Человекочитаемое обозначение главы для списков.
    pub fn label(&self) -> String {
        match (self.volume, self.number, self.title.as_deref()) {
            (Some(v), Some(n), Some(t)) => format!("т.{v} гл.{n} — {t}"),
            (Some(v), Some(n), None) => format!("т.{v} гл.{n}"),
            (None, Some(n), Some(t)) => format!("гл.{n} — {t}"),
            (None, Some(n), None) => format!("гл.{n}"),
            // Том без номера главы — обычный случай для CBZ, собранных
            // по томам: показываем «т.3», а не имя файла.
            (Some(v), None, None) => format!("т.{v}"),
            (Some(v), None, Some(t)) => format!("т.{v} — {t}"),
            (None, None, Some(t)) => t.to_string(),
            (None, None, None) => self
                .path()
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| self.external_id.clone()),
        }
    }
}

/// Прогресс чтения главы.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub chapter_id: i64,
    /// Номер страницы с нуля.
    pub page: u32,
    pub total_pages: Option<u32>,
    pub completed: bool,
}

/// Что записать в библиотеку при сканировании: тайтл вместе с главами.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedManga {
    pub external_id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub genres: Vec<String>,
    pub status: String,
    pub year: Option<u16>,
    pub description: Option<String>,
    pub chapters: Vec<ScannedChapter>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScannedChapter {
    pub external_id: String,
    pub number: Option<f32>,
    pub volume: Option<u16>,
    pub title: Option<String>,
    pub language: String,
    pub scanlator: Option<String>,
    pub page_count: Option<u32>,
    pub kind: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter(volume: Option<u16>, number: Option<f32>, title: Option<&str>) -> LibraryChapter {
        LibraryChapter {
            id: 1,
            manga_id: 1,
            external_id: "/манга/Тайтл/файл.cbz".into(),
            number,
            volume,
            title: title.map(str::to_string),
            language: "ru".into(),
            scanlator: None,
            page_count: None,
            kind: "chapter".into(),
        }
    }

    #[test]
    fn label_prefers_volume_and_number() {
        assert_eq!(chapter(Some(3), Some(12.0), None).label(), "т.3 гл.12");
        assert_eq!(
            chapter(Some(3), Some(12.5), Some("Финал")).label(),
            "т.3 гл.12.5 — Финал"
        );
    }

    #[test]
    fn label_falls_back_to_file_name_when_nothing_is_known() {
        assert_eq!(chapter(None, None, None).label(), "файл.cbz");
    }

    #[test]
    fn volume_without_chapter_number_is_shown_as_volume() {
        // Частый случай: CBZ собран по томам, номера главы внутри нет.
        assert_eq!(chapter(Some(3), None, None).label(), "т.3");
        assert_eq!(
            chapter(Some(3), None, Some("Экстра")).label(),
            "т.3 — Экстра"
        );
    }

    #[test]
    fn label_uses_title_when_number_is_missing() {
        assert_eq!(chapter(None, None, Some("Экстра")).label(), "Экстра");
    }
}
