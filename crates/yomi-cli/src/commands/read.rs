//! `yomi read` — читалка. Основная работа этапа M1.
//!
//! Здесь только связывание: путь → источник страниц (`yomi_viewer::archive`)
//! → протокол вывода (`yomi_viewer::capability`) → интерактивный цикл
//! (`yomi_viewer::reader`). Вся логика — в крейте `yomi-viewer`, тут её
//! не должно прибавляться.

use super::Ctx;
use crate::cli::ReadArgs;
use anyhow::{bail, Result};
use yomi_core::config::{Fit as ConfigFit, Renderer};
use yomi_viewer::archive::PageSource;
use yomi_viewer::capability::{self, Protocol};
use yomi_viewer::fit::Fit;
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

    tracing::info!(
        path = %args.path.display(),
        pages = source.page_count(),
        protocol = protocol.label_ru(),
        ?fit,
        upscale,
        direction = ?ctx.config.reader.direction,
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
    yomi_viewer::reader::run(&source, opts, start)
        .map_err(|e| anyhow::anyhow!(e).context("отображение страниц"))
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
    fn explicit_renderer_choice_is_never_overridden() {
        assert_eq!(resolve_protocol(Renderer::Kitty), Protocol::Kitty);
        assert_eq!(resolve_protocol(Renderer::Sixel), Protocol::Sixel);
        assert_eq!(resolve_protocol(Renderer::Iterm2), Protocol::Iterm2);
    }
}
