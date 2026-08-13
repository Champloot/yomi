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

    #[error("не удалось разобрать PDF {path}: {message}")]
    Pdf { path: PathBuf, message: String },

    #[error("страница {page} в {path}: {reason}")]
    UnsupportedPdfPage {
        path: PathBuf,
        page: usize,
        reason: String,
    },

    #[error(
        "для чтения CBR/RAR нужна внешняя утилита unar — не найдена в PATH.\n\
         Установите её пакетом дистрибутива (например, `unar` в apt/pacman/brew)\n\
         и повторите. Причина, почему это не встроено: лицензия unrar\n\
         несвободна, см. docs/adr/0006-formats.md"
    )]
    MissingUnar,

    #[error("не удалось декодировать изображение: {0}")]
    Decode(#[from] image::ImageError),

    #[error("ошибка терминала: {0}")]
    Terminal(String),
}
