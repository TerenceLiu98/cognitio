use std::{fs, io::Write, path::Path};

use crate::models::AppSettings;

const CONFIG_FILE: &str = "config.json";

pub fn load(config_dir: &Path) -> Result<AppSettings, String> {
    let path = config_dir.join(CONFIG_FILE);
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let contents = fs::read_to_string(&path).map_err(|error| format!("read config: {error}"))?;
    let mut value: serde_json::Value =
        serde_json::from_str(&contents).map_err(|error| format!("parse config: {error}"))?;
    let version = value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    let migrated = version != 3;
    if version == 1 {
        let object = value
            .as_object_mut()
            .ok_or_else(|| "config must be a JSON object".to_string())?;
        object.remove("hostingProvider");
        object.insert("siteTitle".into(), serde_json::json!("Research Library"));
        object.insert("schemaVersion".into(), serde_json::json!(3));
    } else if version == 2 {
        let object = value
            .as_object_mut()
            .ok_or_else(|| "config must be a JSON object".to_string())?;
        object.insert("siteTitle".into(), serde_json::json!("Research Library"));
        object.insert("schemaVersion".into(), serde_json::json!(3));
    } else if version != 3 {
        return Err(format!("unsupported settings schema version {version}"));
    }
    let settings =
        serde_json::from_value(value).map_err(|error| format!("parse config: {error}"))?;
    if migrated {
        save(config_dir, &settings)?;
    }
    Ok(settings)
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

    #[test]
    fn migrates_cloudflare_v1_settings_to_github_pages_v3() {
        let root = std::env::temp_dir().join(format!(
            "llmwiki-config-migration-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("config directory");
        fs::write(
            root.join(CONFIG_FILE),
            serde_json::json!({
                "schemaVersion": 1,
                "locale": "en",
                "workspaceRoot": "",
                "launchAtLogin": false,
                "afterProcessing": "move_to_done",
                "agentProvider": "auto",
                "model": null,
                "mineruMode": "flash",
                "repositoryVisibility": "private",
                "gitRemote": "owner/repo",
                "hostingProvider": "cloudflare",
                "siteUrl": ""
            })
            .to_string(),
        )
        .expect("legacy config");
        let settings = load(&root).expect("migrated config");
        assert_eq!(settings.schema_version, 3);
        assert_eq!(settings.git_remote, "owner/repo");
        assert_eq!(settings.site_title, "Research Library");
        let persisted: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join(CONFIG_FILE)).expect("persisted config"),
        )
        .expect("valid persisted config");
        assert_eq!(persisted["schemaVersion"], 3);
        assert_eq!(persisted["siteTitle"], "Research Library");
        assert!(persisted.get("hostingProvider").is_none());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn adds_site_title_when_migrating_v2_settings() {
        let root = std::env::temp_dir().join(format!(
            "llmwiki-config-v2-migration-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("config directory");
        let mut settings = serde_json::to_value(AppSettings::default()).expect("settings");
        let object = settings.as_object_mut().expect("settings object");
        object.insert("schemaVersion".into(), serde_json::json!(2));
        object.remove("siteTitle");
        fs::write(root.join(CONFIG_FILE), settings.to_string()).expect("legacy config");

        let settings = load(&root).expect("migrated config");

        assert_eq!(settings.schema_version, 3);
        assert_eq!(settings.site_title, "Research Library");
        let persisted: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join(CONFIG_FILE)).expect("persisted config"),
        )
        .expect("valid persisted config");
        assert_eq!(persisted["schemaVersion"], 3);
        assert_eq!(persisted["siteTitle"], "Research Library");
        fs::remove_dir_all(root).expect("cleanup");
    }
}
