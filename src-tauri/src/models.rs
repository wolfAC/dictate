use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use serde::Serialize;
use tokio::io::AsyncWriteExt;

const BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub file: &'static str,
    pub size_mb: u32,
    pub english_only: bool,
    pub description: &'static str,
}

pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        id: "tiny.en",
        name: "Tiny (English)",
        file: "ggml-tiny.en.bin",
        size_mb: 75,
        english_only: true,
        description: "Fastest, least accurate. Good for older machines.",
    },
    ModelInfo {
        id: "base.en",
        name: "Base (English)",
        file: "ggml-base.en.bin",
        size_mb: 142,
        english_only: true,
        description: "Fast and decent. Recommended starting point.",
    },
    ModelInfo {
        id: "small.en",
        name: "Small (English)",
        file: "ggml-small.en.bin",
        size_mb: 466,
        english_only: true,
        description: "Noticeably more accurate, a few seconds per sentence on CPU.",
    },
    ModelInfo {
        id: "base",
        name: "Base (Multilingual)",
        file: "ggml-base.bin",
        size_mb: 142,
        english_only: false,
        description: "99 languages, fast.",
    },
    ModelInfo {
        id: "small",
        name: "Small (Multilingual)",
        file: "ggml-small.bin",
        size_mb: 466,
        english_only: false,
        description: "99 languages, more accurate.",
    },
    ModelInfo {
        id: "large-v3-turbo-q5_0",
        name: "Large v3 Turbo (Multilingual)",
        file: "ggml-large-v3-turbo-q5_0.bin",
        size_mb: 547,
        english_only: false,
        description: "Best accuracy. Slow without a fast CPU or GPU.",
    },
];

pub fn find(id: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.id == id)
}

pub fn path(models_dir: &Path, model: &ModelInfo) -> PathBuf {
    models_dir.join(model.file)
}

/// Downloads a model to `models_dir`, calling `progress(downloaded, total)` as bytes arrive.
/// Writes to a `.part` file first so an interrupted download never looks complete.
pub async fn download(
    models_dir: &Path,
    model: &ModelInfo,
    mut progress: impl FnMut(u64, u64),
) -> anyhow::Result<PathBuf> {
    tokio::fs::create_dir_all(models_dir).await?;
    let dest = path(models_dir, model);
    let part = dest.with_extension("bin.part");

    let resp = reqwest::get(format!("{BASE_URL}/{}", model.file))
        .await?
        .error_for_status()?;
    let total = resp.content_length().unwrap_or(model.size_mb as u64 * 1024 * 1024);

    let mut file = tokio::fs::File::create(&part).await?;
    let mut stream = resp.bytes_stream();
    let mut downloaded = 0u64;
    let mut last_reported = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        // Throttle events to roughly every 512 KB.
        if downloaded - last_reported > 512 * 1024 || downloaded == total {
            progress(downloaded, total);
            last_reported = downloaded;
        }
    }
    file.flush().await?;
    drop(file);
    tokio::fs::rename(&part, &dest).await?;
    progress(total, total);
    Ok(dest)
}
