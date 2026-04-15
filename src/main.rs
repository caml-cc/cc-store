use std::env;
use std::io::IsTerminal;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use reqwest::header::CONTENT_DISPOSITION;
use reqwest::{multipart, Client};
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(name = "cc-store", about = "Upload and download files via your configured API")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    file: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
enum Command {
    Get { id: String },
    Delete { id: String },
}

#[derive(Debug, Deserialize)]
struct Config {
    url: String,
    key: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = read_config()?;
    let base_url = normalize_base_url(&config.url);
    let client = Client::new();

    match (cli.command, cli.file) {
        (Some(Command::Get { id }), None) => download_file(&client, &base_url, &id).await,
        (Some(Command::Delete { id }), None) => delete_file(&client, &base_url, &config.key, &id).await,
        (None, Some(path)) => upload_file(&client, &base_url, &config.key, &path).await,
        (None, None) => bail!(
            "usage:\n  cc-store <file>\n  cc-store get <id>\n  cc-store delete <id>"
        ),
        _ => bail!("provide either a file to upload or a subcommand"),
    }
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

        std::fs::write(&config_path, "url = \"\"\nkey = \"\"\n")
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

async fn upload_file(client: &Client, base_url: &str, api_key: &str, path: &Path) -> Result<()> {
    let bytes = tokio::fs::read(path)
        .await
        .with_context(|| format!("failed to read file {}", path.display()))?;

    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow!("invalid file path: {}", path.display()))?
        .to_string_lossy()
        .into_owned();

    let part = multipart::Part::bytes(bytes).file_name(file_name);
    let form = multipart::Form::new().part("file", part);

    let response = client
        .post(format!("{base_url}/"))
        .header("K", api_key)
        .multipart(form)
        .send()
        .await
        .context("upload request failed")?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if status != reqwest::StatusCode::CREATED {
        bail!("upload failed ({status}): {}", body.trim());
    }

    println!("{}", body.trim());
    Ok(())
}

async fn download_file(client: &Client, base_url: &str, id: &str) -> Result<()> {
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

    let file_name = response
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_content_disposition_filename)
        .unwrap_or_else(|| id.to_string());

    let bytes = response
        .bytes()
        .await
        .context("failed to read download response body")?;

    if std::io::stdout().is_terminal() {
        let output_name = unique_output_name(&file_name);
        tokio::fs::write(&output_name, &bytes)
            .await
            .with_context(|| format!("failed to write downloaded file: {output_name}"))?;
        println!("saved {output_name}");
    } else {
        let mut stdout = std::io::stdout().lock();
        stdout
            .write_all(bytes.as_ref())
            .context("failed to write downloaded bytes to stdout")?;
    }

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
