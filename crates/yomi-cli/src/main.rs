//! Точка входа. Здесь только разбор аргументов, настройка логов
//! и передача управления в `commands`. Бизнес-логики быть не должно.

// Мёртвый код в приложении почти всегда означает забытую при
// переделке возможность — например, глобальный флаг, который больше
// никто не читает. Ошибка, а не предупреждение: старые версии rustc
// не замечают неиспользуемые публичные поля в бинарных крейтах, и
// такое доезжает до пользователя незамеченным.
#![deny(dead_code)]

mod cli;
mod commands;
mod exit;
mod logging;

use clap::Parser;
use cli::Cli;

/// Возвращает поведение SIGPIPE по умолчанию.
///
/// Rust игнорирует SIGPIPE, и печать в закрытую трубу превращается в
/// ошибку записи, а `println!` на такой ошибке паникует. В результате
/// обычное `yomi library list | head` падало с паникой и стек-трейсом
/// вместо тихого завершения, как ведут себя все остальные утилиты.
fn restore_sigpipe() {
    // Безопасно: единственный вызов, до запуска потоков.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

fn main() -> std::process::ExitCode {
    restore_sigpipe();
    let cli = Cli::parse();
    logging::init(cli.verbose, cli.quiet);

    // Рантайм создаём вручную, а не через #[tokio::main]: так видно,
    // что запуск асинхронности — осознанный шаг, и легче будет
    // ограничить число потоков под слабые машины.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("не удалось запустить асинхронный рантайм: {e}");
            return exit::Code::Internal.into();
        }
    };

    match runtime.block_on(commands::dispatch(&cli)) {
        Ok(()) => exit::Code::Ok.into(),
        Err(err) => {
            // {:#} печатает всю цепочку причин, добавленных через .context()
            eprintln!("Ошибка: {err:#}");
            exit::Code::from_error(&err).into()
        }
    }
}
