#!/usr/bin/env python3
"""Phase 1: read-only scan and indexing of Codex sessions.

Streams JSONL files (no full-file memory load), extracts metadata,
builds SQLite index with FTS5, and produces analysis reports.
"""

import json
import os
import sqlite3
import sys
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path

SESSIONS_DIR = Path(os.environ.get("CODEX_SESSIONS", "/Users/kimjeongjin/.codex/sessions"))
STATE_DB = Path(os.environ.get("CODEX_STATE_DB", "/Users/kimjeongjin/.codex/state_5.sqlite"))
INDEX_DB = Path(os.environ.get("CODEX_INDEX_DB", "/Users/kimjeongjin/.hermes/kanban/boards/session-butler/workspaces/t_f3241c77/codex_index.sqlite"))

# ── JSONL parsing (streaming, line-by-line) ─────────────────────────────

def parse_jsonl_stream(path: Path):
    """Yield (line_number, parsed_dict) for each line. Skips corrupt lines."""
    with open(path, "r", encoding="utf-8", errors="replace") as f:
        for lineno, raw in enumerate(f, 1):
            raw = raw.strip()
            if not raw:
                continue
            try:
                yield lineno, json.loads(raw)
            except (json.JSONDecodeError, ValueError):
                yield lineno, {"_corrupt": True, "_raw": raw[:200]}


def extract_session_meta(path: Path):
    """Extract metadata from a single JSONL file (streaming)."""
    meta = {
        "path": str(path),
        "filename": path.name,
        "date": None,  # extracted from filename
        "cwd": None,
        "first_user_prompt": "",
        "model_provider": None,
        "cli_version": None,
        "source": None,
        "model": None,
        "git_sha": None,
        "git_branch": None,
        "git_origin_url": None,
        "tool_call_count": 0,
        "file_change_count": 0,
        "total_tokens": 0,
        "line_count": 0,
        "corrupt_lines": 0,
        "has_user_event": False,
        "session_id": None,
    }

    # Extract date from filename: rollout-2026-03-03T14-37-44-uuid.jsonl
    parts = path.name.split("T")
    if len(parts) >= 2:
        # parts[0] = "rollout-2026-03-03"
        date_part = parts[0].replace("rollout-", "")
        meta["date"] = date_part if len(date_part) == 10 else None
    else:
        meta["date"] = None
    meta["session_id"] = path.stem.replace("rollout-", "")

    for lineno, record in parse_jsonl_stream(path):
        meta["line_count"] += 1
        if record.get("_corrupt"):
            meta["corrupt_lines"] += 1
            continue

        rtype = record.get("type")
        payload = record.get("payload", {})

        if rtype == "session_meta":
            meta["cwd"] = payload.get("cwd")
            meta["model_provider"] = payload.get("model_provider")
            meta["cli_version"] = payload.get("cli_version")
            meta["source"] = payload.get("source")
            git = payload.get("git", {})
            meta["git_sha"] = str(git.get("commit_hash")) if git else None
            meta["git_branch"] = str(git.get("branch")) if git else None
            meta["git_origin_url"] = str(git.get("repository_url")) if git else None

        elif rtype == "response_item":
            # Count tool calls - they appear as response_items with type function_call or custom_tool_call
            if payload.get("type") in ("function_call", "custom_tool_call"):
                meta["tool_call_count"] += 1

        elif rtype == "event_msg":
            if payload.get("type") == "task_started":
                mcw = payload.get("model_context_window")
                meta["model"] = str(mcw) if mcw is not None else None
            elif payload.get("type") == "patch_apply_end":
                meta["file_change_count"] += 1

        # Track token usage from response items with usage data
        if rtype == "response_item" and payload.get("usage"):
            usage = payload["usage"]
            if isinstance(usage, dict):
                meta["total_tokens"] += usage.get("input_tokens", 0) + usage.get("output_tokens", 0)
            elif isinstance(usage, int):
                meta["total_tokens"] += usage

        # Track user events for resume capability
        if rtype == "event_msg" and payload.get("type") in ("task_started",):
            meta["has_user_event"] = True

    return meta


