use std::{fs, path::Path};

pub const REQUIRED_DIRECTORIES: [&str; 5] = ["inbox", "processing", "done", "failed", "wiki"];
pub const COMPLETION_MARKER: &str = ".llmwiki-workspace.json";
const SAFE_METADATA_FILES: [&str; 2] = [".DS_Store", ".localized"];

pub fn initialize(root: &Path) -> Result<(), String> {
    if root.as_os_str().is_empty() {
        return Err("workspace path is empty".into());
    }
    if root.exists() {
        if !root.is_dir() {
            return Err("workspace path is not a directory".into());
        }
        if !can_initialize(root) {
            return Err("workspace must be empty".into());
        }
    } else {
        fs::create_dir_all(root).map_err(|error| format!("create workspace: {error}"))?;
    }

    for directory in REQUIRED_DIRECTORIES {
        fs::create_dir_all(root.join(directory))
            .map_err(|error| format!("create {directory}: {error}"))?;
    }
    Ok(())
}

pub fn can_initialize(root: &Path) -> bool {
    if !root.is_dir() {
        return false;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    entries.flatten().all(|entry| {
        let name = entry.file_name();
        REQUIRED_DIRECTORIES.iter().any(|allowed| name == *allowed)
            || name == COMPLETION_MARKER
            || SAFE_METADATA_FILES.iter().any(|allowed| name == *allowed)
    })
}

pub fn mark_complete(root: &Path) -> Result<(), String> {
    if !root.join("wiki/.git").is_dir() {
        return Err("cannot complete workspace without a Wiki Git repository".into());
    }
    let marker = serde_json::to_vec_pretty(&serde_json::json!({
        "schemaVersion": 1,
        "completedAt": chrono::Utc::now().to_rfc3339()
    }))
    .map_err(|error| format!("serialize workspace marker: {error}"))?;
    fs::write(root.join(COMPLETION_MARKER), marker)
        .map_err(|error| format!("write workspace completion marker: {error}"))
}

pub fn is_initialized(root: &Path) -> bool {
    root.is_dir()
        && root.join(COMPLETION_MARKER).is_file()
        && root.join("wiki/.git").is_dir()
        && REQUIRED_DIRECTORIES
            .iter()
            .all(|directory| root.join(directory).is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("llmwiki-{name}-{}", std::process::id()))
    }

    #[test]
    fn partial_layout_is_not_initialized() {
        let root = temp_root("partial-workspace");
        let _ = fs::remove_dir_all(&root);
        for directory in REQUIRED_DIRECTORIES {
            fs::create_dir_all(root.join(directory)).expect("directory");
        }
        assert!(!is_initialized(&root));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn creates_required_layout() {
        let root = temp_root("workspace");
        let _ = fs::remove_dir_all(&root);
        initialize(&root).expect("initialize");
        assert!(!is_initialized(&root));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_non_empty_unmanaged_directory() {
        let root = temp_root("non-empty");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("notes.txt"), "keep me").expect("fixture");
        assert!(initialize(&root).is_err());
        assert!(root.join("notes.txt").exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn allows_macos_metadata_in_new_workspace() {
        let root = temp_root("macos-metadata");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join(".DS_Store"), "metadata").expect("fixture");

        initialize(&root).expect("initialize");

        assert!(root.join(".DS_Store").is_file());
        for directory in REQUIRED_DIRECTORIES {
            assert!(root.join(directory).is_dir());
        }
        fs::remove_dir_all(root).expect("cleanup");
    }
}
