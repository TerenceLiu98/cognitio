use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::models::{AppSettings, InitializationSummary};

const JOURNAL_FILE: &str = "initialization.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializationJournal {
    pub workspace_root: PathBuf,
    pub settings: AppSettings,
    pub summary: InitializationSummary,
}

pub fn load(config_dir: &Path) -> Result<Option<InitializationJournal>, String> {
    let path = config_dir.join(JOURNAL_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|error| format!("read initialization journal: {error}"))?;
    let mut journal: InitializationJournal = serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse initialization journal: {error}"))?;
    if journal.summary.state == "running" {
        journal.summary.state = "interrupted".into();
        journal.summary.error =
            Some("Initialization was interrupted. Retry to continue safely.".into());
        journal.summary.can_retry = true;
        save(config_dir, &journal)?;
    }
    Ok(Some(journal))
}

pub fn save(config_dir: &Path, journal: &InitializationJournal) -> Result<(), String> {
    fs::create_dir_all(config_dir)
        .map_err(|error| format!("create initialization state directory: {error}"))?;
    let destination = config_dir.join(JOURNAL_FILE);
    let temporary = config_dir.join(format!(".{JOURNAL_FILE}.tmp"));
    let bytes = serde_json::to_vec_pretty(journal)
        .map_err(|error| format!("serialize initialization journal: {error}"))?;
    let mut file = fs::File::create(&temporary)
        .map_err(|error| format!("create initialization journal: {error}"))?;
    file.write_all(&bytes)
        .map_err(|error| format!("write initialization journal: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("sync initialization journal: {error}"))?;
    fs::rename(temporary, destination)
        .map_err(|error| format!("replace initialization journal: {error}"))
}

pub fn clear(config_dir: &Path) -> Result<(), String> {
    let path = config_dir.join(JOURNAL_FILE);
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("remove initialization journal: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_journal_becomes_interrupted_on_load() {
        let root = std::env::temp_dir().join(format!(
            "llmwiki-initialization-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let journal = InitializationJournal {
            workspace_root: root.join("workspace"),
            settings: AppSettings::default(),
            summary: InitializationSummary {
                state: "running".into(),
                ..InitializationSummary::default()
            },
        };
        save(&root, &journal).expect("save journal");
        let loaded = load(&root).expect("load journal").expect("journal");
        assert_eq!(loaded.summary.state, "interrupted");
        assert!(loaded.summary.can_retry);
        fs::remove_dir_all(root).expect("cleanup");
    }
}
