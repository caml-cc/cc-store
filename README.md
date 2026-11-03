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
extract = false # set to true to unpack downloaded folders instead of keeping them as .tar.gz
```

If the file is missing, `cc-store` creates a starter config with empty `url` and `key` values and tells you where it wrote it. `extract` is optional and defaults to `false`.

## Usage

```bash
cc-store upload file.txt # uploads the file and returns a url
cc-store upload ./folder # packs the folder into folder.tar.gz and uploads it
cc-store upload -k 100h file.txt # uploads the file and keeps it for 100 hours
cc-store upload --keep=100h file.txt # same as above
cc-store list # lists files using the LIST request method
cc-store get {id} # downloads a file using the id
cc-store delete {id} # deletes a file using the id
```

`cc-store get` writes to stdout when redirected. If run in a terminal, it saves the file locally without overwriting existing files.

### Large files

Files and folders larger than the server's per-upload limit (`MAX_UPLOAD` on [fs](https://github.com/caml-cc/fs.git), 1GB by default) are split into fragments automatically and joined back together on the server, so they download as one file. Uploads and downloads are streamed, so they don't need to fit in memory. Failed fragments are retried up to 3 times.

### Resuming uploads

Every upload is hashed (BLAKE3) before it's sent, and the hash and upload id are saved in `~/.config/caml/cc-store/pending/<hash>.toml` until the upload finishes. If an upload is interrupted (Ctrl+C, a crash, lost connection), run the same `cc-store upload` command again. If the content is exactly the same, it continues where the server left off. If anything changed, it starts a new upload.

- A resumed upload keeps the expiry from the first attempt, so a new `--keep` is ignored.
- If the partial upload expired or was deleted on the server, a new upload starts automatically.
- Folders resume too, as long as nothing inside them changed (they always pack to the same archive).

### Folders

Uploading a folder packs it into a `.tar.gz` first. The archive is built in your temp directory (`$TMPDIR`, usually `/tmp`), so there needs to be enough room for it. By default `cc-store get` saves the folder as `folder.tar.gz`. With `extract = true` in the config, it unpacks it into `folder/` instead (or `folder-1/` and so on if that already exists).

## Tab completion

The install script automatically installs completion files for zsh, bash, and fish.

## Related projects
[fs](https://github.com/caml-cc/fs.git) a self-hosted file upload API.