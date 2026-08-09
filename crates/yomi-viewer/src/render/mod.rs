//! Рендер страницы в конкретный протокол вывода.
//!
//! Здесь же живёт масштабирование: и kitty, и блоки получают уже
//! подготовленную картинку нужного размера. Масштабировать в каждом
//! рендерере отдельно — верный путь к тому, что протоколы разъедутся
//! по поведению.

pub mod blocks;
pub mod kitty;

use crate::capability::Protocol;
use crate::fit::{self, Fit, Layout};
use crate::terminal::TermSize;
use crate::Result;
use image::DynamicImage;

/// Что и как рисовать.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub protocol: Protocol,
    pub fit: Fit,
    pub upscale: bool,
}

/// Готовит строку для печати в терминал.
///
/// Пропорции сохраняет вызывающий код, а не терминал: протокол kitty при
/// указании и `c`, и `r` растягивает картинку на весь прямоугольник, не
/// глядя на исходное соотношение сторон.
pub fn render(img: &DynamicImage, size: TermSize, opts: Options) -> Result<String> {
    let layout = fit::compute(
        img.width(),
        img.height(),
        size.cols,
        size.rows,
        size.cell,
        opts.fit,
        opts.upscale,
    );

    match opts.protocol {
        Protocol::Kitty => render_kitty(img, layout),
        // iTerm2 и Sixel пока деградируют на блоки: они объявлены в
        // цепочке определения, но не реализованы.
        Protocol::Iterm2 | Protocol::Sixel | Protocol::Blocks => Ok(blocks::render(img, layout)),
    }
}

fn render_kitty(img: &DynamicImage, layout: Layout) -> Result<String> {
    // Масштабируем сами и отправляем ровно столько пикселей, сколько
    // нужно на экран. Иначе на каждое перелистывание в терминал уезжает
    // страница в полном разрешении — мегабайты base64 впустую.
    let scaled = img.resize_exact(
        layout.image_w,
        layout.image_h,
        image::imageops::FilterType::Lanczos3,
    );

    let mut png = Vec::new();
    scaled.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
    Ok(kitty::encode_png(&png, layout.cols, layout.rows))
}
