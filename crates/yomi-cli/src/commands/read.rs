//! `yomi read` — читалка. Основная работа этапа M1.
//!
//! Здесь только связывание: путь → источник страниц (`yomi_viewer::archive`)
//! → протокол вывода (`yomi_viewer::capability`) → интерактивный цикл
//! (`yomi_viewer::reader`). Вся логика — в крейте `yomi-viewer`, тут её
//! не должно прибавляться.

use super::Ctx;
use crate::cli::ReadArgs;
use anyhow::{bail, Result};
use yomi_core::config::{Fit as ConfigFit, ReadingDirection, Renderer};
use yomi_viewer::archive::PageSource;
use yomi_viewer::capability::{self, Protocol};
use yomi_viewer::fit::Fit;
use yomi_viewer::reader::Direction;
use yomi_viewer::reader::{ChapterMarks, Session};
use yomi_viewer::render::Options;

/// Переводит выбор из конфига/флага в протокол. `Auto` — единственный
/// вариант, требующий обращения к окружению процесса; остальные —
/// явный выбор пользователя, и его нужно уважать безоговорочно.
fn resolve_protocol(renderer: Renderer) -> Protocol {
    match renderer {
        Renderer::Auto => capability::detect_from_process_env(),
        Renderer::Kitty => Protocol::Kitty,
        Renderer::Iterm2 => Protocol::Iterm2,
        Renderer::Sixel => Protocol::Sixel,
        Renderer::Blocks => Protocol::Blocks,
    }
}

/// Переводит режим вписывания из конфига в тип рендера. Два одинаковых
/// перечисления существуют намеренно: конфиг — часть публичного формата
/// файла, а `yomi-viewer` не должен от него зависеть.
fn resolve_fit(fit: ConfigFit) -> Fit {
    match fit {
        ConfigFit::Contain => Fit::Contain,
        ConfigFit::Width => Fit::Width,
        ConfigFit::Height => Fit::Height,
        ConfigFit::Original => Fit::Original,
    }
}

fn resolve_direction(d: ReadingDirection) -> Direction {
    match d {
        ReadingDirection::RightToLeft => Direction::RightToLeft,
        ReadingDirection::LeftToRight => Direction::LeftToRight,
        ReadingDirection::Webtoon => Direction::Webtoon,
    }
}

