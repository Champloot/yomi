//! Тесты разбора ответов API на записанных образцах.
//!
//! Сети здесь нет намеренно: тест, ходящий в интернет, краснеет когда
//! сервис недоступен, и молчит когда сервис поменял формат — то есть
//! ведёт себя ровно наоборот нужному. Образцы записаны с реальной
//! структуры ответов, значения вымышлены.

use yomi_source_mangadex::api::{AtHomeResponse, ChapterEntity, ListResponse, MangaEntity};

fn preferred() -> Vec<String> {
    vec!["ru".to_string(), "en".to_string()]
}

#[test]
fn parses_search_response() {
    let raw = include_str!("fixtures/manga_search.json");
    let parsed: ListResponse<MangaEntity> = serde_json::from_str(raw).expect("разбор поиска");

    assert_eq!(parsed.data.len(), 2);
    assert_eq!(parsed.total, Some(2));
}

#[test]
fn prefers_russian_title_when_available() {
    let raw = include_str!("fixtures/manga_search.json");
    let parsed: ListResponse<MangaEntity> = serde_json::from_str(raw).unwrap();
    let manga = parsed
        .data
        .into_iter()
        .next()
        .unwrap()
        .into_domain(&preferred());

    assert_eq!(manga.title, "Пример названия");
    assert_eq!(manga.description.as_deref(), Some("Пример описания."));
    assert_eq!(manga.genres, vec!["Психологическое", "Drama"]);
}

#[test]
fn falls_back_to_english_when_russian_is_absent() {
    let raw = include_str!("fixtures/manga_search.json");
    let parsed: ListResponse<MangaEntity> = serde_json::from_str(raw).unwrap();
    let manga = parsed
        .data
        .into_iter()
        .nth(1)
        .unwrap()
        .into_domain(&preferred());

    // Названия на русском нет — пустое название хуже английского.
    assert_eq!(manga.title, "Second Example");
}

#[test]
fn extracts_authors_and_artists_from_relationships() {
    let raw = include_str!("fixtures/manga_search.json");
    let parsed: ListResponse<MangaEntity> = serde_json::from_str(raw).unwrap();
    let manga = parsed
        .data
        .into_iter()
        .next()
        .unwrap()
        .into_domain(&preferred());

    assert_eq!(manga.authors, vec!["Автор А."]);
    assert_eq!(manga.artists, vec!["Художник Б."]);
}

#[test]
fn unknown_fields_do_not_break_parsing() {
    // В образце есть someFutureFieldWeDoNotKnow: добавление сервисом
    // нового поля не должно ломать нам весь поиск.
    let raw = include_str!("fixtures/manga_search.json");
    let parsed: Result<ListResponse<MangaEntity>, _> = serde_json::from_str(raw);
    assert!(parsed.is_ok());
}

#[test]
fn maps_status_to_domain_values() {
    use yomi_core::model::MangaStatus;
    let raw = include_str!("fixtures/manga_search.json");
    let parsed: ListResponse<MangaEntity> = serde_json::from_str(raw).unwrap();
    let mut items = parsed.data.into_iter();

    assert_eq!(
        items.next().unwrap().into_domain(&preferred()).status,
        MangaStatus::Ongoing
    );
    assert_eq!(
        items.next().unwrap().into_domain(&preferred()).status,
        MangaStatus::Completed
    );
}

#[test]
fn parses_chapter_feed_with_awkward_values() {
    let raw = include_str!("fixtures/chapter_feed.json");
    let parsed: ListResponse<ChapterEntity> = serde_json::from_str(raw).expect("разбор глав");
    let chapters: Vec<_> = parsed
        .data
        .into_iter()
        .map(|c| c.into_domain("manga-id"))
        .collect();

    assert_eq!(chapters.len(), 3);

    // Обычная глава.
    assert_eq!(chapters[0].number, Some(1.0));
    assert_eq!(chapters[0].volume, Some(1));
    assert_eq!(chapters[0].title.as_deref(), Some("Начало"));
    assert_eq!(chapters[0].scanlator.as_deref(), Some("Команда перевода"));

    // Дробный номер и пустое название: пустая строка — не название.
    assert_eq!(chapters[1].number, Some(10.5));
    assert_eq!(chapters[1].volume, None);
    assert_eq!(chapters[1].title, None);

    // Глава без номера — такое бывает у одиночных выпусков.
    assert_eq!(chapters[2].number, None);
    assert_eq!(chapters[2].volume, Some(2));
}

