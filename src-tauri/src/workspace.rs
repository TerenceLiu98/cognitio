use std::{fs, path::Path};

pub const REQUIRED_DIRECTORIES: [&str; 5] = ["inbox", "processing", "done", "failed", "wiki"];

pub fn initialize(root: &Path) -> Result<(), String> {
    if root.as_os_str().is_empty() {
        return Err("workspace path is empty".into());
    }
    if root.exists() {
        if !root.is_dir() {
            return Err("workspace path is not a directory".into());
        }
        let mut entries = fs::read_dir(root).map_err(|error| format!("read workspace: {error}"))?;
        if entries
            .next()
            .transpose()
            .map_err(|error| format!("read workspace: {error}"))?
            .is_some()
            && !is_initialized(root)
        {
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

pub fn is_initialized(root: &Path) -> bool {
    root.is_dir()
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
    fn creates_required_layout() {
        let root = temp_root("workspace");
        let _ = fs::remove_dir_all(&root);
        initialize(&root).expect("initialize");
        assert!(is_initialized(&root));
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
}
