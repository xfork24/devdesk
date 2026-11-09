#!/usr/bin/env python3
"""Check compatible helper reuse and rejection without touching the installed principal or Keychain."""
import hashlib
import http.cookiejar
import json
from pathlib import Path
import queue
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
if sys.platform != 'darwin':
    print('SKIP macOS credential helper startup')
    raise SystemExit(0)

installed = Path.home() / 'Library/Application Support/DevDesk/credential-helper/v1'
binary = installed / 'devdesk-credential-helper'
manifest = installed / 'manifest.json'
assert binary.is_file() and manifest.is_file(), 'Install a signed helper before this platform check'
before = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in [binary, manifest]}

with tempfile.TemporaryDirectory(prefix='devdesk-helper-ensure-') as directory:
    root = Path(directory)
    scripts = root / 'scripts/platform/macos'
    scripts.mkdir(parents=True)
    fingerprint = root / 'scripts/py/credential-helper-fingerprint.py'
    fingerprint.parent.mkdir()
    fingerprint.write_text('print("' + 'b' * 64 + '")\n')
    helper = root / 'compatible helper'
    helper.mkdir()
    source = (ROOT / 'scripts/platform/macos/install-macos-credential-helper.sh').read_text()
    old = 'helper_dir="$HOME/Library/Application Support/DevDesk/credential-helper/v1"'
    assert source.count(old) == 1
    script = scripts / 'install-macos-credential-helper.sh'
    script.write_text(source.replace(old, 'helper_dir=' + shlex.quote(str(helper))))
    copied = helper / binary.name
    metadata = helper / manifest.name

    def reset():
        shutil.copyfile(binary, copied)
        copied.chmod(0o700)
        value = json.loads(manifest.read_text())
        value['source_digest'] = 'a' * 64
        value['protocol'] = 1
        metadata.write_text(json.dumps(value))

    def run(*args):
        return subprocess.run(['bash', str(script), *args], capture_output=True, text=True)

    reset()
    fixture_before = copied.read_bytes(), metadata.read_bytes()
    result = run('--ensure')
    assert result.returncode == 0 and 'Host startup can continue' in result.stdout, result.stderr
    assert fixture_before == (copied.read_bytes(), metadata.read_bytes())
    strict = run()
    assert strict.returncode == 1 and 'explicit --update' in strict.stderr

    value = json.loads(metadata.read_text())
    value['protocol'] = 99
    metadata.write_text(json.dumps(value))
    assert run('--ensure').returncode != 0, 'Unsupported protocol was accepted'

    reset()
    with copied.open('ab') as output:
        output.write(b'altered-fixture')
    assert run('--ensure').returncode != 0, 'Changed helper content was accepted'

    reset()
    shutil.copyfile('/usr/bin/true', copied)
    value = json.loads(metadata.read_text())
    value['binary_sha256'] = hashlib.sha256(copied.read_bytes()).hexdigest()
    metadata.write_text(json.dumps(value))
    assert run('--ensure').returncode != 0, 'Foreign signed executable was accepted'

assert before == {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in [binary, manifest]}
print('PASS compatible startup reuse; strict update guard; protocol, content and signature rejection; installed helper unchanged')

if '--launch' in sys.argv:
    process = subprocess.Popen([str(ROOT / 'scripts/dev/dev.sh'), '--port', '0'], cwd=ROOT,
                               stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    output = queue.Queue()

    def read_output():
        for line in process.stdout:
            output.put(line)
        output.put(None)

    reader = threading.Thread(target=read_output, daemon=True)
    reader.start()
    try:
        deadline = time.monotonic() + 180
        lines = []
        url = None
        while time.monotonic() < deadline:
            try:
                line = output.get(timeout=1)
            except queue.Empty:
                continue
            if line is None:
                break
            lines.append(line.rstrip())
            if line.startswith('http://127.0.0.1:'):
                url = line.strip()
                break
        assert url, '\n'.join(lines[-20:])
        client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        client.open(url + '/', timeout=5).close()
        with client.open(url + '/capabilities', timeout=5) as response:
            assert json.load(response)['setup_required'] is True
        print('PASS actual dev.sh startup and setup HTTP endpoint; no model calls')
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGINT)
            try:
                process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
    assert before == {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in [binary, manifest]}
