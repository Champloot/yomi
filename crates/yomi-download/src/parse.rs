//! Разбор имён файлов: том, глава, название.
//!
//! Позиционного правила не существует. Сравните:
//!
//! ```text
//! -_12_-_221_-_СборДрузей      → том 12, глава 221
//! _-_12_-_20_-_наз             ┐
//! _-_13_-_20_-_наз             ├ том 20, главы 12, 13, 14
//! _-_14_-_20_-_наз             ┘
//! ```
//!
//! В первом случае том стоит первым, во втором — вторым. Отличить их по
//! одному имени нельзя, зато можно по набору: **что меняется от файла к
//! файлу — это глава, что постоянно — том**. Поэтому разбор двухэтапный:
//! сначала из каждого имени вытаскиваются числа, потом набор
//! анализируется целиком.

/// Числа и текст, извлечённые из одного имени.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedName {
    /// Числа в порядке появления.
    pub numbers: Vec<f32>,
    /// Текстовый хвост после последнего числа — вероятное название.
    pub title: Option<String>,
    pub raw: String,
}

/// Символы-разделители: текст, состоящий только из них, названием не является.
const SEPARATORS: &[char] = &['-', '_', '.', ' ', '[', ']', '(', ')', '#'];

fn is_separator_only(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| SEPARATORS.contains(&c))
}

/// Разбирает одно имя (без расширения) на числа и текстовый хвост.
pub fn parse_name(stem: &str) -> ParsedName {
    let mut numbers = Vec::new();
    let mut last_text: Option<String> = None;
    let mut seen_number = false;

    let mut chars = stem.chars().peekable();
    let mut buffer = String::new();

    // Разбиваем на чередующиеся куски цифр и не-цифр.
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            // Текст между числами названием не является: в
            // «Название - Том 3 - Глава 21» это слово «Глава», то есть
            // подпись к номеру, а не имя главы. Названием считаем
            // только хвост после последнего числа.
            buffer.clear();
            let mut digits = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    digits.push(d);
                    chars.next();
                } else if d == '.' {
                    // Точка входит в число только если за ней цифра:
                    // иначе это разделитель или начало расширения.
                    let mut lookahead = chars.clone();
                    lookahead.next();
                    match lookahead.peek() {
                        Some(next) if next.is_ascii_digit() && !digits.contains('.') => {
                            digits.push('.');
                            chars.next();
                        }
                        _ => break,
                    }
                } else {
                    break;
                }
            }
            if let Ok(value) = digits.parse::<f32>() {
                numbers.push(value);
                seen_number = true;
            }
        } else {
            buffer.push(c);
            chars.next();
        }
    }

    // Хвост после последнего числа — единственный кандидат в названия.
    if !buffer.is_empty() && !is_separator_only(&buffer) && seen_number {
        last_text = Some(buffer);
    }

    let title = last_text
        .map(|t| t.trim_matches(|c| SEPARATORS.contains(&c)).to_string())
        .filter(|t| !t.is_empty());

    ParsedName {
        numbers,
        title,
        raw: stem.to_string(),
    }
}

/// Что удалось понять про один файл.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fields {
    pub volume: Option<u16>,
    pub chapter: Option<f32>,
    pub title: Option<String>,
}

/// Насколько уверенно разобран набор.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// Том и главы определены по вариативности — самый надёжный случай.
    ByVariation,
    /// Одно число на файл: считаем его главой.
    ChapterOnly,
    /// Определено по величине чисел, догадка — стоит спросить пользователя.
    ByMagnitude,
    /// Разобрать не удалось.
    None,
}

/// Результат разбора набора файлов.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    pub fields: Vec<Fields>,
    pub confidence: Confidence,
}

