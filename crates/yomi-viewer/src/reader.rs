//! Интерактивный цикл чтения: показ страницы, обработка клавиш, ресайз.

use crate::archive::PageSource;
use crate::cache::PageCache;
use crate::render::Options;
use crate::{terminal, Error, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::io::Write;

/// Направление чтения. Для манги традиционно справа налево: первая
/// страница расположена справа, и «дальше по сюжету» — это влево.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    RightToLeft,
    LeftToRight,
    /// Вертикальная лента — манхва и маньхуа. Стрелки вверх/вниз
    /// становятся основными, горизонтальные работают наравне с ними.
    Webtoon,
}

/// Намерение пользователя, выраженное в терминах экрана, а не сюжета.
///
/// Разделение важно: `Backward`/`Forward` — это «влево»/«вправо» на
/// клавиатуре, а какая из этих сторон означает следующую страницу,
/// решает [`Direction`]. Без разделения RTL пришлось бы вкручивать
/// в разбор клавиш, и обе логики перепутались бы.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Input {
    /// Вправо по экрану.
    Right,
    /// Влево по экрану.
    Left,
    /// Вниз — для вебтунов и как синоним «дальше».
    Down,
    /// Вверх.
    Up,
    First,
    Last,
    Quit,
    Redraw,
    Noop,
}

/// Что делать со списком страниц.
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

fn input_for(event: Event) -> Input {
    match event {
        Event::Resize(_, _) => Input::Redraw,
        Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
            KeyCode::Right | KeyCode::Char('l') => Input::Right,
            KeyCode::Left | KeyCode::Char('h') => Input::Left,
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Char(' ') => Input::Down,
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Backspace => Input::Up,
            KeyCode::Char('g') => Input::First,
            KeyCode::Char('G') => Input::Last,
            KeyCode::Char('q') | KeyCode::Esc => Input::Quit,
            _ => Input::Noop,
        },
        _ => Input::Noop,
    }
}

/// Переводит нажатие в действие с учётом направления чтения.
fn action_for(input: Input, direction: Direction) -> Action {
    match input {
        // Вертикальные клавиши от направления не зависят: «вниз» всегда
        // означает «дальше по сюжету», в любой ориентации.
        Input::Down => Action::Next,
        Input::Up => Action::Prev,
        Input::Right => match direction {
            Direction::RightToLeft => Action::Prev,
            Direction::LeftToRight | Direction::Webtoon => Action::Next,
        },
        Input::Left => match direction {
            Direction::RightToLeft => Action::Next,
            Direction::LeftToRight | Direction::Webtoon => Action::Prev,
        },
        Input::First => Action::First,
        Input::Last => Action::Last,
        Input::Quit => Action::Quit,
        Input::Redraw => Action::Redraw,
        Input::Noop => Action::Noop,
    }
}

/// Запускает интерактивное чтение. Блокирует до выхода пользователя.
pub fn run(
    source: &PageSource,
    opts: Options,
    direction: Direction,
    preload: u8,
    start_page: usize,
) -> Result<()> {
    let mut current = start_page.min(source.page_count().saturating_sub(1));

    enable_raw_mode().map_err(|e| Error::Terminal(e.to_string()))?;
    // Гарантируем возврат терминала в нормальный режим даже при ошибке
    // рендера — иначе пользователь останется с «немым» терминалом.
    let result = run_loop(source, opts, direction, preload, &mut current);
    let _ = disable_raw_mode();
    print!("\r\n");
    let _ = std::io::stdout().flush();
    result
}