#[test]
fn keeps_translated_language_for_filtering() {
    let raw = include_str!("fixtures/chapter_feed.json");
    let parsed: ListResponse<ChapterEntity> = serde_json::from_str(raw).unwrap();
    let langs: Vec<String> = parsed
        .data
        .into_iter()
        .map(|c| c.into_domain("m").language)
        .collect();
    assert_eq!(langs, vec!["ru", "ru", "en"]);
}

#[test]
fn builds_page_urls_in_documented_format() {
    let raw = include_str!("fixtures/at_home.json");
    let parsed: AtHomeResponse = serde_json::from_str(raw).expect("разбор at-home");

    let urls = parsed.page_urls(false);
    assert_eq!(urls.len(), 3);
    assert_eq!(
        urls[0],
        "https://uploads.example.org/data/3303dd03ac8d27452cce3f2a882e94b2/1-aaaa.png"
    );
}

#[test]
fn data_saver_uses_its_own_file_list() {
    let raw = include_str!("fixtures/at_home.json");
    let parsed: AtHomeResponse = serde_json::from_str(raw).unwrap();

    let urls = parsed.page_urls(true);
    assert!(urls[0].contains("/data-saver/"), "{}", urls[0]);
    assert!(urls[0].ends_with("1-dddd.jpg"));
}

#[test]
fn page_count_comes_from_the_file_list() {
    // Длина массива — точное число страниц: это буквально то,
    // что будет скачано, в отличие от поля pages в метаданных.
    let raw = include_str!("fixtures/at_home.json");
    let parsed: AtHomeResponse = serde_json::from_str(raw).unwrap();
    assert_eq!(parsed.page_urls(false).len(), 3);
}

#[test]
fn trailing_slash_in_base_url_does_not_double_up() {
    let raw = r#"{"baseUrl":"https://example.org/","chapter":{"hash":"h","data":["1.png"],"dataSaver":[]}}"#;
    let parsed: AtHomeResponse = serde_json::from_str(raw).unwrap();
    assert_eq!(
        parsed.page_urls(false)[0],
        "https://example.org/data/h/1.png"
    );
}

#[test]
fn external_chapters_are_kept_and_marked() {
    // Лицензированные тайтлы (One Piece и подобные) отдаются ссылкой
    // на сторонний сайт. Прятать такие главы нельзя: пользователь
    // решит, что переводов нет, тогда как они есть.
    let raw = include_str!("fixtures/chapter_feed.json");
    let parsed: ListResponse<ChapterEntity> = serde_json::from_str(raw).unwrap();
    let chapters: Vec<_> = parsed
        .data
        .into_iter()
        .map(|c| c.into_domain("m"))
        .collect();

    assert_eq!(chapters.len(), 3, "внешняя глава должна остаться в списке");

    let external: Vec<_> = chapters.iter().filter(|c| !c.is_downloadable()).collect();
    assert_eq!(external.len(), 1);
    assert_eq!(
        external[0].external_url.as_deref(),
        Some("https://example.com/read-elsewhere")
    );
}

#[test]
fn ordinary_chapters_are_downloadable() {
    let raw = include_str!("fixtures/chapter_feed.json");
    let parsed: ListResponse<ChapterEntity> = serde_json::from_str(raw).unwrap();
    let chapters: Vec<_> = parsed
        .data
        .into_iter()
        .map(|c| c.into_domain("m"))
        .collect();
    assert!(chapters[0].is_downloadable());
    assert!(chapters[1].is_downloadable());
}
