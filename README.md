# cc-store

Small Rust CLI for uploading, downloading, and deleting files from your own self-hosted API.

## Install

### Option 1: Install script (recommended)

```bash
curl -sL https://raw.githubusercontent.com/caml-cc/cc-store/refs/heads/main/scripts/install.sh -o install.sh
bash install.sh
```

### Option 2: Manual install (system-wide)
```bash
curl -sL https://raw.githubusercontent.com/caml-cc/cc-store/refs/heads/main/scripts/install.sh -o install.sh
sudo bash install.sh
```

### Option 3: Build from source

```bash
cargo build --release
```

## Config

Create `$HOME/.config/caml/cc-store/config.toml`:

```toml
url = "domain.com"
key = "abc123"
```

## Usage

```bash
cc-store file.txt # uploads the file and returns a url
cc-store get {id} # downloads a file using the id
cc-store delete {id} # deletes a file using the id
```

`cc-store get` writes to stdout when redirected. If run in a terminal, it saves the file locally without overwriting existing files.

## Related projects
[fs](https://github.com/caml-cc/fs.git) a self-hosted file upload API.