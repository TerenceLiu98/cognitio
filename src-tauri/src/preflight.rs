use crate::{
    agents, credentials,
    models::{AppSettings, PreflightItem, PreflightReport, ToolCapability},
    tools, wiki, workspace,
};
use std::{path::PathBuf, process::Stdio, time::Duration};

pub async fn run(settings: &AppSettings) -> PreflightReport {
    let workspace_root = settings.workspace_root.clone();
    let mineru_mode = settings.mineru_mode.clone();
    let detected = tokio::task::spawn_blocking(move || {
        let path = PathBuf::from(&workspace_root);
        let initialized = workspace::is_initialized(&path);
        let workspace_ok = !workspace_root.is_empty()
            && (initialized || !path.exists() || workspace::can_initialize(&path));
        let workspace_detail = if initialized {
            Some("Initialized Cognitio workspace".into())
        } else if workspace_ok {
            Some("New, empty, or resumable workspace".into())
        } else {
            Some("Choose a new or Cognitio-managed folder".into())
        };
        (
            workspace_ok,
            workspace_detail,
            tools::detect_all(),
            !matches!(mineru_mode, crate::models::MineruMode::Precision)
                || credentials::is_mineru_token_configured(),
        )
    })
    .await
    .unwrap_or((false, None, Vec::new(), false));
    let (workspace_ok, workspace_detail, capabilities, mineru_ok) = detected;
    let mut items = vec![PreflightItem {
        id: "workspace".into(),
        ok: workspace_ok,
        message: "Workspace is ready for Cognitio".into(),
        detail: workspace_detail,
    }];
    let git = capabilities.iter().find(|tool| tool.id == "git");
    let git_ok = git.is_some_and(|tool| tool.detected);
    items.push(PreflightItem {
        id: "git".into(),
        ok: git_ok,
        message: if git_ok {
            "git is installed"
        } else {
            "git was not found"
        }
        .into(),
        detail: git.and_then(capability_detail),
    });
    let gh = capabilities.iter().find(|tool| tool.id == "gh");
    let gh_ok = gh.is_some_and(|tool| tool.detected && tool.authenticated == Some(true));
    items.push(PreflightItem {
        id: "github-auth".into(),
        ok: gh_ok,
        message: match gh {
            Some(tool) if !tool.detected => "GitHub CLI was not found",
            Some(tool) if tool.authenticated != Some(true) => "GitHub CLI is not authenticated",
            _ => "GitHub CLI is authenticated",
        }
        .into(),
        detail: gh.and_then(capability_detail),
    });
    items.push(agent_preflight_item(settings, &capabilities));
    items.push(PreflightItem {
        id: "mineru".into(),
        ok: mineru_ok,
        message: if mineru_ok {
            "MinerU credential is configured for the selected mode"
        } else {
            "MinerU credential is missing for Precision mode"
        }
        .into(),
        detail: None,
    });
    let remote_ok = wiki::validate_remote(settings.git_remote.trim()).is_ok();
    items.push(PreflightItem {
        id: "remote".into(),
        ok: remote_ok,
        message: if remote_ok {
            "GitHub repository is configured"
        } else {
            "GitHub repository is not configured"
        }
        .into(),
        detail: None,
    });
    let mut identity_ok = true;
    for key in ["user.name", "user.email"] {
        identity_ok &= command_has_output("git", &["config", "--global", "--get", key]).await;
    }
    items.push(PreflightItem {
        id: "git-identity".into(),
        ok: identity_ok,
        message: if identity_ok {
            "Git author identity is configured"
        } else {
            "Git author identity is not configured"
        }
        .into(),
        detail: None,
    });
    PreflightReport {
        ready: items.iter().all(|item| item.ok),
        items,
    }
}

fn agent_preflight_item(settings: &AppSettings, capabilities: &[ToolCapability]) -> PreflightItem {
    match agents::select_supported(&settings.agent_provider, capabilities) {
        Ok((kind, version)) => PreflightItem {
            id: "agent".into(),
            ok: true,
            message: "Supported agent found".into(),
            detail: Some(format!("{} {version}", kind.as_str())),
        },
        Err(error) => PreflightItem {
            id: "agent".into(),
            ok: false,
            message: "No supported agent was found".into(),
            detail: Some(error),
        },
    }
}

fn capability_detail(capability: &ToolCapability) -> Option<String> {
    capability
        .version
        .clone()
        .or_else(|| capability.detail.clone())
}

async fn command_has_output(executable: &str, args: &[&str]) -> bool {
    let Ok(mut command) = tools::tokio_command(executable) else {
        return false;
    };
    tokio::time::timeout(
        Duration::from_secs(15),
        command.args(args).stdin(Stdio::null()).output(),
    )
    .await
    .is_ok_and(|output| {
        output.is_ok_and(|output| output.status.success() && !output.stdout.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AgentProvider;

    fn capability(
        id: &str,
        detected: bool,
        version: Option<&str>,
        detail: Option<&str>,
    ) -> ToolCapability {
        ToolCapability {
            id: id.into(),
            detected,
            authenticated: None,
            version: version.map(str::to_owned),
            detail: detail.map(str::to_owned),
        }
    }

    #[test]
    fn agent_preflight_reports_a_detected_supported_agent() {
        let settings = AppSettings::default();
        let item = agent_preflight_item(
            &settings,
            &[
                capability("codex", true, Some("codex-cli 0.149.0"), None),
                capability("claude", false, None, Some("not found")),
                capability("opencode", false, None, Some("not found")),
            ],
        );

        assert!(item.ok);
        assert_eq!(item.message, "Supported agent found");
        assert_eq!(item.detail.as_deref(), Some("codex codex-cli 0.149.0"));
    }

    #[test]
    fn agent_preflight_does_not_claim_a_missing_agent_is_supported() {
        let settings = AppSettings {
            agent_provider: AgentProvider::Codex,
            ..AppSettings::default()
        };
        let item = agent_preflight_item(
            &settings,
            &[capability(
                "codex",
                false,
                None,
                Some("could not find codex in Cognitio CLI path"),
            )],
        );

        assert!(!item.ok);
        assert_eq!(item.message, "No supported agent was found");
        assert!(item
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("could not find codex")));
    }
}
