use std::env;
use std::io::{BufReader, BufWriter, IsTerminal, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_LENGTH};
use reqwest::{Body, Client, Response, StatusCode};
use serde::Deserialize;
use tempfile::NamedTempFile;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt};
use tokio_util::io::ReaderStream;

/// How many times a single fragment is retried before the upload is abandoned.
const MAX_RETRIES: u32 = 3;

#[derive(Parser, Debug)]
#[command(name = "cc-store", about = "Upload and download files via your configured API")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Upload a file or folder (folders are sent as a .tar.gz). Large uploads are split into fragments automatically.
    Upload {
        path: PathBuf,
        #[arg(short = 'k', long = "keep", value_name = "DURATION", value_parser = parse_duration)]
        keep: Option<Duration>,
    },
    List,
    Get { id: String },
    Delete { id: String },
    #[command(hide = true)]
    Completion { shell: Shell },
}

#[derive(Debug, Deserialize)]
struct Config {
    url: String,
    key: String,
    /// Extract downloaded folders instead of saving them as .tar.gz.
    #[serde(default)]
    extract: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Command::Completion { shell } = &cli.command {
        return print_completion_script(shell.clone());
    }

    let config = read_config()?;
    let base_url = normalize_base_url(&config.url);
    let client = Client::new();

    match cli.command {
        Command::Upload { path, keep } => upload_path(&client, &base_url, &config.key, &path, keep).await,
        Command::List => list_files(&client, &base_url, &config.key).await,
        Command::Get { id } => download_file(&client, &base_url, &id, config.extract).await,
        Command::Delete { id } => delete_file(&client, &base_url, &config.key, &id).await,
        Command::Completion { .. } => unreachable!("handled above"),
    }
}

fn print_completion_script(shell: Shell) -> Result<()> {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "cc-store", &mut std::io::stdout());
    Ok(())
}

fn read_config() -> Result<Config> {
    let home = env::var("HOME").context("$HOME is not set")?;
    let config_path = Path::new(&home)
        .join(".config")
        .join("caml")
        .join("cc-store")
        .join("config.toml");

    if !config_path.exists() {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create config directory at {}", parent.display()))?;
        }

        std::fs::write(&config_path, "url = \"\"\nkey = \"\"\nextract = false\n")
            .with_context(|| format!("failed to create starter config at {}", config_path.display()))?;

        bail!(
            "no config file was found, so a starter file was created at {}. fill in url and key, then run cc-store again",
            config_path.display()
        );
    }

    let raw = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read config file at {}", config_path.display()))?;

    let cfg: Config = toml::from_str(&raw)
        .with_context(|| format!("failed to parse TOML in {}", config_path.display()))?;

    if cfg.url.trim().is_empty() {
        bail!("config value 'url' cannot be empty");
    }
    if cfg.key.trim().is_empty() {
        bail!("config value 'key' cannot be empty");
    }

    Ok(cfg)
}

fn normalize_base_url(raw_url: &str) -> String {
    let trimmed = raw_url.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    }
}

async fn upload_path(
    client: &Client,
    base_url: &str,
    api_key: &str,
    path: &Path,
    keep: Option<Duration>,
) -> Result<()> {
    let metadata = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("failed to read {}", path.display()))?;

    if metadata.is_dir() {
        let dir_name = path
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "folder".to_string());

        if std::io::stderr().is_terminal() {
            eprintln!("packing {}/ ...", dir_name);
        }

        let source = path.to_path_buf();
        let archive = tokio::task::spawn_blocking(move || pack_dir(&source))
            .await
            .context("folder packing task failed")??;

        let file = tokio::fs::File::open(archive.path())
            .await
            .context("failed to open packed folder")?;
        let len = file.metadata().await?.len();

        // `archive` stays alive until the upload finishes, then the temp file is removed.
        let result = upload_fragmented(
            client,
            base_url,
            api_key,
            file,
            len,
            &format!("{dir_name}.tar.gz"),
            keep,
            true,
        )
        .await;
        drop(archive);
        return result;
    }

    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow!("invalid file path: {}", path.display()))?
        .to_string_lossy()
        .into_owned();

    let file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("failed to open file {}", path.display()))?;

    upload_fragmented(client, base_url, api_key, file, metadata.len(), &file_name, keep, false).await
}

