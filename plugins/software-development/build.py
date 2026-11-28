#!/usr/bin/env python3
"""Package the software-development domain ZIP through the devdesk-domain CLI.

The package is declarative: domain.json and catalog/ hold what the ZIP carries; this script
only brings in the Web bundle and hands the rest to the SDK artifact's command line tool.
Build independence: the CLI needs the SDK directory, this package directory, Node and Python
(with jsonschema). Set DEVDESK_SDK to an SDK artifact to build without kernel sources.
"""
import argparse
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
PACKAGE = Path(__file__).resolve().parent
VERSION = "0.6.0"


def sdk():
    found = os.environ.get("DEVDESK_SDK") or (REPO / "sdk/domain")
    if not (Path(found) / "cli/devdesk-domain.mjs").exists():
        raise SystemExit(f"找不到 SDK：{found} 里没有 cli/devdesk-domain.mjs")
    return Path(found).resolve()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", action="version", version=VERSION)
    parser.add_argument("target", type=Path)
    parser.add_argument("--web-dir", type=Path, default=REPO / "target/web/software")
    parser.add_argument("--runtime-only", action="store_true",
                        help="omit the Web target; used to build test fixtures")
    args = parser.parse_args()

    command = [
        "node", str(sdk() / "cli/devdesk-domain.mjs"), "build", str(PACKAGE),
        "--sdk", str(sdk()), "--out", str(args.target),
    ]
    if not args.runtime_only:
        if not args.web_dir.exists():
            raise SystemExit(f"Web 构建产物缺失：{args.web_dir}；先运行 npm --prefix web run build:software")
        # The static UI files live in the package; the built modules are brought in here.
        command += ["--asset", f"{args.web_dir}:ui/web"]
    else:
        command += ["--no-ui"]
    run = subprocess.run(command, env={**os.environ, "DEVDESK_PYTHON": sys.executable})
    raise SystemExit(run.returncode)


if __name__ == "__main__":
    main()
