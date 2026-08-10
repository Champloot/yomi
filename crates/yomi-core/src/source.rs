//! Интерфейс источника манги — центральная точка расширения проекта.
//!
//! Всё, что ядро знает о MangaDex, MangaLib или локальной папке, выражается
//! этим трейтом. Добавление источника не должно требовать правок в ядре:
//! новый тип реализует [`Source`] и регистрируется в [`Registry`].
//!
//! Почему `async_trait`, а не встроенный `async fn` в трейте: нам нужна
//! динамическая диспетчеризация (`Box<dyn Source>`), а она для встроенных
//! async-методов в трейтах пока не поддерживается.

use crate::model::{Chapter, Manga, Page, SearchQuery, SearchResult, SortBy, SourceId};
use crate::{Error, Result};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Что источник реально умеет.
///
/// Без этой структуры интерфейс будет врать пользователю: он предложит
/// фильтр по автору там, где сайт его не поддерживает.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Capabilities {
    pub text_search: bool,
    pub filter_by_genre: bool,
    pub filter_by_author: bool,
    pub filter_by_status: bool,
    /// Поддерживаемые способы сортировки.
    pub sorts: Vec<SortBy>,
    /// Языки контента, которые отдаёт источник.
    pub languages: Vec<String>,
    /// Нужны логин/токен.
    pub requires_auth: bool,
    /// Источник за анти-бот защитой — работоспособность не гарантируется.
    pub fragile: bool,
}

/// Источник манги.
///
/// `Send + Sync` обязательны: источники будут дёргаться из нескольких
/// задач одновременно при пакетной загрузке.
#[async_trait]
pub trait Source: Send + Sync {
    /// Machine-readable идентификатор: `mangadex`.
    fn id(&self) -> SourceId;

    /// Отображаемое имя: `MangaDex`.
    fn name(&self) -> &str;

    fn capabilities(&self) -> Capabilities;

    /// Поиск тайтлов.
    async fn search(&self, query: &SearchQuery) -> Result<SearchResult>;

    /// Подробности о тайтле.
    async fn manga(&self, id: &str) -> Result<Manga>;

    /// Список глав тайтла.
    async fn chapters(&self, manga_id: &str) -> Result<Vec<Chapter>>;

    /// Страницы главы.
    async fn pages(&self, chapter_id: &str) -> Result<Vec<Page>>;

    /// Скачивает содержимое страницы.
    ///
    /// Загрузка поручена источнику, а не общему HTTP-клиенту, не из
    /// любви к симметрии: MangaDex обязывает отчитываться о каждой
    /// скачанной картинке, у других сайтов свои требования к заголовкам
    /// и подписанным ссылкам. Знать об этом может только сам источник.
    async fn fetch_page(&self, url: &str) -> Result<Vec<u8>> {
        Err(Error::Unsupported {
            source_id: self.id().to_string(),
            feature: format!("загрузка страниц (запрошено {url})"),
        })
    }
}

/// Реестр доступных источников.
///
/// `Arc<dyn Source>` вместо `Box`: один и тот же источник может
/// одновременно использоваться загрузчиком и интерфейсом.
#[derive(Default, Clone)]
pub struct Registry {
    sources: BTreeMap<SourceId, Arc<dyn Source>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Регистрирует источник. Возвращает `self` для цепочки вызовов.
    pub fn register(mut self, source: Arc<dyn Source>) -> Self {
        self.sources.insert(source.id(), source);
        self
    }

    pub fn get(&self, id: &str) -> Result<Arc<dyn Source>> {
        self.sources
            .get(&SourceId::new(id))
            .cloned()
            .ok_or_else(|| Error::SourceNotFound(id.to_string()))
    }

    /// Все источники, отсортированы по идентификатору (BTreeMap).
    pub fn list(&self) -> Vec<Arc<dyn Source>> {
        self.sources.values().cloned().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    pub fn len(&self) -> usize {
        self.sources.len()
    }
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry")
            .field("sources", &self.sources.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Собирает реестр из источников, скомпилированных в бинарник.
///
/// Сетевые источники живут в отдельных крейтах и регистрируются
/// приложением: ядру нельзя зависеть от `reqwest`, иначе граница
/// «ядро не знает о сети» перестанет существовать.
pub fn default_registry() -> Registry {
    Registry::new().register(Arc::new(crate::sources::demo::DemoSource::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::demo::DemoSource;

    #[test]
    fn registry_finds_registered_source() {
        let r = Registry::new().register(Arc::new(DemoSource::new()));
        assert_eq!(r.len(), 1);
        assert!(r.get("demo").is_ok());
    }

    #[test]
    fn registry_reports_unknown_source() {
        let r = Registry::new();
        // Здесь нельзя unwrap_err(): он требует Debug от значения Ok,
        // а мы сознательно не обязываем источники реализовывать Debug.
        assert!(matches!(r.get("mangadex"), Err(Error::SourceNotFound(_))));
    }

    #[tokio::test]
    async fn demo_source_is_usable_through_trait_object() {
        let registry = default_registry();
        let source = registry.get("demo").unwrap();
        let result = source
            .search(&SearchQuery::new().with_text("пример"))
            .await
            .unwrap();
        assert!(!result.items.is_empty());
    }
}
