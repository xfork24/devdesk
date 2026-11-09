#!/usr/bin/env python3
"""Real host HTTP/security/lifecycle checks; all data lives in TemporaryDirectory."""
import http.client
import http.cookiejar
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv)>1 else ROOT / 'target/debug/devdesk-host'

def run():
    with tempfile.TemporaryDirectory(prefix='devdesk-http-') as directory:
        binary = BINARY
        if '--standalone' in sys.argv:
            binary = Path(directory) / 'devdesk-host'
            shutil.copy2(BINARY, binary)
        # This test covers the HTTP boundary; fail closed if a workflow asks for
        # external infrastructure. Never build images or contact GitHub here.
        commands = Path(directory) / 'commands'
        commands.mkdir()
        for name in ['docker', 'gh']:
            command = commands / name
            command.write_text('#!/bin/sh\nexit 1\n')
            command.chmod(0o755)
        env = {**os.environ, 'PATH': str(commands) + os.pathsep + os.environ['PATH']}
        env.pop('DEVDESK_ACCESS_TOKEN', None)
        dataset = Path(directory) / '中文 data set'
        child = subprocess.Popen([str(binary), '--data', str(dataset), '--port', '0'], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, cwd=directory, env=env)
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
            assert capabilities['execution_mode'] == 'live'
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
            assert rpc('query.workspace', {})['repository_ready'] is False
            preferences = rpc('query.runtime_installations', {})
            assert preferences['default_harness'] == 'codex' and preferences['installations'] == []
            rpc('settings.update', {'default_harness':'dsh'})
            assert rpc('query.workspace', {})['default_harness'] == 'dsh'
            invalid = json.load(req('/rpc', {'jsonrpc':'2.0','id':2,'method':'settings.update','params':{'default_harness':'codex','global_concurrency':0}}))
            assert 'error' in invalid
            assert rpc('query.workspace', {})['default_harness'] == 'dsh'
            rpc('settings.update', {'default_harness':'codex'})
            project = rpc('project.create', {'command_id':'same-id','payload':{'title':'HTTP 持久化'}})
            assert project == rpc('project.create', {'command_id':'same-id','payload':{'title':'HTTP 持久化'}})
            assert len(rpc('query.workspace', {})['projects'])==1
            pid = project['id']
            db_a = dataset / 'projects' / pid / 'engineering.sqlite3'
            assert db_a.is_file()
            assert (dataset / 'global.sqlite3').is_file()
            assert not (dataset / 'engineering.sqlite3').exists()
            def configure(project_id):
                defaults = rpc('query.configuration_defaults', {})
                workspace = rpc('query.workspace', {})
                rpc('project.configuration.update', {'project_id':project_id,'command_id':'configure-'+project_id,'target':project_id,'expected_revision':workspace['active_project']['revision'],'payload':{'workflow_ref':defaults['workflow_ref'],'role_agent_bindings':defaults['role_agent_bindings']}})
            configure(pid)
            feature_a = rpc('feature.create', {'project_id':pid,'command_id':'same-feature-id','payload':{'project_id':pid,'title':'仅属于 A'}})
            rpc('document.revise', {'project_id':pid,'command_id':'a-doc','target':feature_a['id'],'expected_revision':1,'payload':{'kind':'requirement','title':'A 需求','body':'A 的独立产物'}})
            view_a = rpc('query.workspace', {'project_id':pid,'feature_id':feature_a['id']})
            artifact_a = next(document['artifact_id'] for document in view_a['detail']['documents'] if document['title']=='A 需求')
            project_b = rpc('project.create', {'command_id':'create-b','payload':{'title':'项目 B'}})['id']
            configure(project_b)
            feature_b = rpc('feature.create', {'project_id':project_b,'command_id':'same-feature-id','payload':{'project_id':project_b,'title':'仅属于 B'}})
            assert feature_a != feature_b
            view_b = rpc('query.workspace', {'project_id':project_b})
            assert view_b['active_project_id'] == project_b
            assert len(view_b['features']) == 1 and view_b['features'][0]['title'] == '仅属于 B'
            assert req('/artifacts/' + artifact_a + '?project_id=' + project_b).status == 404
            stale = json.load(req('/rpc', {'jsonrpc':'2.0','id':2,'method':'feature.create','params':{'project_id':pid,'command_id':'stale-project','payload':{'project_id':pid,'title':'不能串写'}}}))
            assert stale['error']['data']['code'] == 'project_changed', stale
            rpc('settings.update', {'global_concurrency':7})
            assert not (dataset / 'runtime.json').exists()
            with sqlite3.connect(dataset / 'global.sqlite3') as global_db:
                assert json.loads(global_db.execute("SELECT value FROM settings WHERE key='global_concurrency'").fetchone()[0]) == 7
                assert not global_db.execute("SELECT name FROM sqlite_master WHERE name IN ('projects','features','runs','deliveries')").fetchall()
            for project_id in [pid, project_b]:
                with sqlite3.connect(dataset / 'projects' / project_id / 'engineering.sqlite3') as project_db:
                    assert project_db.execute('SELECT id FROM projects').fetchall() == [(project_id,)]
                    assert project_db.execute('SELECT count(*) FROM features').fetchone()[0] == 1
                    assert not project_db.execute('PRAGMA foreign_key_check').fetchall()
            rpc('project.open', {'command_id':'open-a','target':pid,'payload':{}})
            assert rpc('feature.create', {'project_id':pid,'command_id':'same-feature-id','payload':{'project_id':pid,'title':'仅属于 A'}}) == feature_a
            assert req('/artifacts/' + artifact_a + '?project_id=' + pid).read().decode() == 'A 的独立产物'
            # SSE is pinned to the project selected when connecting; its cursor is namespaced.
            cookie_header = '; '.join(f'{c.name}={c.value}' for c in cookies)
            latest_a = max(e['id'] for e in rpc('query.events', {'project_id':pid}))
            old_stream = http.client.HTTPConnection(authority, timeout=2)
            old_stream.request('GET', '/events?project_id=' + pid + '&cursor=' + str(latest_a), headers={'Cookie':cookie_header})
            old_response = old_stream.getresponse()
            assert old_response.status == 200
            rpc('project.open', {'command_id':'open-b','target':project_b,'payload':{}})
            rpc('feature.create', {'project_id':project_b,'command_id':'b-event','payload':{'project_id':project_b,'title':'只通知 B'}})
            old_stream.sock.settimeout(0.3)
            try:
                leaked = old_response.readline()
                raise AssertionError(f'Inactive SSE leaked a different project event: {leaked!r}')
            except TimeoutError:
                pass
            finally:
                old_stream.close()
            scoped_stream = http.client.HTTPConnection(authority, timeout=2)
            scoped_stream.request('GET', '/events?project_id=' + project_b, headers={'Cookie':cookie_header,'Last-Event-ID':pid+':999999'})
            scoped_response = scoped_stream.getresponse()
            assert scoped_response.readline().decode().strip().startswith('id: ' + project_b + ':')
            scoped_stream.close()
            rpc('project.open', {'command_id':'restore-a','target':pid,'payload':{}})
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
            child = subprocess.Popen([str(binary),'--data',str(dataset),'--port','0'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,cwd=directory,env=env)
            child.stdout.readline()
            url=child.stdout.readline().strip()
            client.open(url).read()
            restored = rpc('query.workspace', {})
            assert restored['projects'][0]['title']=='HTTP 持久化'
            assert restored['active_project_id'] == pid
            assert restored['global_concurrency'] == 7
            assert len(restored['features']) == 1
            req('/shutdown', {}).read()
            assert child.wait(timeout=4)==0
            print('PASS HTTP: auth, Host/Origin, JS/CSS, capabilities, Unicode/space path, process lock, idempotence, Artifact access, SSE shutdown, restart persistence, project database isolation/switching, global SQLite settings, scoped SSE cursors' + ('; standalone copied binary' if '--standalone' in sys.argv else ''))
        finally:
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=4)
            child.stdout.close()
            child.stderr.close()

