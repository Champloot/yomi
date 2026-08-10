//! Интерактивный цикл чтения: показ страницы, обработка клавиш, ресайз.

use crate::archive::PageSource;
use crate::cache::PageCache;
use crate::render::Options;
use crate::{terminal, Error, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::io::Write;

/// Доступ к отметкам глав.
///
/// Трейт, а не структура: `yomi-viewer` не знает про базу данных и не
/// должен узнать. Реализацию поверх хранилища даёт приложение.
pub trait ChapterMarks {
    /// Страницы, отмеченные как начала глав, по возрастанию.
    fn pages(&self) -> Vec<u32>;
    /// Переключает отметку. Возвращает true, если отметка появилась.
    fn toggle(&mut self, page: u32) -> bool;
}

/// Параметры сеанса чтения.
///
/// Отдельная структура вместо шести аргументов: список параметров
/// `run` разрастался с каждым этапом, и перепутать местами два `u8`
/// стало вопросом времени.
pub struct Session<'a> {
    pub render: Options,
    pub direction: Direction,
    /// Сколько соседних страниц готовить заранее.
    pub preload: u8,
    /// С какой страницы начинать, с нуля.
    pub start_page: usize,
    /// Отметки глав, если файл есть в библиотеке.
    pub marks: Option<&'a mut dyn ChapterMarks>,
}

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
    /// Поставить или снять отметку начала главы.
    ToggleMark,
    /// К началу следующей размеченной главы.
    NextChapter,
    /// К началу предыдущей.
    PrevChapter,
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
    /// Поставить или снять отметку начала главы на текущей странице.
    ToggleMark,
    /// К началу следующей размеченной главы.
    NextChapter,
    /// К началу предыдущей.
    PrevChapter,
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
            KeyCode::Char('m') => Input::ToggleMark,
            // Скобки для перехода по главам — привычно тем, кто
            // пользуется vim: там так ходят по абзацам и функциям.
            KeyCode::Char(']') => Input::NextChapter,
            KeyCode::Char('[') => Input::PrevChapter,
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
        Input::ToggleMark => Action::ToggleMark,
        // Переход по главам не зеркалится направлением чтения:
        // «следующая глава» — это дальше по сюжету в любом случае,
        // в отличие от стрелок, которые описывают сторону экрана.
        Input::NextChapter => Action::NextChapter,
        Input::PrevChapter => Action::PrevChapter,
        Input::Quit => Action::Quit,
        Input::Redraw => Action::Redraw,
        Input::Noop => Action::Noop,
    }
}

/// Запускает интерактивное чтение. Блокирует до выхода пользователя.
pub fn run(source: &PageSource, mut session: Session<'_>) -> Result<usize> {
    let mut current = session
        .start_page
        .min(source.page_count().saturating_sub(1));

    enable_raw_mode().map_err(|e| Error::Terminal(e.to_string()))?;
    // Гарантируем возврат терминала в нормальный режим даже при ошибке
    // рендера — иначе пользователь останется с «немым» терминалом.
    let result = run_loop(source, &mut session, &mut current);
    let _ = disable_raw_mode();
    print!("\r\n");
    let _ = std::io::stdout().flush();
    // Страницу возвращаем даже при ошибке: прогресс лучше сохранить
    // до места сбоя, чем потерять его целиком.
    result.map(|()| current)
}

