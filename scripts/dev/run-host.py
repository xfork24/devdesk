#!/usr/bin/env python3
"""Keep a running macOS Host's signed executable independent of later builds."""
import os
from datetime import datetime, timezone
import json
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile


def lifecycle(event, **fields):
    print(
        json.dumps(
            {
                "type": "host_launcher",
                "event": event,
                "time": datetime.now(timezone.utc).isoformat(),
                "launcher_pid": os.getpid(),
                **fields,
            }
        ),
        file=sys.stderr,
        flush=True,
    )


def main():
    if len(sys.argv) < 2:
        raise SystemExit("Usage: run-host.py HOST_BINARY [HOST_ARGUMENTS...]")
    binary = Path(sys.argv[1]).resolve(strict=True)
    arguments = sys.argv[2:]
    if sys.platform != "darwin":
        os.execv(binary, [str(binary), *arguments])
    with tempfile.TemporaryDirectory(prefix="devdesk-host-") as directory:
        runtime = Path(directory)
        executable = runtime / "devdesk-host"
        shutil.copy2(binary, executable)
        executable.chmod(0o700)
        # These resources are resolved beside current_exe(), including in a source-free bundle.
        for name in ("credential-helper", "permissions"):
            resource = binary.parent / name
            if resource.is_dir():
                shutil.copytree(resource, runtime / name)
        subprocess.run(
            ["/usr/bin/codesign", "--verify", "--strict", str(executable)],
            check=True,
        )
        child = subprocess.Popen([str(executable), *arguments])
        lifecycle(
            "started",
            host_pid=child.pid,
            process_group=os.getpgrp(),
            session_id=os.getsid(0),
        )

        def forward(signum, _frame):
            lifecycle("signal_received", signal=signal.Signals(signum).name)
            if child.poll() is None:
                child.send_signal(signum)

        for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            signal.signal(signum, forward)
        try:
            status = child.wait()
        finally:
            if child.poll() is None:
                lifecycle("terminating_child", host_pid=child.pid)
                child.terminate()
                child.wait()
        lifecycle("exited", host_pid=child.pid, returncode=status)
        return status if status >= 0 else 128 - status


if __name__ == "__main__":
    raise SystemExit(main())