# ── SQLite index with FTS5 ───────────────────────────────────────────────

def init_db(db_path: Path):
    conn = sqlite3.connect(str(db_path))
    c = conn.cursor()

    c.execute("""
        CREATE TABLE IF NOT EXISTS sessions (
            session_id TEXT PRIMARY KEY,
            path TEXT NOT NULL,
            date TEXT,
            cwd TEXT,
            first_user_prompt TEXT,
            model_provider TEXT,
            cli_version TEXT,
            source TEXT,
            model TEXT,
            git_sha TEXT,
            git_branch TEXT,
            git_origin_url TEXT,
            tool_call_count INTEGER DEFAULT 0,
            file_change_count INTEGER DEFAULT 0,
            total_tokens INTEGER DEFAULT 0,
            line_count INTEGER DEFAULT 0,
            corrupt_lines INTEGER DEFAULT 0,
            has_user_event INTEGER DEFAULT 0,
            indexed_at TEXT
        )
    """)

    c.execute("""
        CREATE VIRTUAL TABLE IF NOT EXISTS sessions_fts
        USING fts5(
            session_id,
            first_user_prompt,
            cwd,
            git_origin_url
        )
    """)

    # Direct FTS insert will be done in upsert_session
    # (no triggers needed for standalone FTS table)

    conn.commit()
    return conn


def upsert_session(conn, meta: dict):
    c = conn.cursor()
    now = datetime.now(timezone.utc).isoformat()

    # Extract first user prompt from the JSONL
    path = Path(meta["path"])
    first_prompt = ""
    for lineno, record in parse_jsonl_stream(path):
        if record.get("_corrupt"):
            continue
        payload = record.get("payload", {})
        if record.get("type") == "response_item" and isinstance(payload.get("content"), list):
            for item in payload["content"]:
                if isinstance(item, dict) and item.get("type") == "message" and item.get("role") == "user":
                    texts = [c.get("text", "") for c in item["content"] if isinstance(c, dict)]
                    first_prompt = "\n".join(texts).strip()[:2000]
                    break
        if first_prompt:
            break

    # Ensure all values are SQLite-compatible (str, int, float, None)
    for key in ("model", "model_provider", "cli_version", "source",
                "git_sha", "git_branch", "git_origin_url", "cwd"):
        val = meta[key]
        if isinstance(val, dict):
            meta[key] = json.dumps(val, ensure_ascii=False)[:500]
        elif not isinstance(val, (str, int, float, type(None))):
            meta[key] = str(val) if val is not None else None

    c.execute("""
        INSERT INTO sessions (session_id, path, date, cwd, first_user_prompt,
                              model_provider, cli_version, source, model,
                              git_sha, git_branch, git_origin_url,
                              tool_call_count, file_change_count, total_tokens,
                              line_count, corrupt_lines, has_user_event, indexed_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(session_id) DO UPDATE SET
            path=excluded.path, date=excluded.date, cwd=excluded.cwd,
            first_user_prompt=excluded.first_user_prompt, model_provider=excluded.model_provider,
            cli_version=excluded.cli_version, source=excluded.source, model=excluded.model,
            git_sha=excluded.git_sha, git_branch=excluded.git_branch,
            git_origin_url=excluded.git_origin_url, tool_call_count=excluded.tool_call_count,
            file_change_count=excluded.file_change_count, total_tokens=excluded.total_tokens,
            line_count=excluded.line_count, corrupt_lines=excluded.corrupt_lines,
            has_user_event=excluded.has_user_event, indexed_at=excluded.indexed_at
    """, (
        meta["session_id"], meta["path"], meta["date"], meta["cwd"], first_prompt,
        meta["model_provider"], meta["cli_version"], meta["source"], meta["model"],
        meta["git_sha"], meta["git_branch"], meta["git_origin_url"],
        meta["tool_call_count"], meta["file_change_count"], meta["total_tokens"],
        meta["line_count"], meta["corrupt_lines"],
        1 if meta["has_user_event"] else 0, now,
    ))

    # FTS insert
    c.execute("""
        INSERT INTO sessions_fts(session_id, first_user_prompt, cwd, git_origin_url)
        VALUES (?, ?, ?, ?)
    """, (meta["session_id"], first_prompt, meta["cwd"] or "", meta["git_origin_url"] or ""))

    conn.commit()