pub async fn run(ctx: &Ctx, args: &ReadArgs) -> Result<()> {
    // Аргумент — либо путь, либо название тайтла. Разбираем именно в
    // таком порядке: существующий файл всегда важнее совпадения имён,
    // иначе тайтл с названием вроде «том.cbz» перехватил бы открытие
    // настоящего файла.
    let candidate = std::path::PathBuf::from(&args.target);
    let (path, jump_to_page) = if candidate.exists() {
        (candidate, None)
    } else {
        resolve_by_title(&args.target, args.volume, args.chapter)?
    };

    // Проверка номера страницы — это валидация аргумента, она не зависит
    // от содержимого файла и должна отработать раньше, чем мы вообще
    // попытаемся открыть источник.
    if args.page == 0 {
        bail!("страницы нумеруются с единицы");
    }

    let renderer = args
        .renderer
        .map(Into::into)
        .unwrap_or(ctx.config.reader.renderer);
    let protocol = resolve_protocol(renderer);

    let source = PageSource::open(&path)
        .map_err(|e| anyhow::anyhow!(e).context(format!("открытие {}", path.display())))?;

    let fit = resolve_fit(args.fit.map(Into::into).unwrap_or(ctx.config.reader.fit));
    // Флаг командной строки только включает увеличение, но не выключает
    // его: выключить можно в конфиге, а два взаимоисключающих флага ради
    // этого заводить не стоит.
    let upscale = args.upscale || ctx.config.reader.upscale;
    let direction = resolve_direction(
        args.direction
            .map(Into::into)
            .unwrap_or(ctx.config.reader.direction),
    );

    tracing::info!(
        path = %path.display(),
        pages = source.page_count(),
        protocol = protocol.label_ru(),
        ?fit,
        upscale,
        ?direction,
        "запуск читалки"
    );

    let start = (args.page as usize).saturating_sub(1);
    if start >= source.page_count() {
        bail!(
            "страница {} вне диапазона: всего страниц {}",
            args.page,
            source.page_count()
        );
    }

    let opts = Options {
        protocol,
        fit,
        upscale,
    };

    // Если глава есть в библиотеке, продолжаем с сохранённого места и
    // записываем прогресс на выходе. Отсутствие базы или записи — не
    // ошибка: читать файл, которого нет в библиотеке, тоже нужно.
    let store = open_store_quietly();
    let known_chapter = store
        .as_ref()
        .and_then(|s| s.chapter_by_path(&path.to_string_lossy()).ok().flatten());

    // Явно указанная страница всегда важнее сохранённого прогресса.
    // Переход к указанной главе важнее сохранённого прогресса: человек
    // попросил конкретное место, а не «продолжить».
    let start = if let Some(page) = jump_to_page {
        (page as usize).min(source.page_count().saturating_sub(1))
    } else {
        start
    };

    let start = match (&store, &known_chapter) {
        (Some(store), Some(chapter)) if args.page == 1 => match store.progress_of(chapter.id) {
            Ok(Some(p)) if !p.completed && (p.page as usize) < source.page_count() => {
                println!(
                    "Продолжаю с страницы {} из {}",
                    p.page + 1,
                    source.page_count()
                );
                p.page as usize
            }
            _ => start,
        },
        _ => start,
    };

    // Закладки внутри файла читаем всегда: их кладёт `yomi build`,
    // и навигация по главам должна работать сразу после сборки.
    let from_file: Vec<u32> = yomi_viewer::scan::read_comicinfo(&path)
        .map(|info| info.bookmarks.iter().map(|b| b.page).collect())
        .unwrap_or_default();

    let mut marks = Marks {
        store: store.as_ref(),
        chapter_id: known_chapter.as_ref().map(|c| c.id),
        from_file,
    };

    {
        use yomi_viewer::reader::ChapterMarks as _;
        let count = marks.pages().len();
        let editable = known_chapter.is_some();

        // Прогресс хранится в библиотеке. Молчать об этом нельзя:
        // пользователь дочитает до середины, вернётся и обнаружит,
        // что чтение начинается сначала.
        if !editable {
            println!(
                "Файла нет в библиотеке — прогресс чтения сохранён не будет.\n\
                 Добавить: yomi library scan {}",
                path.parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| ".".to_string())
            );
        }
        tracing::info!(chapters = count, editable, "разметка глав");
        if count > 0 && !editable {
            println!(
                "Найдено глав: {count}. Переход — клавиши [ и ].\n\
                 Правка отметок требует библиотеки: `yomi library scan КАТАЛОГ`"
            );
        }
    }

    let session = Session {
        render: opts,
        direction,
        preload: ctx.config.reader.preload_pages,
        start_page: start,
        marks: Some(&mut marks as &mut dyn ChapterMarks),
    };

    let outcome = yomi_viewer::reader::run(&source, session);

    if let (Some(store), Some(chapter)) = (&store, &known_chapter) {
        // При ошибке рендера сохраняем хотя бы стартовую позицию:
        // потерять прогресс целиком хуже, чем записать его неточно.
        let page = *outcome.as_ref().unwrap_or(&start);
        if let Err(e) =
            store.save_progress(chapter.id, page as u32, Some(source.page_count() as u32))
        {
            // Не роняем чтение из-за проблем с базой: главу уже прочитали.
            tracing::warn!(error = %e, "не удалось сохранить прогресс");
        }
    }

    outcome
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!(e).context("отображение страниц"))
}

