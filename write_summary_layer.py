#!/usr/bin/env python3
"""Phase 4: Write summary layer results to JSON files."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from summary_layer import (
    DEFAULT_FTS_PATH,
    DEFAULT_SESSIONS_DIR,
    DEFAULT_SUMMARY_PATH,
    build_summary_layer,
)


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate summary_layer.json and fts5_index.json")
    parser.add_argument("--sessions-dir", type=Path, default=DEFAULT_SESSIONS_DIR)
    parser.add_argument("--summary-path", type=Path, default=DEFAULT_SUMMARY_PATH)
    parser.add_argument("--fts-path", type=Path, default=DEFAULT_FTS_PATH)
    args = parser.parse_args()

    summary_payload, fts_payload = build_summary_layer(args.sessions_dir)

    write_json(args.summary_path, summary_payload)
    write_json(args.fts_path, fts_payload)

    print(f"Wrote {args.summary_path} ({summary_payload['session_count']} sessions)")
    print(f"Wrote {args.fts_path} ({fts_payload['session_count']} sessions)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
