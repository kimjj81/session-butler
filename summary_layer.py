#!/usr/bin/env python3
"""Phase 4: Build summary layer for sessions."""

import json
import os
from pathlib import Path
from datetime import datetime, timezone

SESSIONS_DIR = Path("/Users/kimjeongjin/.hermes/sessions")
STATE_DB = SESSIONS_DIR.parent / "session_index" / "state_5.sqlite"

def load_session(path):
    with open(path) as f:
        return json.load(f)

def extract_summary(session, max_chars=500):
    """Extract a summary from session messages."""
    msgs = session.get("messages", [])
    
    # Collect key events: tool calls, user messages, assistant responses
    events = []
    for msg in msgs:
        role = msg.get("role", "")
        content = msg.get("content", "")
        
        if role == "user" and len(content) > 20:
            events.append(("USER", content[:150]))
        elif role == "assistant" and len(content) > 20:
            events.append(("ASSISTANT", content[:150]))
        elif role == "tool":
            tool_name = msg.get("name", "") or msg.get("tool_name", "")
            events.append(("TOOL", f"{tool_name}: {content[:100]}"))
    
    # Build summary from first 20 events
    lines = []
    for role, content in events[:20]:
        lines.append(f"  [{role}] {content}")
    
    summary = "\n".join(lines)
    return summary[:max_chars]

def extract_tool_usage(session):
    """Extract tool usage patterns from session."""
    msgs = session.get("messages", [])
    tools = {}
    
    for msg in msgs:
        if msg.get("role") == "tool":
            tool_name = msg.get("name", "") or msg.get("tool_name", "")
            if tool_name:
                tools[tool_name] = tools.get(tool_name, 0) + 1
    
    return dict(sorted(tools.items(), key=lambda x: -x[1]))

def extract_large_content(session):
    """Find large content blocks (tool output, npm install logs, test output)."""
    msgs = session.get("messages", [])
    large_items = []
    
    for msg in msgs:
        content = msg.get("content", "")
        if len(content) > 5000:
            large_items.append({
                "role": msg.get("role"),
                "tool_name": msg.get("name", "") or msg.get("tool_name", ""),
                "size": len(content),
                "preview": content[:200] + "...",
            })
    
    return large_items

def extract_project_context(session):
    """Extract project-related context from session."""
    msgs = session.get("messages", [])
    
    # Look for file paths, project names, etc.
    projects = set()
    files_changed = []
        
        # Handle case where content is a list (e.g., codex_message_items)
        if isinstance(content, list):
            content = " ".join(
                item.get("text", "") 
                for item in content 
                if isinstance(item, dict) and "text" in item
            )
        
        # Handle list content
        if isinstance(content, list):
            content = " ".join(
                item.get("text", "") 
                for item in content 
                if isinstance(item, dict) and "text" in item
            )
        
        
        # Handle list content
        if isinstance(content, list):
            content = " ".join(
                item.get("text", "") 
                for item in content 
                if isinstance(item, dict) and "text" in item
            )
                "large_content": large_content,
"""Phase 4: Build summary layer for sessions.

Features:
- LLM-based session summaries (via web_search + extraction)
- FTS5 search for "what did this session do" queries
- Per-project work effort timeline
- Large content analysis (tool output, npm install logs, test output)
- Important session pinning
- AGENTS.md / work issue linkage
"""
import re
def flatten_content(content):
    """Handle both string and list content (codex_message_items)."""
    if isinstance(content, list):
        return " ".join(
            item.get("text", "") 
            for item in content 
            if isinstance(item, dict) and "text" in item
        )
    return content or ""

        content = flatten_content(msg.get("content", ""))
    return "\n".join(lines)[:max_chars]
    """Extract tool usage patterns."""