fn run_loop(source: &PageSource, session: &mut Session<'_>, current: &mut usize) -> Result<()> {
    let mut stdout = std::io::stdout();
    let total = source.page_count();
    let mut cache = PageCache::new(session.preload);

    loop {
        // Список отметок перечитывается каждый кадр: он меняется прямо
        // во время чтения, когда пользователь ставит новую отметку.
        let marks = session
            .marks
            .as_ref()
            .map(|m| m.pages())
            .unwrap_or_default();

        draw_page(
            source,
            &mut cache,
            session.render,
            session.direction,
            *current,
            total,
            &marks,
            &mut stdout,
        )?;
        // Соседние страницы готовим после отрисовки текущей, чтобы
        // предзагрузка не откладывала то, чего читатель ждёт прямо сейчас.
        cache.preload_around(source, *current);

        let input = input_for(event::read().map_err(|e| Error::Terminal(e.to_string()))?);
        match action_for(input, session.direction) {
            Action::Next if *current + 1 < total => *current += 1,
            Action::Next => {}
            Action::Prev if *current > 0 => *current -= 1,
            Action::Prev => {}
            Action::First => *current = 0,
            Action::Last => *current = total.saturating_sub(1),
            Action::ToggleMark => {
                if let Some(marks) = session.marks.as_mut() {
                    marks.toggle(*current as u32);
                }
            }
            Action::NextChapter => {
                if let Some(next) = marks.iter().find(|p| **p as usize > *current) {
                    *current = *next as usize;
                }
            }
            Action::PrevChapter => {
                // К началу текущей главы, а не сразу к предыдущей:
                // так же ведёт себя переход к предыдущему треку в
                // проигрывателях, и это привычнее.
                let current_start = marks
                    .iter()
                    .rev()
                    .find(|p| (**p as usize) < *current)
                    .copied();
                if let Some(start) = current_start {
                    *current = start as usize;
                }
            }
            Action::Redraw | Action::Noop => {}
            Action::Quit => return Ok(()),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_page(
    source: &PageSource,
    cache: &mut PageCache,
    opts: Options,
    direction: Direction,
    index: usize,
    total: usize,
    marks: &[u32],
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
        Direction::RightToLeft => "← дальше",
        Direction::LeftToRight => "→ дальше",
        Direction::Webtoon => "↓ дальше",
    };

    let chapter = chapter_status(marks, index);

    // Про отметку напоминаем только когда том не размечен: на
    // размеченном полезнее знать про переход между главами, а `m`
    // пользователь и так найдёт, когда захочет поправить границу.
    let keys = hint_keys(marks);

    write!(
        stdout,
        "\r\nстраница {}/{}{}  [{hint}, {keys}, q — выход]",
        index + 1,
        total,
        chapter
    )
    .map_err(Error::Io)?;
    stdout.flush().map_err(Error::Io)?;
    Ok(())
}

/// Какие клавиши подсказывать в строке состояния.
///
/// Про отметку напоминаем только когда том не размечен: на размеченном
/// полезнее знать про переход между главами, а `m` пользователь найдёт,
/// когда захочет поправить границу.
fn hint_keys(marks: &[u32]) -> &'static str {
    if marks.is_empty() {
        "m — отметить главу"
    } else {
        "[ ] — главы"
    }
}

/// Описание текущей главы для строки состояния.
///
/// Возвращает пустую строку, когда разметки нет: показывать «глава 1/1»
/// на неразмеченном томе — шум, а не информация.
fn chapter_status(marks: &[u32], page: usize) -> String {
    if marks.is_empty() {
        return String::new();
    }

    let here = marks.iter().any(|p| *p as usize == page);
    let index = marks.iter().filter(|p| (**p as usize) <= page).count();

    if index == 0 {
        // Страница до первой отметки — ещё не глава.
        return format!("  (до главы 1 из {})", marks.len());
    }

    let mark = if here { " ●" } else { "" };
    format!("  гл. {}/{}{}", index, marks.len(), mark)
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
    fn mark_key_works_in_every_direction() {
        for dir in [
            Direction::RightToLeft,
            Direction::LeftToRight,
            Direction::Webtoon,
        ] {
            assert_eq!(act(KeyCode::Char('m'), dir), Action::ToggleMark, "{dir:?}");
        }
    }

    #[test]
    fn chapter_navigation_is_not_mirrored_by_direction() {
        // Скобки описывают движение по сюжету, а не сторону экрана,
        // поэтому направление чтения их не переворачивает.
        for dir in [Direction::RightToLeft, Direction::LeftToRight] {
            assert_eq!(act(KeyCode::Char(']'), dir), Action::NextChapter, "{dir:?}");
            assert_eq!(act(KeyCode::Char('['), dir), Action::PrevChapter, "{dir:?}");
        }
    }

    #[test]
    fn status_line_hint_depends_on_whether_the_volume_is_marked() {
        // На размеченном томе полезнее знать про переход между главами,
        // а не про то, как поставить отметку.
        assert!(
            hint_keys(&[]).contains('m'),
            "неразмеченный том — подсказать отметку"
        );
        assert!(
            hint_keys(&[0, 20]).contains('['),
            "размеченный — подсказать переход"
        );
        assert!(!hint_keys(&[0, 20]).contains('m'));
    }

    #[test]
    fn status_is_empty_without_marks() {
        assert_eq!(chapter_status(&[], 5), "");
    }

    #[test]
    fn status_counts_chapters_from_marks() {
        let marks = [0u32, 20, 40];
        assert!(chapter_status(&marks, 0).contains("гл. 1/3"));
        assert!(chapter_status(&marks, 25).contains("гл. 2/3"));
        assert!(chapter_status(&marks, 45).contains("гл. 3/3"));
    }

    #[test]
    fn status_marks_the_page_that_starts_a_chapter() {
        let marks = [0u32, 20];
        assert!(
            chapter_status(&marks, 20).contains('●'),
            "начало главы должно быть заметно"
        );
        assert!(!chapter_status(&marks, 21).contains('●'));
    }

    #[test]
    fn pages_before_the_first_mark_are_not_a_chapter() {
        // Обложка и оглавление идут до первой главы — нумеровать их
        // как главу неверно.
        let status = chapter_status(&[10u32, 30], 3);
        assert!(status.contains("до главы 1"), "{status}");
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
