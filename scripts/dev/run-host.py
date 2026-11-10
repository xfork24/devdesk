#!/usr/bin/env python3
"""Keep a running macOS Host's signed executable independent of later builds."""
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile


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

        def forward(signum, _frame):
            if child.poll() is None:
                child.send_signal(signum)

        for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            signal.signal(signum, forward)
        try:
            status = child.wait()
        finally:
            if child.poll() is None:
                child.terminate()
                child.wait()
        return status if status >= 0 else 128 - status


if __name__ == "__main__":
    raise SystemExit(main())
