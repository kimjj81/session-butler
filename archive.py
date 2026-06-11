#!/usr/bin/env python3
"""Codex session archive tool — Phase 2.

Compresses rollout-*.jsonl files into .jsonl.zst, generates checksums,
supports restore and dry-run modes. Keeps originals in backup/quarantine.
Retains only the last 30 days of sessions by default.

Usage:
    python archive.py archive [--days N] [--dry-run] [--dest DIR]
    python archive.py restore [--session-id ID] [--all] [--dry-run] [--dest DIR]
    python archive.py list [--days N] [--json]
    python archive.py stats [--days N]
"""

import argparse
import datetime
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

# Defaults
DEFAULT_SOURCE = Path(os.environ.get("CODEX_SESSIONS", "/Users/kimjeongjin/.codex/sessions"))
DEFAULT_DEST = Path(os.environ.get("CODEX_ARCHIVE", "/Users/kimjeongjin/.codex/archive"))
DEFAULT_DAYS = 30
CHECKSUM_FILE = "checksums.jsonl"


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def parse_date_from_filename(filename: str) -> datetime.date | None:
    """Extract date from rollout-YYYY-MM-DDT...jsonl filename."""
    try:
        # rollout-2026-03-03T14-37-44-...jsonl
        date_str = filename.split("T")[0].replace("rollout-", "")
        return datetime.date.fromisoformat(date_str)
    except (ValueError, IndexError):
        return None


def discover_sessions(source: Path) -> list[dict]:
    """Discover all rollout JSONL files with metadata."""
    sessions = []
    for jsonl in source.rglob("rollout-*.jsonl"):
        date = parse_date_from_filename(jsonl.name)
        meta = {}
        try:
            with open(jsonl, "r", encoding="utf-8") as f:
                first_line = f.readline()
                obj = json.loads(first_line)
                payload = obj.get("payload", {})
                meta["id"] = payload.get("id", "")
                meta["model_provider"] = payload.get("model_provider", "unknown")
                meta["cli_version"] = payload.get("cli_version", "")
        except (json.JSONDecodeError, KeyError):
            pass

        sessions.append({
            "path": jsonl,
            "date": date,
            "size_bytes": jsonl.stat().st_size,
            **meta,
        })
    return sessions


def filter_by_days(sessions: list[dict], days: int) -> list[dict]:
    cutoff = datetime.date.today() - datetime.timedelta(days=days)
    return [s for s in sessions if s["date"] is not None and s["date"] >= cutoff]


def archive(sessions: list[dict], dest: Path, dry_run: bool = False):
    """Compress sessions into .jsonl.zst files."""
    dest.mkdir(parents=True, exist_ok=True)

    total_original = 0
    total_compressed = 0
    archived = []
    skipped = []

    for s in sessions:
        src = s["path"]
        # Build destination path preserving date structure
        if s["date"]:
            dest_path = dest / str(s["date"]) / src.relative_to(
                src.parent.parent.parent  # go up to sessions/ level
            )
        else:
            dest_path = dest / src.name

        dest_path.parent.mkdir(parents=True, exist_ok=True)
        zst_path = Path(str(dest_path) + ".zst")

        if dry_run:
            skipped.append(s)
            continue

        # Compress with zstd (level 3 for speed, good ratio)
        result = subprocess.run(
            ["zstd", "-3", "--force", str(src), "-o", str(zst_path)],
            capture_output=True,
        )
        if result.returncode != 0:
            print(f"ERROR compressing {src}: {result.stderr.decode()}", file=sys.stderr)
            skipped.append(s)
            continue

        # Generate checksum
        checksum = sha256_file(src)
        archived.append({
            "original": str(src),
            "compressed": str(zst_path),
            "checksum_sha256": checksum,
            "date": str(s["date"]) if s["date"] else None,
        })

        total_original += s["size_bytes"]
        total_compressed += zst_path.stat().st_size

    # Write checksums file
    if not dry_run:
        checksum_path = dest / CHECKSUM_FILE
        with open(checksum_path, "w", encoding="utf-8") as f:
            for entry in archived:
                f.write(json.dumps(entry, ensure_ascii=False) + "\n")

    # Print summary
    ratio = (1 - total_compressed / total_original) * 100 if total_original > 0 else 0
    print(f"Archived {len(archived)} sessions ({total_original / (1024**3):.1f}GB -> {total_compressed / (1024**3):.1f}GB, {ratio:.0f}% reduction)")
    if skipped:
        print(f"Skipped (dry-run): {len(skipped)} sessions")

    return archived, skipped


