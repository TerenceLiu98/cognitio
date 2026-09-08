use crate::{
    job_runtime::Execution,
    jobs::{self, JobEvent, JobRecord},
    mineru,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ParseManifest {
    schema_version: u32,
    source_sha256: String,
    mode: String,
    profile_version: String,
    markdown_sha256: String,
    markdown_size: u64,
    completed_at: String,
}

pub async fn prepare(
    run: &Execution,
    output: &Path,
    mode: crate::models::MineruMode,
) -> Result<Option<PathBuf>, String> {
    let record = &run.record;
    if record.force_reparse && output.exists() {
        tokio::fs::remove_dir_all(output)
            .await
            .map_err(|error| format!("clear previous MinerU output: {error}"))?;
    }
    if !record.force_reparse {
        if let Some(existing) = reusable_markdown(output, record, &mode).await {
            run.log("info", "Reusing verified MinerU Markdown");
            return Ok(Some(existing));
        }
    }
    let parse_mode = match mode {
        crate::models::MineruMode::Precision => mineru::ParseMode::Precision,
        crate::models::MineruMode::Flash => mineru::ParseMode::Flash,
    };
    let parse = mineru::parse(&record.input_path, output, parse_mode);
    tokio::pin!(parse);
    let parsed = loop {
        tokio::select! {
            result = &mut parse => break result?,
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                if run.is_cancelled() {
                    return Ok(None);
                }
            }
        }
    };
    let hash_path = parsed.clone();
    let markdown_sha256 = tokio::task::spawn_blocking(move || jobs::sha256(&hash_path))
        .await
        .map_err(|error| format!("join Markdown checksum: {error}"))??;
    let markdown_size = tokio::fs::metadata(&parsed)
        .await
        .map_err(|error| format!("read parsed Markdown metadata: {error}"))?
        .len();
    if markdown_size == 0 {
        return Err("MinerU returned empty Markdown".into());
    }
    let manifest = ParseManifest {
        schema_version: 1,
        source_sha256: record.sha256.clone(),
        mode: mineru_mode_name(&mode).into(),
        profile_version: mineru::PROFILE_VERSION.into(),
        markdown_sha256,
        markdown_size,
        completed_at: chrono::Utc::now().to_rfc3339(),
    };
    let temporary = output.join(".manifest.json.tmp");
    tokio::fs::write(
        &temporary,
        serde_json::to_vec_pretty(&manifest)
            .map_err(|error| format!("serialize MinerU manifest: {error}"))?,
    )
    .await
    .map_err(|error| format!("write MinerU manifest: {error}"))?;
    tokio::fs::rename(&temporary, output.join("manifest.json"))
        .await
        .map_err(|error| format!("finish MinerU manifest: {error}"))?;
    run.apply(JobEvent::ParseReady).await?;
    Ok(Some(parsed))
}

async fn reusable_markdown(
    output: &Path,
    record: &JobRecord,
    mode: &crate::models::MineruMode,
) -> Option<PathBuf> {
    let manifest: ParseManifest =
        serde_json::from_slice(&tokio::fs::read(output.join("manifest.json")).await.ok()?).ok()?;
    if manifest.schema_version != 1
        || manifest.source_sha256 != record.sha256
        || manifest.mode != mineru_mode_name(mode)
        || manifest.profile_version != mineru::PROFILE_VERSION
    {
        return None;
    }
    let markdown = output.join("full.md");
    let metadata = tokio::fs::metadata(&markdown).await.ok()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() != manifest.markdown_size {
        return None;
    }
    let hash_path = markdown.clone();
    let checksum = tokio::task::spawn_blocking(move || jobs::sha256(&hash_path))
        .await
        .ok()?
        .ok()?;
    (checksum == manifest.markdown_sha256).then_some(markdown)
}

fn mineru_mode_name(mode: &crate::models::MineruMode) -> &'static str {
    match mode {
        crate::models::MineruMode::Precision => "precision",
        crate::models::MineruMode::Flash => "flash",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reuses_only_verified_task_markdown() {
        let root = std::env::temp_dir().join(format!(
            "cognitio-markdown-reuse-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let workspace = root.join("workspace");
        for directory in ["inbox", "processing"] {
            tokio::fs::create_dir_all(workspace.join(directory))
                .await
                .expect("workspace directory");
        }
        let source = workspace.join("inbox/paper.pdf");
        tokio::fs::write(&source, b"%PDF-1.4\n")
            .await
            .expect("source PDF");
        let record =
            jobs::create(&workspace, &source, &crate::models::AppSettings::default()).expect("job");
        let output = workspace.join("processing").join(&record.id).join("parsed");
        tokio::fs::create_dir_all(&output)
            .await
            .expect("parse output directory");
        let markdown = output.join("full.md");

        tokio::fs::write(&markdown, b"# Paper\n")
            .await
            .expect("parsed markdown");
        assert_eq!(
            reusable_markdown(&output, &record, &crate::models::MineruMode::Precision).await,
            None
        );

        let manifest = ParseManifest {
            schema_version: 1,
            source_sha256: record.sha256.clone(),
            mode: "flash".into(),
            profile_version: mineru::PROFILE_VERSION.into(),
            markdown_sha256: jobs::sha256(&markdown).expect("checksum"),
            markdown_size: 8,
            completed_at: chrono::Utc::now().to_rfc3339(),
        };
        tokio::fs::write(
            output.join("manifest.json"),
            serde_json::to_vec(&manifest).expect("manifest"),
        )
        .await
        .expect("write manifest");
        assert_eq!(
            reusable_markdown(&output, &record, &crate::models::MineruMode::Precision).await,
            None
        );
        assert_eq!(
            reusable_markdown(&output, &record, &crate::models::MineruMode::Flash).await,
            Some(markdown)
        );
        tokio::fs::remove_dir_all(root).await.expect("cleanup");
    }
}
