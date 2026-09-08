use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use reqwest::Client;
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;

use crate::credentials;

const API_ROOT: &str = "https://mineru.net/api";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const POLL_INTERVAL: Duration = Duration::from_secs(3);
const PARSE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 10_000;
const MAX_EXPANDED_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_MARKDOWN_BYTES: u64 = 64 * 1024 * 1024;
pub const PROFILE_VERSION: &str = "mineru-api-v1";

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
    parse_with_root(input, output, mode, API_ROOT).await
}

async fn parse_with_root(
    input: &Path,
    output: &Path,
    mode: ParseMode,
    api_root: &str,
) -> Result<PathBuf, String> {
    let validate_input_path = input.to_path_buf();
    let validate_output_path = output.to_path_buf();
    tokio::task::spawn_blocking(move || {
        validate_input(&validate_input_path, mode)?;
        fs::create_dir_all(&validate_output_path)
            .map_err(|error| format!("create parse output: {error}"))
    })
    .await
    .map_err(|error| format!("join MinerU input validation: {error}"))??;
    let client = Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| format!("build HTTP client: {error}"))?;
    match mode {
        ParseMode::Precision => parse_precision(&client, input, output, api_root).await,
        ParseMode::Flash => parse_flash(&client, input, output, api_root).await,
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

async fn parse_flash(
    client: &Client,
    input: &Path,
    output: &Path,
    api_root: &str,
) -> Result<PathBuf, String> {
    let filename = filename(input)?;
    let response = api_json(
        client
            .post(format!("{api_root}/v1/agent/parse/file"))
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
        let response = api_json(client.get(format!("{api_root}/v1/agent/parse/{task_id}"))).await?;
        match field(&response, &["data", "state"])? {
            "done" => {
                let url = field(&response, &["data", "markdown_url"])?;
                let destination = output.join("full.md");
                let temporary = output.join(".full.md.tmp");
                download_to(client, url, &temporary, MAX_MARKDOWN_BYTES).await?;
                let size = tokio::fs::metadata(&temporary)
                    .await
                    .map_err(|error| format!("read Markdown metadata: {error}"))?
                    .len();
                if size == 0 {
                    return Err("MinerU returned empty Markdown".into());
                }
                tokio::fs::rename(&temporary, &destination)
                    .await
                    .map_err(|error| format!("finish Markdown: {error}"))?;
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

async fn parse_precision(
    client: &Client,
    input: &Path,
    output: &Path,
    api_root: &str,
) -> Result<PathBuf, String> {
    let token = tokio::task::spawn_blocking(credentials::mineru_token)
        .await
        .map_err(|error| format!("join Keychain read: {error}"))??;
    let token =
        token.ok_or_else(|| "Precision mode requires a MinerU token in Keychain".to_string())?;
    let filename = filename(input)?;
    let response = api_json(client.post(format!("{api_root}/v4/file-urls/batch")).bearer_auth(&token).json(&json!({
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
                .get(format!("{api_root}/v4/extract-results/batch/{batch_id}"))
                .bearer_auth(&token),
        )
        .await?;
        let result = response
            .pointer("/data/extract_result/0")
            .ok_or_else(|| "MinerU response did not contain a result".to_string())?;
        match field(result, &["state"])? {
            "done" => {
                let url = field(result, &["full_zip_url"])?;
                let archive = output.join(".mineru-result.zip.tmp");
                download_to(client, url, &archive, MAX_DOWNLOAD_BYTES).await?;
                let destination = output.to_path_buf();
                return tokio::task::spawn_blocking(move || {
                    let result = extract_zip(&archive, &destination);
                    let _ = fs::remove_file(archive);
                    result
                })
                .await
                .map_err(|error| format!("join MinerU archive extraction: {error}"))?;
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
    let file = tokio::fs::File::open(input)
        .await
        .map_err(|error| format!("open PDF for upload: {error}"))?;
    let size = file
        .metadata()
        .await
        .map_err(|error| format!("read PDF metadata for upload: {error}"))?
        .len();
    client
        .put(url)
        .timeout(UPLOAD_TIMEOUT)
        .header(reqwest::header::CONTENT_LENGTH, size)
        .body(reqwest::Body::wrap_stream(ReaderStream::new(file)))
        .send()
        .await
        .map_err(|error| format!("upload PDF: {error}"))?
        .error_for_status()
        .map_err(|error| format!("upload PDF: {error}"))?;
    Ok(())
}

async fn download_to(
    client: &Client,
    url: &str,
    destination: &Path,
    maximum: u64,
) -> Result<(), String> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("download MinerU result: {error}"))?
        .error_for_status()
        .map_err(|error| format!("download MinerU result: {error}"))?;
    if response.content_length().is_some_and(|size| size > maximum) {
        return Err(format!("MinerU result exceeds the {} byte limit", maximum));
    }
    let mut file = tokio::fs::File::create(destination)
        .await
        .map_err(|error| format!("create MinerU result: {error}"))?;
    let mut written = 0_u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("read MinerU result: {error}"))?
    {
        written = written.saturating_add(chunk.len() as u64);
        if written > maximum {
            let _ = tokio::fs::remove_file(destination).await;
            return Err(format!("MinerU result exceeds the {} byte limit", maximum));
        }
        file.write_all(&chunk)
            .await
            .map_err(|error| format!("write MinerU result: {error}"))?;
    }
    file.sync_all()
        .await
        .map_err(|error| format!("sync MinerU result: {error}"))
}

fn extract_zip(archive_path: &Path, output: &Path) -> Result<PathBuf, String> {
    let file = fs::File::open(archive_path)
        .map_err(|error| format!("open MinerU result archive: {error}"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("open MinerU result archive: {error}"))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err("MinerU archive contains too many entries".into());
    }
    let mut markdown = None;
    let mut expanded = 0_u64;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| format!("read MinerU archive: {error}"))?;
        let Some(relative) = file.enclosed_name() else {
            return Err("MinerU archive contains an unsafe path".into());
        };
        if file.size() > MAX_ENTRY_BYTES {
            return Err("MinerU archive contains an oversized entry".into());
        }
        expanded = expanded.saturating_add(file.size());
        if expanded > MAX_EXPANDED_BYTES {
            return Err("MinerU archive expands beyond the configured limit".into());
        }
        let is_markdown = relative.file_name().is_some_and(|name| name == "full.md");
        if is_markdown && file.size() > MAX_MARKDOWN_BYTES {
            return Err("MinerU Markdown exceeds the configured limit".into());
        }
        let destination = if is_markdown {
            output.join("full.md")
        } else {
            output.join(&relative)
        };
        if file.is_dir() {
            fs::create_dir_all(&destination)
                .map_err(|error| format!("create archive directory: {error}"))?;
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create archive directory: {error}"))?;
        }
        let temporary = destination.with_extension("cognitio-part");
        let mut target = fs::File::create(&temporary)
            .map_err(|error| format!("create archive file: {error}"))?;
        std::io::copy(&mut file, &mut target)
            .map_err(|error| format!("extract archive file: {error}"))?;
        target
            .flush()
            .map_err(|error| format!("flush archive file: {error}"))?;
        target
            .sync_all()
            .map_err(|error| format!("sync archive file: {error}"))?;
        fs::rename(&temporary, &destination)
            .map_err(|error| format!("finish archive file: {error}"))?;
        if is_markdown {
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
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

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

    #[test]
    fn extracts_markdown_to_a_stable_path() {
        let root = std::env::temp_dir().join(format!(
            "cognitio-mineru-zip-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("test directory");
        let archive_path = root.join("result.zip");
        let file = fs::File::create(&archive_path).expect("archive");
        let mut archive = zip::ZipWriter::new(file);
        archive
            .start_file("nested/full.md", zip::write::SimpleFileOptions::default())
            .expect("entry");
        archive.write_all(b"# Paper\n").expect("markdown");
        archive.finish().expect("finish archive");

        let markdown = extract_zip(&archive_path, &root).expect("extract");
        assert_eq!(markdown, root.join("full.md"));
        assert_eq!(fs::read_to_string(markdown).expect("read"), "# Paper\n");
        assert!(!root.join("full.cognitio-part").exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn flash_parse_uses_the_http_contract_without_real_network() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/parse/file"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 0,
                "data": {
                    "task_id": "task-1",
                    "file_url": server.uri() + "/upload"
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/upload"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/agent/parse/task-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 0,
                "data": {
                    "state": "done",
                    "markdown_url": server.uri() + "/paper.md"
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/paper.md"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"# Parsed\n"))
            .mount(&server)
            .await;

        let root = std::env::temp_dir().join(format!(
            "cognitio-mineru-http-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("test directory");
        let input = root.join("paper.pdf");
        fs::write(&input, b"%PDF-1.4\n").expect("PDF");
        let output = root.join("parsed");
        let markdown = parse_with_root(&input, &output, ParseMode::Flash, &server.uri())
            .await
            .expect("parse");
        assert_eq!(
            fs::read_to_string(markdown).expect("Markdown"),
            "# Parsed\n"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn upload_overrides_the_client_request_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/upload"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(100)))
            .mount(&server)
            .await;

        let root = std::env::temp_dir().join(format!(
            "cognitio-mineru-upload-timeout-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("test directory");
        let input = root.join("paper.pdf");
        fs::write(&input, b"%PDF-1.4\n").expect("PDF");
        let client = Client::builder()
            .timeout(Duration::from_millis(25))
            .build()
            .expect("HTTP client");

        upload(&client, &(server.uri() + "/upload"), &input)
            .await
            .expect("upload should use its dedicated timeout");
        fs::remove_dir_all(root).expect("cleanup");
    }
}
