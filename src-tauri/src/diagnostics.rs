use std::{fs, io::Write, path::Path};

use crate::{jobs, logging, models::AppSnapshot};

pub fn export(destination: &Path, snapshot: &AppSnapshot, config_dir: &Path) -> Result<(), String> {
    let file = fs::File::create(destination)
        .map_err(|error| format!("create diagnostics archive: {error}"))?;
    let mut archive = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut safe_snapshot = snapshot.clone();
    safe_snapshot.settings.git_remote = logging::redact(&safe_snapshot.settings.git_remote);
    safe_snapshot.settings.site_url = logging::redact(&safe_snapshot.settings.site_url);
    add(
        &mut archive,
        "snapshot.json",
        &serde_json::to_vec_pretty(&safe_snapshot)
            .map_err(|error| format!("serialize diagnostics: {error}"))?,
        options,
    )?;
    if let Ok(logs) = fs::read(config_dir.join("activity.jsonl")) {
        add(&mut archive, "activity.jsonl", &logs, options)?;
    }
    let workspace = Path::new(&snapshot.settings.workspace_root);
    for record in jobs::load_all(workspace) {
        let bytes = serde_json::to_vec_pretty(&record)
            .map_err(|error| format!("serialize job diagnostics: {error}"))?;
        add(
            &mut archive,
            &format!("jobs/{}/job.json", record.id),
            &bytes,
            options,
        )?;
    }
    archive
        .finish()
        .map_err(|error| format!("finish diagnostics archive: {error}"))?;
    Ok(())
}

fn add<W: Write + std::io::Seek>(
    archive: &mut zip::ZipWriter<W>,
    name: &str,
    bytes: &[u8],
    options: zip::write::SimpleFileOptions,
) -> Result<(), String> {
    archive
        .start_file(name, options)
        .map_err(|error| format!("create diagnostics entry: {error}"))?;
    archive
        .write_all(bytes)
        .map_err(|error| format!("write diagnostics entry: {error}"))
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use crate::models::{AppSettings, AppSnapshot};

    use super::*;

    #[test]
    fn exported_archive_does_not_contain_credentials() {
        let root = std::env::temp_dir().join(format!(
            "llmwiki-diagnostics-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let config_dir = root.join("config");
        fs::create_dir_all(&config_dir).expect("config directory");
        logging::append(
            &config_dir,
            "error",
            "Authorization: Bearer diagnostic-secret",
            None,
        )
        .expect("log entry");
        let settings = AppSettings {
            workspace_root: root.join("workspace").to_string_lossy().into_owned(),
            git_remote: "https://user:remote-secret@github.com/owner/repo".into(),
            ..AppSettings::default()
        };
        let snapshot = AppSnapshot {
            ready: true,
            configured: false,
            watching: false,
            mineru_token_configured: true,
            settings,
            tools: Vec::new(),
            jobs: Vec::new(),
            logs: Vec::new(),
            initialization: Default::default(),
        };
        let destination = root.join("diagnostics.zip");
        export(&destination, &snapshot, &config_dir).expect("diagnostics export");

        let file = fs::File::open(&destination).expect("archive");
        let mut archive = zip::ZipArchive::new(file).expect("zip archive");
        let mut contents = String::new();
        for index in 0..archive.len() {
            archive
                .by_index(index)
                .expect("archive entry")
                .read_to_string(&mut contents)
                .expect("text entry");
        }
        assert!(!contents.contains("diagnostic-secret"));
        assert!(!contents.contains("remote-secret"));
        fs::remove_dir_all(root).expect("cleanup");
    }
}
