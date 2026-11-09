#!/usr/bin/env python3
"""Real HTTP/SQLite boundary with an explicit fake Docker CLI; no models/network/images."""
import concurrent.futures
import http.cookiejar
import json
import os
from pathlib import Path
import subprocess
import shutil
import sys
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / 'target/debug/devdesk-host'

with tempfile.TemporaryDirectory(prefix='devdesk-prepare-') as tmp:
    root = Path(tmp)
    # The GitHub API is a declared fixture; clone/worktree operations use real Git
    # against a private local repository, never the user's repositories or network.
    real_git = shutil.which('git')
    source = root / 'source'
    subprocess.run([real_git, 'init', '-q', '-b', 'main', str(source)], check=True)
    (source/'README.md').write_text('isolated Git fixture\n')
    subprocess.run([real_git, '-C', str(source), 'add', 'README.md'], check=True)
    subprocess.run([real_git, '-C', str(source), '-c', 'user.name=Fixture', '-c', 'user.email=fixture@localhost', '-c', 'commit.gpgSign=false', 'commit', '-qm', 'fixture'], check=True)
    head = subprocess.check_output([real_git, '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
    gh = root / 'gh'
    gh.write_text('#!' + sys.executable + '\n' + "import json,os\nprint(json.dumps({'data':{'repository':{'id':'fixture-repository','nameWithOwner':'fixture/repository','viewerPermission':'WRITE','ref':{'target':{'oid':os.environ['DEVDESK_GIT_HEAD']}},'branchProtectionRules':{'totalCount':0,'pageInfo':{'hasNextPage':False}},'rulesets':{'nodes':[],'pageInfo':{'hasNextPage':False}}}}}))\n")
    gh.chmod(0o755)
    git = root / 'git'
    git.write_text('#!' + sys.executable + '\n' + r'''
import os,subprocess,sys
from pathlib import Path
args=sys.argv[1:];real=os.environ['DEVDESK_REAL_GIT']
if 'clone' in args:
    assert args[-2]=='https://github.com/fixture/repository.git'
    parent=Path(args[args.index('-C')+1])
    args[args.index('protocol.file.allow=never')]='protocol.file.allow=always'
    args[-2]=os.environ['DEVDESK_GIT_SOURCE']
    with (Path(os.environ['DEVDESK_DOCKER_FIXTURE'])/'clones').open('a') as f:f.write('clone\n')
    root=Path(os.environ['DEVDESK_DOCKER_FIXTURE'])
    if not (root/'interrupted-clone').exists():
        (root/'interrupted-clone').touch()
        subprocess.run([real,'init','--bare','-q',str(parent/'repository')],check=True)
        subprocess.run([real,'-C',str(parent/'repository'),'config','remote.origin.url','https://github.com/fixture/repository.git'],check=True)
        (parent/'repository'/'preserve-fixture').write_text('must survive reconciliation')
        sys.exit(1)
    result=subprocess.run([real,*args])
    if result.returncode==0:
        subprocess.run([real,'-C',str(parent/'repository'),'config','remote.origin.url','https://github.com/fixture/repository.git'],check=True)
    sys.exit(result.returncode)
if 'fetch' in args:
    assert args[-2]=='origin'
    args[args.index('protocol.file.allow=never')]='protocol.file.allow=always'
    args[-2]=os.environ['DEVDESK_GIT_SOURCE']
    with (Path(os.environ['DEVDESK_DOCKER_FIXTURE'])/'fetches').open('a') as f:f.write('fetch\n')
os.execv(real,[real,*args])
''')
    git.chmod(0o755)
    fixture = root / 'docker'
    fixture.write_text('#!' + sys.executable + '\n' + r'''
import hashlib,json,os,sys,time
from pathlib import Path
root=Path(os.environ['DEVDESK_DOCKER_FIXTURE']);args=sys.argv[1:]
with (root/'calls').open('a') as f:f.write(json.dumps(args)+'\n')
if args[0]=='info':
    if (root/'offline').exists():sys.exit(1)
    print('linux');sys.exit(0)
if args[:2]==['image','inspect']:
    ref=args[-1];key=hashlib.sha256(ref.encode()).hexdigest()
    file=root/key
    if not file.exists():sys.exit(1)
    print(file.read_text());sys.exit(0)
if args[0]=='build':
    tag=args[args.index('--tag')+1];context=Path(args[-1])
    assert (context/'Dockerfile').is_file()
    if 'managed-dsh' in tag:
        for name in ['package.json','tool-policy.mjs','bwrap-runner.sh']:
            assert (context/'permissions/backends/dsh'/name).is_file()
    time.sleep(.3)
    if (root/'fail-build').exists():sys.exit(2)
    digest='sha256:'+hashlib.sha256(tag.encode()).hexdigest()
    value=json.dumps({'id':digest,'os':'linux','arch':'arm64'})
    for ref in [tag,digest]:(root/hashlib.sha256(ref.encode()).hexdigest()).write_text(value)
    print(digest);sys.exit(0)
sys.exit(3)
''')
    fixture.chmod(0o755)
    env = {**os.environ, 'PATH': str(root) + os.pathsep + os.environ['PATH'], 'DEVDESK_DOCKER_FIXTURE': str(root), 'DEVDESK_REAL_GIT': real_git, 'DEVDESK_GIT_SOURCE': str(source), 'DEVDESK_GIT_HEAD': head}
    child = subprocess.Popen([str(BINARY), '--data', str(root / 'data'), '--port', '0'], cwd=root, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        child.stdout.readline()
        url = child.stdout.readline().strip()
        assert url.startswith('http://127.0.0.1:'), url
        def client():
            opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
            opener.open(url + '/').close()
            return opener
        def rpc(method, params=None):
            request = urllib.request.Request(url + '/rpc', json.dumps({'jsonrpc':'2.0','id':1,'method':method,'params':params or {}}).encode(), {'Content-Type':'application/json'})
            with client().open(request, timeout=20) as response: return json.load(response)
        # Queries and project opening do not prepare any Harness.
        assert rpc('runtime.preparation_status')['result']['preparations'] == []
        assert rpc('query.runtime_installations')['result']['installations'] == []
        assert not (root / 'calls').exists()
        defaults = rpc('query.default_configuration')['result']
        assert len(defaults['bundle']['data']['role_refs']) == 5
        model = next(row for row in defaults['records'] if row['id'] == defaults['bundle']['data']['model_ref'])
        assert model['data']['model'] == 'gpt-6.1-sol' and model['data']['parameters']['reasoning_effort'] == 'high'
        opened = rpc('project.open_github', {'command_id':'github-open-fixture', 'payload':{'repository':'https://github.com/fixture/repository.git'}})
        assert 'result' in opened, opened
        github_id = opened['result']['id']
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            state = rpc('query.repository', {'project_id':github_id})['result']
            if state['operations'][0]['status'] in ['completed','needs_reconciliation']: break
            time.sleep(.1)
        # A partial bare repository is left by the failed first clone. Nothing else
        # in this project is runnable: reconciliation alone must wake the scheduler.
        assert state['operations'][0]['status'] == 'needs_reconciliation', state
        op = state['operations'][0]
        resumed = rpc('repository.reconcile', {'project_id':github_id,'command_id':'resume-clone','target':op['id'],'expected_revision':op['revision'],'payload':{}})
        assert 'result' in resumed, resumed
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            state = rpc('query.repository', {'project_id':github_id})['result']
            op = state['operations'][0]
            if op['status'] == 'completed' or (op['status'] == 'needs_reconciliation' and op['reason'] != 'reconcile_requested'): break
            time.sleep(.1)
        assert state['operations'][0]['status'] == 'completed', state
        assert (root/'fetches').read_text().splitlines() == ['fetch']
        assert (root/'data'/'projects'/github_id/'repository'/'preserve-fixture').read_text() == 'must survive reconciliation'

        assert state['project']['execution_mode'] == 'live'
        configuration = rpc('query.project_configuration', {'project_id':github_id})['result']['configuration']
        assert configuration['data']['defaults_bundle_ref'] == defaults['bundle']['id']
        assert len(configuration['data']['agent_refs']) == 5
        assert len(state['worktrees']) == 1 and state['worktrees'][0]['base'] == head
        assert rpc('project.open_github', {'command_id':'github-reopen-fixture','target':github_id,'payload':{}})['result']['id'] == github_id
        assert (root/'clones').read_text().splitlines() == ['clone']
        assert not (root / 'calls').exists(), 'opening a GitHub project must not prepare a Harness'
        assert rpc('query.workspace', {'github_only':True})['result']['total_projects'] == 1

        # A backend-only fixture selects Codex for a discussion. It must prepare that
        # Harness before reporting missing credentials, without sending any model input.
        pid = rpc('project.create', {'command_id':'preparation-fixture', 'payload':{'title':'Runtime preparation fixture'}})['result']['id']
        def open_session(n):
            return rpc('session.open', {'project_id':pid,'command_id':'open-'+str(n),'target':pid,'expected_revision':1,
                'payload':{'agent':'builtin-agent-analyst-v1','provider':'codex','model':'offline-model-fixture'}})
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            results = list(pool.map(open_session, range(4)))
        assert all(r.get('error',{}).get('data',{}).get('code') == 'authentication_required' for r in results), results
        installations = rpc('query.runtime_installations')['result']['installations']
        assert len(installations) == 1 and installations[0]['key']['provider'] == 'codex'
        assert rpc('runtime.prepare', {'provider':'codex'})['result']['id'] == installations[0]['id']
        assert rpc('runtime.prepare_defaults')['result']['id'] == installations[0]['id']
        assert rpc('runtime.prepare_defaults')['result']['id'] == installations[0]['id']
        materialized = rpc('query.default_configuration')['result']['materializations']
        assert len(materialized) == 1 and materialized[0]['data']['installation_id'] == installations[0]['id']
        # Losing the pinned image cannot silently materialize this revision again.
        image = installations[0]['artifact']['image_id']
        import hashlib
        pinned_file = root / hashlib.sha256(image.encode()).hexdigest()
        previous_image = pinned_file.read_bytes();pinned_file.unlink()
        assert 'error' in rpc('runtime.prepare_defaults')
        assert rpc('query.default_configuration')['result']['materializations'] == materialized
        pinned_file.write_bytes(previous_image)
        calls = [json.loads(line) for line in (root/'calls').read_text().splitlines()]
        assert sum(c[0] == 'build' for c in calls) == 1
        assert not any('dsh' in ' '.join(c) for c in calls)
        assert rpc('runtime.preparation_status')['result']['preparations'][0]['status'] == 'ready'
        # A different Harness is prepared only when requested; failure creates no installation.
        (root/'fail-build').touch()
        assert 'error' in rpc('runtime.prepare', {'provider':'dsh'})
        assert len(rpc('query.runtime_installations')['result']['installations']) == 1
        (root/'fail-build').unlink()
        dsh = rpc('runtime.prepare', {'provider':'dsh'})
        assert dsh['result']['key']['provider'] == 'dsh', dsh
        assert len(rpc('query.runtime_installations')['result']['installations']) == 2
        # Embedded build contexts are temporary; no source checkout was available to the host.
        assert not list((root/'data'/'runtimes').glob('build-*'))
        (root/'offline').touch()
        assert 'error' in rpc('runtime.prepare', {'provider':'codex'})
        assert len(rpc('query.runtime_installations')['result']['installations']) == 2
        assert 'error' in rpc('runtime.prepare', {'provider':'unknown'})
        print('PASS: interrupted clone reconciliation/resume/worktree/reopen, lazy per-Harness preparation, concurrent reuse, failure/retry, daemon failure, standalone embedded recipes')
    finally:
        child.terminate()
        try: child.wait(timeout=10)
        except subprocess.TimeoutExpired: child.kill(); child.wait()
