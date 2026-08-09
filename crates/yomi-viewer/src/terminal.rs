//! Взаимодействие с реальным терминалом: размер окна в ячейках и пикселях.
//!
//! Обёртка над `crossterm`, изолированная в один модуль — если способ
//! получения размера придётся менять, правки не разойдутся по коду.

use crate::fit::Cell;
use crate::{Error, Result};

/// Размер видимой области терминала.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermSize {
    pub cols: u16,
    pub rows: u16,
    /// Размер ячейки в пикселях. Если терминал не сообщил пиксельные
    /// размеры окна, здесь окажется [`Cell::FALLBACK`].
    pub cell: Cell,
}

/// Спрашивает у терминала размер окна.
///
/// Сначала пробует `window_size()` — он отдаёт и ячейки, и пиксели через
/// `TIOCGWINSZ`. Часть окружений (в первую очередь tmux) заполняет
/// пиксельные поля нулями; тогда переходим на `size()` и запасную
/// пропорцию ячейки.
pub fn size() -> Result<TermSize> {
    if let Ok(ws) = crossterm::terminal::window_size() {
        if ws.columns > 0 && ws.rows > 0 {
            return Ok(TermSize {
                cols: ws.columns,
                rows: ws.rows,
                cell: Cell::from_window(ws.width, ws.height, ws.columns, ws.rows),
            });
        }
    }

    let (cols, rows) = crossterm::terminal::size().map_err(|e| Error::Terminal(e.to_string()))?;
    Ok(TermSize {
        cols,
        rows,
        cell: Cell::FALLBACK,
    })
}
