//! Точка входа. Здесь только разбор аргументов, настройка логов
//! и передача управления в `commands`. Бизнес-логики быть не должно.

mod cli;
mod commands;
mod exit;
mod logging;

use clap::Parser;
use cli::Cli;

fn main() -> std::process::ExitCode {
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
