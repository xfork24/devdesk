#!/usr/bin/env python3
"""Exercise signed caller checks across rebuilds without opening Keychain."""
import hashlib
import json
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
if sys.platform != "darwin":
    print("SKIP macOS credential helper runtime")
    raise SystemExit(0)

installed = Path.home() / "Library/Application Support/DevDesk/credential-helper/v1"
helper = installed / "devdesk-credential-helper"
manifest = installed / "manifest.json"
before = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in (helper, manifest)}
source = r'''
use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};
fn main() {
    println!("{}", std::env::current_exe().unwrap().display());
    io::stdout().flush().unwrap();
    for _ in io::stdin().lock().lines() {
        let mut child = Command::new(std::env::args().nth(1).unwrap())
            .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        // Invalid input proves caller verification succeeded; it never reaches Keychain.
        child.stdin.take().unwrap().write_all(b"{}").unwrap();
        let output = child.wait_with_output().unwrap();
        print!("{}", String::from_utf8(output.stdout).unwrap());
        io::stdout().flush().unwrap();
    }
}
'''

with tempfile.TemporaryDirectory(prefix="devdesk-helper-runtime-test-") as directory:
    root = Path(directory)
    rust = root / "probe.rs"
    rust.write_text(source)
    binary = root / "host"

    def build():
        subprocess.run(["rustc", "--edition=2024", str(rust), "-o", str(binary)], check=True)
        subprocess.run([str(ROOT / "scripts/platform/macos/sign-macos.sh"), str(binary)], check=True)

    def start(command):
        return subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

    def probe(process):
        process.stdin.write("probe\n")
        process.stdin.flush()
        return json.loads(process.stdout.readline())["code"]

    build()
    direct = start([str(binary), str(helper)])
    assert Path(direct.stdout.readline().strip()) == binary
    try:
        assert probe(direct) == "invalid_params"
        # Replacing/re-signing the build path reproduces the old launcher's failure.
        rust.write_text(source.replace('println!("{}",', 'println!("{0}",'))
        build()
        assert probe(direct) == "credential_helper_caller"
    finally:
        direct.stdin.close()
        direct.wait(timeout=10)

    wrapped = start([sys.executable, str(ROOT / "scripts/dev/run-host.py"), str(binary), str(helper)])
    runtime = Path(wrapped.stdout.readline().strip())
    try:
        assert runtime != binary and runtime.is_file()
        assert runtime.parent.stat().st_mode & 0o077 == 0
        assert probe(wrapped) == "invalid_params"
        rust.write_text(source)
        build()
        assert probe(wrapped) == "invalid_params", "Rebuild broke the running signed caller"
        # A copied untrusted caller must still be denied by the unchanged helper.
        foreign = root / "foreign"
        shutil.copyfile(binary, foreign)
        foreign.chmod(0o700)
        subprocess.run(["/usr/bin/codesign", "--force", "--sign", "-", str(foreign)], check=True)
        denied = start([str(foreign), str(helper)])
        try:
            denied.stdout.readline()
            assert probe(denied) == "credential_helper_caller"
        finally:
            denied.stdin.close()
            denied.wait(timeout=10)
        wrapped.send_signal(signal.SIGTERM)
        wrapped.wait(timeout=10)
    finally:
        if wrapped.poll() is None:
            wrapped.terminate()
            wrapped.wait(timeout=10)
        wrapped.stdin.close()
    assert not runtime.parent.exists(), "Runtime copy was not cleaned up"

assert before == {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in (helper, manifest)}
print("PASS reproduced overwritten Host rejection; isolated runtime survives rebuild; foreign caller denied; signal/cleanup; shared helper unchanged; no Keychain access")
