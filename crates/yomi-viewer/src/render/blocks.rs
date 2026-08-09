//! Рендер в юникод-блоках — гарантированный запасной вариант.
//!
//! Работает в любом терминале с поддержкой 24-битного цвета: используем
//! символ верхней половины блока `▀` (U+2580), окрашивая его цвет текста
//! в цвет верхнего пикселя, а цвет фона — в цвет нижнего. Так одна строка
//! символов терминала передаёт две строки пикселей — вдвое лучше, чем
//! просто закрашенные клетки.

use crate::fit::Layout;
use image::DynamicImage;
use std::fmt::Write as _;

/// Рендерит изображение в строку с ANSI-кодами, готовую для печати.
///
/// Размеры берёт из уже посчитанной [`Layout`] — та же геометрия, что и
/// у kitty, поэтому смена протокола не меняет масштаб страницы.
///
/// Качество здесь принципиально ниже: одна ячейка терминала передаёт два
/// пикселя, то есть страница ужимается до примерно 80x50 «пикселей».
/// Это последний рубеж деградации для терминалов без графики, а не
/// равноценная замена kitty.
pub fn render(img: &DynamicImage, layout: Layout) -> String {
    // Пиксельная сетка блоков: одна ячейка по горизонтали — один пиксель,
    // по вертикали — два (символ верхнего полублока).
    let grid_w = layout.cols.max(1) as u32;
    let grid_h = (layout.rows.max(1) as u32) * 2;

    // Пропорции уже учтены в Layout, но там они выражены в пикселях
    // экрана; переводим в сетку блоков, сохраняя соотношение сторон.
    let scale = f64::min(
        grid_w as f64 / layout.image_w as f64,
        grid_h as f64 / layout.image_h as f64,
    );
    let mut new_w = ((layout.image_w as f64 * scale).round() as u32).max(1);
    let mut new_h = ((layout.image_h as f64 * scale).round() as u32).max(2);
    new_w = new_w.min(grid_w);
    // Высота обязана быть чётной: пара строк пикселей — одна строка текста.
    if new_h % 2 != 0 {
        new_h += 1;
    }
    new_h = new_h.min(grid_h);
    if new_h < 2 {
        new_h = 2;
    }

    // Lanczos3 заметно чище Triangle на сильном уменьшении, а уменьшение
    // здесь всегда сильное.
    let resized = img
        .resize_exact(new_w, new_h, image::imageops::FilterType::Lanczos3)
        .to_rgb8();

    let mut out = String::with_capacity((new_w * new_h / 2 * 20) as usize);
    for row in (0..new_h).step_by(2) {
        for col in 0..new_w {
            let top = resized.get_pixel(col, row);
            let bottom = resized.get_pixel(col, row + 1);
            let _ = write!(
                out,
                "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m▀",
                top[0], top[1], top[2], bottom[0], bottom[1], bottom[2]
            );
        }
        out.push_str("\x1b[0m\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fit::{Cell, Fit};
    use image::{Rgb, RgbImage};

    fn solid(w: u32, h: u32, color: [u8; 3]) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, Rgb(color)))
    }

    fn layout_for(img: &DynamicImage, cols: u16, rows: u16) -> Layout {
        crate::fit::compute(
            img.width(),
            img.height(),
            cols,
            rows,
            Cell::FALLBACK,
            Fit::Contain,
            true,
        )
    }

    #[test]
    fn output_ends_each_row_with_reset_and_newline() {
        let img = solid(4, 4, [10, 20, 30]);
        let out = render(&img, layout_for(&img, 10, 10));
        assert!(out.contains("\x1b[0m\r\n"));
    }

    #[test]
    fn embeds_true_color_escape_codes() {
        let img = solid(8, 16, [255, 128, 0]);
        let out = render(&img, layout_for(&img, 4, 4));
        assert!(out.contains("\x1b[38;2;255;128;0m"));
    }

    #[test]
    fn number_of_text_rows_never_exceeds_available_rows() {
        let img = solid(100, 400, [1, 2, 3]);
        let rows_available = 10;
        let out = render(&img, layout_for(&img, 40, rows_available));
        let printed = out.matches("\r\n").count();
        assert!(
            printed <= rows_available as usize,
            "напечатано {printed} строк при доступных {rows_available}"
        );
    }

    #[test]
    fn does_not_panic_on_odd_dimensions() {
        let img = solid(3, 3, [1, 2, 3]);
        let _ = render(&img, layout_for(&img, 5, 5));
    }

    #[test]
    fn tall_page_and_wide_page_both_stay_within_bounds() {
        for (w, h) in [(400u32, 1200u32), (1200, 400)] {
            let img = solid(w, h, [7, 7, 7]);
            let out = render(&img, layout_for(&img, 80, 24));
            let printed = out.matches("\r\n").count();
            assert!(printed <= 24, "{w}x{h}: {printed} строк");
        }
    }
}
