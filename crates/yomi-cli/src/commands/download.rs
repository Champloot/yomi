//! `yomi download` — загрузка глав. Этап M3.

use super::{not_implemented, Ctx};
use crate::cli::DownloadArgs;
use anyhow::{Context, Result};

pub async fn run(ctx: &Ctx, args: &DownloadArgs) -> Result<()> {
    let source = ctx
        .registry
        .get(&args.source)
        .with_context(|| format!("источник «{}»", args.source))?;

    let output = match &args.output {
        Some(p) => p.clone(),
        None => ctx.config.download_dir()?,
    };

    // Проверяем, что тайтл вообще существует — это уже работает.
    let manga = source.manga(&args.manga_id).await?;
    let chapters = source.chapters(&args.manga_id).await?;

    tracing::info!(
        manga = %manga.title,
        chapters = chapters.len(),
        selector = %args.chapters,
        output = %output.display(),
        concurrency = ctx.config.download.concurrency,
        "загрузка подготовлена"
    );

    Err(not_implemented("загрузка глав", "M3"))
}