# ── Scanning ─────────────────────────────────────────────────────────────

def scan_all_sessions():
    """Walk all rollout-*.jsonl files and return list of metadata dicts."""
    jsonl_files = sorted(SESSIONS_DIR.rglob("rollout-*.jsonl"))
    print(f"Found {len(jsonl_files)} JSONL files")

    results = []
    for i, path in enumerate(jsonl_files):
        try:
            meta = extract_session_meta(path)
            results.append(meta)
            if (i + 1) % 500 == 0:
                print(f"  scanned {i+1}/{len(jsonl_files)}...")
        except Exception as e:
            print(f"  ERROR processing {path}: {e}")

    return results


# ── Analysis queries ─────────────────────────────────────────────────────

def run_analysis(conn):
    c = conn.cursor()

    print("\n" + "=" * 60)
    print("SESSION ANALYSIS")
    print("=" * 60)

    # Total sessions indexed
    c.execute("SELECT COUNT(*) FROM sessions")
    total = c.fetchone()[0]
    print(f"\nTotal sessions indexed: {total}")

    # Volume by month
    c.execute("""
        SELECT substr(date, 1, 7) AS month, COUNT(*) as cnt,
               SUM(tool_call_count) as total_tools,
               SUM(file_change_count) as total_changes,
               SUM(total_tokens) as total_tokens
        FROM sessions WHERE date IS NOT NULL
        GROUP BY month ORDER BY month
    """)
    print("\n--- Volume by Month ---")
    print(f"{'Month':<12} {'Sessions':>8} {'Tool Calls':>10} {'File Changes':>12} {'Tokens (M)':>12}")
    for row in c.fetchall():
        print(f"{row[0]:<12} {row[1]:>8} {row[2]:>10} {row[3]:>12} {row[4]/1e6:>12.1f}")

    # Volume by project (cwd)
    c.execute("""
        SELECT cwd, COUNT(*) as cnt, AVG(tool_call_count) as avg_tools
        FROM sessions WHERE cwd IS NOT NULL AND cwd != ''
        GROUP BY cwd ORDER BY cnt DESC LIMIT 15
    """)
    print("\n--- Top Projects by Session Count ---")
    print(f"{'Project':<50} {'Sessions':>8} {'Avg Tools':>10}")
    for row in c.fetchall():
        print(f"{row[0]:<50} {row[1]:>8} {row[2]:>10.1f}")

    # Volume by model
    c.execute("""
        SELECT COALESCE(model, 'unknown') AS m, COUNT(*) as cnt
        FROM sessions GROUP BY m ORDER BY cnt DESC LIMIT 10
    """)
    print("\n--- Sessions by Model ---")
    for row in c.fetchall():
        print(f"  {row[0]:<30} {row[1]}")

    # Volume by model_provider
    c.execute("""
        SELECT COALESCE(model_provider, 'unknown') AS m, COUNT(*) as cnt
        FROM sessions GROUP BY m ORDER BY cnt DESC
    """)
    print("\n--- Sessions by Model Provider ---")
    for row in c.fetchall():
        print(f"  {row[0]:<20} {row[1]}")

    # Large sessions (top 20 by line count)
    c.execute("""
        SELECT session_id, date, cwd, line_count, tool_call_count, total_tokens
        FROM sessions ORDER BY line_count DESC LIMIT 20
    """)
    print("\n--- Top 20 Largest Sessions (by lines) ---")
    print(f"{'Session ID':<45} {'Date':<12} {'Lines':>6} {'Tools':>6} {'Tokens (M)':>10}")
    for row in c.fetchall():
        print(f"{row[0]:<45} {str(row[1])[:12]:<12} {row[2]:>6} {row[3]:>6} {row[4]/1e6:>10.1f}")

    # Duplicate sessions (same cwd + similar first_user_prompt)
    c.execute("""
        SELECT cwd, COUNT(*) as cnt, GROUP_CONCAT(session_id) as ids
        FROM (
            SELECT cwd, substr(first_user_prompt, 1, 50) as prompt_key, session_id
            FROM sessions WHERE cwd IS NOT NULL AND first_user_prompt != ''
        ) GROUP BY cwd, prompt_key HAVING cnt > 1 ORDER BY cnt DESC LIMIT 20
    """)
    print("\n--- Duplicate Sessions (same cwd + similar prompt) ---")
    for row in c.fetchall():
        print(f"  {row[0]}: {row[1]} sessions (ids: {row[2][:80]}...)")

    # Corrupt JSONL files
    c.execute("""
        SELECT session_id, path, corrupt_lines, line_count
        FROM sessions WHERE corrupt_lines > 0 ORDER BY corrupt_lines DESC LIMIT 10
    """)
    print("\n--- Sessions with Corrupt Lines ---")
    for row in c.fetchall():
        print(f"  {row[0]}: {row[1]} ({row[2]}/{row[3]} corrupt)")

    # FTS5 search demo
    c.execute("""
        SELECT s.session_id, s.date, s.cwd, s.first_user_prompt
        FROM sessions s
        WHERE s.session_id IN (
            SELECT session_id FROM sessions_fts WHERE sessions_fts MATCH '코드' LIMIT 5
        )
    """)
    print("\n--- FTS5 Search: '코드' ---")
    for row in c.fetchall():
        print(f"  {row[0]}: {str(row[3])[:80]}")

    # Resume capability (sessions with task_started event)
    c.execute("SELECT COUNT(*) FROM sessions WHERE has_user_event = 1")
    resume_capable = c.fetchone()[0]
    print(f"\nSessions with resume capability: {resume_capable}/{total} ({100*resume_capable/total:.1f}%)")


