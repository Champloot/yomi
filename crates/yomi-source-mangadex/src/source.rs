//! Реализация трейта [`Source`] поверх API MangaDex.

use crate::api::{self, AtHomeResponse, ChapterEntity, EntityResponse, ListResponse, MangaEntity};
use crate::client::{MangaDexClient, API_BASE};
use async_trait::async_trait;
use std::time::Duration;
use yomi_core::model::{
    Chapter, Manga, MangaStatus, Page, PageLocation, SearchQuery, SearchResult, SortBy, SourceId,
};
use yomi_core::source::{Capabilities, Source};
use yomi_core::{Error, Result};

pub struct MangaDexSource {
    client: MangaDexClient,
    /// Языки контента в порядке предпочтения — из конфигурации.
    languages: Vec<String>,
    data_saver: bool,
}

impl MangaDexSource {
    pub fn new(user_agent: &str, timeout: Duration, languages: Vec<String>) -> Result<Self> {
        Ok(Self {
            client: MangaDexClient::new(user_agent, timeout)?,
            languages: if languages.is_empty() {
                vec!["ru".to_string(), "en".to_string()]
            } else {
                languages
            },
            data_saver: false,
        })
    }

    /// Переключает загрузку на сжатое качество.
    pub fn with_data_saver(mut self, enabled: bool) -> Self {
        self.data_saver = enabled;
        self
    }

    pub fn client(&self) -> &MangaDexClient {
        &self.client
    }
}

/// Собирает строку запроса для поиска.
///
/// Вынесено из сетевого кода, чтобы проверять тестами: ошибка в
/// параметрах даёт не сбой, а тихо неверную выдачу — худший вид дефекта.
pub fn build_search_url(query: &SearchQuery, languages: &[String]) -> String {
    let mut params: Vec<String> = Vec::new();

    if let Some(text) = &query.text {
        if !text.trim().is_empty() {
            params.push(format!("title={}", urlencode(text)));
        }
    }

    let limit = query.per_page.clamp(1, 100);
    let page = query.page.max(1);
    params.push(format!("limit={limit}"));
    params.push(format!("offset={}", (page - 1) * limit));

    for lang in languages {
        params.push(format!("availableTranslatedLanguage[]={}", urlencode(lang)));
    }

    for status in &query.statuses {
        if let Some(value) = status_param(*status) {
            params.push(format!("status[]={value}"));
        }
    }

    // Порядок сортировки: у API он задаётся вложенным параметром.
    let order = match query.sort {
        SortBy::Relevance => "order[relevance]=desc",
        SortBy::Popularity => "order[followedCount]=desc",
        SortBy::Rating => "order[rating]=desc",
        SortBy::Updated => "order[latestUploadedChapter]=desc",
        SortBy::Created => "order[createdAt]=desc",
        SortBy::Title => "order[title]=asc",
    };
    params.push(order.to_string());

    // Автор и художник нужны, чтобы показать их в выдаче: иначе
    // пришлось бы делать отдельный запрос на каждый тайтл.
    params.push("includes[]=author".to_string());
    params.push("includes[]=artist".to_string());

    format!("{API_BASE}/manga?{}", params.join("&"))
}

pub fn build_feed_url(manga_id: &str, languages: &[String], offset: u32) -> String {
    let mut params = vec![
        "limit=100".to_string(),
        format!("offset={offset}"),
        "order[volume]=asc".to_string(),
        "order[chapter]=asc".to_string(),
        "includes[]=scanlation_group".to_string(),
    ];
    for lang in languages {
        params.push(format!("translatedLanguage[]={}", urlencode(lang)));
    }
    format!("{API_BASE}/manga/{manga_id}/feed?{}", params.join("&"))
}

fn status_param(status: MangaStatus) -> Option<&'static str> {
    match status {
        MangaStatus::Ongoing => Some("ongoing"),
        MangaStatus::Completed => Some("completed"),
        MangaStatus::Hiatus => Some("hiatus"),
        MangaStatus::Cancelled => Some("cancelled"),
        MangaStatus::Unknown => None,
    }
}

/// Кодирование для строки запроса.
///
/// Своя реализация вместо зависимости: правило простое, а лишний крейт
/// ради десяти строк не окупается.
fn urlencode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push_str("%20"),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[async_trait]
impl Source for MangaDexSource {
    fn id(&self) -> SourceId {
        SourceId::new(api::ID)
    }