/// Разбирает набор имён, сравнивая их между собой.
pub fn analyze(stems: &[String]) -> Analysis {
    let parsed: Vec<ParsedName> = stems.iter().map(|s| parse_name(s)).collect();

    if parsed.is_empty() {
        return Analysis {
            fields: Vec::new(),
            confidence: Confidence::None,
        };
    }

    let titles: Vec<Option<String>> = parsed.iter().map(|p| p.title.clone()).collect();

    // Сколько чисел в именах. Разное количество означает разнородный
    // набор — тогда сравнивать позиции бессмысленно.
    let count = parsed[0].numbers.len();
    let uniform = parsed.iter().all(|p| p.numbers.len() == count);

    if count == 0 {
        return Analysis {
            fields: titles
                .into_iter()
                .map(|t| Fields {
                    title: t,
                    ..Default::default()
                })
                .collect(),
            confidence: Confidence::None,
        };
    }

    if count == 1 {
        // Единственное число — это глава: том без главы не встречается.
        return Analysis {
            fields: parsed
                .iter()
                .zip(titles)
                .map(|(p, t)| Fields {
                    volume: None,
                    chapter: p.numbers.first().copied(),
                    title: t,
                })
                .collect(),
            confidence: Confidence::ChapterOnly,
        };
    }

    if uniform && parsed.len() > 1 {
        // Ключевая эвристика: считаем, сколько различных значений
        // принимает каждая позиция. Меняется — глава, постоянна — том.
        let distinct: Vec<usize> = (0..count)
            .map(|position| {
                let mut values: Vec<f32> = parsed.iter().map(|p| p.numbers[position]).collect();
                values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                values.dedup();
                values.len()
            })
            .collect();

        let most = distinct.iter().copied().max().unwrap_or(0);
        let least = distinct.iter().copied().min().unwrap_or(0);

        if most > 1 && least == 1 {
            let chapter_pos = distinct.iter().position(|d| *d == most).unwrap();
            let volume_pos = distinct.iter().position(|d| *d == 1).unwrap();

            return Analysis {
                fields: parsed
                    .iter()
                    .zip(titles)
                    .map(|(p, t)| Fields {
                        volume: Some(p.numbers[volume_pos] as u16),
                        chapter: Some(p.numbers[chapter_pos]),
                        title: t,
                    })
                    .collect(),
                confidence: Confidence::ByVariation,
            };
        }
    }

    // Остаётся догадка по величине: номер главы обычно больше номера
    // тома, потому что в томе их несколько. Верно не всегда, поэтому
    // уверенность помечена как низкая и результат стоит подтвердить.
    Analysis {
        fields: parsed
            .iter()
            .zip(titles)
            .map(|(p, t)| {
                // Набор может быть разнородным: в одном имени два числа,
                // в другом одно. Обращаться ко второму вслепую нельзя.
                match (p.numbers.first(), p.numbers.get(1)) {
                    (Some(&a), Some(&b)) => {
                        let (volume, chapter) = if b >= a { (a, b) } else { (b, a) };
                        Fields {
                            volume: Some(volume as u16),
                            chapter: Some(chapter),
                            title: t,
                        }
                    }
                    // Одно число — глава, как и в однородном случае.
                    (Some(&a), None) => Fields {
                        volume: None,
                        chapter: Some(a),
                        title: t,
                    },
                    _ => Fields {
                        title: t,
                        ..Default::default()
                    },
                }
            })
            .collect(),
        confidence: Confidence::ByMagnitude,
    }
}

