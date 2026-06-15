# Session Butler

> 🌐 Languages: **English** · [한국어](./docs/README.ko.md)

Compress, archive, and turn your **Codex** / **Hermes** session logs into a searchable knowledge base.

---

## Why I built this

Tools like **Codex** and **Hermes** silently record every conversation as JSONL/JSON under your home directory. Use them daily for a few months and those files pile up — my own `~/.codex/sessions/` had grown past **3 GB**.

That history is genuinely valuable: months of debugging notes, design decisions, and hard-won prompts. But at 3 GB it was too bulky to leave sitting on disk, too painful to scroll through raw, and too precious to delete. It was dead weight — taking up space yet doing nothing for me.

**Session Butler resolves that tension.** It shrinks the storage footprint of old sessions *while* making them searchable and reusable as a personal knowledge base, instead of letting them rot in a folder.

## Goal

One safe pipeline that:

1. **Indexes** every session into SQLite with full-text search.
2. **Compresses** old sessions with zstd — originals are never auto-deleted.
3. **Compacts** sessions and screens them for secrets (API keys, tokens).
4. **Summarizes** sessions into a queryable summary + keyword layer.

…so months of AI-assisted work stays small on disk and actually useful in your hands.

## How it works — the 4-phase pipeline

| Phase | Command | What it does |
|-------|---------|--------------|
| 1 · Scan | `scan` | Walk Codex `rollout-*.jsonl`, write metadata + FTS5 index to SQLite |
| 2 · Archive | `archive` / `restore` / `list` / `stats` | zstd-compress sessions older than N days, with SHA-256 checksums |
| 3 · Compact | `compact` | Safe compaction + sensitive-info detection (`.env`, tokens, keys) |
| 4 · Summarize | `summarize` | Analyze Hermes sessions → summary + FTS5 keyword JSON |

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

The TUI is a single menu over all four phases (Scan, Archive, Restore, List, Stats, Compact, Summarize, Pipeline) with editable arguments — handy for one-off runs.

### CLI

```bash
# Phase 1 — scan + index Codex sessions
session-butler scan [--analyze]

# Phase 2 — archive
session-butler archive --days 30 --dry-run   # preview
session-butler archive --days 30             # compress (originals kept)
session-butler restore --all                 # restore everything
session-butler list   --days 30 [--json]
session-butler stats  --days 30

# Phase 3 — compaction
session-butler compact --scan-sensitive      # scan for secrets only
session-butler compact --days 0 --dry-run    # preview compaction

# Phase 4 — summarize Hermes sessions
session-butler summarize                     # summary + FTS5 JSON
session-butler summarize --summary-only
session-butler summarize --fts-only

# Run everything, in order
session-butler pipeline --days 30 --dry-run
```

### Global options (any command)

| Flag | Default |
|------|---------|
| `-C, --codex-sessions <DIR>` | `~/.codex/sessions` |
| `-H, --hermes-sessions <DIR>` | `~/.hermes/sessions` |
| `-A, --codex-archive <DIR>` | `~/.codex/archive` |
| `-I, --codex-index-db <PATH>` | `./codex_index.sqlite` |
| `-S, --summary-layer <PATH>` | `./summary_layer.json` |
| `-F, --fts5-index <PATH>` | `./fts5_index.json` |
| `-v, --verbose` | verbose output |

### Environment variables

`CODEX_SESSIONS`, `CODEX_ARCHIVE`, `HERMES_SESSIONS`, `CODEX_STATE_DB`, `CODEX_INDEX_DB`, `SUMMARY_LAYER_JSON`, `FTS5_INDEX_JSON` — same meaning/defaults as the flags above.

## Results

Measured on my own session history:

| Target | Files | Raw size | After |
|--------|------:|---------:|-------|
| Codex sessions | 3,037 | 3.1 GB | **2.42 GB → 0.86 GB** for the archived set (2,303 sessions, ≈64% smaller) |
| Hermes sessions | 82 | 47 MB | summarized → `summary_layer.json` + `fts5_index.json` |

In other words, **~2.4 GB of old Codex sessions now lives in ~860 MB** while remaining fully searchable through the SQLite + FTS5 index. Deleting the originals after archiving reclaims the rest.

## Safety

- Original session files are **never auto-deleted** by any phase. Compression produces `.jsonl.zst` copies; originals are only moved aside when you explicitly restore/quarantine.
- Sessions newer than the retention window (`--days`, default `30`) are skipped.
- Every archived file gets a SHA-256 checksum in `checksums.jsonl`, so `restore` can verify integrity.

## Status

Rust port of an original Python prototype. The 4-phase pipeline, TUI, and CLI are all functional. Output of phases 1 & 4 is verified to match the Python reference; phase 2 is faithful.

## License

[MIT](./LICENSE) © Kim Jeongjin
