#!/usr/bin/env python3
"""Check the packaged, descriptive reverse-reference index of each kernel catalog.

The index algorithm has one implementation, the SDK artifact's usage-index.mjs; this entry
point checks every kernel catalog against the index stored next to its manifest, and --write
refreshes it. The same artifact serves a domain package's build.
"""
import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CATALOGS = [ROOT / "kernel/core", ROOT / "kernel/engineering"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true", help="update the packaged index")
    args = parser.parse_args()
    sdk = Path(sys.argv[0] if False else ROOT / "sdk/domain")
    cli = sdk / "cli/usage-index.mjs"
    if not cli.exists():
        raise SystemExit(f"找不到 SDK 的引用索引工具：{cli}")
    for catalog in CATALOGS:
        command = ["node", str(cli)]
        if args.write:
            command.append("--write")
        command.append("--check" if not args.write else str(catalog))
        if not args.write:
            command.append(str(catalog))
        run = subprocess.run(command, capture_output=True, text=True, timeout=60)
        if run.returncode != 0:
            raise SystemExit(run.stdout + run.stderr)
        print(run.stdout.strip())


if __name__ == "__main__":
    main()