/// Packs a directory into a gzipped tarball in a temporary file. Entries are
/// stored relative to the directory itself, without a top-level prefix.
fn pack_dir(dir: &Path) -> Result<NamedTempFile> {
    let tmp = NamedTempFile::new().context("failed to create temporary archive")?;

    let encoder = GzEncoder::new(BufWriter::new(tmp.as_file()), Compression::default());
    let mut builder = tar::Builder::new(encoder);
    builder.follow_symlinks(false);
    builder
        .append_dir_all(".", dir)
        .with_context(|| format!("failed to pack folder {}", dir.display()))?;

    let mut writer = builder
        .into_inner()
        .context("failed to finish tar archive")?
        .finish()
        .context("failed to finish gzip stream")?;
    writer.flush().context("failed to write temporary archive")?;
    drop(writer);

    Ok(tmp)
}

/// Uploads `file` using the server's fragment protocol: one `POST /fragment`
/// to start, then one `PATCH /{id}` per fragment of at most the server's
/// max upload size. Small files are simply sent as a single fragment.
#[allow(clippy::too_many_arguments)]
async fn upload_fragmented(
    client: &Client,
    base_url: &str,
    api_key: &str,
    file: tokio::fs::File,
    len: u64,
    name: &str,
    keep: Option<Duration>,
    is_dir: bool,
) -> Result<()> {
    let mut request = client
        .post(format!("{base_url}/fragment"))
        .header("K", api_key)
        .header("Upload-Length", len.to_string())
        .header("Upload-Name", name);

    if let Some(keep) = keep {
        request = request.header("Upload-Keep", format!("{}s", keep.as_secs()));
    }
    if is_dir {
        request = request.header("Upload-Dir", "1");
    }

    let response = request.send().await.context("upload request failed")?;
    let status = response.status();
    if status != StatusCode::CREATED {
        let body = response.text().await.unwrap_or_default();
        bail!("upload failed ({status}): {}", body.trim());
    }

    let id = header_str(&response, "Upload-Id")
        .ok_or_else(|| anyhow!("server did not return an upload id (is the server up to date?)"))?;
    let max_chunk = header_u64(&response, "Upload-Max-Chunk")
        .filter(|&n| n > 0)
        .ok_or_else(|| anyhow!("server did not return a valid max fragment size"))?;

    let parts = len.div_ceil(max_chunk).max(1);
    let show_progress = parts > 1 && std::io::stderr().is_terminal();

    let mut offset: u64 = 0;
    let mut failures: u32 = 0;

    loop {
        let chunk_len = max_chunk.min(len - offset);
        let part = offset / max_chunk + 1;
        if show_progress {
            eprint!("\ruploading part {part}/{parts}");
        }

        let mut reader = file.try_clone().await.context("failed to reopen upload file")?;
        reader
            .seek(SeekFrom::Start(offset))
            .await
            .context("failed to seek in upload file")?;
        let body = Body::wrap_stream(ReaderStream::new(reader.take(chunk_len)));

        let result = client
            .patch(format!("{base_url}/{id}"))
            .header("K", api_key)
            .header("Upload-Offset", offset.to_string())
            .header(CONTENT_LENGTH, chunk_len.to_string())
            .body(body)
            .send()
            .await;

        let error = match result {
            Ok(response) => match response.status() {
                StatusCode::CREATED => {
                    if show_progress {
                        eprintln!();
                    }
                    let body = response.text().await.unwrap_or_default();
                    println!("{}", body.trim());
                    return Ok(());
                }
                StatusCode::NO_CONTENT => {
                    let next = header_u64(&response, "Upload-Offset")
                        .ok_or_else(|| anyhow!("server did not return the upload offset"))?;
                    if next <= offset || next > len {
                        bail!("server returned an unexpected upload offset ({next})");
                    }
                    offset = next;
                    failures = 0;
                    continue;
                }
                StatusCode::CONFLICT => match header_u64(&response, "Upload-Offset") {
                    // The server already has a different amount of data (e.g. a
                    // previous attempt succeeded but its response was lost): resume there.
                    Some(next) if next <= len => {
                        offset = next;
                        format!("offset mismatch, resuming at byte {next}")
                    }
                    _ => {
                        let body = response.text().await.unwrap_or_default();
                        bail!("upload failed (409 Conflict): {}", body.trim());
                    }
                },
                status if status.is_server_error() => {
                    let body = response.text().await.unwrap_or_default();
                    format!("{status}: {}", body.trim())
                }
                status => {
                    let body = response.text().await.unwrap_or_default();
                    bail!("upload failed ({status}): {}", body.trim());
                }
            },
            Err(error) => error.to_string(),
        };

        failures += 1;
        if failures > MAX_RETRIES {
            if show_progress {
                eprintln!();
            }
            bail!("upload {id} failed after {MAX_RETRIES} retries: {error}");
        }
        tokio::time::sleep(Duration::from_secs(u64::from(failures))).await;
    }
}

