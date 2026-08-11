//! `yomi generate` — автодополнение и man-страница.
//!
//! Генерируются из того же описания команд, что разбирает аргументы,
//! поэтому расходиться со справкой не могут: добавили флаг — он сам
//! появился и в автодополнении, и в man.
//!
//! Вывод идёт в stdout, а не в файл: так пользователь сам решает, куда
//! положить, а сборщик пакета — куда установить.

use crate::cli::{Cli, GenerateCommand};
use anyhow::Result;
use clap::CommandFactory;

pub async fn run(cmd: &GenerateCommand) -> Result<()> {
    let mut command = Cli::command();

    match cmd {
        GenerateCommand::Completions { shell } => {
            clap_complete::generate(*shell, &mut command, "yomi", &mut std::io::stdout());
        }
        GenerateCommand::Manpage => {
            clap_mangen::Man::new(command).render(&mut std::io::stdout())?;
        }
    }
    Ok(())
}
