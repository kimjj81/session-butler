#!/usr/bin/env python3
"""Phase 4: Write summary layer results to state DB and JSON."""

import json
from pathlib import Path
from datetime import datetime, timezone

SESSIONS_DIR = Path("/Users/kimjeongjin/.hermes/sessions")
STATE_DB = SESSIONS_DIR.parent / "session_index" / "state_5.sqlite"

def load_session(path):
    with open(path) as f:
        return json.load(f)

def flatten_content(content):
    if isinstance(content, list):
        return " ".join(
            item.get("text", "") 
            for item in content 
            if isinstance(item, dict) and "text" in item
        )
    return content or ""

def extract_summary(session, max_chars=500):
    msgs = session.get("messages", [])
    events = []
    for msg in msgs:
        role = msg.get("role", "")
        content = flatten_content(msg.get("content", ""))
        
        if role == "user" and len(content) > 20:
            events.append(("USER", content[:150]))
        elif role == "assistant" and len(content) > 20:
            events.append(("ASSISTANT", content[:150]))
        elif role == "tool":
            tool_name = msg.get("name", "") or msg.get("tool_name", "")
            events.append(("TOOL", f"{tool_name}: {content[:100]}"))
    
    lines = []
    for role, content in events[:20]:
        lines.append(f"  [{role}] {content}")
    return "\n".join(lines)[:max_chars]

def extract_keywords(session):
    msgs = session.get("messages", [])
    keywords = set()
    
    for msg in msgs:
        content = flatten_content(msg.get("content", ""))
        import re
        keywords.update(re.findall(r'(\w+\.py|\w+\.js|\w+\.md)', content))
        keywords.update(re.findall(r'(terminal|search_files|read_file|write_file)', content))
        kr_words = re.findall(r'[가-힣]{2,}', content)
        keywords.update(kr_words[:10])
    
    return " ".join(sorted(keywords))

def main():
    json_files = list(SESSIONS_DIR.glob("session_*.json"))
    
    summaries = []
    for f in json_files:
        try:
            session = load_session(f)
            
            summary = extract_summary(session)
            keywords = extract_keywords(session)
            
            summaries.append({
                "file": f.name,
                "session_id": session.get("session_id", ""),
                "model": session.get("model", ""),
                "start": session.get("session_start", ""),
                "size_bytes": f.stat().st_size,
                "message_count": len(session.get("messages", [])),
                "summary": summary,
                "keywords": keywords,
            })
        json.dump(fts_data, f, ensure_ascii=False, indent=2)