fn header_str(response: &Response, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn header_u64(response: &Response, name: &str) -> Option<u64> {
    header_str(response, name).and_then(|v| v.parse().ok())
}

fn parse_duration(value: &str) -> std::result::Result<Duration, String> {
    humantime::parse_duration(value).map_err(|error| error.to_string())
}

async fn list_files(client: &Client, base_url: &str, api_key: &str) -> Result<()> {
    let response = client
        .request(
            reqwest::Method::from_bytes(b"LIST").context("invalid LIST method")?,
            format!("{base_url}/"),
        )
        .header("K", api_key)
        .send()
        .await
        .context("list request failed")?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        bail!("list failed ({status}): {}", body.trim());
    }

    println!("{}", body.trim());
    Ok(())
}

async fn download_file(client: &Client, base_url: &str, id: &str, extract: bool) -> Result<()> {
    if id.trim().is_empty() {
        bail!("id cannot be empty");
    }

    let response = client
        .get(format!("{base_url}/{id}"))
        .send()
        .await
        .context("download request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("download failed ({status}): {}", body.trim());
    }

    let is_folder = header_str(&response, "X-Folder").as_deref() == Some("1");

    // Only keep the final path component so a server-supplied name can't
    // write outside the current directory.
    let file_name = response
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_content_disposition_filename)
        .and_then(|name| {
            Path::new(&name)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| id.to_string());

    if !std::io::stdout().is_terminal() {
        let mut stdout = tokio::io::stdout();
        return stream_response(response, &mut stdout)
            .await
            .context("failed to write downloaded bytes to stdout");
    }

    if is_folder && extract {
        let tmp = NamedTempFile::new().context("failed to create temporary archive")?;
        let mut out = tokio::fs::File::from_std(tmp.reopen().context("failed to open temporary archive")?);
        stream_response(response, &mut out).await?;
        drop(out);

        let dir_name = file_name.strip_suffix(".tar.gz").unwrap_or(&file_name);
        let target = unique_output_name(dir_name);
        let unpack_target = target.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let archive_file = std::fs::File::open(tmp.path()).context("failed to open downloaded archive")?;
            tar::Archive::new(GzDecoder::new(BufReader::new(archive_file)))
                .unpack(&unpack_target)
                .with_context(|| format!("failed to extract folder into {unpack_target}"))
        })
        .await
        .context("folder extraction task failed")??;

        println!("extracted {target}/");
        return Ok(());
    }

    let output_name = unique_output_name(&file_name);
    let mut out = tokio::fs::File::create(&output_name)
        .await
        .with_context(|| format!("failed to create downloaded file: {output_name}"))?;

    if let Err(error) = stream_response(response, &mut out).await {
        drop(out);
        let _ = tokio::fs::remove_file(&output_name).await;
        return Err(error.context(format!("failed to write downloaded file: {output_name}")));
    }

    println!("saved {output_name}");
    Ok(())
}

/// Streams a response body into `out` without buffering it all in memory.
async fn stream_response<W: AsyncWrite + Unpin>(mut response: Response, out: &mut W) -> Result<()> {
    while let Some(chunk) = response
        .chunk()
        .await
        .context("failed to read download response body")?
    {
        out.write_all(&chunk).await?;
    }
    out.flush().await?;
    Ok(())
}

async fn delete_file(client: &Client, base_url: &str, api_key: &str, id: &str) -> Result<()> {
    if id.trim().is_empty() {
        bail!("id cannot be empty");
    }

    let response = client
        .delete(format!("{base_url}/{id}"))
        .header("K", api_key)
        .send()
        .await
        .context("delete request failed")?;

    if response.status() != reqwest::StatusCode::NO_CONTENT {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("delete failed ({status}): {}", body.trim());
    }

    println!("deleted {id}");
    Ok(())
}

fn parse_content_disposition_filename(value: &str) -> Option<String> {
    let marker = "filename=";
    let idx = value.find(marker)?;
    let mut name = value[idx + marker.len()..].trim();

    if let Some(stripped) = name.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        name = stripped;
    }

    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn unique_output_name(file_name: &str) -> String {
    let path = Path::new(file_name);
    if !path.exists() {
        return file_name.to_string();
    }

    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("file");

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();

    for index in 1.. {
        let candidate = format!("{stem}-{index}{ext}");
        if !Path::new(&candidate).exists() {
            return candidate;
        }
    }

    unreachable!("infinite iterator should always find an available file name")
}
