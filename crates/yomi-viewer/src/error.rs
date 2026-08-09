//! Ошибки крейта отображения.

use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ошибка ввода-вывода: {0}")]
    Io(#[from] std::io::Error),

    #[error("неподдерживаемый формат: {0}")]
    UnsupportedFormat(PathBuf),

    #[error("в источнике {0} не найдено ни одной страницы")]
    NoPages(PathBuf),

    #[error("страница {0} вне диапазона")]
    PageOutOfRange(usize),

    #[error("ошибка архива {path}: {message}")]
    Archive { path: PathBuf, message: String },

    #[error("не удалось декодировать изображение: {0}")]
    Decode(#[from] image::ImageError),

    #[error("ошибка терминала: {0}")]
    Terminal(String),
}
