use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use reqwest::Client;
use serde_json::{json, Value};

use crate::credentials;

const API_ROOT: &str = "https://mineru.net/api";
const POLL_INTERVAL: Duration = Duration::from_secs(3);
const PARSE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseMode {
    Precision,
    Flash,
}

impl ParseMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "precision" => Ok(Self::Precision),
            "flash" => Ok(Self::Flash),
            _ => Err("mode must be precision or flash".into()),
        }
    }
}

pub async fn parse(input: &Path, output: &Path, mode: ParseMode) -> Result<PathBuf, String> {
    validate_input(input, mode)?;
    fs::create_dir_all(output).map_err(|error| format!("create parse output: {error}"))?;
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| format!("build HTTP client: {error}"))?;
    match mode {
        ParseMode::Precision => parse_precision(&client, input, output).await,
        ParseMode::Flash => parse_flash(&client, input, output).await,
    }
}

fn validate_input(input: &Path, mode: ParseMode) -> Result<(), String> {
    if !input.is_file()
        || !input
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
    {
        return Err("input must be an existing PDF file".into());
    }
    let maximum = if mode == ParseMode::Precision {
        200
    } else {
        10
    } * 1024
        * 1024;
    let size = input
        .metadata()
        .map_err(|error| format!("read PDF metadata: {error}"))?
        .len();
    if size == 0 || size > maximum {
        return Err(format!(
            "PDF size must be between 1 byte and {} MB for this mode",
            maximum / 1024 / 1024
        ));
    }
    Ok(())
}

async fn parse_flash(client: &Client, input: &Path, output: &Path) -> Result<PathBuf, String> {
    let filename = filename(input)?;
    let response = api_json(
        client
            .post(format!("{API_ROOT}/v1/agent/parse/file"))
            .json(&json!({
                "file_name": filename, "enable_table": true, "enable_formula": true, "is_ocr": false
            })),
    )
    .await?;
    let task_id = field(&response, &["data", "task_id"])?;
    let upload_url = field(&response, &["data", "file_url"])?;
    upload(client, upload_url, input).await?;

    let deadline = Instant::now() + PARSE_TIMEOUT;
    loop {
        ensure_before(deadline)?;
        let response = api_json(client.get(format!("{API_ROOT}/v1/agent/parse/{task_id}"))).await?;
        match field(&response, &["data", "state"])? {
            "done" => {
                let url = field(&response, &["data", "markdown_url"])?;
                let bytes = download(client, url).await?;
                let destination = output.join("full.md");
                fs::write(&destination, bytes)
                    .map_err(|error| format!("write Markdown: {error}"))?;
                return Ok(destination);
            }
            "failed" => {
                return Err(optional_field(&response, &["data", "err_msg"])
                    .unwrap_or("MinerU parsing failed")
                    .into())
            }
            _ => tokio::time::sleep(POLL_INTERVAL).await,
        }
    }
}

async fn parse_precision(client: &Client, input: &Path, output: &Path) -> Result<PathBuf, String> {
    let token = std::env::var("MINERU_TOKEN")
        .ok()
        .filter(|value| !value.is_empty())
        .or(credentials::mineru_token()?)
        .ok_or_else(|| "Precision mode requires a MinerU token in Keychain".to_string())?;
    let filename = filename(input)?;
    let response = api_json(client.post(format!("{API_ROOT}/v4/file-urls/batch")).bearer_auth(&token).json(&json!({
        "files": [{ "name": filename }], "model_version": "vlm", "enable_table": true, "enable_formula": true
    }))).await?;
    let batch_id = field(&response, &["data", "batch_id"])?;
    let upload_url = response
        .pointer("/data/file_urls/0")
        .and_then(Value::as_str)
        .ok_or_else(|| "MinerU response did not contain an upload URL".to_string())?;
    upload(client, upload_url, input).await?;

    let deadline = Instant::now() + PARSE_TIMEOUT;
    loop {
        ensure_before(deadline)?;
        let response = api_json(
            client
                .get(format!("{API_ROOT}/v4/extract-results/batch/{batch_id}"))
                .bearer_auth(&token),
        )
        .await?;
        let result = response
            .pointer("/data/extract_result/0")
            .ok_or_else(|| "MinerU response did not contain a result".to_string())?;
        match field(result, &["state"])? {
            "done" => {
                let url = field(result, &["full_zip_url"])?;
                let bytes = download(client, url).await?;
                return extract_zip(&bytes, output);
            }
            "failed" => {
                return Err(optional_field(result, &["err_msg"])
                    .unwrap_or("MinerU parsing failed")
                    .into())
            }
            _ => tokio::time::sleep(POLL_INTERVAL).await,
        }
    }
}