/// Ищет файл по названию тайтла и возвращает то, на чём остановились.
///
/// Нужен, чтобы `yomi read Usogui` открывал нужный том сам. Название
/// разбирается только когда пути с таким именем не существует, поэтому
/// перехватить открытие настоящего файла оно не может.
fn resolve_by_title(
    title: &str,
    volume: Option<u16>,
    chapter: Option<f32>,
) -> Result<(std::path::PathBuf, Option<u32>)> {
    let Some(store) = open_store_quietly() else {
        bail!(
            "«{title}» — не файл и не тайтл: библиотека пуста.\n\
             Создать: yomi library scan КАТАЛОГ"
        );
    };

    let found = store.list_manga(Some(title))?;
    let manga = match found.len() {
        0 => bail!(
            "«{title}» — не файл и не название тайтла из библиотеки.\n\
             Что есть: yomi library"
        ),
        1 => &found[0],
        _ => {
            println!("Под «{title}» подходит несколько тайтлов:");
            for m in &found {
                println!("  {}", m.title);
            }
            bail!("уточните название");
        }
    };

    // Явно указанная глава важнее всего: человек знает, что ищет.
    if let Some(number) = chapter {
        return match store.locate_chapter(manga.id, number)? {
            Some((found, page)) => {
                let position = page
                    .map(|p| format!(", страница {}", p + 1))
                    .unwrap_or_default();
                println!(
                    "{} — глава {number} в {}{}",
                    manga.title,
                    found.label(),
                    position
                );
                Ok((found.path(), page))
            }
            None => bail!(
                "главы {number} нет в библиотеке «{}».\n\
                 Что есть: yomi library \"{}\"",
                manga.title,
                manga.title
            ),
        };
    }

    if let Some(number) = volume {
        return match store.chapter_by_volume(manga.id, number)? {
            Some(found) => {
                println!("{} — {}", manga.title, found.label());
                Ok((found.path(), None))
            }
            None => bail!(
                "тома {number} нет в библиотеке «{}».\n\
                 Что есть: yomi library \"{}\"",
                manga.title,
                manga.title
            ),
        };
    }

    match store.resume_target(manga.id)? {
        Some((found, progress)) => {
            let position = progress
                .map(|p| format!(", страница {}", p.page + 1))
                .unwrap_or_default();
            println!("{} — {}{}", manga.title, found.label(), position);
            Ok((found.path(), None))
        }
        None => bail!(
            "«{}» прочитан целиком. Открыть том явно: yomi read \"{}\" --volume НОМЕР",
            manga.title,
            manga.title
        ),
    }
}

/// Отметки глав: из библиотеки, а при их отсутствии — из самого файла.
///
/// Два источника нужны, потому что разметка приходит двумя путями.
/// Ручные отметки живут в базе, а собранный `yomi build` том несёт
/// закладки внутри `ComicInfo.xml` — и без чтения вторых навигация по
/// главам не работала бы ровно там, где разметка заведомо есть.
///
/// Закладки из файла доступны и когда файла нет в библиотеке: чтобы
/// листать главы, база не нужна.
struct Marks<'a> {
    store: Option<&'a yomi_db::Store>,
    chapter_id: Option<i64>,
    /// Закладки из `ComicInfo.xml`.
    from_file: Vec<u32>,
}

impl Marks<'_> {
    fn stored(&self) -> Vec<u32> {
        let (Some(store), Some(id)) = (self.store, self.chapter_id) else {
            return Vec::new();
        };
        match store.marks_of(id) {
            Ok(marks) => marks.into_iter().map(|m| m.page).collect(),
            Err(e) => {
                tracing::warn!(error = %e, "не удалось прочитать отметки глав");
                Vec::new()
            }
        }
    }
}

impl ChapterMarks for Marks<'_> {
    fn pages(&self) -> Vec<u32> {
        let stored = self.stored();
        if stored.is_empty() {
            self.from_file.clone()
        } else {
            stored
        }
    }

    fn toggle(&mut self, page: u32) -> bool {
        let (Some(store), Some(id)) = (self.store, self.chapter_id) else {
            // Хранить негде: файл вне библиотеки. Закладки из файла
            // остаются доступными для чтения, но правка требует базы.
            tracing::info!("файла нет в библиотеке: отметку сохранять некуда");
            return false;
        };

        // Первая же правка переносит закладки файла в базу целиком:
        // иначе снятая отметка вернулась бы из файла при следующем
        // кадре, и пользователь решил бы, что программа его не слушает.
        if self.stored().is_empty() && !self.from_file.is_empty() {
            for existing in &self.from_file {
                if let Err(e) = store.add_mark(id, *existing, None) {
                    tracing::warn!(error = %e, "не удалось перенести закладку из файла");
                }
            }
        }

        match store.toggle_mark(id, page, None) {
            Ok(added) => added,
            Err(e) => {
                tracing::warn!(error = %e, "не удалось изменить отметку");
                false
            }
        }
    }
}

