use std::process::{Command, Stdio};

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

pub fn detect_all() -> Vec<ToolCapability> {
    TOOLS.iter().map(detect).collect()
}

fn detect(spec: &ToolSpec) -> ToolCapability {
    match Command::new(spec.executable)
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
    Command::new("gh")
        .args(["auth", "status"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
