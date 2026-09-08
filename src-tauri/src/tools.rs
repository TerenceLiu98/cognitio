use std::{
    collections::HashSet,
    env,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
    thread,
    time::{Duration, Instant},
};

use crate::models::ToolCapability;

struct ToolSpec {
    id: &'static str,
    executable: &'static str,
    version_args: &'static [&'static str],
}

const TOOLS: [ToolSpec; 6] = [
    ToolSpec {
        id: "git",
        executable: "git",
        version_args: &["--version"],
    },
    ToolSpec {
        id: "gh",
        executable: "gh",
        version_args: &["--version"],
    },
    ToolSpec {
        id: "node",
        executable: "node",
        version_args: &["--version"],
    },
    ToolSpec {
        id: "codex",
        executable: "codex",
        version_args: &["--version"],
    },
    ToolSpec {
        id: "claude",
        executable: "claude",
        version_args: &["--version"],
    },
    ToolSpec {
        id: "opencode",
        executable: "opencode",
        version_args: &["--version"],
    },
];

static EFFECTIVE_PATH: OnceLock<OsString> = OnceLock::new();

pub fn effective_path() -> &'static OsStr {
    EFFECTIVE_PATH
        .get_or_init(|| {
            build_effective_path(
                env::var_os("PATH").as_deref(),
                login_shell_path().as_deref(),
                env::var_os("HOME").as_deref().map(Path::new),
            )
        })
        .as_os_str()
}

pub fn resolve_executable(executable: &str) -> Result<PathBuf, String> {
    resolve_executable_in(executable, effective_path()).ok_or_else(|| {
        format!(
            "could not find {executable} in Cognitio CLI path ({})",
            effective_path().to_string_lossy()
        )
    })
}

pub fn command(executable: &str) -> Result<Command, String> {
    let path = resolve_executable(executable)?;
    let mut command = Command::new(path);
    command.env("PATH", effective_path());
    Ok(command)
}

pub fn tokio_command(executable: &str) -> Result<tokio::process::Command, String> {
    command(executable).map(tokio::process::Command::from)
}

pub fn detect_all() -> Vec<ToolCapability> {
    TOOLS.iter().map(detect).collect()
}

fn detect(spec: &ToolSpec) -> ToolCapability {
    let mut command = match command(spec.executable) {
        Ok(command) => command,
        Err(error) => {
            return ToolCapability {
                id: spec.id.into(),
                detected: false,
                authenticated: None,
                version: None,
                detail: Some(error),
            };
        }
    };
    match command
        .args(spec.version_args)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .output()
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let version = stdout
                .lines()
                .chain(stderr.lines())
                .find(|line| !line.trim().is_empty())
                .map(|line| line.trim().to_owned());
            let authenticated = (spec.id == "gh").then(gh_authenticated);
            ToolCapability {
                id: spec.id.into(),
                detected: true,
                authenticated,
                version,
                detail: None,
            }
        }
        Ok(output) => ToolCapability {
            id: spec.id.into(),
            detected: false,
            authenticated: None,
            version: None,
            detail: Some(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
        },
        Err(error) => ToolCapability {
            id: spec.id.into(),
            detected: false,
            authenticated: None,
            version: None,
            detail: Some(error.to_string()),
        },
    }
}

fn gh_authenticated() -> bool {
    command("gh")
        .and_then(|mut command| {
            command
                .args(["auth", "status"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|error| error.to_string())
        })
        .is_ok_and(|status| status.success())
}

fn build_effective_path(
    inherited: Option<&OsStr>,
    shell_path: Option<&OsStr>,
    home: Option<&Path>,
) -> OsString {
    let mut directories = Vec::new();
    let mut seen = HashSet::new();
    for source in [inherited, shell_path].into_iter().flatten() {
        append_path_entries(&mut directories, &mut seen, source);
    }

    for directory in [
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/local/bin",
        "/usr/local/sbin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ] {
        append_directory(&mut directories, &mut seen, PathBuf::from(directory));
    }
    if let Some(home) = home {
        for relative in [
            ".local/bin",
            ".cargo/bin",
            ".opencode/bin",
            ".bun/bin",
            ".local/share/pnpm",
            ".volta/bin",
        ] {
            append_directory(&mut directories, &mut seen, home.join(relative));
        }
    }

    env::join_paths(directories).unwrap_or_else(|_| inherited.unwrap_or_default().to_os_string())
}

fn append_path_entries(directories: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: &OsStr) {
    for directory in env::split_paths(path).filter(|directory| !directory.as_os_str().is_empty()) {
        append_directory(directories, seen, directory);
    }
}

fn append_directory(
    directories: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    directory: PathBuf,
) {
    if seen.insert(directory.clone()) {
        directories.push(directory);
    }
}

fn resolve_executable_in(executable: &str, path: &OsStr) -> Option<PathBuf> {
    let candidate = Path::new(executable);
    if candidate.is_absolute() || candidate.components().count() > 1 {
        return is_executable(candidate).then(|| candidate.to_path_buf());
    }
    env::split_paths(path)
        .map(|directory| directory.join(candidate))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(target_os = "macos")]
fn login_shell_path() -> Option<OsString> {
    const MARKER: &str = "__COGNITIO_PATH__";
    let shell = env::var_os("SHELL").unwrap_or_else(|| OsString::from("/bin/zsh"));
    let mut child = Command::new(shell)
        .args(["-lc", "printf '__COGNITIO_PATH__%s\\n' \"$PATH\""])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        if child.try_wait().ok().flatten().is_some() {
            let output = child.wait_with_output().ok()?;
            if !output.status.success() {
                return None;
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            return stdout
                .lines()
                .find_map(|line| line.strip_prefix(MARKER))
                .filter(|path| !path.is_empty())
                .map(OsString::from);
        }
        if started.elapsed() >= Duration::from_secs(3) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(not(target_os = "macos"))]
fn login_shell_path() -> Option<OsString> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn augments_a_finder_style_path_with_cli_directories() {
        let home = Path::new("/Users/tester");
        let path = build_effective_path(
            Some(OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin")),
            Some(OsStr::new("/custom/bin:/usr/bin")),
            Some(home),
        );
        let entries: Vec<_> = env::split_paths(&path).collect();

        assert_eq!(entries[0], PathBuf::from("/usr/bin"));
        assert_eq!(entries[4], PathBuf::from("/custom/bin"));
        assert!(entries.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert!(entries.contains(&home.join(".opencode/bin")));
        assert_eq!(entries.iter().filter(|path| *path == "/usr/bin").count(), 1);
    }

    #[test]
    fn resolves_only_executable_files_from_the_effective_path() {
        let root = env::temp_dir().join(format!(
            "cognitio-tool-path-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("tool directory");
        let executable = root.join("codex");
        fs::write(&executable, "#!/bin/sh\n").expect("tool executable");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))
                .expect("tool permissions");
        }
        let path = env::join_paths([&root]).expect("test PATH");

        assert_eq!(resolve_executable_in("codex", &path), Some(executable));
        assert_eq!(resolve_executable_in("missing", &path), None);
        fs::remove_dir_all(root).expect("cleanup");
    }
}
