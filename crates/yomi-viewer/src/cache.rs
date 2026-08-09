//! Кэш декодированных страниц с предзагрузкой соседних.
//!
//! Без кэша каждое нажатие стрелки заново читает файл с диска и
//! декодирует JPEG — на странице 2000x3000 это десятки миллисекунд,
//! которые чувствуются как задержка при листании.
//!
//! Предзагрузка сознательно синхронная: она выполняется после того, как
//! текущая страница уже нарисована, и потому не задерживает отклик.
//! Фоновая загрузка в отдельном потоке дала бы больше, но потребовала бы
//! разделяемого состояния и отмены задач — это оправдано только если
//! синхронного варианта окажется мало.

use crate::archive::PageSource;
use crate::Result;
use image::DynamicImage;
use std::collections::HashMap;

/// Декодированные страницы, лежащие наготове.
pub struct PageCache {
    pages: HashMap<usize, DynamicImage>,
    /// Сколько страниц держать по каждую сторону от текущей.
    radius: usize,
}

impl PageCache {
    pub fn new(radius: u8) -> Self {
        Self {
            pages: HashMap::new(),
            radius: radius as usize,
        }
    }

    /// Отдаёт страницу, декодируя её при промахе кэша.
    pub fn get(&mut self, source: &PageSource, index: usize) -> Result<&DynamicImage> {
        // `entry().or_insert_with()` здесь не подходит: декодирование
        // возвращает Result, а замыкание не умеет прерываться ошибкой.
        if let std::collections::hash_map::Entry::Vacant(slot) = self.pages.entry(index) {
            slot.insert(decode(source, index)?);
        }
        Ok(self
            .pages
            .get(&index)
            .expect("страница только что добавлена"))
    }

    /// Готовит соседние страницы и выбрасывает уехавшие далеко.
    ///
    /// Ошибки предзагрузки намеренно проглатываются: соседняя страница
    /// может быть битой, но это не повод прерывать чтение текущей.
    /// Настоящая ошибка всплывёт, когда до страницы дойдёт очередь.
    pub fn preload_around(&mut self, source: &PageSource, current: usize) {
        if self.radius == 0 {
            self.evict_far_from(current);
            return;
        }
        let total = source.page_count();
        let first = current.saturating_sub(self.radius);
        let last = (current + self.radius).min(total.saturating_sub(1));

        for index in first..=last {
            if self.pages.contains_key(&index) {
                continue;
            }
            match decode(source, index) {
                Ok(img) => {
                    self.pages.insert(index, img);
                }
                Err(e) => {
                    tracing::debug!(index, error = %e, "предзагрузка страницы не удалась");
                }
            }
        }
        self.evict_far_from(current);
    }

    /// Держим окно вокруг текущей страницы: на 195 страницах манги
    /// кэш целиком — это гигабайты декодированных пикселей.
    fn evict_far_from(&mut self, current: usize) {
        let radius = self.radius;
        self.pages.retain(|&index, _| {
            let distance = if index > current {
                index - current
            } else {
                current - index
            };
            distance <= radius
        });
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }
}

fn decode(source: &PageSource, index: usize) -> Result<DynamicImage> {
    let bytes = source.read_page(index)?;
    Ok(image::load_from_memory(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbImage};
    use std::io::Write;

    /// Собирает CBZ из нескольких валидных PNG.
    fn make_cbz(dir: &std::path::Path, pages: usize) -> std::path::PathBuf {
        let path = dir.join("test.cbz");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::FileOptions::default();

        for i in 0..pages {
            let img = DynamicImage::ImageRgb8(RgbImage::new(4, 4));
            let mut png = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .unwrap();
            zip.start_file(format!("{i:03}.png"), opts).unwrap();
            zip.write_all(&png).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    #[test]
    fn get_decodes_on_miss_and_reuses_on_hit() {
        let dir = tempfile::tempdir().unwrap();
        let source = PageSource::open_cbz(&make_cbz(dir.path(), 5)).unwrap();
        let mut cache = PageCache::new(0);

        assert!(cache.is_empty());
        cache.get(&source, 2).unwrap();
        assert_eq!(cache.len(), 1);
        cache.get(&source, 2).unwrap();
        assert_eq!(
            cache.len(),
            1,
            "повторный запрос не должен декодировать заново"
        );
    }

    #[test]
    fn preload_fills_neighbours_within_radius() {
        let dir = tempfile::tempdir().unwrap();
        let source = PageSource::open_cbz(&make_cbz(dir.path(), 10)).unwrap();
        let mut cache = PageCache::new(2);

        cache.get(&source, 5).unwrap();
        cache.preload_around(&source, 5);
        // Страницы 3,4,5,6,7 — пять штук.
        assert_eq!(cache.len(), 5);
    }

    #[test]
    fn preload_does_not_run_past_the_edges() {
        let dir = tempfile::tempdir().unwrap();
        let source = PageSource::open_cbz(&make_cbz(dir.path(), 3)).unwrap();
        let mut cache = PageCache::new(5);

        cache.preload_around(&source, 0);
        assert_eq!(cache.len(), 3, "всего три страницы, больше взять неоткуда");
    }

    #[test]
    fn far_pages_are_evicted_when_reader_moves_on() {
        let dir = tempfile::tempdir().unwrap();
        let source = PageSource::open_cbz(&make_cbz(dir.path(), 20)).unwrap();
        let mut cache = PageCache::new(1);

        cache.preload_around(&source, 2);
        assert_eq!(cache.len(), 3); // 1,2,3
        cache.preload_around(&source, 15);
        assert_eq!(cache.len(), 3, "старое окно должно быть вытеснено"); // 14,15,16
    }

    #[test]
    fn zero_radius_disables_preloading_but_keeps_current_page() {
        let dir = tempfile::tempdir().unwrap();
        let source = PageSource::open_cbz(&make_cbz(dir.path(), 5)).unwrap();
        let mut cache = PageCache::new(0);

        cache.get(&source, 1).unwrap();
        cache.preload_around(&source, 1);
        assert_eq!(cache.len(), 1);
    }
}