def connection_contract():
    with tempfile.TemporaryDirectory(prefix='devdesk-connection-http-') as directory:
        token = 'synthetic-http-connection-key-not-a-real-secret'
        env = {**os.environ, 'DEVDESK_ACCESS_TOKEN':token}
        clean_env = dict(env)
        del clean_env['DEVDESK_ACCESS_TOKEN']
        rejected_data = Path(directory) / 'must-not-be-created'
        denied = subprocess.run([str(BINARY),'--host','192.0.2.10','--data',str(rejected_data)], env=clean_env, capture_output=True, text=True, timeout=10)
        assert denied.returncode != 0 and 'DEVDESK_ACCESS_TOKEN' in denied.stderr
        assert not rejected_data.exists()
        child = subprocess.Popen([str(BINARY),'--host','127.0.0.1','--data',str(Path(directory)/'data'),'--port','0'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        peer = None
        try:
            child.stdout.readline()
            url = child.stdout.readline().strip()
            assert url.startswith('http://127.0.0.1:'), child.stderr.read() if child.poll() is not None else url
            cookies = http.cookiejar.CookieJar()
            client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(cookies))
            def req(path, data=None, headers=None):
                request=urllib.request.Request(url+path, json.dumps(data).encode() if data is not None else None, {'Content-Type':'application/json',**(headers or {})})
                try:return client.open(request,timeout=10)
                except urllib.error.HTTPError as error:return error
            probe=req('/connection', headers={'Origin':'http://another-kernel.invalid'})
            assert probe.status==200 and probe.headers['Access-Control-Allow-Origin']=='*'
            assert probe.headers.get('Set-Cookie') is None
            assert json.load(probe)=={'product':'devdesk','protocol_version':1,'authentication_required':True}
            assert req('/connection',headers={'Host':'evil.invalid'}).status==403
            assert req('/').status==200 and len(cookies)==0
            for endpoint in ['/capabilities','/events','/artifacts/unknown']:
                assert req(endpoint).status==401
            assert req('/rpc',{}).status==401
            assert req('/session',{'access_token':token},{'Origin':'http://evil.invalid'}).status==403
            assert req('/session',{'access_token':'wrong'}).status==401 and len(cookies)==0
            session=req('/session',{'access_token':token},{'Origin':url})
            assert session.status==200
            assert 'HttpOnly' in session.headers['Set-Cookie'] and 'SameSite=Strict' in session.headers['Set-Cookie']
            assert token not in session.headers['Set-Cookie']
            assert json.load(session)=={'ok':True}
            assert json.load(req('/capabilities'))['setup_required'] is False
            # Browsers share cookies across ports. Distinct names must keep
            # both protected kernels signed in without overwriting a session.
            peer = subprocess.Popen([str(BINARY),'--data',str(Path(directory)/'peer'),'--port','0'], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            peer.stdout.readline()
            peer_url = peer.stdout.readline().strip()
            original_url = url
            url = peer_url
            assert req('/capabilities').status==401
            assert req('/session',{'access_token':token}).status==200
            assert len(cookies)==2
            assert json.loads(req('/capabilities').read())['setup_required'] is False
            assert json.load(req('/shutdown',{}))['ok']
            assert peer.wait(timeout=5)==0
            url = original_url
            assert req('/capabilities').status==200
            rpc=req('/rpc',{'jsonrpc':'2.0','id':1,'method':'query.workspace','params':{}})
            assert rpc.headers.get('Access-Control-Allow-Origin') is None
            assert json.load(rpc)['result']['projects']==[]
            assert req('/rpc',{}, {'Origin':'http://evil.invalid'}).status==403
            assert req('/connection').headers.get('Set-Cookie') is None
            assert json.load(req('/shutdown',{}))['ok']
            assert child.wait(timeout=5)==0
            print('PASS connection: discovery-only CORS, no anonymous remote session, key authentication, Host/Origin protection, HttpOnly cookies with separate port names, remote opt-in before dataset initialization')
        finally:
            if peer is not None:
                if peer.poll() is None:peer.terminate();peer.wait(timeout=5)
                peer.stdout.close();peer.stderr.close()
            if child.poll() is None:
                child.terminate();child.wait(timeout=5)
            child.stdout.close();child.stderr.close()

if __name__=='__main__':
    run()
    connection_contract()