/// Открывает библиотеку, молча возвращая None при любой проблеме./// Открывает библиотеку, молча возвращая None при любой проблеме.
///
/// У команды `read` есть смысл и без библиотеки: чтение файла не должно
/// зависеть от того, заведена ли база.
fn open_store_quietly() -> Option<yomi_db::Store> {
    let path = yomi_core::paths::database_file().ok()?;
    if !path.exists() {
        return None;
    }
    match yomi_db::Store::open(&path) {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::debug!(error = %e, "библиотека недоступна, читаю без прогресса");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Библиотека с одной записью — возвращает хранилище и её номер.
    fn library_with_entry() -> (yomi_db::Store, i64) {
        use yomi_db::model::{ScannedChapter, ScannedManga};
        let mut store = yomi_db::Store::open_in_memory().unwrap();
        let scanned = ScannedManga {
            external_id: "/м/Тайтл".into(),
            title: "Тайтл".into(),
            authors: vec![],
            genres: vec![],
            status: "unknown".into(),
            year: None,
            description: None,
            chapters: vec![ScannedChapter {
                external_id: "/м/Тайтл/том.cbz".into(),
                number: None,
                volume: Some(1),
                title: None,
                language: "ru".into(),
                scanlator: None,
                page_count: Some(40),
                kind: "volume".into(),
            }],
        };
        let (manga_id, _) = store.upsert_scanned(&scanned).unwrap();
        let id = store.chapters_of(manga_id).unwrap()[0].id;
        (store, id)
    }

    #[test]
    fn file_bookmarks_are_used_when_the_library_has_none() {
        // Собранный `yomi build` том несёт закладки внутри себя —
        // без этого навигация не работала бы сразу после сборки.
        let (store, id) = library_with_entry();
        let marks = Marks {
            store: Some(&store),
            chapter_id: Some(id),
            from_file: vec![0, 10, 25],
        };
        assert_eq!(marks.pages(), vec![0, 10, 25]);
    }

    #[test]
    fn manual_marks_take_priority_over_the_file() {
        let (store, id) = library_with_entry();
        store.add_mark(id, 7, None).unwrap();

        let marks = Marks {
            store: Some(&store),
            chapter_id: Some(id),
            from_file: vec![0, 10, 25],
        };
        assert_eq!(marks.pages(), vec![7], "правка пользователя важнее файла");
    }

    #[test]
    fn first_edit_imports_file_bookmarks_so_nothing_reappears() {
        // Иначе снятая отметка вернулась бы из файла на следующем кадре.
        let (store, id) = library_with_entry();
        let mut marks = Marks {
            store: Some(&store),
            chapter_id: Some(id),
            from_file: vec![0, 10, 25],
        };

        assert!(!marks.toggle(10), "повторное нажатие снимает отметку");
        assert_eq!(
            marks.pages(),
            vec![0, 25],
            "остальные закладки должны уцелеть"
        );
    }

    #[test]
    fn bookmarks_are_readable_without_a_library() {
        let mut marks = Marks {
            store: None,
            chapter_id: None,
            from_file: vec![0, 12],
        };
        assert_eq!(marks.pages(), vec![0, 12], "листать главы можно и без базы");
        assert!(!marks.toggle(5), "а сохранять правку некуда");
        assert_eq!(marks.pages(), vec![0, 12]);
    }

    #[test]
    fn auto_renderer_resolves_from_environment() {
        // Пустое окружение детерминированно даёт блоки — единственный
        // протокол, гарантированно работающий без графических escape-кодов.
        std::env::remove_var("KITTY_WINDOW_ID");
        std::env::remove_var("TERM_PROGRAM");
        assert_eq!(resolve_protocol(Renderer::Auto), Protocol::Blocks);
    }

    #[test]
    fn direction_maps_from_config_without_surprises() {
        assert_eq!(
            resolve_direction(ReadingDirection::RightToLeft),
            Direction::RightToLeft
        );
        assert_eq!(
            resolve_direction(ReadingDirection::Webtoon),
            Direction::Webtoon
        );
    }

    #[test]
    fn explicit_renderer_choice_is_never_overridden() {
        assert_eq!(resolve_protocol(Renderer::Kitty), Protocol::Kitty);
        assert_eq!(resolve_protocol(Renderer::Sixel), Protocol::Sixel);
        assert_eq!(resolve_protocol(Renderer::Iterm2), Protocol::Iterm2);
    }
}
