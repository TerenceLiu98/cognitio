use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub schema_version: u32,
    pub locale: Locale,
    pub workspace_root: String,
    pub launch_at_login: bool,
    pub after_processing: AfterProcessing,
    pub agent_provider: AgentProvider,
    pub model: Option<String>,
    pub mineru_mode: MineruMode,
    pub repository_visibility: RepositoryVisibility,
    #[serde(default)]
    pub git_remote: String,
    pub hosting_provider: HostingProvider,
    pub site_url: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            locale: Locale::En,
            workspace_root: String::new(),
            launch_at_login: false,
            after_processing: AfterProcessing::MoveToDone,
            agent_provider: AgentProvider::Auto,
            model: None,
            mineru_mode: MineruMode::Precision,
            repository_visibility: RepositoryVisibility::Private,
            git_remote: String::new(),
            hosting_provider: HostingProvider::Cloudflare,
            site_url: String::new(),
        }
    }
}

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Clone, Debug, Deserialize, Serialize)]
        pub enum $name { $(#[serde(rename = $value)] $variant),+ }
    };
}

string_enum!(Locale { En => "en", Zh => "zh" });
string_enum!(AfterProcessing { Keep => "keep", MoveToDone => "move_to_done", Trash => "trash" });
string_enum!(AgentProvider { Auto => "auto", Codex => "codex", Claude => "claude", Opencode => "opencode" });
string_enum!(MineruMode { Precision => "precision", Flash => "flash" });
string_enum!(RepositoryVisibility { Private => "private", Public => "public" });
string_enum!(HostingProvider { Cloudflare => "cloudflare", GithubPages => "github_pages" });

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCapability {
    pub id: String,
    pub detected: bool,
    pub authenticated: Option<bool>,
    pub version: Option<String>,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSummary {
    pub id: String,
    pub filename: String,
    pub state: String,
    pub phase: String,
    pub progress: u8,
    pub agent: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: String,
    pub timestamp: String,
    pub level: String,
    pub message: String,
    pub job_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub configured: bool,
    pub watching: bool,
    pub mineru_token_configured: bool,
    pub settings: AppSettings,
    pub tools: Vec<ToolCapability>,
    pub jobs: Vec<JobSummary>,
    pub logs: Vec<LogEntry>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightItem {
    pub id: String,
    pub ok: bool,
    pub message: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PreflightReport {
    pub ready: bool,
    pub items: Vec<PreflightItem>,
}
