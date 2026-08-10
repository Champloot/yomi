//! Структуры ответов API MangaDex и перевод их в доменную модель.
//!
//! Разбор отделён от сети намеренно: именно он ломается, когда сервис
//! меняет формат, и именно его нужно уметь проверять тестами на
//! записанных ответах, без обращения к живому API.
//!
//! Подход снисходительный: незнакомые поля игнорируются, отсутствующие
//! необязательные — дают `None`. Строгий разбор означал бы, что
//! добавление сервисом нового поля ломает нам весь поиск.

use serde::Deserialize;
use std::collections::HashMap;
use yomi_core::model::{Chapter, Manga, MangaStatus, SourceId};

pub const ID: &str = "mangadex";

/// Обёртка над списком сущностей.
#[derive(Debug, Deserialize)]
pub struct ListResponse<T> {
    pub data: Vec<T>,
    #[serde(default)]
    pub total: Option<u32>,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub offset: Option<u32>,
}

/// Обёртка над одной сущностью.
#[derive(Debug, Deserialize)]
pub struct EntityResponse<T> {
    pub data: T,
}

#[derive(Debug, Deserialize)]
pub struct MangaEntity {
    pub id: String,
    pub attributes: MangaAttributes,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
}

/// Локализованный текст: ключ — код языка.
type Localized = HashMap<String, String>;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MangaAttributes {
    #[serde(default)]
    pub title: Localized,
    #[serde(default)]
    pub alt_titles: Vec<Localized>,
    #[serde(default)]
    pub description: Localized,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub year: Option<u16>,
    #[serde(default)]
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub original_language: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Tag {
    pub attributes: TagAttributes,
}

#[derive(Debug, Deserialize)]
pub struct TagAttributes {
    #[serde(default)]
    pub name: Localized,
    #[serde(default)]
    pub group: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Relationship {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub attributes: Option<RelationshipAttributes>,
}

#[derive(Debug, Deserialize)]
pub struct RelationshipAttributes {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChapterEntity {
    pub id: String,
    pub attributes: ChapterAttributes,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterAttributes {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub volume: Option<String>,
    #[serde(default)]
    pub chapter: Option<String>,
    #[serde(default)]
    pub translated_language: Option<String>,
    /// Число страниц по данным сервиса.
    #[serde(default)]
    pub pages: Option<u32>,
    #[serde(default)]
    pub publish_at: Option<String>,
    /// Заполнено у глав, которые читаются на стороннем сайте:
    /// страниц у нас для них не будет.
    #[serde(default)]
    pub external_url: Option<String>,
}

/// Ответ `/at-home/server/:id` — откуда качать страницы.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtHomeResponse {
    pub base_url: String,
    pub chapter: AtHomeChapter,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtHomeChapter {
    pub hash: String,
    /// Имена файлов в исходном качестве, по порядку.
    #[serde(default)]
    pub data: Vec<String>,
    /// Имена файлов в сжатом качестве.
    #[serde(default)]
    pub data_saver: Vec<String>,
}

impl AtHomeResponse {
    /// Собирает адреса страниц.
    ///
    /// Формат: `baseUrl / качество / hash / имя файла`. Базовый адрес
    /// подставляется как есть: документация прямо предупреждает, что это
    /// произвольная строка, а не обязательно домен, и что хардкодить её
    /// нельзя — она подбирается под геолокацию и живёт около 15 минут.
    pub fn page_urls(&self, data_saver: bool) -> Vec<String> {
        let (quality, files) = if data_saver {
            ("data-saver", &self.chapter.data_saver)
        } else {
            ("data", &self.chapter.data)
        };
        let base = self.base_url.trim_end_matches('/');
        files
            .iter()
            .map(|name| format!("{base}/{quality}/{}/{name}", self.chapter.hash))
            .collect()
    }
}

/// Выбирает текст на самом подходящем языке.
///
/// Порядок предпочтений задаёт пользователь через конфигурацию; если
/// ничего не подошло, берём английский, а затем что угодно — пустое
/// название хуже названия не на том языке.
pub fn pick_localized(map: &Localized, preferred: &[String]) -> Option<String> {
    for lang in preferred {
        if let Some(value) = map.get(lang) {
            return Some(value.clone());
        }
    }
    if let Some(value) = map.get("en") {
        return Some(value.clone());
    }
    map.values().next().cloned()
}

fn parse_status(raw: Option<&str>) -> MangaStatus {
    match raw {
        Some("ongoing") => MangaStatus::Ongoing,
        Some("completed") => MangaStatus::Completed,
        Some("hiatus") => MangaStatus::Hiatus,
        Some("cancelled") => MangaStatus::Cancelled,
        _ => MangaStatus::Unknown,
    }
}

fn names_of(relationships: &[Relationship], kind: &str) -> Vec<String> {
    relationships
        .iter()
        .filter(|r| r.kind == kind)
        .filter_map(|r| r.attributes.as_ref()?.name.clone())
        .collect()
}

impl MangaEntity {
    pub fn into_domain(self, preferred: &[String]) -> Manga {
        let attrs = self.attributes;

        let title =
            pick_localized(&attrs.title, preferred).unwrap_or_else(|| "Без названия".to_string());

        let alt_titles = attrs
            .alt_titles
            .iter()
            .filter_map(|m| pick_localized(m, preferred))
            .filter(|t| *t != title)
            .collect();

        let genres = attrs
            .tags
            .iter()
            .filter_map(|t| pick_localized(&t.attributes.name, preferred))
            .collect();

        Manga {
            source: SourceId::new(ID),
            id: self.id,
            title,
            alt_titles,
            authors: names_of(&self.relationships, "author"),
            artists: names_of(&self.relationships, "artist"),
            description: pick_localized(&attrs.description, preferred),
            genres,
            status: parse_status(attrs.status.as_deref()),
            year: attrs.year,
            cover_url: None,
        }
    }
}

impl ChapterEntity {
    pub fn into_domain(self, manga_id: &str) -> Chapter {
        let attrs = self.attributes;
        Chapter {
            source: SourceId::new(ID),
            id: self.id,
            manga_id: manga_id.to_string(),
            // Номера приходят строками: бывают «10.5» и вовсе пустые.
            number: attrs.chapter.as_deref().and_then(|v| v.parse().ok()),
            volume: attrs.volume.as_deref().and_then(|v| v.parse().ok()),
            title: attrs.title.filter(|t| !t.trim().is_empty()),
            language: attrs
                .translated_language
                .unwrap_or_else(|| "en".to_string()),
            scanlator: names_of(&self.relationships, "scanlation_group")
                .into_iter()
                .next(),
            published_at: attrs.publish_at,
            external_url: attrs.external_url,
        }
    }
}
