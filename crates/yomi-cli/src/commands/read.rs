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
    if !args.path.exists() {
        bail!("путь не существует: {}", args.path.display());
    }

    // CBR/RAR узнаём раньше PageSource::open: у отказа есть конкретная
    // причина (несвободная лицензия unrar, см. ADR-0006), и пользователь
    // должен увидеть её, а не обезличенное «формат не поддерживается».
    if let Some(ext) = args.path.extension().and_then(|e| e.to_str()) {
        if matches!(ext.to_lowercase().as_str(), "cbr" | "rar") {
            bail!("CBR/RAR пока не поддерживается, см. docs/adr/0006-formats.md");
        }
    }

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

    let source = PageSource::open(&args.path)
        .map_err(|e| anyhow::anyhow!(e).context(format!("открытие {}", args.path.display())))?;

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
        path = %args.path.display(),
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
    let known_chapter = store.as_ref().and_then(|s| {
        s.chapter_by_path(&args.path.to_string_lossy())
            .ok()
            .flatten()
    });

    // Явно указанная страница всегда важнее сохранённого прогресса.
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

    let outcome = yomi_viewer::reader::run(
        &source,
        opts,
        direction,
        ctx.config.reader.preload_pages,
        start,
    );

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

/// Открывает библиотеку, молча возвращая None при любой проблеме.
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
