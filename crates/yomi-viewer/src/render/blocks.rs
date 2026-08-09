//! Рендер в юникод-блоках — гарантированный запасной вариант.
//!
//! Работает в любом терминале с поддержкой 24-битного цвета: используем
//! символ верхней половины блока `▀` (U+2580), окрашивая его цвет текста
//! в цвет верхнего пикселя, а цвет фона — в цвет нижнего. Так одна строка
//! символов терминала передаёт две строки пикселей — вдвое лучше, чем
//! просто закрашенные клетки.

use image::{DynamicImage, GenericImageView};
use std::fmt::Write as _;

/// Рендерит изображение в строку с ANSI-кодами, готовую для печати.
///
/// `max_cols`/`max_rows` — доступное место в терминале, в ячейках текста.
/// Изображение вписывается с сохранением пропорций.
pub fn render(img: &DynamicImage, max_cols: u16, max_rows: u16) -> String {
    // Ячейка терминала визуально примерно вдвое выше, чем широка,
    // а мы кодируем два вертикальных пикселя на одну ячейку — итоговое
    // приближение к квадратному пикселю на выходе.
    let target_w = (max_cols as u32).max(1);
    let target_h = (max_rows as u32 * 2).max(2);

    let (orig_w, orig_h) = img.dimensions();
    let scale = f64::min(
        target_w as f64 / orig_w as f64,
        target_h as f64 / orig_h as f64,
    );
    let new_w = ((orig_w as f64 * scale).round() as u32).max(1);
    // Высота обязана быть чётной: каждая пара строк пикселей — одна строка текста.
    let mut new_h = ((orig_h as f64 * scale).round() as u32).max(2);
    if new_h % 2 != 0 {
        new_h += 1;
    }

    let resized = img
        .resize_exact(new_w, new_h, image::imageops::FilterType::Triangle)
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
    use image::{Rgb, RgbImage};

    fn solid(w: u32, h: u32, color: [u8; 3]) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, Rgb(color)))
    }

    #[test]
    fn output_ends_each_row_with_reset_and_newline() {
        let img = solid(4, 4, [10, 20, 30]);
        let out = render(&img, 10, 10);
        assert!(out.contains("\x1b[0m\r\n"));
    }

    #[test]
    fn produces_one_text_row_per_two_pixel_rows() {
        let img = solid(2, 4, [255, 0, 0]);
        // max_rows=2 -> target_h=4, совпадает с высотой картинки без масштаба.
        let out = render(&img, 2, 2);
        let rows = out.matches("\r\n").count();
        assert_eq!(rows, 2, "4 пиксельные строки должны дать 2 строки текста");
    }

    #[test]
    fn embeds_true_color_escape_codes() {
        let img = solid(1, 2, [255, 128, 0]);
        let out = render(&img, 1, 1);
        assert!(out.contains("\x1b[38;2;255;128;0m"));
    }

    #[test]
    fn does_not_panic_on_odd_height_source() {
        let img = solid(3, 3, [1, 2, 3]);
        let _ = render(&img, 5, 5);
    }
}
