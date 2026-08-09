//! Разбор `ComicInfo.xml` — стандарта де-факто для метаданных манги.
//!
//! Файл лежит внутри CBZ и понимается Komga, Kavita, Mihon и ComicRack.
//! Поддержка даёт совместимость со всей экосистемой бесплатно и решает
//! вопрос «откуда взять автора и жанры» для локальных файлов.
//!
//! Разбор сознательно снисходительный: файлы в дикой природе неполны и
//! часто содержат мусор. Неизвестный тег игнорируется, битое число
//! превращается в `None`, а не в ошибку — метаданные не настолько важны,
//! чтобы из-за них отказываться открывать главу.

/// Метаданные одной главы или тома.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ComicInfo {
    /// Название тайтла (тег `Series`).
    pub series: Option<String>,
    /// Название конкретной главы (тег `Title`).
    pub title: Option<String>,
    pub number: Option<f32>,
    pub volume: Option<u16>,
    pub year: Option<u16>,
    pub summary: Option<String>,
    pub writers: Vec<String>,
    pub pencillers: Vec<String>,
    pub genres: Vec<String>,
    pub language: Option<String>,
    /// Команда перевода (тег `Translator` или `Publisher`).
    pub scanlator: Option<String>,
    pub page_count: Option<u32>,
}

/// Значения тега, разделённые запятой: `Автор А., Автор Б.`
fn split_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Извлекает содержимое первого тега с указанным именем.
///
/// Полноценный XML-парсер здесь избыточен: структура ComicInfo плоская,
/// вложенности нет, а зависимость ради десятка тегов не окупается.
fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    let value = xml[start..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Разбирает содержимое `ComicInfo.xml`.
pub fn parse(xml: &str) -> ComicInfo {
    ComicInfo {
        series: tag(xml, "Series").map(unescape),
        title: tag(xml, "Title").map(unescape),
        number: tag(xml, "Number").and_then(|v| v.parse().ok()),
        volume: tag(xml, "Volume").and_then(|v| v.parse().ok()),
        year: tag(xml, "Year").and_then(|v| v.parse().ok()),
        summary: tag(xml, "Summary").map(unescape),
        writers: tag(xml, "Writer").map(split_list).unwrap_or_default(),
        pencillers: tag(xml, "Penciller").map(split_list).unwrap_or_default(),
        genres: tag(xml, "Genre").map(split_list).unwrap_or_default(),
        language: tag(xml, "LanguageISO").map(str::to_string),
        scanlator: tag(xml, "Translator")
            .or_else(|| tag(xml, "Publisher"))
            .map(unescape),
        page_count: tag(xml, "PageCount").and_then(|v| v.parse().ok()),
    }
}

/// Возвращает пять обязательных XML-сущностей в исходный вид.
fn unescape(raw: &str) -> String {
    raw.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        // Амперсанд последним, иначе он испортит уже раскрытые сущности.
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<ComicInfo>
  <Series>Усогуй</Series>
  <Title>Ставка на жизнь</Title>
  <Number>12.5</Number>
  <Volume>32</Volume>
  <Year>2011</Year>
  <Writer>Мадзима Тосио</Writer>
  <Penciller>Мадзима Тосио</Penciller>
  <Genre>сэйнэн, психологический, азартные игры</Genre>
  <LanguageISO>ru</LanguageISO>
  <Translator>Вымышленная команда</Translator>
  <PageCount>195</PageCount>
  <Summary>Описание тома</Summary>
</ComicInfo>"#;

    #[test]
    fn parses_a_complete_file() {
        let info = parse(SAMPLE);
        assert_eq!(info.series.as_deref(), Some("Усогуй"));
        assert_eq!(info.title.as_deref(), Some("Ставка на жизнь"));
        assert_eq!(info.number, Some(12.5));
        assert_eq!(info.volume, Some(32));
        assert_eq!(info.year, Some(2011));
        assert_eq!(info.page_count, Some(195));
        assert_eq!(info.language.as_deref(), Some("ru"));
        assert_eq!(info.scanlator.as_deref(), Some("Вымышленная команда"));
    }

    #[test]
    fn splits_comma_separated_lists() {
        let info = parse(SAMPLE);
        assert_eq!(
            info.genres,
            vec!["сэйнэн", "психологический", "азартные игры"]
        );
        assert_eq!(info.writers, vec!["Мадзима Тосио"]);
    }

    #[test]
    fn missing_tags_become_none_not_errors() {
        let info = parse("<ComicInfo><Series>Только название</Series></ComicInfo>");
        assert_eq!(info.series.as_deref(), Some("Только название"));
        assert_eq!(info.number, None);
        assert_eq!(info.volume, None);
        assert!(info.genres.is_empty());
    }

    #[test]
    fn broken_numbers_are_ignored_rather_than_fatal() {
        let info = parse("<ComicInfo><Number>не число</Number><Volume>3</Volume></ComicInfo>");
        assert_eq!(info.number, None);
        assert_eq!(
            info.volume,
            Some(3),
            "соседний корректный тег должен уцелеть"
        );
    }

    #[test]
    fn empty_tags_are_treated_as_absent() {
        let info = parse("<ComicInfo><Series></Series><Title>   </Title></ComicInfo>");
        assert_eq!(info.series, None);
        assert_eq!(info.title, None);
    }

    #[test]
    fn entities_are_unescaped() {
        let info = parse("<ComicInfo><Series>Кровь &amp; сталь</Series></ComicInfo>");
        assert_eq!(info.series.as_deref(), Some("Кровь & сталь"));
    }

    #[test]
    fn garbage_input_does_not_panic() {
        for input in ["", "не xml вовсе", "<ComicInfo>", "<<<>>>"] {
            let _ = parse(input);
        }
    }

    #[test]
    fn publisher_is_used_when_translator_is_absent() {
        let info = parse("<ComicInfo><Publisher>Издатель</Publisher></ComicInfo>");
        assert_eq!(info.scanlator.as_deref(), Some("Издатель"));
    }
}
