//! Упаковка страниц в CBZ с метаданными `ComicInfo.xml`.
//!
//! Формат CBZ — обычный ZIP: именно его понимают Komga, Kavita, Mihon и
//! сам yomi. Вложенный `ComicInfo.xml` делает скачанное равноправным
//! с тем, что собирают другие читалки, а не пригодным только нам.

use crate::naming::sanitize;
use std::io::Write;
use std::path::Path;
use yomi_core::{Error, Result};

/// Метаданные для `ComicInfo.xml`.
///
/// Отдельная от [`yomi_core::model::Chapter`] структура намеренно:
/// упаковывать в CBZ нужно и то, что не приходило из источника —
/// например, папку сканов, которую пользователь собрал сам. Привязка
/// к модели источника делала бы упаковку недоступной для этого случая.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackMeta {
    /// Название тайтла (тег `Series`).
    pub series: String,
    /// Название главы или тома.
    pub title: Option<String>,
    pub number: Option<f32>,
    pub volume: Option<u16>,
    pub language: Option<String>,
    pub scanlator: Option<String>,
    /// Откуда взялся файл — попадёт в `Notes`.
    pub origin: Option<String>,
}

impl PackMeta {
    /// Метаданные для локальной упаковки: известно только название.
    pub fn local(series: impl Into<String>) -> Self {
        Self {
            series: series.into(),
            ..Default::default()
        }
    }

    /// Метаданные из главы источника.
    pub fn from_chapter(series: impl Into<String>, chapter: &yomi_core::model::Chapter) -> Self {
        Self {
            series: series.into(),
            title: chapter.title.clone(),
            number: chapter.number,
            volume: chapter.volume,
            language: Some(chapter.language.clone()),
            scanlator: chapter.scanlator.clone(),
            origin: Some(format!("источник {}", chapter.source)),
        }
    }
}

/// Скачанная страница.
pub struct PagePayload {
    /// Порядковый номер с нуля — определяет имя файла в архиве.
    pub index: u32,
    pub bytes: Vec<u8>,
    /// Расширение исходного файла: `png`, `jpg`.
    pub extension: String,
}

/// Определяет расширение по сигнатуре файла.
///
/// Полагаться на расширение в URL нельзя: сервис отдаёт имена вида
/// `1-<хеш>.png`, но пережатые варианты приходят как jpg, а иногда
/// расширения нет вовсе.
pub fn detect_extension(bytes: &[u8]) -> &'static str {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => "png",
        [0xFF, 0xD8, 0xFF, ..] => "jpg",
        [b'G', b'I', b'F', ..] => "gif",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "webp",
        _ => "jpg",
    }
}

/// Собирает `ComicInfo.xml`.
pub fn build_comicinfo(meta: &PackMeta, page_count: u32) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ComicInfo>\n");

    let tag = |xml: &mut String, name: &str, value: &str| {
        if !value.trim().is_empty() {
            xml.push_str(&format!("  <{name}>{}</{name}>\n", escape(value)));
        }
    };

    tag(&mut xml, "Series", &meta.series);
    if let Some(title) = &meta.title {
        tag(&mut xml, "Title", title);
    }
    if let Some(number) = meta.number {
        let value = if number.fract().abs() < f32::EPSILON {
            format!("{}", number as u32)
        } else {
            format!("{number}")
        };
        tag(&mut xml, "Number", &value);
    }
    if let Some(volume) = meta.volume {
        tag(&mut xml, "Volume", &volume.to_string());
    }
    if let Some(group) = &meta.scanlator {
        tag(&mut xml, "Translator", group);
    }
    if let Some(language) = &meta.language {
        tag(&mut xml, "LanguageISO", language);
    }
    tag(&mut xml, "PageCount", &page_count.to_string());
    // Помечаем происхождение: через полгода будет неочевидно, откуда файл.
    let origin = meta
        .origin
        .clone()
        .unwrap_or_else(|| "собрано yomi".to_string());
    tag(&mut xml, "Notes", &origin);

    xml.push_str("</ComicInfo>\n");
    xml
}