/// Общая часть имён — вероятное название тайтла.
///
/// Обрезается по границе разделителей: иначе от «Название 01» и
/// «Название 02» осталось бы «Название 0».
pub fn common_title(stems: &[String]) -> Option<String> {
    if stems.len() < 2 {
        return None;
    }

    let first = stems[0].as_bytes();
    let mut length = first.len();
    for stem in &stems[1..] {
        let bytes = stem.as_bytes();
        length = length.min(bytes.len());
        length = (0..length).take_while(|i| first[*i] == bytes[*i]).count();
    }

    let prefix = String::from_utf8_lossy(&first[..length]).to_string();
    let mut trimmed = prefix
        .trim_matches(|c: char| SEPARATORS.contains(&c) || c.is_ascii_digit())
        .trim()
        .to_string();

    // Отрезаем хвостовые подписи к номерам: от «Название Том 3 Глава»
    // должно остаться «Название», иначе тайтл в библиотеке будет
    // называться вместе с разметкой.
    const NUMBER_WORDS: &[&str] = &[
        "том",
        "тома",
        "глава",
        "гл",
        "часть",
        "vol",
        "volume",
        "chapter",
        "ch",
        "v",
        "c",
    ];
    loop {
        let lower = trimmed.to_lowercase();
        let Some(last_word) = lower.split(|c: char| SEPARATORS.contains(&c)).next_back() else {
            break;
        };
        if last_word.is_empty() || !NUMBER_WORDS.contains(&last_word) {
            break;
        }
        let cut = trimmed.len() - last_word.len();
        trimmed = trimmed[..cut]
            .trim_matches(|c: char| SEPARATORS.contains(&c) || c.is_ascii_digit())
            .trim()
            .to_string();
    }

    // Слишком короткий остаток названием не считаем.
    if trimmed.chars().count() >= 2 {
        Some(trimmed)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn extracts_numbers_and_trailing_title() {
        let parsed = parse_name("-_12_-_221_-_СборДрузей");
        assert_eq!(parsed.numbers, vec![12.0, 221.0]);
        assert_eq!(parsed.title.as_deref(), Some("СборДрузей"));
    }

    #[test]
    fn single_file_falls_back_to_magnitude() {
        // Одно имя сравнивать не с чем: считаем, что больший номер —
        // глава, потому что в томе их несколько.
        let result = analyze(&names(&["-_12_-_221_-_СборДрузей"]));
        assert_eq!(result.confidence, Confidence::ByMagnitude);
        assert_eq!(result.fields[0].volume, Some(12));
        assert_eq!(result.fields[0].chapter, Some(221.0));
        assert_eq!(result.fields[0].title.as_deref(), Some("СборДрузей"));
    }

    #[test]
    fn constant_position_is_the_volume_even_when_it_is_second() {
        // Случай пользователя: 20 повторяется во всех файлах — это том,
        // а 12, 13, 14 меняются — это главы. Позиция тут вторая,
        // в отличие от предыдущего примера.
        let result = analyze(&names(&[
            "_-_12_-_20_-_asdasd",
            "_-_13_-_20_-_asdaasdsd",
            "_-_14_-_20_-_ad",
        ]));
        assert_eq!(result.confidence, Confidence::ByVariation);
        assert!(result.fields.iter().all(|f| f.volume == Some(20)));
        assert_eq!(
            result
                .fields
                .iter()
                .filter_map(|f| f.chapter)
                .collect::<Vec<_>>(),
            vec![12.0, 13.0, 14.0]
        );
    }

    #[test]
    fn constant_position_is_the_volume_when_it_is_first() {
        let result = analyze(&names(&[
            "-_12_-_221_-_а",
            "-_12_-_222_-_б",
            "-_12_-_223_-_в",
        ]));
        assert_eq!(result.confidence, Confidence::ByVariation);
        assert!(result.fields.iter().all(|f| f.volume == Some(12)));
        assert_eq!(result.fields[2].chapter, Some(223.0));
    }

    #[test]
    fn variation_beats_magnitude() {
        // Здесь по величине том и глава определились бы наоборот:
        // 100 больше 5. Но 100 постоянно, значит это том.
        let result = analyze(&names(&["a_5_100", "a_6_100", "a_7_100"]));
        assert_eq!(result.confidence, Confidence::ByVariation);
        assert!(result.fields.iter().all(|f| f.volume == Some(100)));
        assert_eq!(result.fields[0].chapter, Some(5.0));
    }

    #[test]
    fn single_number_is_always_a_chapter() {
        let result = analyze(&names(&["ch012", "ch013"]));
        assert_eq!(result.confidence, Confidence::ChapterOnly);
        assert_eq!(result.fields[0].chapter, Some(12.0));
        assert_eq!(result.fields[0].volume, None);
    }

    #[test]
    fn fractional_chapter_numbers_survive() {
        let result = analyze(&names(&["Глава 10.5"]));
        assert_eq!(result.fields[0].chapter, Some(10.5));
    }

    #[test]
    fn text_before_numbers_is_not_a_title() {
        // «ch» — это префикс нумерации, а не название главы.
        let parsed = parse_name("ch012");
        assert_eq!(parsed.title, None);
    }

    #[test]
    fn separator_tail_is_not_a_title() {
        let parsed = parse_name("_-_12_-_20_-_");
        assert_eq!(parsed.title, None);
    }

    #[test]
    fn names_without_numbers_yield_nothing() {
        let result = analyze(&names(&["просто имя", "другое имя"]));
        assert_eq!(result.confidence, Confidence::None);
        assert!(result.fields.iter().all(|f| f.chapter.is_none()));
    }

    #[test]
    fn empty_input_does_not_panic() {
        assert_eq!(analyze(&[]).confidence, Confidence::None);
    }

    #[test]
    fn common_prefix_becomes_the_series_title() {
        let title = common_title(&names(&[
            "Название тайтла - 01",
            "Название тайтла - 02",
            "Название тайтла - 03",
        ]));
        assert_eq!(title.as_deref(), Some("Название тайтла"));
    }

    #[test]
    fn common_prefix_is_cut_on_separator_not_mid_number() {
        // От «Тайтл 01» и «Тайтл 02» не должно остаться «Тайтл 0».
        let title = common_title(&names(&["Тайтл 01", "Тайтл 02"]));
        assert_eq!(title.as_deref(), Some("Тайтл"));
    }

    #[test]
    fn text_between_numbers_is_not_a_title() {
        // «Глава» здесь — подпись к номеру, а не имя главы.
        let parsed = parse_name("Название - Том 3 - Глава 21");
        assert_eq!(parsed.numbers, vec![3.0, 21.0]);
        assert_eq!(parsed.title, None);
    }

    #[test]
    fn common_prefix_drops_numbering_words() {
        let title = common_title(&names(&[
            "Название - Том 3 - Глава 21",
            "Название - Том 3 - Глава 22",
        ]));
        assert_eq!(title.as_deref(), Some("Название"));
    }

    #[test]
    fn common_prefix_drops_latin_numbering_words() {
        let title = common_title(&names(&["Title v05 c034", "Title v05 c035"]));
        assert_eq!(title.as_deref(), Some("Title"));
    }

    #[test]
    fn no_common_prefix_yields_nothing() {
        assert_eq!(common_title(&names(&["абв", "где"])), None);
        assert_eq!(common_title(&names(&["один файл"])), None);
    }

    #[test]
    fn mixed_field_counts_do_not_crash() {
        let result = analyze(&names(&["a_1_2", "b_3"]));
        assert!(!result.fields.is_empty());
    }
}