    fn name(&self) -> &str {
        "MangaDex"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            text_search: true,
            filter_by_genre: false, // фильтр по тегам требует их идентификаторов, см. M5
            filter_by_author: false,
            filter_by_status: true,
            sorts: vec![
                SortBy::Relevance,
                SortBy::Popularity,
                SortBy::Rating,
                SortBy::Updated,
                SortBy::Created,
                SortBy::Title,
            ],
            languages: self.languages.clone(),
            requires_auth: false,
            fragile: false,
        }
    }

    async fn search(&self, query: &SearchQuery) -> Result<SearchResult> {
        let url = build_search_url(query, &self.languages);
        let response: ListResponse<MangaEntity> = self.client.get_json(&url).await?;

        Ok(SearchResult {
            items: response
                .data
                .into_iter()
                .map(|m| m.into_domain(&self.languages))
                .collect(),
            total: response.total,
            page: query.page.max(1),
        })
    }

    async fn manga(&self, id: &str) -> Result<Manga> {
        let url = format!("{API_BASE}/manga/{id}?includes[]=author&includes[]=artist");
        let response: EntityResponse<MangaEntity> = self.client.get_json(&url).await?;
        Ok(response.data.into_domain(&self.languages))
    }

    async fn chapters(&self, manga_id: &str) -> Result<Vec<Chapter>> {
        let mut all = Vec::new();
        let mut offset = 0u32;

        // Лента отдаётся страницами по сто; тайтлы с тысячей глав
        // существуют, поэтому идём до конца.
        loop {
            let url = build_feed_url(manga_id, &self.languages, offset);
            let response: ListResponse<ChapterEntity> = self.client.get_json(&url).await?;

            let received = response.data.len() as u32;
            all.extend(
                response
                    .data
                    .into_iter()
                    // Главы, читаемые на стороннем сайте, скачать нельзя:
                    // показывать их в списке — вводить в заблуждение.
                    .filter(|c| c.attributes.external_url.is_none())
                    .map(|c| c.into_domain(manga_id)),
            );

            offset += received;
            match response.total {
                Some(total) if offset < total && received > 0 => continue,
                _ => break,
            }
        }

        Ok(all)
    }

    async fn fetch_page(&self, url: &str) -> Result<Vec<u8>> {
        // Клиент сам отправит отчёт о результате: это условие
        // пользования сетью MangaDex@Home, а не пожелание.
        self.client.download_page(url).await
    }

    async fn pages(&self, chapter_id: &str) -> Result<Vec<Page>> {
        let url = format!("{API_BASE}/at-home/server/{chapter_id}");
        let response: AtHomeResponse = self.client.get_json(&url).await?;

        let urls = response.page_urls(self.data_saver);
        if urls.is_empty() {
            return Err(Error::NotFound(format!("у главы {chapter_id} нет страниц")));
        }

        Ok(urls
            .into_iter()
            .enumerate()
            .map(|(index, url)| Page {
                index: index as u32,
                location: PageLocation::Url(url),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn langs() -> Vec<String> {
        vec!["ru".to_string(), "en".to_string()]
    }

    #[test]
    fn search_url_contains_query_and_pagination() {
        let query = SearchQuery::new().with_text("тест");
        let url = build_search_url(&query, &langs());

        assert!(url.starts_with("https://api.mangadex.org/manga?"));
        assert!(url.contains("limit=20"));
        assert!(url.contains("offset=0"));
    }

    #[test]
    fn cyrillic_query_is_percent_encoded() {
        let query = SearchQuery::new().with_text("манга");
        let url = build_search_url(&query, &langs());
        assert!(
            url.contains("title=%D0%BC%D0%B0%D0%BD%D0%B3%D0%B0"),
            "{url}"
        );
    }

    #[test]
    fn spaces_are_encoded_not_left_raw() {
        let query = SearchQuery::new().with_text("two words");
        let url = build_search_url(&query, &langs());
        assert!(url.contains("title=two%20words"), "{url}");
    }

    #[test]
    fn page_two_shifts_the_offset() {
        let mut query = SearchQuery::new().with_text("x");
        query.page = 3;
        query.per_page = 10;
        let url = build_search_url(&query, &langs());
        assert!(url.contains("offset=20"), "{url}");
    }

    #[test]
    fn limit_is_clamped_to_api_maximum() {
        let mut query = SearchQuery::new();
        query.per_page = 5000;
        let url = build_search_url(&query, &langs());
        assert!(
            url.contains("limit=100"),
            "API не принимает больше сотни: {url}"
        );
    }

    #[test]
    fn preferred_languages_are_requested() {
        let url = build_search_url(&SearchQuery::new(), &langs());
        assert!(url.contains("availableTranslatedLanguage[]=ru"));
        assert!(url.contains("availableTranslatedLanguage[]=en"));
    }

    #[test]
    fn sort_maps_to_api_parameters() {
        let mut query = SearchQuery::new();
        query.sort = SortBy::Popularity;
        assert!(build_search_url(&query, &langs()).contains("order[followedCount]=desc"));

        query.sort = SortBy::Title;
        assert!(build_search_url(&query, &langs()).contains("order[title]=asc"));
    }

    #[test]
    fn status_filter_is_passed_through() {
        let mut query = SearchQuery::new();
        query.statuses = vec![MangaStatus::Completed];
        assert!(build_search_url(&query, &langs()).contains("status[]=completed"));
    }

    #[test]
    fn unknown_status_is_not_sent() {
        let mut query = SearchQuery::new();
        query.statuses = vec![MangaStatus::Unknown];
        assert!(!build_search_url(&query, &langs()).contains("status[]"));
    }

    #[test]
    fn empty_query_text_is_omitted() {
        let query = SearchQuery::new().with_text("   ");
        assert!(!build_search_url(&query, &langs()).contains("title="));
    }

    #[test]
    fn feed_url_requests_scanlation_groups_and_ordering() {
        let url = build_feed_url("manga-id", &langs(), 100);
        assert!(url.contains("/manga/manga-id/feed?"));
        assert!(url.contains("offset=100"));
        assert!(url.contains("includes[]=scanlation_group"));
        assert!(url.contains("order[chapter]=asc"));
        assert!(url.contains("translatedLanguage[]=ru"));
    }
}
