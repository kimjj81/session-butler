# Session Butler

> 🌐 Languages: **English** · [한국어](./docs/README.ko.md)

Compress, archive, and turn your **Codex** / **Hermes** session logs into a searchable knowledge base.

- **Codex** sessions → scan/index, compress/archive (+ restore), compact & secret-screen.
- **Hermes** sessions → summarize into a queryable summary + keyword layer.
- Enable only the backend you use (Codex, Hermes, or both).

---

## Why I built this

Tools like **Codex** and **Hermes** silently record every conversation as JSONL/JSON under your home directory. Use them daily for a few months and those files pile up — my own `~/.codex/sessions/` had grown past **3 GB**.

That history is genuinely valuable: months of debugging notes, design decisions, and hard-won prompts. But at 3 GB it was too bulky to leave on disk, too painful to scroll through raw, and too precious to delete. It was dead weight — taking up space yet doing nothing for me.

**Session Butler resolves that tension.** It shrinks the storage footprint of old sessions *while* making them searchable and reusable as a personal knowledge base, instead of letting them rot in a folder.

## What it does

The tool manages **Codex** session logs and summarizes **Hermes** session logs. Each command targets one backend; enable/disable backends via settings (see below).

### Codex — manage session logs

| Command | What it does |
|---------|--------------|
| `scan` | Walk Codex `rollout-*.jsonl`, write metadata + FTS5 full-text index to SQLite |
| `archive` | zstd-compress sessions older than N days. `--move` deletes originals after; `--skip-scan` skips the pre-archive scan |
| `restore` | Restore from `.zst` — reads the **DB archive index** (works even if originals are gone). `--purge` deletes the `.zst` afterward |
| `list` / `stats` | List / summarize archived + active sessions |
| `compact` | Safe compaction + sensitive-info detection (`.env`, tokens, keys) |

Archive state and SHA-256 checksums are stored in the **SQLite index**, so `restore` can verify integrity and find sessions even after originals are removed.

### Hermes — summarize session logs

| Command | What it does |
|---------|--------------|
| `summarize` | Analyze Hermes `session_*.json` → summary + FTS5 keyword JSON |

Note: Hermes writes several file types (`session_*.json`, `request_dump_*.json`, …). Only `session_*.json` (the actual conversation logs) are summarized; request/error dumps are skipped.

## Build

Requires Rust (edition 2024).

```bash
cargo build --release
# → ./target/release/session-butler
```

Or run directly during development:

```bash
cargo run --release -- <command>
```

## Usage

### Interactive (TUI)

```bash
session-butler          # no args → launches the TUI
session-butler --tui    # explicit
```

The TUI shows a **status bar** with the active backends, session paths, and retention days at all times, and lists only the commands for enabled backends.

### CLI

```bash
# Codex — scan + index
session-butler scan [--analyze]

# Codex — archive / restore
session-butler archive  --days 30 --dry-run    # preview
session-butler archive  --days 30              # compress (originals kept)
session-butler archive  --days 30 --move       # compress then delete originals
session-butler restore  --all                  # restore (keeps .zst for re-restore)
session-butler restore  --all --purge          # restore then delete .zst

# Codex — list / stats / compact
session-butler list   --days 30 [--json]
session-butler stats  --days 30
session-butler compact --scan-sensitive        # scan for secrets only

# Hermes — summarize
session-butler summarize                       # summary + FTS5 JSON
session-butler summarize --summary-only
session-butler summarize --fts-only

# Everything, in order
session-butler pipeline --days 30 --dry-run
```

## Settings

### Backend enable/disable

Codex and Hermes can each be enabled or disabled. Precedence (highest wins):

1. CLI flags: `--no-codex`, `--no-hermes`
2. Environment variables: `CODEX_ENABLED`, `HERMES_ENABLED` (`0`/`false`/`off`/`no` → disabled)
3. Config file: `~/.config/session-butler/config.json`
4. Default: both enabled

A disabled backend's commands become no-ops, and `pipeline` skips that backend's work automatically.

Example config file:

```json
{
  "enabled_codex": true,
  "enabled_hermes": false
}
```

### Paths & outputs (global options or env vars)

| Flag | Env var | Default |
|------|---------|---------|
| `-C, --codex-sessions <DIR>` | `CODEX_SESSIONS` | `~/.codex/sessions` |
| `-H, --hermes-sessions <DIR>` | `HERMES_SESSIONS` | `~/.hermes/sessions` |
| `-A, --codex-archive <DIR>` | `CODEX_ARCHIVE` | `~/.codex/archive` |
| `-I, --codex-index-db <PATH>` | `CODEX_INDEX_DB` | `./codex_index.sqlite` |
| `-S, --summary-layer <PATH>` | `SUMMARY_LAYER_JSON` | `./summary_layer.json` |
| `-F, --fts5-index <PATH>` | `FTS5_INDEX_JSON` | `./fts5_index.json` |

## Results

Measured on my own session history:

| Target | Files | Raw size | Result |
|--------|------:|---------:|-------|
| Codex sessions | 3,037 | 3.1 GB | archived set (2,303 sessions) **2.42 GB → 0.86 GB** (~64% smaller) |
| Hermes sessions | 82 (52 `session_*.json`) | 47 MB | 52 sessions summarized → `summary_layer.json` + `fts5_index.json` |

**~2.4 GB of old Codex sessions now lives in ~860 MB** while remaining fully searchable through the SQLite + FTS5 index. Deleting originals with `archive --move` reclaims the rest.

## Safety

- By default, no command deletes originals. Compression produces `.jsonl.zst` copies; originals stay.
- Explicit deletion only: `archive --move` (delete originals after compressing), `restore --purge` (delete `.zst` after restoring).
- Sessions newer than the retention window (`--days`, default `30`) are skipped.
- Every archived file gets a SHA-256 checksum stored in the SQLite index, so `restore` verifies integrity and detects corruption.

## Status

Scan/index, archive/restore (SQLite-backed), compaction, summarization, TUI, and CLI are all functional.

## License

[MIT](./LICENSE) © Kim Jeongjin
