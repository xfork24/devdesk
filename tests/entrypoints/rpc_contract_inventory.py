#!/usr/bin/env python3
"""Require review of every RPC/platform literal before it enters a dispatcher."""
import json
from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[2]
PREFIXES = set("domain query project feature task work workflow conversation session turn interaction proposal orchestration manager settings credentials runtime codex models model model_provider model_catalog repository integration tool skill role agent capability permission sandbox harness standard spec delivery_type configuration context document snapshot definition binding decision execution github experience workspace tasks materials tests git".split())


def main():
    inventory = json.loads((ROOT / "crates/core/src/rpc_contracts.json").read_text())
    entries = {entry["method"]: entry for entry in inventory["entries"]}
    assert len(entries) == len(inventory["entries"]), "duplicate ingress contract"
    ignored = set(inventory["non_methods"])
    found = set()
    for base in (ROOT / "crates/host/src", ROOT / "crates/core/src"):
        for path in base.glob("*.rs"):
            if path.name in {"loading_performance.rs", "history_tests.rs", "workflow_live.rs"}:
                continue
            content = re.split(r"\n#\[cfg\(test\)\]\s*\nmod tests\b", path.read_text(), maxsplit=1)[0]
            for name in re.findall(r'"([a-z][a-z_]*(?:\.[a-z][a-z_]*)+)"', content):
                if name.split(".")[0] in PREFIXES:
                    found.add(name)
    missing = found - entries.keys() - ignored
    assert not missing, f"unreviewed ingress literals: {sorted(missing)}"
    governance = (ROOT / "crates/core/src/governance.rs").read_text()
    dynamic = re.search(r'let config_create = \[(.*?)\]\s*\.contains\(&family\)', governance, re.S)
    assert dynamic, "generic source dispatcher changed; review ingress inventory"
    families = set(re.findall(r'"([a-z_]+)"', dynamic.group(1))) | {"agent", "delivery_type"}
    for family in families:
        for action in ("create", "revise"):
            assert f"{family}.{action}" in entries, f"missing generic ingress: {family}.{action}"
    for method, entry in entries.items():
        assert entry["access"] in {"read", "mutation", "platform_read", "platform_mutation", "retired"}, method
        assert entry["scope"] in {"global", "explicit"}, method
        if entry["access"] in {"mutation", "platform_mutation"}:
            assert entry["precondition"] not in {"", "none", "rejected"}, method
    routes = set(re.findall(r'\.route\(\s*"([^\"]+)"', (ROOT / "crates/host/src/main.rs").read_text()))
    declared = {entry["path"] for entry in inventory["http"]}
    assert routes == declared, f"HTTP ingress inventory changed: {sorted(routes ^ declared)}"
    for entry in inventory["http"]:
        if entry["access"] in {"lifecycle", "callback"}:
            assert entry["precondition"] not in {"", "none"}, entry["path"]
    print(f"PASS RPC inventory: {len(entries)} contracts, {len(routes)} HTTP routes, {len(ignored)} non-method literals")


if __name__ == "__main__":
    main()
