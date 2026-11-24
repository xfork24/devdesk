#!/usr/bin/env python3
"""Kill a release Host at two real reset publication boundaries on private copies."""

import argparse
import contextlib
import concurrent.futures
import tempfile
import time
from pathlib import Path

from workbench import (
    REPO, client, clone, host, project_generations, required, reset_command,
    reset_seed, rpc, write,
)


def application_receipts(root):
    import sqlite3

    receipts = {}
    for database in root.rglob("engineering.sqlite3"):
        with contextlib.closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as conn:
            receipts[str(database.relative_to(root))] = conn.execute(
                "SELECT id,fingerprint,cursor FROM operations WHERE id LIKE 'factory-reset-sync:%' ORDER BY id",
            ).fetchall()
    return receipts


def probe(args, seed, after, number):
    name = f"reset-recovery-after-{after}"
    with tempfile.TemporaryDirectory(dir=args.output) as temp:
        fixture = Path(temp) / "fixture"
        clone(seed, fixture)
        initial = project_generations(fixture)
        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
            with host(args.host, fixture, args.output, name, expose_process=True) as (origin, _, _, process):
                preview_opener = client(origin)
                preview = required(preview_opener, origin, "query.template_reset_preview", {})
                command = reset_command(preview, False, number)
                mutator = client(origin)
                future = pool.submit(rpc, mutator, origin, "configuration.templates.reset", command)
                deadline = time.monotonic() + 20
                progress_params = {"command_id": command["command_id"]}
                while time.monotonic() < deadline:
                    observed = required(preview_opener, origin, "query.template_reset_progress", progress_params)
                    applied = observed.get("applied", -1)
                    if applied > after:
                        raise RuntimeError(f"Missed the exit point after {after} applications")
                    if applied == after:
                        process.kill()
                        process.wait(timeout=5)
                        break
                    time.sleep(0.002)
                else:
                    raise RuntimeError("Reset did not reach the intended exit point")
            interrupted, _ = future.result(timeout=10)
        if not interrupted.get("error"):
            raise RuntimeError("Reset completed before interruption")
        on_exit = project_generations(fixture)
        already_applied = [path for path in initial
                           if path != "global.sqlite3" and on_exit[path] != initial[path]]
        if on_exit["global.sqlite3"] != observed["generation"] or len(already_applied) != after:
            raise RuntimeError(f"Unexpected durable publication state: {on_exit}")
        before_receipts = application_receipts(fixture)
        with host(args.host, fixture, args.output, name + "-restart") as (origin, _, _):
            opener = client(origin)
            recovered = required(opener, origin, "query.template_reset_progress", progress_params)
            replayed = required(opener, origin, "configuration.templates.reset", command)
            replayed_again = required(opener, origin, "configuration.templates.reset", command)
            final = project_generations(fixture)
            after_receipts = application_receipts(fixture)
            if (
                recovered["phase"] != "completed" or recovered["total"] != len(final) - 1
                or replayed != replayed_again or replayed["generation"] != observed["generation"]
                or set(final.values()) != {observed["generation"]}
                or set(after_receipts) != set(before_receipts)
                or any(rows != before_receipts[path] for path, rows in after_receipts.items()
                       if path in already_applied)
                or any(len(rows) != len(before_receipts[path]) + 1 for path, rows in after_receipts.items()
                       if path not in already_applied)
            ):
                raise RuntimeError("Reset did not recover the original generation exactly once")
        return {
            "before_exit": observed, "on_exit": on_exit, "recovered": recovered,
            "complete_generations": final, "original_commit_preserved": already_applied,
            "replay": replayed, "interrupted_rpc_error": interrupted["error"],
        }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("host", "dataset", "output"):
        parser.add_argument(f"--{option}", required=True, type=lambda path: Path(path).resolve())
    args = parser.parse_args()
    if args.output.is_relative_to(args.dataset) or args.dataset == (REPO / "data").resolve():
        parser.error("Use an isolated backup and a separate output directory")
    if not (args.dataset / ".devdesk-dataset").is_file():
        parser.error("The backup is missing its dataset marker")
    if any(path.is_file() or path.is_symlink()
           for name in ("credentials", "shared-cache") for path in (args.dataset / name).rglob("*")):
        parser.error("Provide a backup stripped of runtime credentials and shared caches")
    args.output.mkdir(mode=0o700)
    with tempfile.TemporaryDirectory(dir=args.output) as temp:
        seed = Path(temp) / "seed"
        reset_seed(args, seed)
        results = [probe(args, seed, applied, number)
                   for number, applied in enumerate((0, 1), start=100)]
    write(args.output / "recovery.json", results)


if __name__ == "__main__":
    main()
