//! Рендер страницы в конкретный протокол вывода.

pub mod blocks;
pub mod kitty;

use crate::capability::Protocol;
use crate::Result;
use image::DynamicImage;

/// Готовит строку для печати в терминал согласно выбранному протоколу.
///
/// Для kitty картинка перекодируется в PNG перед base64: формат `f=100`
/// протокола требует именно его, исходный JPEG/WebP не годится напрямую.
pub fn render(img: &DynamicImage, protocol: Protocol, cols: u16, rows: u16) -> Result<String> {
    match protocol {
        Protocol::Kitty => {
            let mut png_bytes = Vec::new();
            img.write_to(
                &mut std::io::Cursor::new(&mut png_bytes),
                image::ImageFormat::Png,
            )?;
            Ok(kitty::encode_png(&png_bytes, cols, rows))
        }
        // iTerm2 и Sixel — реализация запланирована следом; пока оба
        // деградируют на блоки, которые работают уже сейчас и везде.
        Protocol::Iterm2 | Protocol::Sixel | Protocol::Blocks => {
            Ok(blocks::render(img, cols, rows))
        }
    }
}