# ── Compare with state_5.sqlite ─────────────────────────────────────────

def compare_with_state_db(conn):
    """Compare indexed sessions with Codex's state_5.sqlite."""
    if not STATE_DB.exists():
        print(f"\n[SKIP] state_5.sqlite not found at {STATE_DB}")
        return

    c = conn.cursor()

    # Get all session_ids from our index
    c.execute("SELECT session_id FROM sessions")
    indexed_ids = set(row[0] for row in c.fetchall())

    # Get session IDs from state_5.sqlite
    import subprocess
    result = subprocess.run(
        ["sqlite3", str(STATE_DB), "SELECT id FROM threads"],
        capture_output=True, text=True
    )
    state_ids = set(line.strip() for line in result.stdout.strip().split("\n") if line.strip())

    # Find discrepancies
    only_in_state = state_ids - indexed_ids  # in SQLite but not indexed
    only_indexed = indexed_ids - state_ids   # indexed but not in SQLite

    print("\n" + "=" * 60)
    print("COMPARISON WITH state_5.sqlite")
    print("=" * 60)
    print(f"State DB threads: {len(state_ids)}")
    print(f"Indexed sessions: {len(indexed_ids)}")
    print(f"In state but not indexed: {len(only_in_state)}")
    print(f"Indexed but not in state: {len(only_indexed)}")

    if only_in_state:
        print("\n--- Examples of sessions in state_5.sqlite but NOT indexed ---")
        for sid in list(only_in_state)[:10]:
            print(f"  {sid}")

    if only_indexed:
        print("\n--- Examples of indexed sessions NOT in state_5.sqlite ---")
        for sid in list(only_indexed)[:10]:
            print(f"  {sid}")


# ── Main ─────────────────────────────────────────────────────────────────

def main():
    print(f"Scanning sessions from: {SESSIONS_DIR}")
    print(f"Index DB: {INDEX_DB}")

    # Scan all JSONL files (streaming)
    metas = scan_all_sessions()
    print(f"\nExtracted metadata for {len(metas)} sessions")

    # Initialize and populate index
    conn = init_db(INDEX_DB)
    for meta in metas:
        upsert_session(conn, meta)

    # Run analysis
    run_analysis(conn)

    # Compare with state_5.sqlite
    compare_with_state_db(conn)

    conn.close()
    print(f"\nDone. Index saved to {INDEX_DB}")


if __name__ == "__main__":
    main()
