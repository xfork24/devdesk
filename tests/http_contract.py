#!/usr/bin/env python3
"""Real host HTTP/security/lifecycle checks; all data lives in TemporaryDirectory."""
import http.client
import http.cookiejar
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv)>1 else ROOT / 'target/debug/devdesk-host'

def run():
    with tempfile.TemporaryDirectory(prefix='devdesk-http-') as directory:
        binary = BINARY
        if '--standalone' in sys.argv:
            binary = Path(directory) / 'devdesk-host'
            shutil.copy2(BINARY, binary)
        dataset = Path(directory) / '中文 data set'
        child = subprocess.Popen([str(binary), '--data', str(dataset), '--port', '0'], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, cwd=directory)
        try:
            child.stdout.readline()
            url = child.stdout.readline().strip()
            assert url.startswith('http://127.0.0.1:')
            authority = url.removeprefix('http://')
            cookies = http.cookiejar.CookieJar()
            client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(cookies))
            def req(path, data=None, extra=None, authenticated=True):
                body = json.dumps(data).encode() if data is not None else None
                request = urllib.request.Request(url + path, body, {'Content-Type':'application/json', **(extra or {})})
                try:
                    return (client.open(request, timeout=10) if authenticated else urllib.request.urlopen(request, timeout=10))
                except urllib.error.HTTPError as error:
                    return error
            for endpoint in ['/rpc','/events','/artifacts/unknown']:
                assert req(endpoint, {} if endpoint=='/rpc' else None, authenticated=False).status==401
            assert req('/', extra={'Host':'evil.invalid'}, authenticated=False).status==403
            assert req('/').status==200
            html = req('/').read().decode()
            assert '<div id="root"></div>' in html
            assets = re.findall(r'(?:src|href)="(/assets/[^\"]+)"', html)
            assert any(path.endswith('.js') for path in assets), assets
            assert any(path.endswith('.css') for path in assets), assets
            for path in assets:
                with req(path) as asset:
                    assert asset.status == 200
                    assert len(asset.read()) > 100
                    assert 'text/html' not in asset.headers['Content-Type']
            capabilities = json.load(req('/capabilities'))
            assert capabilities['execution_mode'] == 'simulation'
            assert capabilities['platform']['macos'] == 'runtime_verified_by_reports'
            assert capabilities['runtime']['os'] in ('macos', 'linux')
            # A second host must not acquire the same dataset, including a path
            # containing Unicode and spaces. It must not disturb the first host.
            locked = subprocess.run([str(binary), '--data', str(dataset), '--port', '0'], cwd=directory, capture_output=True, text=True, timeout=10)
            assert locked.returncode != 0, locked
            assert 'lock' in locked.stderr.lower() or '锁' in locked.stderr, locked.stderr
            assert req('/rpc', {}, {'Origin':'http://evil.invalid'}).status==403
            def rpc(method, params):
                result = json.load(req('/rpc', {'jsonrpc':'2.0','id':1,'method':method,'params':params}))
                assert 'error' not in result, result
                return result['result']
            project = rpc('project.create', {'command_id':'same-id','payload':{'title':'HTTP 持久化'}})
            assert project == rpc('project.create', {'command_id':'same-id','payload':{'title':'HTTP 持久化'}})
            assert len(rpc('query.workspace', {})['projects'])==1
            assert req('/artifacts/unknown').status==404
            # SSE stays connected while the host shuts down.
            stream = http.client.HTTPConnection(authority)
            stream.request('GET', '/events', headers={'Cookie':'; '.join(f'{c.name}={c.value}' for c in cookies)})
            response = stream.getresponse()
            assert response.status==200
            assert json.load(req('/shutdown', {}))['ok']
            assert child.wait(timeout=4)==0
            stream.close()
            child.stdout.close()
            child.stderr.close()
            child = subprocess.Popen([str(binary),'--data',str(dataset),'--port','0'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,cwd=directory)
            child.stdout.readline()
            url=child.stdout.readline().strip()
            client.open(url).read()
            assert rpc('query.workspace', {})['projects'][0]['title']=='HTTP 持久化'
            req('/shutdown', {}).read()
            assert child.wait(timeout=4)==0
            print('PASS HTTP: auth, Host/Origin, JS/CSS, capabilities, Unicode/space path, process lock, idempotence, Artifact access, SSE shutdown, restart persistence' + ('; standalone copied binary' if '--standalone' in sys.argv else ''))
        finally:
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=4)
            child.stdout.close()
            child.stderr.close()

if __name__=='__main__':
    run()
