//! Взаимодействие с реальным терминалом: размер окна в ячейках и пикселях.
//!
//! Обёртка над `crossterm`, изолированная в один модуль — если протокол
//! получения размера придётся менять (например, добавить XTWINOPS-запрос
//! для точного пиксельного размера ячейки), правки не разойдутся по коду.

use crate::{Error, Result};

/// Размер видимой области терминала в ячейках текста.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermSize {
    pub cols: u16,
    pub rows: u16,
}

pub fn size() -> Result<TermSize> {
    let (cols, rows) = crossterm::terminal::size().map_err(|e| Error::Terminal(e.to_string()))?;
    Ok(TermSize { cols, rows })
}
