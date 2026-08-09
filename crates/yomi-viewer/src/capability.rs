//! Определение того, какой протокол вывода изображений умеет терминал.
//!
//! Реализует цепочку деградации из ADR-0003: kitty → iTerm2 → Sixel → блоки.
//! Функция [`detect`] чистая — принимает срез переменных окружения, а не
//! читает их сама. Это осознанное решение: без него протестировать
//! определение возможностей нельзя, не подменяя реальное окружение процесса
//! (а тесты, как правило, гоняются параллельно и делят одно окружение).

use std::collections::HashMap;

/// Протокол вывода изображений, от лучшего к худшему.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// Графический протокол kitty. Поддерживают kitty, ghostty, WezTerm.
    Kitty,
    /// Протокол инлайн-картинок iTerm2. Поддерживают также WezTerm, Konsole.
    Iterm2,
    /// Sixel. Поддерживают foot, xterm (с ключом -ti), mlterm, WezTerm.
    Sixel,
    /// Юникод-блоки через ANSI 24-бит цвет. Работает всегда и везде —
    /// последний рубеж деградации, единственный протокол без картинок.
    Blocks,
}

impl Protocol {
    pub fn label_ru(&self) -> &'static str {
        match self {
            Self::Kitty => "kitty graphics",
            Self::Iterm2 => "iTerm2 inline images",
            Self::Sixel => "Sixel",
            Self::Blocks => "юникод-блоки",
        }
    }
}

/// Определяет протокол по переменным окружения.
///
/// Не делает запросов к терминалу (DA1/XTGETTCAP) — те асинхронны, требуют
/// сырого режима терминала и тайм-аута, и живут в [`crate::terminal`].
/// Здесь — быстрый и надёжный первый рубеж: большинство терминалов честно
/// объявляют себя через окружение.
pub fn detect(env: &HashMap<String, String>) -> Protocol {
    let get = |k: &str| env.get(k).map(String::as_str).unwrap_or_default();

    // kitty и его потомки (ghostty) выставляют собственную переменную.
    if !get("KITTY_WINDOW_ID").is_empty() {
        return Protocol::Kitty;
    }

    let term_program = get("TERM_PROGRAM");
    let term = get("TERM");

    // WezTerm умеет и kitty, и iTerm2, и Sixel; берём лучший — kitty.
    if term_program == "WezTerm" {
        return Protocol::Kitty;
    }

    // ghostty может не выставлять KITTY_WINDOW_ID, но объявляет себя в TERM_PROGRAM.
    if term_program == "ghostty" {
        return Protocol::Kitty;
    }

    // Настоящий терминал iTerm2 на macOS — не наша основная аудитория
    // (Linux), но проверка дешёвая и не помешает при работе по SSH с Mac.
    if term_program == "iTerm.app" || term_program == "WezTerm" {
        return Protocol::Iterm2;
    }

    // Konsole поддерживает протокол iTerm2 с версии 22.04.
    if !get("KONSOLE_VERSION").is_empty() {
        return Protocol::Iterm2;
    }

    // foot — нативный Sixel.
    if term == "foot" || term.starts_with("foot") {
        return Protocol::Sixel;
    }

    // mlterm объявляет себя через TERM или переменную MLTERM.
    if !get("MLTERM").is_empty() {
        return Protocol::Sixel;
    }

    // xterm умеет Sixel только при явной компиляции с --enable-sixel-graphics;
    // проверить это через окружение нельзя. Не рискуем: xterm остаётся
    // на блоках, точное определение — через DA1-запрос в terminal.rs.

    Protocol::Blocks
}

/// Удобная обёртка над [`detect`], читающая настоящее окружение процесса.
pub fn detect_from_process_env() -> Protocol {
    let env: HashMap<String, String> = std::env::vars().collect();
    detect(&env)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn kitty_window_id_wins_over_everything() {
        let e = env(&[("KITTY_WINDOW_ID", "1"), ("TERM_PROGRAM", "iTerm.app")]);
        assert_eq!(detect(&e), Protocol::Kitty);
    }

    #[test]
    fn wezterm_prefers_kitty_protocol() {
        let e = env(&[("TERM_PROGRAM", "WezTerm")]);
        assert_eq!(detect(&e), Protocol::Kitty);
    }

    #[test]
    fn ghostty_is_kitty_protocol() {
        let e = env(&[("TERM_PROGRAM", "ghostty")]);
        assert_eq!(detect(&e), Protocol::Kitty);
    }

    #[test]
    fn iterm_app_is_iterm2() {
        let e = env(&[("TERM_PROGRAM", "iTerm.app")]);
        assert_eq!(detect(&e), Protocol::Iterm2);
    }

    #[test]
    fn konsole_is_iterm2() {
        let e = env(&[("KONSOLE_VERSION", "22.08.0")]);
        assert_eq!(detect(&e), Protocol::Iterm2);
    }

    #[test]
    fn foot_is_sixel() {
        let e = env(&[("TERM", "foot")]);
        assert_eq!(detect(&e), Protocol::Sixel);
    }

    #[test]
    fn foot_extended_term_is_sixel() {
        let e = env(&[("TERM", "foot-extra")]);
        assert_eq!(detect(&e), Protocol::Sixel);
    }

    #[test]
    fn unknown_terminal_falls_back_to_blocks() {
        let e = env(&[("TERM", "xterm-256color")]);
        assert_eq!(detect(&e), Protocol::Blocks);
    }

    #[test]
    fn empty_environment_falls_back_to_blocks() {
        assert_eq!(detect(&HashMap::new()), Protocol::Blocks);
    }

    #[test]
    fn alacritty_falls_back_to_blocks() {
        // Alacritty принципиально не поддерживает графические протоколы —
        // это задокументированное поведение, не пробел в определении.
        let e = env(&[("TERM", "alacritty")]);
        assert_eq!(detect(&e), Protocol::Blocks);
    }
}
