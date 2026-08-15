use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const VERSION: &str = "1";
const FILES: [(&str, &str); 4] = [
    (
        "SKILL.md",
        include_str!("../resources/skills/llmwiki/SKILL.md"),
    ),
    (
        "references/paper-format.md",
        include_str!("../resources/skills/llmwiki/references/paper-format.md"),
    ),
    (
        "references/concept-format.md",
        include_str!("../resources/skills/llmwiki/references/concept-format.md"),
    ),
    (
        "references/writing-guide.md",
        include_str!("../resources/skills/llmwiki/references/writing-guide.md"),
    ),
];
const SCRIPT: &str = include_str!("../resources/skills/llmwiki/scripts/parse-paper");

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    version: String,
    files: std::collections::BTreeMap<String, String>,
}

pub fn install_all(home: &Path) -> Result<Vec<PathBuf>, String> {
    let roots = [
        home.join(".codex/skills/llmwiki"),
        home.join(".claude/skills/llmwiki"),
        home.join(".config/opencode/skills/llmwiki"),
    ];
    roots.iter().map(|root| install(root)).collect()
}

fn install(root: &Path) -> Result<PathBuf, String> {
    verify_existing(root)?;
    for (relative, contents) in FILES.into_iter().chain([("scripts/parse-paper", SCRIPT)]) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create Skill directory: {error}"))?;
        }
        write_atomic(&path, contents.as_bytes())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            root.join("scripts/parse-paper"),
            fs::Permissions::from_mode(0o755),
        )
        .map_err(|error| format!("make parse-paper executable: {error}"))?;
    }
    let manifest = expected_manifest();
    let bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("serialize Skill manifest: {error}"))?;
    write_atomic(&root.join(".llmwiki-install.json"), &bytes)?;
    Ok(root.to_path_buf())
}

fn verify_existing(root: &Path) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    let path = root.join(".llmwiki-install.json");
    let json = fs::read_to_string(&path).map_err(|_| {
        format!(
            "refusing to overwrite unmanaged Skill at {}",
            root.display()
        )
    })?;
    let installed: Manifest = serde_json::from_str(&json)
        .map_err(|_| format!("refusing to overwrite modified Skill at {}", root.display()))?;
    for (relative, expected) in installed.files {
        let bytes = fs::read(root.join(&relative))
            .map_err(|_| format!("installed Skill file is missing: {relative}"))?;
        if checksum(&bytes) != expected {
            return Err(format!(
                "refusing to overwrite user-modified Skill file: {relative}"
            ));
        }
    }
    Ok(())
}

fn expected_manifest() -> Manifest {
    let files = FILES
        .into_iter()
        .chain([("scripts/parse-paper", SCRIPT)])
        .map(|(path, contents)| (path.into(), checksum(contents.as_bytes())))
        .collect();
    Manifest {
        version: VERSION.into(),
        files,
    }
}

fn checksum(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension("llmwiki-tmp");
    let mut file =
        fs::File::create(&temporary).map_err(|error| format!("create Skill file: {error}"))?;
    file.write_all(bytes)
        .map_err(|error| format!("write Skill file: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("sync Skill file: {error}"))?;
    fs::rename(temporary, path).map_err(|error| format!("replace Skill file: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_user_modified_installation() {
        let root = std::env::temp_dir().join(format!("llmwiki-skill-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        install(&root).expect("install");
        fs::write(root.join("SKILL.md"), "user edit").expect("edit");
        assert!(install(&root).is_err());
        assert_eq!(
            fs::read_to_string(root.join("SKILL.md")).expect("read"),
            "user edit"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
