//! Интерактивный цикл чтения: показ страницы, обработка клавиш, ресайз.

use crate::archive::PageSource;
use crate::capability::Protocol;
use crate::{terminal, Error, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::io::Write;

/// Направление перелистывания. Слева-направо/справа-налево решается
/// на уровне вызывающего кода (конфиг `reader.direction`); здесь —
/// только «вперёд» и «назад» по списку страниц.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Next,
    Prev,
    First,
    Last,
    Quit,
    Redraw,
    Noop,
}

fn action_for(event: Event) -> Action {
    match event {
        Event::Resize(_, _) => Action::Redraw,
        Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') => Action::Next,
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => Action::Prev,
            KeyCode::Char('g') => Action::First,
            KeyCode::Char('G') => Action::Last,
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            _ => Action::Noop,
        },
        _ => Action::Noop,
    }
}

/// Запускает интерактивное чтение. Блокирует до выхода пользователя.
pub fn run(source: &PageSource, protocol: Protocol, start_page: usize) -> Result<()> {
    let mut current = start_page.min(source.page_count().saturating_sub(1));

    enable_raw_mode().map_err(|e| Error::Terminal(e.to_string()))?;
    // Гарантируем возврат терминала в нормальный режим даже при ошибке
    // рендера — иначе пользователь останется с «немым» терминалом.
    let result = run_loop(source, protocol, &mut current);
    let _ = disable_raw_mode();
    print!("\r\n");
    let _ = std::io::stdout().flush();
    result
}

fn run_loop(source: &PageSource, protocol: Protocol, current: &mut usize) -> Result<()> {
    let mut stdout = std::io::stdout();
    let total = source.page_count();

    loop {
        draw_page(source, protocol, *current, total, &mut stdout)?;

        match action_for(event::read().map_err(|e| Error::Terminal(e.to_string()))?) {
            Action::Next if *current + 1 < total => *current += 1,
            Action::Next => {}
            Action::Prev if *current > 0 => *current -= 1,
            Action::Prev => {}
            Action::First => *current = 0,
            Action::Last => *current = total.saturating_sub(1),
            Action::Redraw | Action::Noop => {}
            Action::Quit => return Ok(()),
        }
    }
}

fn draw_page(
    source: &PageSource,
    protocol: Protocol,
    index: usize,
    total: usize,
    stdout: &mut impl Write,
) -> Result<()> {
    let bytes = source.read_page(index)?;
    let img = image::load_from_memory(&bytes)?;
    let size = terminal::size()?;
    // Строка снизу оставлена под статус — не отдаём под картинку весь экран.
    let usable_rows = size.rows.saturating_sub(1);

    let rendered = crate::render::render(&img, protocol, size.cols, usable_rows)?;

    // 2J очищает экран, H переводит курсор в начало — полная перерисовка
    // при каждой странице проще инкрементальной и достаточно быстрая
    // для смены страниц по нажатию клавиши.
    write!(stdout, "\x1b[2J\x1b[H").map_err(Error::Io)?;
    write!(stdout, "{rendered}").map_err(Error::Io)?;
    write!(
        stdout,
        "\r\nстраница {}/{}  [q — выход, ←/→ — листать]",
        index + 1,
        total
    )
    .map_err(Error::Io)?;
    stdout.flush().map_err(Error::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn arrow_right_and_vim_l_both_advance() {
        assert_eq!(action_for(key(KeyCode::Right)), Action::Next);
        assert_eq!(action_for(key(KeyCode::Char('l'))), Action::Next);
    }

    #[test]
    fn arrow_left_and_vim_h_both_go_back() {
        assert_eq!(action_for(key(KeyCode::Left)), Action::Prev);
        assert_eq!(action_for(key(KeyCode::Char('h'))), Action::Prev);
    }

    #[test]
    fn q_and_esc_both_quit() {
        assert_eq!(action_for(key(KeyCode::Char('q'))), Action::Quit);
        assert_eq!(action_for(key(KeyCode::Esc)), Action::Quit);
    }

    #[test]
    fn resize_event_triggers_redraw() {
        assert_eq!(action_for(Event::Resize(80, 24)), Action::Redraw);
    }

    #[test]
    fn unmapped_key_is_noop() {
        assert_eq!(action_for(key(KeyCode::Char('z'))), Action::Noop);
    }

    #[test]
    fn g_and_shift_g_jump_to_edges() {
        assert_eq!(action_for(key(KeyCode::Char('g'))), Action::First);
        assert_eq!(action_for(key(KeyCode::Char('G'))), Action::Last);
    }
}
