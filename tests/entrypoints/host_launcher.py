#!/usr/bin/env python3
"""Exercise launcher ownership/signals with a disposable worker, never a dataset."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest


@unittest.skipUnless(sys.platform == "darwin", "macOS launcher owns the signed child")
class LauncherLifecycle(unittest.TestCase):
    def test_detached_launcher_records_and_forwards_termination(self):
        launcher = Path(__file__).resolve().parents[2] / "scripts/dev/run-host.py"
        # Only code-signature validation is replaced. Popen, signal forwarding,
        # temporary copies, process ownership and waits run through production code.
        wrapper = """
import runpy, subprocess, sys
from unittest.mock import patch
launcher, worker = sys.argv[1:]
sys.argv = [launcher, worker]
with patch('subprocess.run', return_value=subprocess.CompletedProcess([], 0)):
    runpy.run_path(launcher, run_name='__main__')
"""
        for signum in (signal.SIGTERM, signal.SIGHUP):
            with self.subTest(signal=signum.name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                worker = root / "worker"
                worker.write_text(
                    "#!" + sys.executable + "\n"
                    "import time\n"
                    "print('fixture-worker-ready', flush=True)\n"
                    "time.sleep(30)\n"
                )
                worker.chmod(0o700)
                output = root / "launcher.log"
                with output.open("w") as log:
                    process = subprocess.Popen(
                        [sys.executable, "-c", wrapper, str(launcher), str(worker)],
                        stdin=subprocess.DEVNULL,
                        stdout=log,
                        stderr=subprocess.STDOUT,
                        start_new_session=True,
                    )
                    try:
                        deadline = time.monotonic() + 5
                        while "fixture-worker-ready" not in output.read_text():
                            self.assertIsNone(process.poll(), output.read_text())
                            self.assertLess(time.monotonic(), deadline, output.read_text())
                            time.sleep(0.01)
                        process.send_signal(signum)
                        self.assertEqual(process.wait(timeout=5), 128 + signum)
                    finally:
                        if process.poll() is None:
                            os.killpg(process.pid, signal.SIGKILL)
                            process.wait(timeout=5)
                events = [json.loads(line) for line in output.read_text().splitlines() if line.startswith("{")]
                self.assertEqual([event["event"] for event in events], ["started", "signal_received", "exited"])
                start, received, exit_event = events
                self.assertEqual(start["process_group"], process.pid)
                self.assertEqual(start["session_id"], process.pid)
                self.assertEqual(received["signal"], signum.name)
                self.assertEqual(exit_event["returncode"], -signum)
                self.assertEqual(exit_event["host_pid"], start["host_pid"])
                with self.assertRaises(ProcessLookupError):
                    os.kill(start["host_pid"], 0)


if __name__ == "__main__":
    unittest.main()
