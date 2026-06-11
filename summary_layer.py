#!/usr/bin/env python3
"""Phase 4: Build summary layer for sessions."""

from __future__ import annotations

import json
import os
import re
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

BASE_DIR = Path(__file__).resolve().parent
DEFAULT_SESSIONS_DIR = Path(os.environ.get("SESSIONS_DIR", "/Users/kimjeongjin/.hermes/sessions"))
DEFAULT_SUMMARY_PATH = Path(os.environ.get("SUMMARY_LAYER_JSON", str(BASE_DIR / "summary_layer.json")))
DEFAULT_FTS_PATH = Path(os.environ.get("FTS5_INDEX_JSON", str(BASE_DIR / "fts5_index.json")))

STOP_WORDS = {
    "a",
    "an",
    "and",
    "are",
    "as",
    "at",
    "be",
    "by",
    "can",
    "do",
    "for",
    "from",
    "has",
    "have",
    "i",
    "if",
    "in",
    "is",
    "it",
    "json",
    "me",
    "my",
    "of",
    "on",
    "or",
    "path",
    "please",
    "python",
    "python3",
    "read",
    "session",
    "sessions",
    "the",
    "this",
    "that",
    "to",
    "tool",
    "tools",
    "true",
    "false",
    "user",
    "assistant",
    "with",
    "write",
    "you",
    "your",
}

TOKEN_RE = re.compile(r"[A-Za-z0-9_가-힣./-]{2,}")
FILE_RE = re.compile(r"\b[\w./-]+\.(?:py|js|ts|tsx|md|json|ya?ml|toml|sh|bash|sql|csv|txt|xml|zst|sqlite)\b")
PATH_RE = re.compile(r"(?<!\w)(?:~/?|/)[^\s\"'`<>]+")
TOOL_TAG_RE = re.compile(r"^\[(?P<tool>[A-Za-z0-9_.:-]+)\]")


