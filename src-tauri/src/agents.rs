use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::{AgentProvider, ToolCapability};

pub fn select_supported<'a>(
    provider: &AgentProvider,
    capabilities: &'a [ToolCapability],
) -> Result<(AgentKind, &'a str), String> {
    let mut failures = Vec::new();
    for kind in [AgentKind::Codex, AgentKind::Claude, AgentKind::Opencode] {
        if !matches!(provider, AgentProvider::Auto)
            && select(provider, |candidate| candidate == kind).is_err()
        {
            continue;
        }
        match capabilities.iter().find(|tool| tool.id == kind.as_str()) {
            Some(tool) if tool.detected => {
                if let Some(version) = tool.version.as_deref() {
                    match validate_version(kind, version) {
                        Ok(()) => return Ok((kind, version)),
                        Err(error) => failures.push(error),
                    }
                } else {
                    failures.push(format!("{} version is unavailable", kind.as_str()));
                }
            }
            Some(tool) => failures.push(format!(
                "{}: {}",
                kind.as_str(),
                tool.detail.as_deref().unwrap_or("executable was not found")
            )),
            None => failures.push(format!("{} was not found", kind.as_str())),
        }
    }
    Err(failures.join("; "))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentKind {
    Codex,
    Claude,
    Opencode,
}

impl AgentKind {
    pub fn executable(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Opencode => "opencode",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Opencode => "opencode",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    pub executable: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

pub fn select(
    provider: &AgentProvider,
    detected: impl Fn(AgentKind) -> bool,
) -> Result<AgentKind, String> {
    match provider {
        AgentProvider::Codex if detected(AgentKind::Codex) => Ok(AgentKind::Codex),
        AgentProvider::Claude if detected(AgentKind::Claude) => Ok(AgentKind::Claude),
        AgentProvider::Opencode if detected(AgentKind::Opencode) => Ok(AgentKind::Opencode),
        AgentProvider::Auto => [AgentKind::Codex, AgentKind::Claude, AgentKind::Opencode]
            .into_iter()
            .find(|kind| detected(*kind))
            .ok_or_else(|| "no supported agent is installed".into()),
        _ => Err("selected agent is not installed".into()),
    }
}

pub fn build_command(
    kind: AgentKind,
    version: &str,
    cwd: &Path,
    prompt: &str,
    model: Option<&str>,
    env: BTreeMap<String, String>,
) -> Result<CommandSpec, String> {
    validate_version(kind, version)?;
    let cwd = cwd
        .to_str()
        .ok_or_else(|| "Wiki path is not valid UTF-8".to_string())?;
    let mut args: Vec<String> = match kind {
        AgentKind::Codex => vec![
            "exec",
            "--json",
            "--sandbox",
            "workspace-write",
            "--cd",
            cwd,
        ],
        AgentKind::Claude => vec![
            "-p",
            "--output-format",
            "stream-json",
            "--permission-mode",
            "acceptEdits",
        ],
        AgentKind::Opencode => vec!["run", "--format", "json", "--dir", cwd],
    }
    .into_iter()
    .map(str::to_owned)
    .collect();
    if let Some(model) = model.filter(|value| !value.is_empty()) {
        args.extend(match kind {
            AgentKind::Opencode => ["-m".into(), model.into()],
            _ => ["--model".into(), model.into()],
        });
    }
    args.push(prompt.into());
    Ok(CommandSpec {
        executable: kind.executable().into(),
        args,
        env,
    })
}

pub fn validate_version(kind: AgentKind, version: &str) -> Result<(), String> {
    let number = version
        .split_whitespace()
        .find(|part| {
            part.chars()
                .next()
                .is_some_and(|value| value.is_ascii_digit())
        })
        .ok_or_else(|| format!("cannot determine {} version", kind.as_str()))?;
    let major: u32 = number
        .split('.')
        .next()
        .ok_or_else(|| "missing major version".to_string())?
        .parse()
        .map_err(|_| format!("cannot parse {} version", kind.as_str()))?;
    let supported = match kind {
        AgentKind::Codex => major == 0,
        AgentKind::Claude => major == 2,
        AgentKind::Opencode => major == 1,
    };
    if supported {
        Ok(())
    } else {
        Err(format!(
            "unsupported {} major version: {major}",
            kind.as_str()
        ))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedEvent {
    pub kind: String,
    pub message: Option<String>,
}

pub fn parse_event(line: &str) -> NormalizedEvent {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return NormalizedEvent {
            kind: "message".into(),
            message: Some(line.into()),
        };
    };
    let raw = value
        .get("type")
        .or_else(|| value.get("event"))
        .and_then(Value::as_str)
        .unwrap_or("message");
    let kind = if raw.contains("error") || raw.contains("fail") {
        "failed"
    } else if raw.contains("complete") || raw.contains("result") {
        "completed"
    } else if raw.contains("tool") {
        "tool"
    } else {
        "message"
    };
    let message = [
        "/message",
        "/text",
        "/item/text",
        "/part/text",
        "/message/content/0/text",
        "/content/0/text",
        "/result",
    ]
    .into_iter()
    .find_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
    .map(str::to_owned);
    NormalizedEvent {
        kind: kind.into(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_skips_incompatible_agents_before_processing_starts() {
        let tools = vec![
            ToolCapability {
                id: "codex".into(),
                detected: true,
                authenticated: None,
                version: Some("codex-cli 9.0.0".into()),
                detail: None,
            },
            ToolCapability {
                id: "claude".into(),
                detected: true,
                authenticated: None,
                version: Some("2.1.0".into()),
                detail: None,
            },
        ];
        assert_eq!(
            select_supported(&AgentProvider::Auto, &tools).unwrap().0,
            AgentKind::Claude
        );
        assert!(select_supported(&AgentProvider::Codex, &tools).is_err());
    }

    #[test]
    fn command_builders_use_safe_non_interactive_flags() {
        let spec = build_command(
            AgentKind::Codex,
            "codex-cli 0.146.0",
            Path::new("/tmp/wiki"),
            "$llmwiki process",
            None,
            BTreeMap::new(),
        )
        .expect("command");
        assert_eq!(spec.executable, "codex");
        assert!(spec
            .args
            .windows(2)
            .any(|pair| pair == ["--sandbox", "workspace-write"]));
        assert!(!spec.args.iter().any(|arg| arg.contains("dangerously")));
    }

    #[test]
    fn unknown_major_versions_fail_closed() {
        assert!(build_command(
            AgentKind::Claude,
            "3.0.0",
            Path::new("/tmp/wiki"),
            "task",
            None,
            BTreeMap::new()
        )
        .is_err());
    }

    #[test]
    fn normalizes_supported_agent_event_shapes() {
        assert_eq!(
            parse_event(r#"{"type":"item.completed","item":{"text":"codex done"}}"#)
                .message
                .as_deref(),
            Some("codex done")
        );
        assert_eq!(
            parse_event(
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"claude done"}]}}"#
            )
            .message
            .as_deref(),
            Some("claude done")
        );
        assert_eq!(
            parse_event(r#"{"type":"text","part":{"text":"opencode done"}}"#)
                .message
                .as_deref(),
            Some("opencode done")
        );
    }
}
