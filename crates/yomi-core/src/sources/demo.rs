//! Демонстрационный источник с вымышленными данными.
//!
//! Назначение: эталонный пример реализации [`Source`] и подопытный кролик
//! для тестов, которым нельзя ходить в сеть. Данные вымышлены целиком.

use crate::model::{
    Chapter, Manga, MangaStatus, Page, PageLocation, SearchQuery, SearchResult, SortBy, SourceId,
};
use crate::source::{Capabilities, Source};
use crate::{Error, Result};
use async_trait::async_trait;

pub const ID: &str = "demo";

#[derive(Debug, Default, Clone)]
pub struct DemoSource {
    catalog: Vec<Manga>,
}

impl DemoSource {
    pub fn new() -> Self {
        Self {
            catalog: sample_catalog(),
        }
    }
}

fn sample_catalog() -> Vec<Manga> {
    vec![
        Manga {
            source: SourceId::new(ID),
            id: "1".into(),
            title: "Пример первый".into(),
            alt_titles: vec!["Example One".into()],
            authors: vec!["Иванов И.".into()],
            artists: vec!["Иванов И.".into()],
            description: Some("Вымышленный тайтл для проверки вывода.".into()),
            genres: vec!["сёнэн".into(), "приключения".into()],
            status: MangaStatus::Ongoing,
            year: Some(2021),
            cover_url: None,
        },
        Manga {
            source: SourceId::new(ID),
            id: "2".into(),
            title: "Пример второй".into(),
            alt_titles: vec![],
            authors: vec!["Петрова А.".into()],
            artists: vec![],
            description: Some("Ещё один вымышленный тайтл.".into()),
            genres: vec!["сэйнэн".into(), "драма".into()],
            status: MangaStatus::Completed,
            year: Some(2018),
            cover_url: None,
        },
    ]
}

#[async_trait]
impl Source for DemoSource {
    fn id(&self) -> SourceId {
        SourceId::new(ID)
    }

    fn name(&self) -> &str {
        "Демо-источник"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            text_search: true,
            filter_by_genre: true,
            filter_by_author: false,
            filter_by_status: true,
            sorts: vec![SortBy::Relevance, SortBy::Title],
            languages: vec!["ru".into()],
            requires_auth: false,
            fragile: false,
        }
    }

    async fn search(&self, query: &SearchQuery) -> Result<SearchResult> {
        let needle = query.text.as_deref().unwrap_or_default().to_lowercase();

        let mut items: Vec<Manga> = self
            .catalog
            .iter()
            .filter(|m| needle.is_empty() || m.title.to_lowercase().contains(&needle))
            .filter(|m| {
                query.include_genres.is_empty()
                    || query.include_genres.iter().any(|g| m.genres.contains(g))
            })
            .filter(|m| query.statuses.is_empty() || query.statuses.contains(&m.status))
            .cloned()
            .collect();

        if query.sort == SortBy::Title {
            items.sort_by(|a, b| a.title.cmp(&b.title));
        }

        let total = items.len() as u32;
        Ok(SearchResult {
            items,
            total: Some(total),
            page: query.page.max(1),
        })
    }

    async fn manga(&self, id: &str) -> Result<Manga> {
        self.catalog
            .iter()
            .find(|m| m.id == id)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("тайтл {id}")))
    }

    async fn chapters(&self, manga_id: &str) -> Result<Vec<Chapter>> {
        // Проверяем, что тайтл существует.
        self.manga(manga_id).await?;
        Ok((1..=3)
            .map(|n| Chapter {
                source: SourceId::new(ID),
                id: format!("{manga_id}-{n}"),
                manga_id: manga_id.to_string(),
                number: Some(n as f32),
                volume: Some(1),
                title: Some(format!("Глава {n}")),
                language: "ru".into(),
                scanlator: Some("Вымышленная команда".into()),
                published_at: Some("2024-01-01T00:00:00Z".into()),
            })
            .collect())
    }

    async fn pages(&self, chapter_id: &str) -> Result<Vec<Page>> {
        Ok((0..5)
            .map(|i| Page {
                index: i,
                location: PageLocation::Url(format!("demo://{chapter_id}/{i}.png")),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn search_without_text_returns_whole_catalog() {
        let s = DemoSource::new();
        let r = s.search(&SearchQuery::new()).await.unwrap();
        assert_eq!(r.items.len(), 2);
        assert_eq!(r.total, Some(2));
    }

    #[tokio::test]
    async fn search_is_case_insensitive() {
        let s = DemoSource::new();
        let r = s
            .search(&SearchQuery::new().with_text("ПЕРВЫЙ"))
            .await
            .unwrap();
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.items[0].id, "1");
    }

    #[tokio::test]
    async fn genre_filter_narrows_results() {
        let s = DemoSource::new();
        let mut q = SearchQuery::new();
        q.include_genres = vec!["драма".into()];
        let r = s.search(&q).await.unwrap();
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.items[0].id, "2");
    }

    #[tokio::test]
    async fn chapters_of_missing_manga_is_not_found() {
        let s = DemoSource::new();
        let err = s.chapters("404").await.unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
    }

    #[tokio::test]
    async fn every_chapter_has_pages() {
        let s = DemoSource::new();
        let chapters = s.chapters("1").await.unwrap();
        assert_eq!(chapters.len(), 3);
        let pages = s.pages(&chapters[0].id).await.unwrap();
        assert_eq!(pages.len(), 5);
        assert_eq!(pages[0].index, 0);
    }
}
