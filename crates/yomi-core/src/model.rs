//! Доменная модель: то, чем оперирует приложение независимо от источника.
//!
//! Сознательное решение M0: даты хранятся строками в формате ISO-8601.
//! Полноценные типы времени (`time`/`chrono`) добавим на этапе M3, когда
//! появится реальный источник и станет ясно, какая точность нужна.

use serde::{Deserialize, Serialize};

/// Идентификатор источника: `mangadex`, `local`, `demo`.
/// Newtype вместо голого `String` — чтобы компилятор не дал перепутать
/// идентификатор источника с идентификатором тайтла.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceId(pub String);

impl SourceId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SourceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Статус выпуска тайтла.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MangaStatus {
    Ongoing,
    Completed,
    Hiatus,
    Cancelled,
    #[default]
    Unknown,
}

impl MangaStatus {
    /// Человекочитаемое название по-русски — для вывода в CLI и TUI.
    pub fn label_ru(&self) -> &'static str {
        match self {
            Self::Ongoing => "выходит",
            Self::Completed => "завершён",
            Self::Hiatus => "приостановлен",
            Self::Cancelled => "отменён",
            Self::Unknown => "неизвестно",
        }
    }
}

/// Тайтл.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manga {
    pub source: SourceId,
    /// Идентификатор внутри источника. Уникален только в паре с `source`.
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub alt_titles: Vec<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub artists: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub status: MangaStatus,
    #[serde(default)]
    pub year: Option<u16>,
    #[serde(default)]
    pub cover_url: Option<String>,
}

/// Глава.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chapter {
    pub source: SourceId,
    pub id: String,
    pub manga_id: String,
    /// Номер главы. `f32`, потому что бывают 10.5, 7.1 и прочие «экстры».
    #[serde(default)]
    pub number: Option<f32>,
    #[serde(default)]
    pub volume: Option<u16>,
    #[serde(default)]
    pub title: Option<String>,
    /// Код языка по ISO 639-1: `ru`, `en`.
    pub language: String,
    /// Команда перевода.
    #[serde(default)]
    pub scanlator: Option<String>,
    /// ISO-8601, см. примечание в начале модуля.
    #[serde(default)]
    pub published_at: Option<String>,
    /// Заполнено, если глава читается на стороннем сайте.
    ///
    /// Так помечены лицензированные тайтлы: сам файл источник не отдаёт,
    /// скачать главу нельзя. Молча прятать такие главы из списка —
    /// вводить в заблуждение: пользователь решит, что переводов нет.
    #[serde(default)]
    pub external_url: Option<String>,
}

impl Chapter {
    /// Можно ли скачать главу.
    pub fn is_downloadable(&self) -> bool {
        self.external_url.is_none()
    }
}

/// Страница главы: либо ссылка в сети, либо файл на диске.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    /// Порядковый номер, начиная с нуля.
    pub index: u32,
    pub location: PageLocation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageLocation {
    Url(String),
    File(std::path::PathBuf),
    /// Файл внутри архива: путь к архиву + путь внутри него.
    Archive {
        archive: std::path::PathBuf,
        entry: String,
    },
}

/// Способ сортировки результатов поиска.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    #[default]
    Relevance,
    Popularity,
    Rating,
    Updated,
    Created,
    Title,
}

/// Унифицированный поисковый запрос.
///
/// Источник может поддерживать не всё — что именно, он честно сообщает
/// через [`crate::source::Capabilities`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SearchQuery {
    pub text: Option<String>,
    pub include_genres: Vec<String>,
    pub exclude_genres: Vec<String>,
    pub authors: Vec<String>,
    pub statuses: Vec<MangaStatus>,
    pub languages: Vec<String>,
    pub sort: SortBy,
    /// Нумерация с единицы.
    pub page: u32,
    pub per_page: u32,
}

impl SearchQuery {
    pub fn new() -> Self {
        Self {
            page: 1,
            per_page: 20,
            ..Default::default()
        }
    }
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }
}

/// Страница результатов поиска.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub items: Vec<Manga>,
    /// Всего найдено, если источник сообщает.
    pub total: Option<u32>,
    pub page: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_defaults_to_unknown() {
        assert_eq!(MangaStatus::default(), MangaStatus::Unknown);
    }

    #[test]
    fn search_query_builder_sets_sane_pagination() {
        let q = SearchQuery::new().with_text("берсерк");
        assert_eq!(q.page, 1);
        assert_eq!(q.per_page, 20);
        assert_eq!(q.text.as_deref(), Some("берсерк"));
    }
}
