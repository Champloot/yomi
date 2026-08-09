//! Геометрия вписывания страницы в окно терминала.
//!
//! Модуль намеренно не знает ни про картинки, ни про терминал: на входе
//! размеры в пикселях и ячейках, на выходе — размеры в пикселях и ячейках.
//! Благодаря этому вся арифметика, из-за которой страница выглядит
//! растянутой или мелкой, проверяется тестами без запуска терминала.
//!
//! Ключевой факт, который здесь учитывается: графический протокол kitty
//! при указании и `c`, и `r` растягивает изображение ровно на этот
//! прямоугольник, игнорируя пропорции. Значит, сохранять пропорции обязан
//! вызывающий код — то есть мы.

/// Размер одной ячейки терминала в пикселях.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub width: u32,
    pub height: u32,
}

impl Cell {
    /// Запасное значение, когда терминал не сообщает пиксельный размер
    /// (`TIOCGWINSZ` вернул нули — так делают tmux и часть эмуляторов).
    /// Пропорция 1:2 близка к типичному моноширинному шрифту.
    pub const FALLBACK: Self = Self {
        width: 8,
        height: 16,
    };

    pub fn from_window(px_w: u16, px_h: u16, cols: u16, rows: u16) -> Self {
        if px_w == 0 || px_h == 0 || cols == 0 || rows == 0 {
            return Self::FALLBACK;
        }
        Self {
            width: (px_w as u32 / cols as u32).max(1),
            height: (px_h as u32 / rows as u32).max(1),
        }
    }
}

/// Способ вписывания страницы.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    /// Целиком в окно с сохранением пропорций. Разумно по умолчанию.
    #[default]
    Contain,
    /// По ширине окна; по высоте страница может не поместиться.
    /// Обычный режим для вебтунов.
    Width,
    /// По высоте окна.
    Height,
    /// Один пиксель картинки — один пиксель экрана.
    Original,
}

/// Результат вписывания.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// Размер картинки в пикселях после масштабирования.
    pub image_w: u32,
    pub image_h: u32,
    /// Размер прямоугольника, который займёт картинка, в ячейках терминала.
    pub cols: u16,
    pub rows: u16,
}

/// Считает размеры страницы под доступную область.
///
/// `upscale` разрешает увеличивать картинку сверх её собственного
/// разрешения. По умолчанию выключено: растянутая на весь экран страница
/// в 800 пикселей шириной выглядит мыльной, и это ровно та жалоба,
/// ради которой модуль появился.
pub fn compute(
    img_w: u32,
    img_h: u32,
    avail_cols: u16,
    avail_rows: u16,
    cell: Cell,
    fit: Fit,
    upscale: bool,
) -> Layout {
    let img_w = img_w.max(1);
    let img_h = img_h.max(1);

    let avail_px_w = (avail_cols as u32 * cell.width).max(1);
    let avail_px_h = (avail_rows as u32 * cell.height).max(1);

    let scale = match fit {
        Fit::Original => 1.0,
        Fit::Width => avail_px_w as f64 / img_w as f64,
        Fit::Height => avail_px_h as f64 / img_h as f64,
        Fit::Contain => f64::min(
            avail_px_w as f64 / img_w as f64,
            avail_px_h as f64 / img_h as f64,
        ),
    };

    // Без разрешения на увеличение никогда не переходим границу 1.0.
    let scale = if upscale { scale } else { scale.min(1.0) };

    let image_w = ((img_w as f64 * scale).round() as u32).max(1);
    let image_h = ((img_h as f64 * scale).round() as u32).max(1);

    // Ячейки округляем вверх: картинка должна поместиться целиком,
    // а не быть срезанной на пиксель из-за округления вниз.
    let cols = image_w.div_ceil(cell.width).min(u16::MAX as u32) as u16;
    let rows = image_h.div_ceil(cell.height).min(u16::MAX as u32) as u16;

    Layout {
        image_w,
        image_h,
        cols: cols.max(1),
        rows: rows.max(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: Cell = Cell {
        width: 10,
        height: 20,
    };

    #[test]
    fn contain_preserves_aspect_ratio() {
        // Страница 1000x1500 (2:3) в окно 100x30 ячеек = 1000x600 пикселей.
        // По высоте ограничение жёстче: 600/1500 = 0.4.
        let l = compute(1000, 1500, 100, 30, CELL, Fit::Contain, false);
        assert_eq!(l.image_w, 400);
        assert_eq!(l.image_h, 600);
        // Пропорция сохранена.
        assert!((l.image_w as f64 / l.image_h as f64 - 2.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn wide_window_does_not_stretch_the_page() {
        // Это регресс-тест на исходную жалобу: окно во весь экран
        // не должно раздувать страницу по ширине.
        let narrow = compute(1000, 1500, 50, 30, CELL, Fit::Contain, false);
        let wide = compute(1000, 1500, 300, 30, CELL, Fit::Contain, false);
        // Высота окна не менялась, значит и картинка не должна измениться.
        assert_eq!(narrow.image_h, wide.image_h);
        assert_eq!(narrow.image_w, wide.image_w);
    }

    #[test]
    fn small_image_is_not_upscaled_by_default() {
        // Картинка 200x300 в огромное окно остаётся собой.
        let l = compute(200, 300, 200, 60, CELL, Fit::Contain, false);
        assert_eq!((l.image_w, l.image_h), (200, 300));
    }

    #[test]
    fn upscale_flag_allows_growing_beyond_native_size() {
        let l = compute(200, 300, 200, 60, CELL, Fit::Contain, true);
        assert!(
            l.image_w > 200,
            "с разрешением на увеличение картинка должна вырасти"
        );
    }

    #[test]
    fn fit_width_uses_full_width_even_if_taller_than_window() {
        let l = compute(500, 5000, 100, 30, CELL, Fit::Width, false);
        assert_eq!(l.image_w, 1000.min(500)); // без upscale ограничены натурой
        let l2 = compute(2000, 20000, 100, 30, CELL, Fit::Width, false);
        assert_eq!(l2.image_w, 1000);
        assert_eq!(l2.image_h, 10000, "вебтун остаётся длинным, обрезки нет");
    }

    #[test]
    fn fit_original_ignores_window_entirely() {
        let l = compute(1234, 567, 10, 5, CELL, Fit::Original, false);
        assert_eq!((l.image_w, l.image_h), (1234, 567));
    }

    #[test]
    fn cells_are_rounded_up_so_image_is_never_clipped() {
        // 405 пикселей при ячейке 10 — это 41 колонка, а не 40.
        let l = compute(405, 100, 100, 30, CELL, Fit::Original, false);
        assert_eq!(l.cols, 41);
    }

    #[test]
    fn fallback_cell_is_used_when_terminal_reports_zero_pixels() {
        assert_eq!(Cell::from_window(0, 0, 80, 24), Cell::FALLBACK);
        assert_eq!(Cell::from_window(640, 0, 80, 24), Cell::FALLBACK);
    }

    #[test]
    fn cell_size_is_derived_from_window_pixels() {
        // Окно 1600x800 пикселей на 100x25 ячеек -> ячейка 16x32.
        assert_eq!(
            Cell::from_window(1600, 800, 100, 25),
            Cell {
                width: 16,
                height: 32
            }
        );
    }

    #[test]
    fn degenerate_input_does_not_panic_or_produce_zero() {
        let l = compute(0, 0, 0, 0, Cell::FALLBACK, Fit::Contain, false);
        assert!(l.image_w >= 1 && l.image_h >= 1 && l.cols >= 1 && l.rows >= 1);
    }
}
