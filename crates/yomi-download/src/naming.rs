//! Формирование имён файлов по шаблону из конфигурации.
//!
//! Шаблон по умолчанию: `{manga}/{volume}-{chapter} {title}.cbz`.
//! Подстановки: `{manga}`, `{volume}`, `{chapter}`, `{title}`,
//! `{scanlator}`, `{language}`.

use yomi_core::model::Chapter;

/// Символы, недопустимые в именах файлов.
///
/// Список объединяет ограничения Linux и Windows: коллекция часто живёт
/// на общем диске или синхронизируется, и файл с двоеточием в имени
/// станет там недоступен.
const FORBIDDEN: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|', '\0'];

/// Заменяет недопустимые символы и подрезает длину, не подставляя
/// ничего вместо пустой строки.
///
/// Нужна для подстановок в шаблон: отсутствующее название главы должно
/// исчезнуть из имени файла, а не превратиться в «без_названия».
pub fn sanitize_part(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if FORBIDDEN.contains(&c) || (c as u32) < 0x20 {
                '_'
            } else {
                c
            }
        })
        .collect();

    // Точки и пробелы в конце имени ломают доступ к файлу в Windows.
    let trimmed = cleaned.trim().trim_end_matches('.').trim();

    // Ограничение большинства файловых систем — 255 байт на имя.
    // Режем по границам символов, иначе получим невалидный UTF-8.
    let mut result = String::new();
    for ch in trimmed.chars() {
        if result.len() + ch.len_utf8() > 200 {
            break;
        }
        result.push(ch);
    }

    result
}

/// То же, но для имени целиком: пустое имя файла недопустимо.
pub fn sanitize(raw: &str) -> String {
    let cleaned = sanitize_part(raw);
    if cleaned.is_empty() {
        "без_названия".to_string()
    } else {
        cleaned
    }
}

/// Подставляет значения главы в шаблон.
pub fn format_path(template: &str, manga_title: &str, chapter: &Chapter) -> String {
    let volume = chapter
        .volume
        .map(|v| format!("т{v:02}"))
        .unwrap_or_else(|| "тбн".to_string());

    let number = chapter
        .number
        .map(|n| {
            // Целые номера без дробной части: «гл05», а не «гл05.0».
            if (n.fract()).abs() < f32::EPSILON {
                format!("гл{:03}", n as u32)
            } else {
                format!("гл{n}")
            }
        })
        .unwrap_or_else(|| "глбн".to_string());

    let result = template
        .replace("{manga}", &sanitize(manga_title))
        .replace("{volume}", &volume)
        .replace("{chapter}", &number)
        .replace(
            "{title}",
            &sanitize_part(chapter.title.as_deref().unwrap_or("")),
        )
        .replace(
            "{scanlator}",
            &sanitize_part(chapter.scanlator.as_deref().unwrap_or("")),
        )
        .replace("{language}", &chapter.language);

    // Подстановка пустого названия оставляет двойные пробелы.
    let mut cleaned = result.replace("  ", " ");
    while cleaned.contains("  ") {
        cleaned = cleaned.replace("  ", " ");
    }
    cleaned.replace(" .cbz", ".cbz").replace("/ ", "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use yomi_core::model::SourceId;

    fn chapter(volume: Option<u16>, number: Option<f32>, title: Option<&str>) -> Chapter {
        Chapter {
            source: SourceId::new("mangadex"),
            id: "c1".into(),
            manga_id: "m1".into(),
            number,
            volume,
            title: title.map(str::to_string),
            language: "ru".into(),
            scanlator: Some("Команда".into()),
            published_at: None,
            external_url: None,
        }
    }

    const DEFAULT: &str = "{manga}/{volume}-{chapter} {title}.cbz";

    #[test]
    fn builds_expected_path() {
        let path = format_path(
            DEFAULT,
            "Название",
            &chapter(Some(3), Some(12.0), Some("Глава")),
        );
        assert_eq!(path, "Название/т03-гл012 Глава.cbz");
    }

    #[test]
    fn fractional_chapter_numbers_are_preserved() {
        let path = format_path(DEFAULT, "Название", &chapter(Some(1), Some(10.5), None));
        assert!(path.contains("гл10.5"), "{path}");
    }

    #[test]
    fn missing_title_does_not_leave_double_spaces() {
        let path = format_path(DEFAULT, "Название", &chapter(Some(1), Some(2.0), None));
        assert_eq!(path, "Название/т01-гл002.cbz");
    }

    #[test]
    fn slashes_in_titles_do_not_create_directories() {
        // Иначе тайтл «Кровь/сталь» разложился бы по вложенным каталогам.
        let path = format_path(DEFAULT, "Кровь/сталь", &chapter(Some(1), Some(1.0), None));
        assert!(path.starts_with("Кровь_сталь/"), "{path}");
    }

    #[test]
    fn windows_forbidden_characters_are_replaced() {
        for ch in [':', '*', '?', '"', '<', '>', '|'] {
            let title = format!("до{ch}после");
            assert!(
                !sanitize(&title).contains(ch),
                "символ {ch} должен быть заменён"
            );
        }
    }

    #[test]
    fn trailing_dots_and_spaces_are_trimmed() {
        assert_eq!(sanitize("название...  "), "название");
    }

    #[test]
    fn overly_long_names_are_cut_at_character_boundaries() {
        let long = "я".repeat(300);
        let result = sanitize(&long);
        assert!(result.len() <= 200);
        // Проверка, что не порезали посреди многобайтового символа.
        assert!(result.chars().all(|c| c == 'я'));
    }

    #[test]
    fn empty_name_gets_a_placeholder() {
        assert_eq!(sanitize("   "), "без_названия");
        assert_eq!(sanitize("///"), "___");
    }

    #[test]
    fn empty_substitution_disappears_instead_of_becoming_a_placeholder() {
        // Иначе глава без названия получала бы файл
        // «т01-гл002 без_названия.cbz».
        assert_eq!(sanitize_part("   "), "");
    }

    #[test]
    fn chapters_without_numbers_still_get_a_name() {
        let path = format_path(DEFAULT, "Название", &chapter(None, None, Some("Экстра")));
        assert_eq!(path, "Название/тбн-глбн Экстра.cbz");
    }

    #[test]
    fn custom_template_with_scanlator_works() {
        let path = format_path(
            "{manga} [{scanlator}] {chapter}.cbz",
            "Тайтл",
            &chapter(None, Some(7.0), None),
        );
        assert_eq!(path, "Тайтл [Команда] гл007.cbz");
    }
}
