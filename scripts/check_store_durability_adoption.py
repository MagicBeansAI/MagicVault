#!/usr/bin/env python3
"""Preserve the extraction baseline's two-way durable-I/O source ratchet.

The rules and per-file counts were transferred from Magician aef928c. They are
review debt, not claims that every matched rename is incorrect. Run through
make check only when verification is permitted. Changes to the baseline require
source review; moving a file must not silently discard its existing counts.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import defaultdict
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BASELINE_PATH = Path(__file__).resolve().parent / "store_durability_baseline.json"

# Crates with filesystem-backed stores. `magios/` and `magdroid/` are not Rust
# store surfaces and `ui/` is not Rust at all.
SCAN_ROOTS = ("magicvault-core/src",)

RULES: dict[str, re.Pattern[str]] = {
    # `with_extension("tmp")`, `with_extension("json.tmp")`, and friends.
    "fixed_temp_name": re.compile(r'with_extension\(\s*"[^"]*tmp"\s*\)'),
    # Any direct rename is a hand-rolled atomic write. The shared helper is the
    # only place that should be calling this.
    "hand_rolled_atomic_write": re.compile(r"\bfs::rename\("),
    # The reader whose first bad line fails the whole read.
    "intolerant_jsonl_reader": re.compile(r"\bread_jsonl_path\b"),
}

# The helper definitions themselves, which must keep doing the thing the rules
# forbid everyone else from doing.
EXEMPT_DEFINITION_MARKERS = (
    "fn write_bytes_atomic",
    "fn write_json_atomic_path",
    "fn write_json_compact_atomic_path",
    "fn read_jsonl_path",
    "fn atomic_write",
)


def rust_sources() -> list[Path]:
    files: list[Path] = []
    for root in SCAN_ROOTS:
        base = ROOT / root
        if base.is_dir():
            files.extend(sorted(base.rglob("*.rs")))
    return files


def scan() -> dict[str, dict[str, int]]:
    """`{relative path: {rule: count}}` for every file with at least one hit."""
    found: dict[str, dict[str, int]] = defaultdict(dict)
    for path in rust_sources():
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError as error:  # unreadable file is a real failure, not a skip
            raise SystemExit(f"store-durability guard: cannot read {path}: {error}")
        rel = path.relative_to(ROOT).as_posix()
        for line in text.splitlines():
            stripped = line.lstrip()
            # Comments describe these helpers; they do not call them. Without
            # this, documenting *why* a store should stop using `read_jsonl_path`
            # counts as another use of it — which is what happened the first time
            # a tolerant reader was added and explained itself in a doc link.
            if stripped.startswith("//") or stripped.startswith("*"):
                continue
            if any(marker in line for marker in EXEMPT_DEFINITION_MARKERS):
                continue
            for rule, pattern in RULES.items():
                if pattern.search(line):
                    found[rel][rule] = found[rel].get(rule, 0) + 1
    return dict(found)


def load_baseline() -> dict[str, dict[str, int]]:
    if not BASELINE_PATH.exists():
        raise SystemExit(
            f"store-durability guard: no baseline at {BASELINE_PATH}. "
            "Generate one with --update."
        )
    return json.loads(BASELINE_PATH.read_text(encoding="utf-8"))["files"]


def write_baseline(found: dict[str, dict[str, int]]) -> int:
    total = sum(sum(rules.values()) for rules in found.values())
    payload = {
        "_comment": (
            "Known-bad counts per file for scripts/check_store_durability_adoption.py. "
            "Counts, not line numbers, so ordinary edits do not churn this file. "
            "The ratchet is two-way: fix a store, re-run with --update, commit the "
            "smaller number."
        ),
        "total_violations": total,
        "files": dict(sorted(found.items())),
    }
    BASELINE_PATH.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    return total


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--update",
        action="store_true",
        help="rewrite the baseline from the current tree",
    )
    args = parser.parse_args()

    found = scan()

    if args.update:
        total = write_baseline(found)
        print(
            f"store-durability baseline updated: {total} known violations across "
            f"{len(found)} files"
        )
        return 0

    baseline = load_baseline()
    regressions: list[str] = []
    improvements: list[str] = []

    for rel in sorted(set(found) | set(baseline)):
        current = found.get(rel, {})
        allowed = baseline.get(rel, {})
        for rule in sorted(set(current) | set(allowed)):
            now = current.get(rule, 0)
            was = allowed.get(rule, 0)
            if now > was:
                regressions.append(
                    f"  {rel}: {rule} {was} -> {now}"
                    + ("  (new file)" if rel not in baseline else "")
                )
            elif now < was:
                improvements.append(f"  {rel}: {rule} {was} -> {now}")

    if regressions:
        print("store-durability adoption regressed:", file=sys.stderr)
        print("\n".join(regressions), file=sys.stderr)
        print(
            "\nThese helpers already exist and handle the cases a hand-rolled\n"
            "version misses — a unique temp name, sync_all plus a parent-directory\n"
            "sync, and a reader that survives a torn append:\n"
            "  use this library\'s reviewed durable publication boundary\n"
            "  retain append/journal recovery and parent-sync semantics\n"
            "Use one of those. If this hit is genuinely correct as written, say why\n"
            "in the code and re-run with --update.\n"
            "See README.md for extraction provenance.",
            file=sys.stderr,
        )
        return 1

    if improvements:
        print("store-durability adoption improved — commit the smaller baseline:")
        print("\n".join(improvements))
        print("\n  python3 scripts/check_store_durability_adoption.py --update")
        return 1

    total = sum(sum(rules.values()) for rules in found.values())
    print(
        f"store-durability adoption guard passed: {total} known violations across "
        f"{len(found)} files, none added"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