async fn api_json(builder: reqwest::RequestBuilder) -> Result<Value, String> {
    let response = builder
        .send()
        .await
        .map_err(|error| format!("MinerU request failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("MinerU HTTP error: {error}"))?;
    let value: Value = response
        .json()
        .await
        .map_err(|error| format!("decode MinerU response: {error}"))?;
    if value.get("code").and_then(Value::as_i64) != Some(0) {
        return Err(optional_field(&value, &["msg"])
            .unwrap_or("MinerU returned an error")
            .into());
    }
    Ok(value)
}

async fn upload(client: &Client, url: &str, input: &Path) -> Result<(), String> {
    let bytes = fs::read(input).map_err(|error| format!("read PDF for upload: {error}"))?;
    client
        .put(url)
        .body(bytes)
        .send()
        .await
        .map_err(|error| format!("upload PDF: {error}"))?
        .error_for_status()
        .map_err(|error| format!("upload PDF: {error}"))?;
    Ok(())
}

async fn download(client: &Client, url: &str) -> Result<Vec<u8>, String> {
    client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("download MinerU result: {error}"))?
        .error_for_status()
        .map_err(|error| format!("download MinerU result: {error}"))?
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| format!("read MinerU result: {error}"))
}

fn extract_zip(bytes: &[u8], output: &Path) -> Result<PathBuf, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("open MinerU result archive: {error}"))?;
    let mut markdown = None;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| format!("read MinerU archive: {error}"))?;
        let Some(relative) = file.enclosed_name() else {
            return Err("MinerU archive contains an unsafe path".into());
        };
        let destination = output.join(&relative);
        if file.is_dir() {
            fs::create_dir_all(&destination)
                .map_err(|error| format!("create archive directory: {error}"))?;
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create archive directory: {error}"))?;
        }
        let mut target = fs::File::create(&destination)
            .map_err(|error| format!("create archive file: {error}"))?;
        std::io::copy(&mut file, &mut target)
            .map_err(|error| format!("extract archive file: {error}"))?;
        target
            .flush()
            .map_err(|error| format!("flush archive file: {error}"))?;
        if relative.file_name().is_some_and(|name| name == "full.md") {
            markdown = Some(destination);
        }
    }
    markdown.ok_or_else(|| "MinerU result archive did not contain full.md".into())
}

fn filename(path: &Path) -> Result<&str, String> {
    path.file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "PDF filename is not valid UTF-8".into())
}
fn field<'a>(value: &'a Value, path: &[&str]) -> Result<&'a str, String> {
    optional_field(value, path)
        .ok_or_else(|| format!("MinerU response is missing {}", path.join(".")))
}
fn optional_field<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(Value::as_str)
}
fn ensure_before(deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline {
        Err("MinerU parsing timed out".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mode_is_fail_closed() {
        assert_eq!(ParseMode::parse("flash"), Ok(ParseMode::Flash));
        assert!(ParseMode::parse("fast").is_err());
    }

    #[test]
    fn reads_nested_response_fields() {
        let value = json!({"data": {"state": "done"}});
        assert_eq!(field(&value, &["data", "state"]), Ok("done"));
    }
}
