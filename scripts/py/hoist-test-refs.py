#!/usr/bin/env python3
"""Hoist whitelisted reference keys out of `"data":{...}` in test json! payloads
for <family>.create/.revise commands, mirroring the flattened envelope."""
import re
import sys
from pathlib import Path

REFERENCE_FIELDS = {
    "capability": ["harness_ref", "model_ref", "skills", "tools"],
    "agent": ["role_ref", "capability_ref"],
    "delivery_type": ["spec_refs"],
    "harness": ["model_providers", "model_catalog_ref"],
    "permission_context": ["sandbox_ref"],
    "standard": ["root_workflow_refs"],
}


def balanced(src: str, start: int) -> int:
    depth = 0
    for i in range(start, len(src)):
        if src[i] == "{":
            depth += 1
        elif src[i] == "}":
            depth -= 1
            if depth == 0:
                return i
    raise ValueError("unbalanced")


def split_top(content: str) -> list[str]:
    parts, depth, cur, instr = [], 0, "", False
    i = 0
    while i < len(content):
        c = content[i]
        if instr:
            cur += c
            if c == "\\" and i + 1 < len(content):
                cur += content[i + 1]
                i += 2
                continue
            if c == '"':
                instr = False
        elif c == '"':
            instr = True
            cur += c
        elif c in "{([":
            depth += 1
            cur += c
        elif c in "})]":
            depth -= 1
            cur += c
        elif c == "," and depth == 0:
            parts.append(cur)
            cur = ""
        else:
            cur += c
        i += 1
    if cur.strip():
        parts.append(cur)
    return parts


def transform(src: str) -> str:
    out = src
    pattern = re.compile(r'"(\w+)\.(?:create|revise)"')
    for m in list(pattern.finditer(out)):
        family = m.group(1)
        fields = REFERENCE_FIELDS.get(family)
        if not fields:
            continue
        probe = out[m.end() : m.end() + 4000]
        dm = re.search(r'"data":\{', probe)
        if not dm:
            continue
        lit_start = m.end() + dm.start()
        data_start = lit_start + len('"data":{') - 1  # at '{'
        try:
            data_end = balanced(out, data_start)
        except ValueError:
            continue
        content = out[data_start + 1 : data_end]
        keep, hoist = [], []
        for item in split_top(content):
            key = item.split(":", 1)[0].strip().strip('"')
            (hoist if key in fields else keep).append(item.strip())
        if not hoist:
            continue
        new_seg = (
            ",".join(hoist)
            + ',"data":{'
            + ",".join(x for x in keep if x)
            + "}"
        )
        out = out[:lit_start] + new_seg + out[data_end + 1 :]
    return out


def main() -> None:
    for base in sys.argv[1:]:
        for f in Path(base).rglob("*.rs"):
            src = f.read_text()
            new = src
            for _ in range(4):  # repeats catch nested/adjacent statements
                step = transform(new)
                if step == new:
                    break
                new = step
            if new != src:
                f.write_text(new)
                print("hoisted", f)


if __name__ == "__main__":
    main()