fn escape(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Записывает страницы и метаданные в CBZ.
///
/// Пишет во временный файл рядом с целевым и переименовывает в конце:
/// прерванная загрузка не должна оставлять недособранный архив, который
/// при следующем сканировании попадёт в библиотеку как настоящий.
pub fn write_cbz(target: &Path, meta: &PackMeta, mut pages: Vec<PagePayload>) -> Result<()> {
    if pages.is_empty() {
        return Err(Error::NotFound("нечего упаковывать: страниц нет".into()));
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }

    pages.sort_by_key(|p| p.index);

    let temp = target.with_extension("cbz.part");
    {
        let file = std::fs::File::create(&temp)?;
        let mut zip = zip::ZipWriter::new(file);
        // Изображения уже сжаты: повторное сжатие тратит время
        // и почти ничего не даёт.
        let stored =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::FileOptions::default();

        for page in &pages {
            let name = format!("{:03}.{}", page.index + 1, sanitize(&page.extension));
            zip.start_file(&name, stored)
                .map_err(|e| Error::NotFound(format!("запись {name}: {e}")))?;
            zip.write_all(&page.bytes)?;
        }

        let xml = build_comicinfo(meta, pages.len() as u32);
        zip.start_file("ComicInfo.xml", deflated)
            .map_err(|e| Error::NotFound(format!("запись ComicInfo.xml: {e}")))?;
        zip.write_all(xml.as_bytes())?;

        zip.finish()
            .map_err(|e| Error::NotFound(format!("закрытие архива: {e}")))?;
    }

    std::fs::rename(&temp, target)?;
    tracing::info!(path = %target.display(), pages = pages.len(), "глава упакована");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> PackMeta {
        PackMeta {
            series: "Название серии".into(),
            title: Some("Название главы".into()),
            number: Some(12.0),
            volume: Some(3),
            language: Some("ru".into()),
            scanlator: Some("Команда & Ко".into()),
            origin: None,
        }
    }

    fn page(index: u32, bytes: &[u8]) -> PagePayload {
        PagePayload {
            index,
            bytes: bytes.to_vec(),
            extension: detect_extension(bytes).into(),
        }
    }

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    const JPG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0];

    #[test]
    fn detects_format_by_signature_not_by_name() {
        assert_eq!(detect_extension(PNG), "png");
        assert_eq!(detect_extension(JPG), "jpg");
        assert_eq!(detect_extension(b"GIF89a"), "gif");
        assert_eq!(detect_extension(&[]), "jpg", "неизвестное считаем jpg");
    }

    #[test]
    fn comicinfo_contains_expected_tags() {
        let xml = build_comicinfo(&meta(), 24);
        assert!(xml.contains("<Series>Название серии</Series>"));
        assert!(
            xml.contains("<Number>12</Number>"),
            "целый номер без дробной части"
        );
        assert!(xml.contains("<Volume>3</Volume>"));
        assert!(xml.contains("<PageCount>24</PageCount>"));
        assert!(xml.contains("<LanguageISO>ru</LanguageISO>"));
    }

    #[test]
    fn comicinfo_escapes_special_characters() {
        let xml = build_comicinfo(&meta(), 1);
        assert!(xml.contains("Команда &amp; Ко"));
    }

    #[test]
    fn empty_fields_are_omitted_entirely() {
        let xml = build_comicinfo(&PackMeta::local("Серия"), 5);
        assert!(!xml.contains("<Title>"));
        assert!(!xml.contains("<Translator>"));
        assert!(!xml.contains("<LanguageISO>"));
        assert!(xml.contains("<Series>Серия</Series>"));
    }

    #[test]
    fn locally_packed_files_are_marked_as_such() {
        // Через полгода должно быть понятно, что файл собран вручную,
        // а не скачан откуда-то.
        let xml = build_comicinfo(&PackMeta::local("Серия"), 1);
        assert!(xml.contains("собрано yomi"), "{xml}");
    }

    #[test]
    fn writes_readable_archive_with_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("вложенный").join("глава.cbz");

        write_cbz(&target, &meta(), vec![page(0, PNG), page(1, JPG)]).unwrap();
        assert!(target.exists(), "каталоги должны создаваться сами");

        let file = std::fs::File::open(&target).unwrap();
        let archive = zip::ZipArchive::new(file).unwrap();
        let names: Vec<&str> = archive.file_names().collect();
        assert!(names.contains(&"001.png"));
        assert!(names.contains(&"002.jpg"));
        assert!(names.contains(&"ComicInfo.xml"));
    }

    #[test]
    fn pages_are_ordered_regardless_of_input_order() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("глава.cbz");

        write_cbz(
            &target,
            &meta(),
            vec![page(2, JPG), page(0, PNG), page(1, PNG)],
        )
        .unwrap();

        let file = std::fs::File::open(&target).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert_eq!(names[0], "001.png");
        assert_eq!(names[1], "002.png");
        assert_eq!(names[2], "003.jpg");
    }

    #[test]
    fn no_partial_file_is_left_behind() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("глава.cbz");
        write_cbz(&target, &meta(), vec![page(0, PNG)]).unwrap();
        assert!(!target.with_extension("cbz.part").exists());
    }

    #[test]
    fn empty_page_list_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("пусто.cbz");
        assert!(write_cbz(&target, &meta(), vec![]).is_err());
        assert!(!target.exists(), "пустой архив создаваться не должен");
    }
}