fn run_loop(
    source: &PageSource,
    opts: Options,
    direction: Direction,
    preload: u8,
    current: &mut usize,
) -> Result<()> {
    let mut stdout = std::io::stdout();
    let total = source.page_count();
    let mut cache = PageCache::new(preload);

    loop {
        draw_page(
            source,
            &mut cache,
            opts,
            direction,
            *current,
            total,
            &mut stdout,
        )?;
        // Соседние страницы готовим после отрисовки текущей, чтобы
        // предзагрузка не откладывала то, чего читатель ждёт прямо сейчас.
        cache.preload_around(source, *current);

        let input = input_for(event::read().map_err(|e| Error::Terminal(e.to_string()))?);
        match action_for(input, direction) {
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
    cache: &mut PageCache,
    opts: Options,
    direction: Direction,
    index: usize,
    total: usize,
    stdout: &mut impl Write,
) -> Result<()> {
    let mut size = terminal::size()?;
    // Строка снизу оставлена под статус — не отдаём под картинку весь экран.
    size.rows = size.rows.saturating_sub(1);

    let img = cache.get(source, index)?;
    let rendered = crate::render::render(img, size, opts)?;

    // 2J очищает экран, H переводит курсор в начало — полная перерисовка
    // при каждой странице проще инкрементальной и достаточно быстрая
    // для смены страниц по нажатию клавиши.
    write!(stdout, "\x1b[2J\x1b[H").map_err(Error::Io)?;
    write!(stdout, "{rendered}").map_err(Error::Io)?;
    // Подсказка обязана соответствовать направлению: в RTL стрелка
    // «влево» листает вперёд, и молча оставлять «←/→ — листать»
    // означало бы вводить читателя в заблуждение.
    let hint = match direction {
        Direction::RightToLeft => "← дальше, → назад",
        Direction::LeftToRight => "→ дальше, ← назад",
        Direction::Webtoon => "↓ дальше, ↑ назад",
    };
    write!(
        stdout,
        "\r\nстраница {}/{}  [{hint}, q — выход]",
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

    fn act(code: KeyCode, dir: Direction) -> Action {
        action_for(input_for(key(code)), dir)
    }

    #[test]
    fn right_to_left_is_the_manga_default() {
        // Главное свойство RTL: стрелка влево ведёт вперёд по сюжету.
        assert_eq!(act(KeyCode::Left, Direction::RightToLeft), Action::Next);
        assert_eq!(act(KeyCode::Right, Direction::RightToLeft), Action::Prev);
    }

    #[test]
    fn left_to_right_is_mirrored() {
        assert_eq!(act(KeyCode::Right, Direction::LeftToRight), Action::Next);
        assert_eq!(act(KeyCode::Left, Direction::LeftToRight), Action::Prev);
    }

    #[test]
    fn vertical_keys_mean_the_same_in_every_direction() {
        for dir in [
            Direction::RightToLeft,
            Direction::LeftToRight,
            Direction::Webtoon,
        ] {
            assert_eq!(act(KeyCode::Down, dir), Action::Next, "{dir:?}");
            assert_eq!(act(KeyCode::Up, dir), Action::Prev, "{dir:?}");
            assert_eq!(act(KeyCode::Char(' '), dir), Action::Next, "{dir:?}");
        }
    }

    #[test]
    fn vim_keys_follow_the_same_rules_as_arrows() {
        assert_eq!(
            act(KeyCode::Char('h'), Direction::RightToLeft),
            Action::Next
        );
        assert_eq!(
            act(KeyCode::Char('l'), Direction::RightToLeft),
            Action::Prev
        );
        assert_eq!(
            act(KeyCode::Char('j'), Direction::RightToLeft),
            Action::Next
        );
    }

    #[test]
    fn quit_and_jumps_ignore_direction() {
        for dir in [
            Direction::RightToLeft,
            Direction::LeftToRight,
            Direction::Webtoon,
        ] {
            assert_eq!(act(KeyCode::Char('q'), dir), Action::Quit);
            assert_eq!(act(KeyCode::Esc, dir), Action::Quit);
            assert_eq!(act(KeyCode::Char('g'), dir), Action::First);
            assert_eq!(act(KeyCode::Char('G'), dir), Action::Last);
        }
    }

    #[test]
    fn resize_event_triggers_redraw() {
        assert_eq!(
            action_for(input_for(Event::Resize(80, 24)), Direction::RightToLeft),
            Action::Redraw
        );
    }

    #[test]
    fn unmapped_key_is_noop() {
        assert_eq!(
            act(KeyCode::Char('z'), Direction::RightToLeft),
            Action::Noop
        );
    }
}
