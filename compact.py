#!/usr/bin/env python3
"""Phase 3: safe compaction of Hermes sessions.

Compresses sessions older than today, moves originals to trash/quarantine
without deleting them, updates state_5.sqlite with compaction metadata,
and discovers sensitive data patterns (.env, token, key).
"""

import json
import os
import sqlite3
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

SESSIONS_DIR = Path(os.environ.get("HERMES_SESSIONS", "/Users/kimjeongjin/.hermes/sessions"))
TRASH_DIR = SESSIONS_DIR / "trash"
STATE_DB = SESSIONS_DIR / "session_index" / "state_5.sqlite"


def today_utc():
    return datetime.now(timezone.utc).replace(hour=0, minute=0, second=0, microsecond=0)


def session_mtime(path):
    """Extract modification time from a session file."""
    return datetime.fromtimestamp(os.path.getmtime(path), tz=timezone.utc)


def session_date_str(path):
    """Extract date string from filename (e.g., 20260524_085035 -> 2026-05-24)."""
    name = path.stem.split("_")[0]  # first part before _
    if len(name) == 8:
        return f"{name[:4]}-{name[4:6]}-{name[6:]}"
    return None


def is_sensitive_content(path):
    """Check if file contains sensitive data patterns."""
    try:
        content = path.read_text(errors="replace")[:50_000]  # read first 50KB
    except Exception:
        return False

    patterns = [
        r'"key"\s*:\s*"sk-[a-zA-Z0-9]{20,}',
        r'"token"\s*:\s*"eyJ[a-zA-Z0-9_-]{20,}',
        r'"api_key"\s*:\s*"sk-[a-zA-Z0-9]{20,}',
        r'"access_token"\s*:\s*"[a-zA-Z0-9_-]{20,}',
        r'"secret"\s*:\s*"[a-zA-Z0-9_-]{16,}',
        r'"password"\s*:\s*"[^"]{8,}',
    ]

    count = sum(1 for p in patterns if __import__("re").search(p, content))
    return count > 0


def discover_sensitive_files():
    """Scan all session files for sensitive data patterns."""
    results = []
    for path in SESSIONS_DIR.rglob("*.jsonl"):
        if "trash" in str(path):
            continue
        sensitive = is_sensitive_content(path)
        if sensitive:
            results.append({
                "path": str(path),
                "date": session_date_str(path) or "unknown",
                "size_bytes": path.stat().st_size,
            })
    return results


def create_trash_dir():
    """Create trash directory if it doesn't exist."""
    TRASH_DIR.mkdir(parents=True, exist_ok=True)