def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def load_session(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def flatten_content(content: Any) -> str:
    """Handle message content that may be a string, list, or dict."""
    if content is None:
        return ""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts: list[str] = []
        for item in content:
            if isinstance(item, dict):
                text = item.get("text")
                if text:
                    parts.append(str(text))
            elif item is not None:
                parts.append(str(item))
        return " ".join(parts)
    if isinstance(content, dict):
        if "text" in content:
            return str(content.get("text") or "")
        return json.dumps(content, ensure_ascii=False, sort_keys=True)
    return str(content)


def clean_text(text: str) -> str:
    return re.sub(r"\s+", " ", text).strip()


def truncate(text: str, limit: int) -> str:
    text = clean_text(text)
    return text[:limit]


def extract_tool_name(content: str) -> str:
    match = TOOL_TAG_RE.match(content.strip())
    if match:
        return match.group("tool")
    return ""


def extract_summary(session: dict[str, Any], max_chars: int = 500) -> str:
    """Extract a compact summary from the first interesting session events."""
    events: list[tuple[str, str]] = []
    for msg in session.get("messages", []):
        role = msg.get("role", "")
        content = truncate(flatten_content(msg.get("content", "")), 150)
        if len(content) <= 20:
            continue
        if role in {"user", "assistant"}:
            events.append((role.upper(), content))
        elif role == "tool":
            tool_name = extract_tool_name(flatten_content(msg.get("content", "")))
            if not tool_name:
                tool_name = msg.get("name", "") or msg.get("tool_name", "")
            prefix = f"{tool_name}: " if tool_name else ""
            events.append(("TOOL", f"{prefix}{truncate(flatten_content(msg.get('content', '')), 100)}"))
        if len(events) >= 20:
            break

    lines = [f"  [{role}] {content}" for role, content in events]
    return "\n".join(lines)[:max_chars]


def extract_tool_usage(session: dict[str, Any]) -> dict[str, int]:
    """Count tools referenced by tool messages or tool_calls."""
    counter: Counter[str] = Counter()
    for msg in session.get("messages", []):
        content = flatten_content(msg.get("content", ""))
        if msg.get("role") == "tool":
            tool_name = extract_tool_name(content) or msg.get("name", "") or msg.get("tool_name", "")
            if tool_name:
                counter[str(tool_name)] += 1
        tool_calls = msg.get("tool_calls")
        if isinstance(tool_calls, list):
            for call in tool_calls:
                if not isinstance(call, dict):
                    continue
                fn = call.get("function", {})
                if isinstance(fn, dict):
                    name = fn.get("name")
                    if name:
                        counter[str(name)] += 1
    return dict(sorted(counter.items(), key=lambda item: (-item[1], item[0])))


def _extract_paths(text: str) -> list[str]:
    results: list[str] = []
    for pattern in (PATH_RE, FILE_RE):
        for match in pattern.findall(text):
            candidate = match.strip(".,;:)]}>")
            if not candidate:
                continue
            if "://" in candidate or candidate.startswith("data:"):
                continue
            results.append(candidate)
    return results


def extract_large_content(session: dict[str, Any]) -> list[dict[str, Any]]:
    """Find especially large content blocks."""
    large_items: list[dict[str, Any]] = []
    for msg in session.get("messages", []):
        content = flatten_content(msg.get("content", ""))
        if len(content) > 5000:
            large_items.append(
                {
                    "role": msg.get("role"),
                    "tool_name": extract_tool_name(content) or msg.get("name", "") or msg.get("tool_name", ""),
                    "size": len(content),
                    "preview": truncate(content, 200) + "...",
                }
            )
    return large_items


def extract_project_context(session: dict[str, Any]) -> list[str]:
    """Pull out the most relevant paths and file names mentioned in the session."""
    seen: set[str] = set()
    items: list[str] = []
    for msg in session.get("messages", []):
        content = flatten_content(msg.get("content", ""))
        for candidate in _extract_paths(content):
            if candidate in seen:
                continue
            seen.add(candidate)
            items.append(candidate)
            if len(items) >= 25:
                return items
    return items


def extract_keywords(session: dict[str, Any], max_keywords: int = 40) -> list[str]:
    """Generate a stable keyword list from the session content."""
    counter: Counter[str] = Counter()
    for msg in session.get("messages", []):
        role = msg.get("role", "")
        content = flatten_content(msg.get("content", ""))
        if not content:
            continue

        if role == "tool":
            tool_name = extract_tool_name(content) or msg.get("name", "") or msg.get("tool_name", "")
            if tool_name:
                counter[str(tool_name)] += 5

        for candidate in _extract_paths(content):
            base = Path(candidate).name
            stem = Path(base).stem
            if base:
                counter[base] += 4
            if stem and stem != base:
                counter[stem] += 2

        for token in TOKEN_RE.findall(content):
            token = token.strip("._-/")
            if not token:
                continue
            normalized = token.lower() if token.isascii() else token
            if normalized in STOP_WORDS or normalized.isdigit():
                continue
            counter[normalized] += 1

    ordered = [token for token, _ in counter.most_common()]
    return ordered[:max_keywords]


def _first_user_prompt(session: dict[str, Any]) -> str:
    for msg in session.get("messages", []):
        if msg.get("role") == "user":
            return flatten_content(msg.get("content", "")).strip()
    return ""


def _build_search_text(record: dict[str, Any]) -> str:
    parts = [
        record.get("first_user_prompt", ""),
        record.get("summary", ""),
        " ".join(record.get("keywords", [])),
        " ".join(record.get("project_context", [])),
        " ".join(record.get("tool_usage", {}).keys()),
        record.get("model", ""),
        record.get("platform", ""),
    ]
    return clean_text(" ".join(part for part in parts if part))


def build_session_record(path: Path) -> dict[str, Any]:
    session = load_session(path)
    messages = session.get("messages", [])
    first_user_prompt = _first_user_prompt(session)
    keywords = extract_keywords(session)
    tool_usage = extract_tool_usage(session)
    project_context = extract_project_context(session)
    summary = extract_summary(session)
    large_content = extract_large_content(session)

    record = {
        "session_id": session.get("session_id") or path.stem,
        "source_file": path.name,
        "path": str(path),
        "model": session.get("model", ""),
        "base_url": session.get("base_url", ""),
        "platform": session.get("platform", ""),
        "session_start": session.get("session_start", ""),
        "last_updated": session.get("last_updated", ""),
        "message_count": session.get("message_count", len(messages)),
        "title": truncate(first_user_prompt, 120),
        "first_user_prompt": first_user_prompt,
        "summary": summary,
        "keywords": keywords,
        "keyword_text": " ".join(keywords),
        "tool_usage": tool_usage,
        "large_content": large_content,
        "project_context": project_context,
    }
    return record


def build_summary_layer(sessions_dir: Path = DEFAULT_SESSIONS_DIR) -> tuple[dict[str, Any], dict[str, Any]]:
    """Build summary and FTS payloads from the session JSON files."""
    sessions_dir = Path(sessions_dir)
    if not sessions_dir.exists():
        raise FileNotFoundError(f"Sessions directory not found: {sessions_dir}")

    session_files = sorted(sessions_dir.glob("session_*.json"))
    summaries: list[dict[str, Any]] = []
    index_entries: list[dict[str, Any]] = []

    for path in session_files:
        record = build_session_record(path)
        summaries.append(record)
        index_entries.append(
            {
                "session_id": record["session_id"],
                "source_file": record["source_file"],
                "path": record["path"],
                "session_start": record["session_start"],
                "last_updated": record["last_updated"],
                "message_count": record["message_count"],
                "title": record["title"],
                "first_user_prompt": record["first_user_prompt"],
                "summary": record["summary"],
                "keywords": record["keywords"],
                "keyword_text": record["keyword_text"],
                "tool_usage": record["tool_usage"],
                "project_context": record["project_context"],
                "search_text": _build_search_text(record),
            }
        )

    generated_at = now_iso()
    summary_payload = {
        "schema_version": 1,
        "generated_at": generated_at,
        "sessions_dir": str(sessions_dir),
        "session_count": len(summaries),
        "sessions": summaries,
    }
    fts_payload = {
        "schema_version": 1,
        "generated_at": generated_at,
        "sessions_dir": str(sessions_dir),
        "session_count": len(index_entries),
        "index": index_entries,
    }
    return summary_payload, fts_payload


if __name__ == "__main__":
    summary_payload, fts_payload = build_summary_layer()
    print(
        json.dumps(
            {
                "summary_sessions": summary_payload["session_count"],
                "fts_sessions": fts_payload["session_count"],
                "sessions_dir": summary_payload["sessions_dir"],
            },
            ensure_ascii=False,
            indent=2,
        )
    )
