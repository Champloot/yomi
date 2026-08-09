//! # yomi-viewer
//!
//! Отображение страниц манги в терминале: источники страниц (каталог, CBZ),
//! определение возможностей терминала, рендер (юникод-блоки, kitty) и
//! интерактивный цикл чтения.
//!
//! Правило то же, что и для `yomi-core`: этот крейт не разбирает аргументы
//! командной строки и не знает про `clap` — только про терминал и картинки.

pub mod archive;
pub mod capability;
mod error;
mod natural_sort;
pub mod reader;
pub mod render;
pub mod terminal;

pub use capability::Protocol;
pub use error::{Error, Result};
