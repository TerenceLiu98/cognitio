use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::Path,
};

use chrono::Utc;

use crate::models::LogEntry;

const LOG_FILE: &str = "activity.jsonl";
const MAX_BYTES: u64 = 5 * 1024 * 1024;
const MAX_LOADED_ENTRIES: usize = 200;

pub fn append(
    config_dir: &Path,
    level: &str,
    message: &str,
    job_id: Option<&str>,
) -> Result<LogEntry, String> {
    fs::create_dir_all(config_dir).map_err(|error| format!("create log directory: {error}"))?;
    let path = config_dir.join(LOG_FILE);
    rotate_if_needed(&path)?;
    let entry = LogEntry {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: Utc::now().to_rfc3339(),
        level: level.into(),
        message: redact(message),
        job_id: job_id.map(str::to_owned),
    };
    let line =
        serde_json::to_string(&entry).map_err(|error| format!("serialize log entry: {error}"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("open log: {error}"))?;
    writeln!(file, "{line}").map_err(|error| format!("append log: {error}"))?;
    Ok(entry)
}

pub fn load(config_dir: &Path) -> Vec<LogEntry> {
    let Ok(file) = fs::File::open(config_dir.join(LOG_FILE)) else {
        return Vec::new();
    };
    let mut entries: Vec<_> = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect();
    if entries.len() > MAX_LOADED_ENTRIES {
        entries.drain(..entries.len() - MAX_LOADED_ENTRIES);
    }
    entries
}

fn rotate_if_needed(path: &Path) -> Result<(), String> {
    if path
        .metadata()
        .is_ok_and(|metadata| metadata.len() >= MAX_BYTES)
    {
        let rotated = path.with_extension("jsonl.1");
        if rotated.exists() {
            fs::remove_file(&rotated).map_err(|error| format!("remove old log: {error}"))?;
        }
        fs::rename(path, rotated).map_err(|error| format!("rotate log: {error}"))?;
    }
    Ok(())
}

pub fn redact(message: &str) -> String {
    let mut redact_next = false;
    message
        .split_whitespace()
        .map(|word| {
            if redact_next {
                redact_next = false;
                return "[REDACTED]".into();
            }
            let lower = word.to_ascii_lowercase();
            if lower.starts_with("mineru_token=")
                || lower.starts_with("api_key=")
                || lower.starts_with("authorization=")
            {
                word.split_once('=').map_or_else(
                    || "[REDACTED]".into(),
                    |(key, _)| format!("{key}=[REDACTED]"),
                )
            } else if lower.trim_matches(|character: char| !character.is_ascii_alphanumeric())
                == "bearer"
            {
                redact_next = true;
                word.into()
            } else if (word.starts_with("http://") || word.starts_with("https://"))
                && word.contains('@')
            {
                let scheme_end = word.find("://").map_or(0, |index| index + 3);
                let host = word.rsplit_once('@').map_or(word, |(_, host)| host);
                format!("{}[REDACTED]@{host}", &word[..scheme_end])
            } else {
                word.into()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn redacts_tokens_and_url_credentials() {
        let value = redact(
            "MINERU_TOKEN=secret Authorization: Bearer secret2 push https://user:pass@example.com/repo",
        );
        assert_eq!(
            value,
            "MINERU_TOKEN=[REDACTED] Authorization: Bearer [REDACTED] push https://[REDACTED]@example.com/repo"
        );
    }
}
