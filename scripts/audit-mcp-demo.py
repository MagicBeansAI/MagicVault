#!/usr/bin/env python3
"""Audit the recorded PUBLIC practice-account demo, without opening the vault.

This is a literal canary check of one Codex rollout, not a general security proof.
The canaries are published by the practice website. Never pass private secrets
to this script or use this demo-specific parser as a production DLP scanner.
"""
import argparse
import hashlib
import json
import re
import time
from pathlib import Path

CANARIES = {"public username": "student", "public password": "Password123"}


def call_arguments(source, start):
    depth, quote, escape = 1, None, False
    for i in range(start, len(source)):
        char = source[i]
        if quote:
            if escape:
                escape = False
            elif char == "\\":
                escape = True
            elif char == quote:
                quote = None
        elif char in "\"'`":
            quote = char
        elif char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            if depth == 0:
                return source[start:i]
    raise ValueError("Incomplete MCP call expression")


def mcp_replies(value):
    if isinstance(value, dict):
        content = value.get("content", [])
        if isinstance(content, list):
            for block in content:
                if isinstance(block, dict) and block.get("type") == "text":
                    try:
                        parsed = json.loads(block["text"])
                    except (ValueError, KeyError):
                        continue
                    if isinstance(parsed, dict) and "kind" in parsed and "data" in parsed:
                        yield parsed
        for item in value.values():
            if isinstance(item, (dict, list)):
                yield from mcp_replies(item)
    elif isinstance(value, list):
        for item in value:
            yield from mcp_replies(item)


def audit(path, server="magicvault_demo", require_jit=False):
    if not re.fullmatch(r"[a-z0-9_]+", server):
        raise ValueError("Invalid MCP server namespace")
    method = re.compile(r"tools\.mcp__" + re.escape(server) + r"__(\w+)\(")
    raw = path.read_bytes()
    requests, responses = [], []
    for line in raw.decode().splitlines():
        event = json.loads(line)
        if event.get("type") != "response_item":
            continue
        payload = event.get("payload", {})
        if payload.get("type") == "custom_tool_call":
            source = payload.get("input", "")
            for match in method.finditer(source):
                requests.append({"timestamp": event["timestamp"],
                                 "method": match[1],
                                 "arguments_source": call_arguments(source, match.end())})
        if payload.get("type") in ("function_call_output", "custom_tool_call_output"):
            for block in payload.get("output", []) if isinstance(payload.get("output"), list) else []:
                try:
                    parsed = json.loads(block.get("text", ""))
                except ValueError:
                    continue
                responses.extend({"timestamp": event["timestamp"], "response": reply}
                                 for reply in mcp_replies(parsed))
    if not requests or len(requests) != len(responses):
        raise ValueError(f"Incomplete extraction: {len(requests)} calls, {len(responses)} replies")
    if not any(r["response"].get("kind") == "fill" and
               r["response"]["data"].get("state") == "filled" for r in responses):
        raise ValueError("No successful fill receipt found")
    counts = {}
    for label, canary in CANARIES.items():
        counts[label] = {
            "mcp_call_source": sum(r["arguments_source"].count(canary) for r in requests),
            "mcp_replies": sum(json.dumps(r["response"]).count(canary) for r in responses),
            "entire_rollout": raw.decode().count(canary),
        }
    evidence = {}
    if require_jit:
        methods = [r["method"] for r in requests]
        if methods.count("secure_prompt_fill") != 1 or "secure_fill" in methods:
            raise ValueError("Expected exactly one JIT fill and no saved fill")
        snapshots = [r["response"]["data"] for r in responses if r["response"].get("kind") == "credentials"]
        if len(snapshots) < 2 or snapshots[0] != snapshots[-1]:
            raise ValueError("Missing or changed before/after credential metadata")
        evidence = {"jit_fill_calls": 1, "saved_fill_calls": 0,
                    "saved_records_before": len(snapshots[0]), "saved_records_after": len(snapshots[-1]),
                    "saved_metadata_unchanged": True}
    return {"source_file": path.name, "source_sha256": hashlib.sha256(raw).hexdigest(),
            "scope": "Literal public canaries in one recorded Codex rollout; not a general security proof.",
            "canary_source": "https://practicetestautomation.com/practice-test-login/",
            "counts": counts, "requests": requests, "responses": responses,
            "mcp_server": server, "jit_evidence": evidence}


def display(report, pause, only=None):
    def screen(title, lines):
        if only is not None and not title.startswith(str(only) + "."):
            return
        print("\033[2J\033[HMagicVault | Actual Codex transcript audit\n", flush=True)
        print(title + "\n")
        for line in lines:
            print(line)
        print("\nSource: recorded Codex session, not vault storage.", flush=True)
        time.sleep(pause)
    credential = next(r["response"] for r in report["responses"] if r["response"]["kind"] == "credentials")
    filled = next(r["response"] for r in report["responses"]
                  if r["response"]["kind"] == "fill" and r["response"]["data"]["state"] == "filled")
    screen("1. What MagicVault actually returned", json.dumps(credential, indent=2).splitlines())
    screen("2. The actual completion receipt", json.dumps(filled, indent=2).splitlines())
    lines = [f"Extracted {len(report['requests'])} MCP calls and {len(report['responses'])} replies.", ""]
    for label, counts in report["counts"].items():
        lines += [label.capitalize() + " literal matches:",
                  f"  MCP call source:   {counts['mcp_call_source']}",
                  f"  MCP replies:       {counts['mcp_replies']}",
                  f"  Entire rollout:    {counts['entire_rollout']}", ""]
    lines += ["Public test-account canaries; values omitted here.",
              "This checks this recording, not every possible browser tool."]
    screen("3. Computed credential-value check", lines)
    if report.get("jit_evidence"):
        evidence = report["jit_evidence"]
        screen("4. One-time input did not add a saved credential", [
            f"JIT fill calls:            {evidence['jit_fill_calls']}",
            f"Saved-credential fills:    {evidence['saved_fill_calls']}",
            f"Saved records before:     {evidence['saved_records_before']}",
            f"Saved records after:      {evidence['saved_records_after']}",
            f"All metadata unchanged:   {evidence['saved_metadata_unchanged']}",
            "", "Computed from the actual before/after MCP replies."])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rollout", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--server", default="magicvault_demo", help="Normalized MCP server namespace")
    parser.add_argument("--require-jit", action="store_true", help="Require one JIT fill and unchanged before/after metadata")
    parser.add_argument("--display", action="store_true")
    parser.add_argument("--pause", type=float, default=8)
    parser.add_argument("--screen", type=int, choices=(1, 2, 3, 4), help="Display one audit screen")
    args = parser.parse_args()
    report = audit(args.rollout, args.server, args.require_jit)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    if args.display:
        display(report, args.pause, args.screen)
    else:
        print(json.dumps(report["counts"], indent=2))
    if any(count for counts in report["counts"].values() for count in counts.values()):
        raise SystemExit("Public credential canary found; inspect the report.")
