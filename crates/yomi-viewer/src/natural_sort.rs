//! Естественная сортировка имён файлов.
//!
//! Лексикографически `page10.jpg` идёт раньше `page2.jpg` — это неверно
//! для человека и для порядка страниц. Естественная сортировка сравнивает
//! числовые куски как числа: разбивает строку на чередующиеся отрезки
//! цифр и не-цифр и сравнивает их поэлементно.

/// Ключ для сортировки: последовательность кусков, где числовой кусок
/// сравнивается как число, а не как строка.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Chunk {
    Text(String),
    Number(u64),
}

fn split(name: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut chars = name.chars().peekable();

    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            let mut digits = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    digits.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            // Переполнение u64 на реальных именах файлов практически
            // невозможно; при переполнении откатываемся на текст, чтобы
            // не паниковать на экзотическом входе.
            match digits.parse::<u64>() {
                Ok(n) => chunks.push(Chunk::Number(n)),
                Err(_) => chunks.push(Chunk::Text(digits)),
            }
        } else {
            let mut text = String::new();
            while let Some(&t) = chars.peek() {
                if !t.is_ascii_digit() {
                    text.push(t);
                    chars.next();
                } else {
                    break;
                }
            }
            chunks.push(Chunk::Text(text));
        }
    }
    chunks
}

impl PartialOrd for Chunk {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Chunk {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use Chunk::*;
        match (self, other) {
            (Number(a), Number(b)) => a.cmp(b),
            (Text(a), Text(b)) => a.cmp(b),
            // Число и текст на одной позиции — редкость (разная структура
            // имён в одном каталоге). Считаем число «меньше», это как
            // минимум детерминированно и не паникует.
            (Number(_), Text(_)) => std::cmp::Ordering::Less,
            (Text(_), Number(_)) => std::cmp::Ordering::Greater,
        }
    }
}

/// Сравнивает два имени файла в естественном порядке.
pub fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    split(a).cmp(&split(b))
}

/// Сортирует список имён на месте.
pub fn sort<T, F: Fn(&T) -> &str>(items: &mut [T], key: F) {
    items.sort_by(|a, b| compare(key(a), key(b)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn numeric_suffix_orders_by_value_not_lexically() {
        assert_eq!(compare("page2.jpg", "page10.jpg"), Ordering::Less);
        assert_eq!(compare("page10.jpg", "page2.jpg"), Ordering::Greater);
    }

    #[test]
    fn equal_names_are_equal() {
        assert_eq!(compare("page01.jpg", "page01.jpg"), Ordering::Equal);
    }

    #[test]
    fn zero_padded_and_unpadded_compare_by_value() {
        // "001" и "1" как числа равны — распространённый случай на практике.
        assert_eq!(compare("001.jpg", "1.jpg"), Ordering::Equal);
    }

    #[test]
    fn sorts_a_realistic_page_list_correctly() {
        let mut names = vec![
            "page10.jpg".to_string(),
            "page1.jpg".to_string(),
            "page2.jpg".to_string(),
            "page20.jpg".to_string(),
        ];
        sort(&mut names, |s| s.as_str());
        assert_eq!(
            names,
            vec!["page1.jpg", "page2.jpg", "page10.jpg", "page20.jpg"]
        );
    }

    #[test]
    fn volume_and_chapter_numbers_both_sort_naturally() {
        let mut names = vec![
            "т1 гл10.cbz".to_string(),
            "т1 гл2.cbz".to_string(),
            "т2 гл1.cbz".to_string(),
        ];
        sort(&mut names, |s| s.as_str());
        assert_eq!(names, vec!["т1 гл2.cbz", "т1 гл10.cbz", "т2 гл1.cbz"]);
    }

    #[test]
    fn purely_alphabetic_names_sort_lexically() {
        let mut names = vec!["b.jpg".to_string(), "a.jpg".to_string()];
        sort(&mut names, |s| s.as_str());
        assert_eq!(names, vec!["a.jpg", "b.jpg"]);
    }
}