def restore(sessions: list[dict], dest: Path, days: int = DEFAULT_DAYS, dry_run: bool = False):
    """Restore sessions from .jsonl.zst back to source location."""
    filtered = filter_by_days(sessions, days)
    restored = []

    for s in filtered:
        src = s["path"]
        # Compute the .zst path relative to dest, mirroring archive layout.
        # Archive stores: dest/YYYY-MM-DD/<month>/<day>/file.jsonl.zst
        if s.get("date"):
            rel = src.relative_to(DEFAULT_SOURCE)  # e.g. "2026/06/03/file.jsonl"
            # Strip the year prefix to get month/day path used by archive
            rel_no_year = Path(*rel.parts[1:])  # "06/03/file.jsonl"
            zst_path = dest / str(s["date"]) / (rel_no_year.parent / (rel_no_year.name + ".zst"))
        else:
            zst_path = dest / (src.name + ".zst")

        if not zst_path.exists():
            print(f"WARNING: {zst_path} not found, skipping", file=sys.stderr)
            continue

        if dry_run:
            print(f"  restore {zst_path} -> {src}")
            restored.append(s)
            continue

        # Decompress
        result = subprocess.run(
            ["zstd", "-d", "--force", str(zst_path), "-o", str(src)],
            capture_output=True,
        )
        if result.returncode != 0:
            print(f"ERROR restoring {zst_path}: {result.stderr.decode()}", file=sys.stderr)
            continue

        restored.append(s)

    print(f"Restored {len(restored)} sessions")
    return restored


def list_sessions(sessions: list[dict], days: int, as_json: bool = False):
    """List sessions with metadata."""
    filtered = filter_by_days(sessions, days)

    if as_json:
        print(json.dumps(filtered, default=str, ensure_ascii=False))
    else:
        # Group by date
        by_date = {}
        for s in filtered:
            key = str(s["date"]) if s["date"] else "unknown"
            by_date.setdefault(key, []).append(s)

        for date in sorted(by_date.keys()):
            items = by_date[date]
            total_size = sum(s["size_bytes"] for s in items)
            print(f"\n{date}: {len(items)} sessions, {total_size / (1024**2):.1f}MB")
            for s in items[:5]:  # show first 5 per day
                print(f"  {s['path'].name} ({s['size_bytes'] / 1024:.0f}KB) model={s.get('model_provider', '?')}")
            if len(items) > 5:
                print(f"  ... and {len(items) - 5} more")


def show_stats(sessions: list[dict], days: int):
    """Show statistics about sessions."""
    filtered = filter_by_days(sessions, days)

    if not filtered:
        print("No sessions found.")
        return

    total_size = sum(s["size_bytes"] for s in filtered)
    by_provider = {}
    by_month = {}

    for s in filtered:
        provider = s.get("model_provider", "unknown")
        by_provider[provider] = by_provider.get(provider, 0) + 1

        month = str(s["date"])[:7] if s["date"] else "unknown"
        by_month[month] = by_month.get(month, 0) + 1

    print(f"Sessions (last {days} days): {len(filtered)}")
    print(f"Total size: {total_size / (1024**3):.2f} GB")
    print(f"\nBy provider:")
    for p, c in sorted(by_provider.items(), key=lambda x: -x[1]):
        print(f"  {p}: {c}")
    print(f"\nBy month:")
    for m, c in sorted(by_month.items()):
        print(f"  {m}: {c}")


def main():
    parser = argparse.ArgumentParser(description="Codex session archive tool")
    subparsers = parser.add_subparsers(dest="command", required=True)

    # archive command
    arch_parser = subparsers.add_parser("archive", help="Compress sessions to .jsonl.zst")
    arch_parser.add_argument("--days", type=int, default=DEFAULT_DAYS, help="Only archive sessions from last N days (default: 30)")
    arch_parser.add_argument("--dry-run", action="store_true", help="Show what would be archived without doing it")
    arch_parser.add_argument("--dest", type=str, default=DEFAULT_DEST, help="Archive destination directory")

    # restore command
    rst_parser = subparsers.add_parser("restore", help="Restore sessions from .jsonl.zst")
    rst_parser.add_argument("--session-id", type=str, default=None, help="Restore specific session by ID")
    rst_parser.add_argument("--all", action="store_true", help="Restore all archived sessions")
    rst_parser.add_argument("--days", type=int, default=DEFAULT_DAYS, help="Restore sessions from last N days")
    rst_parser.add_argument("--dry-run", action="store_true", help="Show what would be restored")
    rst_parser.add_argument("--dest", type=str, default=DEFAULT_DEST, help="Archive source directory")

    # list command
    lst_parser = subparsers.add_parser("list", help="List sessions")
    lst_parser.add_argument("--days", type=int, default=DEFAULT_DAYS, help="Show sessions from last N days")
    lst_parser.add_argument("--json", action="store_true", help="Output as JSON")

    # stats command
    st_parser = subparsers.add_parser("stats", help="Show session statistics")
    st_parser.add_argument("--days", type=int, default=DEFAULT_DAYS, help="Analyze last N days")

    args = parser.parse_args()
    sessions = discover_sessions(DEFAULT_SOURCE)

    if args.command == "archive":
        filtered = filter_by_days(sessions, args.days)
        archive(filtered, Path(args.dest), dry_run=args.dry_run)

    elif args.command == "restore":
        if args.session_id:
            filtered = [s for s in sessions if s.get("id") == args.session_id]
        elif args.all:
            filtered = sessions
        else:
            filtered = filter_by_days(sessions, args.days)

        restore(filtered, Path(args.dest), days=args.days, dry_run=args.dry_run)

    elif args.command == "list":
        list_sessions(sessions, args.days, as_json=args.json)

    elif args.command == "stats":
        show_stats(sessions, args.days)


if __name__ == "__main__":
    main()
