use std::{fs, io::Write, path::Path};

use crate::models::AppSettings;

const CONFIG_FILE: &str = "config.json";

pub fn load(config_dir: &Path) -> Result<AppSettings, String> {
    let path = config_dir.join(CONFIG_FILE);
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let contents = fs::read_to_string(&path).map_err(|error| format!("read config: {error}"))?;
    serde_json::from_str(&contents).map_err(|error| format!("parse config: {error}"))
}

pub fn save(config_dir: &Path, settings: &AppSettings) -> Result<(), String> {
    fs::create_dir_all(config_dir).map_err(|error| format!("create config directory: {error}"))?;
    let destination = config_dir.join(CONFIG_FILE);
    let temporary = config_dir.join(format!(".{CONFIG_FILE}.tmp"));
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("serialize config: {error}"))?;
    let mut file = fs::File::create(&temporary)
        .map_err(|error| format!("create temporary config: {error}"))?;
    file.write_all(&bytes)
        .map_err(|error| format!("write temporary config: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("sync temporary config: {error}"))?;
    fs::rename(&temporary, &destination).map_err(|error| format!("replace config: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_uses_defaults_and_round_trips() {
        let root = std::env::temp_dir().join(format!("llmwiki-config-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let mut settings = load(&root).expect("defaults");
        settings.workspace_root = "/tmp/wiki".into();
        save(&root, &settings).expect("save");
        assert_eq!(load(&root).expect("load").workspace_root, "/tmp/wiki");
        fs::remove_dir_all(root).expect("cleanup");
    }
}
