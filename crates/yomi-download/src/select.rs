//! Разбор указания «какие главы качать»: `5`, `1-10`, `all`, `1,3,7-9`.
//!
//! Чистая функция: ошибка здесь означает, что пользователь скачает не то,
//! что просил, и заметит это нескоро.

use yomi_core::model::Chapter;

/// Выбирает главы по строке-указателю.
///
/// Сравнение идёт по номеру главы, а не по её месту в списке: человек
/// пишет «12-15», имея в виду номера глав, а не позиции.
pub fn select<'a>(chapters: &'a [Chapter], spec: &str) -> Vec<&'a Chapter> {
    let spec = spec.trim();
    if spec.is_empty() || spec.eq_ignore_ascii_case("all") || spec == "все" {
        return chapters.iter().collect();
    }

    let mut selected: Vec<&Chapter> = Vec::new();

    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // Диапазон вида «1-10». Дефис может быть и в дробном номере,
        // поэтому разделяем только по первому вхождению.
        if let Some((from, to)) = part.split_once('-') {
            let (from, to) = (from.trim(), to.trim());
            if let (Ok(from), Ok(to)) = (from.parse::<f32>(), to.parse::<f32>()) {
                let (low, high) = if from <= to { (from, to) } else { (to, from) };
                for chapter in chapters {
                    if let Some(number) = chapter.number {
                        if number >= low && number <= high && !contains(&selected, chapter) {
                            selected.push(chapter);
                        }
                    }
                }
                continue;
            }
        }

        if let Ok(number) = part.parse::<f32>() {
            for chapter in chapters {
                if chapter.number == Some(number) && !contains(&selected, chapter) {
                    selected.push(chapter);
                }
            }
        }
    }

    selected
}

fn contains(list: &[&Chapter], chapter: &Chapter) -> bool {
    list.iter().any(|c| c.id == chapter.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use yomi_core::model::SourceId;

    fn chapters(numbers: &[f32]) -> Vec<Chapter> {
        numbers
            .iter()
            .enumerate()
            .map(|(i, n)| Chapter {
                source: SourceId::new("s"),
                id: format!("c{i}"),
                manga_id: "m".into(),
                number: Some(*n),
                volume: None,
                title: None,
                language: "ru".into(),
                scanlator: None,
                published_at: None,
            })
            .collect()
    }

    fn numbers(selected: &[&Chapter]) -> Vec<f32> {
        selected.iter().filter_map(|c| c.number).collect()
    }

    #[test]
    fn all_selects_everything() {
        let list = chapters(&[1.0, 2.0, 3.0]);
        assert_eq!(numbers(&select(&list, "all")).len(), 3);
        assert_eq!(numbers(&select(&list, "все")).len(), 3);
        assert_eq!(numbers(&select(&list, "")).len(), 3);
    }

    #[test]
    fn single_number_selects_one() {
        let list = chapters(&[1.0, 2.0, 3.0]);
        assert_eq!(numbers(&select(&list, "2")), vec![2.0]);
    }

    #[test]
    fn range_is_inclusive_on_both_ends() {
        let list = chapters(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(numbers(&select(&list, "2-3")), vec![2.0, 3.0]);
    }

    #[test]
    fn reversed_range_still_works() {
        let list = chapters(&[1.0, 2.0, 3.0]);
        assert_eq!(numbers(&select(&list, "3-1")), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn comma_separated_parts_are_combined() {
        let list = chapters(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(numbers(&select(&list, "1,3-4")), vec![1.0, 3.0, 4.0]);
    }

    #[test]
    fn duplicates_are_not_downloaded_twice() {
        let list = chapters(&[1.0, 2.0, 3.0]);
        assert_eq!(numbers(&select(&list, "2,2,1-2")), vec![2.0, 1.0]);
    }

    #[test]
    fn fractional_chapters_are_selectable() {
        let list = chapters(&[10.0, 10.5, 11.0]);
        assert_eq!(numbers(&select(&list, "10.5")), vec![10.5]);
        assert_eq!(numbers(&select(&list, "10-10.5")), vec![10.0, 10.5]);
    }

    #[test]
    fn garbage_selects_nothing_rather_than_everything() {
        // Опечатка не должна оборачиваться загрузкой всего тайтла.
        let list = chapters(&[1.0, 2.0]);
        assert!(select(&list, "абв").is_empty());
    }

    #[test]
    fn chapters_without_numbers_are_skipped_by_ranges() {
        let mut list = chapters(&[1.0, 2.0]);
        list[1].number = None;
        assert_eq!(numbers(&select(&list, "1-99")), vec![1.0]);
    }
}
