#!/usr/bin/env python3
"""Compiler-driven sweep: rewrite `.kind` -> `.r#type` and `.project()` -> `.project_id()`
only on lines the compiler proves are ConfigurationRecord/Scope accesses."""
import json
import re
import subprocess
import sys
from pathlib import Path

CRATE = sys.argv[1] if len(sys.argv) > 1 else "devdesk-domain"
ROOT = Path(__file__).resolve().parents[1]


def check():
    p = subprocess.run(
        ["cargo", "check", "-p", CRATE, "--all-targets", "--message-format=json"],
        cwd=ROOT, capture_output=True, text=True,
    )
    return p.stdout


def fix_once():
    edits = {}
    for line in check().splitlines():
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        for d in msg.get("message", {}).get("children", []) if isinstance(msg.get("message"), dict) else []:
            pass
        m = msg.get("message") or {}
        if not isinstance(m, dict):
            continue
        text = m.get("message", "")
        spans = list(m.get("spans", []))
        for child in m.get("children", []):
            if isinstance(child, dict):
                spans += child.get("spans", [])
        targets = None
        if "no field `kind` on type" in text and ("ConfigurationRecord" in text or "SourceDefinition" in text):
            targets = (".kind", ".r#type")
        elif "no method named `project`" in text and "Scope" in text:
            targets = (".project()", ".project_id()")
        if not targets:
            continue
        for span in spans:
            if span.get("is_primary") and span.get("file_name", "").endswith(".rs"):
                key = (span["file_name"], span["line_start"])
                edits.setdefault(key, []).append(targets)
    for (file, lineno), subs in sorted(edits.items()):
        path = ROOT / file
        lines = path.read_text().splitlines(keepends=True)
        if lineno - 1 >= len(lines):
            continue
        new = lines[lineno - 1]
        for old, repl in subs:
            new = new.replace(old, repl)
        if new != lines[lineno - 1]:
            lines[lineno - 1] = new
            path.write_text("".join(lines))
            print(f"fixed {file}:{lineno}")
    return len(edits)


for _ in range(40):
    if fix_once() == 0:
        break
