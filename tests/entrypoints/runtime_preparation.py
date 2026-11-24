#!/usr/bin/env python3
"""Real HTTP/SQLite boundary with an explicit fake Docker CLI; no models/network/images."""
import concurrent.futures
import http.cookiejar
import json
import os
from pathlib import Path
import subprocess
import shutil
import signal
import sys
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / 'target/fixtures/debug/devdesk-host'

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
    gh.write_text('#!' + sys.executable + '\n' + r'''
import json,os,sys
from pathlib import Path
args = sys.argv[1:]
root = Path(os.environ['DEVDESK_DOCKER_FIXTURE'])
assert args[:3] == ['api', '--hostname', 'github.com']
if (root/'require-github-token').exists():
    token = os.environ.get('GH_TOKEN')
    if not token:
        print('To get started with GitHub CLI, please run: gh auth login', file=sys.stderr)
        sys.exit(4)
    if token != 'fixture-valid-token':
        print(json.dumps({'status': '401'}))
        sys.exit(1)
if args[3] == 'user':
    assert args[4:] == ['--jq', '.login']
    print('fixture-account')
else:
    assert args[3] == 'graphql'
    print(json.dumps({'data':{'repository':{'id':'fixture-repository','nameWithOwner':'fixture/repository','viewerPermission':'WRITE','ref':{'target':{'oid':os.environ['DEVDESK_GIT_HEAD']}},'branchProtectionRules':{'totalCount':0,'pageInfo':{'hasNextPage':False}},'rulesets':{'nodes':[],'pageInfo':{'hasNextPage':False}}}}}))
''')
    gh.chmod(0o755)
    if sys.platform.startswith('linux'):
        secret_tool = root / 'secret-tool'
        secret_tool.write_text('#!' + sys.executable + '\n' + r'''
import os, sys
from pathlib import Path
args = sys.argv[1:]
service = args[args.index('service') + 1]
account = args[args.index('account') + 1]
entry = Path(os.environ['DEVDESK_DOCKER_FIXTURE']) / (service + '-' + account)
if args[0] == 'lookup':
    if not entry.exists():
        sys.exit(1)
    sys.stdout.write(entry.read_text())
elif args[0] == 'store':
    entry.write_text(sys.stdin.read())
else:
    sys.exit(2)
''')
        secret_tool.chmod(0o755)
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
    if (root/'require-github-token').exists():
        assert os.environ.get('GH_TOKEN') == 'fixture-valid-token'
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
if args[0]=='rm':sys.exit(0)
if args[0]=='run':
    for line in sys.stdin:
        request=json.loads(line)
        if 'id' not in request:continue
        result={'thread':{'id':'probe-thread'}} if request['method']=='thread/start' else {}
        print(json.dumps({'id':request['id'],'result':result}),flush=True)
    sys.exit(0)
if args[0]=='info':
    if (root/'offline').exists():sys.exit(1)
    print('linux');sys.exit(0)
if args[:2]==['image','inspect']:
    ref=args[-1];key=hashlib.sha256(ref.encode()).hexdigest()
    file=root/key
    if not file.exists():sys.exit(1)
    print(file.read_text());sys.exit(0)
if args[:2]==['image','tag']:
    source=root/hashlib.sha256(args[2].encode()).hexdigest()
    if not source.exists():sys.exit(1)
    value=json.loads(source.read_text())
    value['tags']=sorted(set(value.get('tags',[])+[args[3]]))
    for ref in [value['id'],*value['tags']]:
        (root/hashlib.sha256(ref.encode()).hexdigest()).write_text(json.dumps(value))
    sys.exit(0)
if args[:2]==['image','rm']:
    target=root/hashlib.sha256(args[2].encode()).hexdigest()
    if not target.exists():sys.exit(1)
    value=json.loads(target.read_text());target.unlink()
    value['tags'].remove(args[2])
    for ref in [value['id'],*value['tags']]:
        (root/hashlib.sha256(ref.encode()).hexdigest()).write_text(json.dumps(value))
    sys.exit(0)
if args[0]=='build':
    context=Path(args[-1]);label=args[args.index('--label')+1]
    assert (context/'Dockerfile').is_file()
    for line in (context/'Dockerfile').read_text().splitlines():
        if line.startswith('COPY '):
            source = json.JSONDecoder().raw_decode(line[5:])[0]
            assert (context/source).is_file(), source
    time.sleep(.3)
    if (root/'fail-build').exists():sys.exit(2)
    digest='sha256:'+hashlib.sha256(label.encode()).hexdigest()
    value=json.dumps({'id':digest,'os':'linux','arch':'arm64','labels':dict([label.split('=',1)]),'tags':[]})
    (root/hashlib.sha256(digest.encode()).hexdigest()).write_text(value)
    print(digest);sys.exit(0)
sys.exit(3)
''')
    fixture.chmod(0o755)
    env = {**os.environ, 'PATH': str(root) + os.pathsep + os.environ['PATH'], 'DEVDESK_DOCKER_FIXTURE': str(root), 'DEVDESK_REAL_GIT': real_git, 'DEVDESK_GIT_SOURCE': str(source), 'DEVDESK_GIT_HEAD': head}
    env.pop('GH_TOKEN', None)
    env.pop('GITHUB_TOKEN', None)
    def launch(dataset):
        child = subprocess.Popen([str(BINARY), '--data', str(root / dataset), '--port', '0'], cwd=root, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        child.stdout.readline()
        url = child.stdout.readline().strip()
        assert url.startswith('http://127.0.0.1:'), url
        return child, url
    child, url = launch('data-interrupted')
    try:
        def client():
            opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
            opener.open(urllib.request.Request(url + '/session', b'{}', {'Content-Type': 'application/json'}), timeout=10).close()
            return opener
        def rpc(method, params=None):
            request = urllib.request.Request(url + '/rpc', json.dumps({'jsonrpc':'2.0','id':1,'method':method,'params':params or {}}).encode(), {'Content-Type':'application/json'})
            with client().open(request, timeout=20) as response: return json.load(response)
        # Queries and project opening do not prepare any Harness.
        assert rpc('runtime.preparation_status')['result']['preparations'] == []
        assert rpc('query.runtime_installations')['result']['installations'] == []
        assert not (root / 'calls').exists()
        defaults = rpc('query.configuration_defaults')['result']
        assert len(defaults['role_agent_bindings']) == 6
        model = next(row for row in defaults['records'] if row['id'] == defaults['model_ref'])
        configuration = ROOT / 'plugins/core'
        manifest = json.loads((configuration/'manifest.json').read_text())
        packaged_defaults = json.loads((configuration/manifest['defaults']).read_text())
        assert defaults['model_ref'] == packaged_defaults['model_ref']
        packaged_models = [json.loads((configuration/name).read_text()) for name in manifest['types']['model']]
        expected_model = next(source for source in packaged_models if f"{source['identity']}:{source['revision']}" == defaults['model_ref'])
        assert model['data'] == expected_model['data']
        opened = rpc('project.open_github', {'command_id':'github-open-fixture', 'payload':{'repository':'https://github.com/fixture/repository.git'}})
        assert 'result' in opened, opened
        github_id = opened['result']['id']
        assert rpc('query.workspace', {'project_id':github_id})['result']['repository_ready'] is False
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
        # Inspection cannot authorize another clone/fetch. This partial clone
        # has no saved physical completion receipt and must remain unknown.
        assert op['status'] == 'needs_reconciliation', state
        assert op['reason'] != 'reconcile_requested', state
        assert (root/'clones').read_text().splitlines() == ['clone']
        assert not (root/'fetches').exists()
        preserved = root/'data-interrupted'/'projects'/github_id/'repository'/'preserve-fixture'
        assert preserved.read_text() == 'must survive reconciliation'
        child.send_signal(signal.SIGTERM)
        child.communicate(timeout=20)
        assert child.returncode == 0

        # Verify successful onboarding independently, on a fresh dataset. The
        # interrupted dataset is retained; no business status is patched.
        child, url = launch('data')
        opened = rpc('project.open_github', {'command_id':'github-open-ready', 'payload':{'repository':'https://github.com/fixture/repository.git'}})
        assert 'result' in opened, opened
        github_id = opened['result']['id']
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            state = rpc('query.repository', {'project_id':github_id})['result']
            if state['operations'][0]['status'] in ['completed','needs_reconciliation']: break
            time.sleep(.1)
        assert state['operations'][0]['status'] == 'completed', state
        assert rpc('query.workspace', {'project_id':github_id})['result']['repository_ready'] is True
        assert not (root/'fetches').exists()
        assert preserved.read_text() == 'must survive reconciliation'

        assert state['project']['execution_mode'] == 'live'
        configuration = rpc('query.project_configuration', {'project_id':github_id})['result']['configuration']
        assert configuration is None, 'repository connection must leave configuration reviewable'
        assert rpc('query.workspace', {'project_id':github_id})['result']['configuration_ready'] is False
        assert len(state['worktrees']) == 1 and state['worktrees'][0]['base'] == head
        assert rpc('project.open_github', {'command_id':'github-reopen-fixture','target':github_id,'payload':{}})['result']['id'] == github_id
        assert (root/'clones').read_text().splitlines() == ['clone','clone']
        assert not (root / 'calls').exists(), 'opening a GitHub project must not prepare a Harness'
        assert rpc('query.workspace', {'github_only':True})['result']['total_projects'] == 1

        # A backend-only fixture selects Codex for a discussion. It must prepare that
        # Harness before reporting missing credentials, without sending any model input.
        pid = rpc('project.create', {'command_id':'preparation-fixture', 'payload':{'title':'Runtime preparation fixture'}})['result']['id']
        revision = rpc('query.workspace', {'project_id':pid})['result']['active_project']['revision']
        agent = defaults['role_agent_bindings']['builtin-role-analyst:1']
        # Pin a shared instance first. This scene measures concurrent runtime
        # preparation, not simultaneous independent catalog publications.
        selected = rpc('agent.instance.create', {'project_id':pid,'command_id':'preparation-agent',
            'payload':{'name':'Runtime preparation analyst','agent_revision_id':agent,'scope':'project:'+pid}})
        assert 'result' in selected, selected
        instance = selected['result']['id']
        def open_session(n):
            return rpc('session.open', {'project_id':pid,'command_id':'open-'+str(n),'target':pid,'expected_revision':revision,
                'payload':{'agent':agent,'agent_instance_id':instance}})
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            results = list(pool.map(open_session, range(4)))
        assert all(r.get('error',{}).get('data',{}).get('code') == 'authentication_required' for r in results), results
        installations = rpc('query.runtime_installations')['result']['installations']
        assert len(installations) == 1 and installations[0]['key']['provider'] == 'codex'
        assert rpc('runtime.prepare', {'provider':'codex'})['result']['id'] == installations[0]['id']
        assert rpc('runtime.prepare_harness', {'id':defaults['harness_ref']})['result']['id'] == installations[0]['id']
        assert rpc('runtime.prepare_harness', {'id':defaults['harness_ref']})['result']['id'] == installations[0]['id']
        materialized = rpc('query.configuration_defaults')['result']['materializations']
        assert len(materialized) == 1 and materialized[0]['data']['installation_id'] == installations[0]['id']
        # Losing the pinned image cannot silently materialize this revision again.
        image = installations[0]['artifact']['image_id']
        import hashlib
        pinned_file = root / hashlib.sha256(image.encode()).hexdigest()
        info = json.loads(pinned_file.read_text())
        assert info['tags'] == ['devdesk/managed-codex:' + installations[0]['recipe_digest'][:24]], info
        # Upgrade a legacy retention tag without changing the frozen artifact.
        legacy = 'devdesk/prepared-codex:' + image.removeprefix('sha256:')
        subprocess.run([str(fixture), 'image', 'tag', image, legacy], env=env, check=True)
        assert rpc('runtime.prepare', {'provider':'codex'})['result']['artifact']['image_id'] == image
        assert not (root / hashlib.sha256(legacy.encode()).hexdigest()).exists()
        assert json.loads(pinned_file.read_text())['tags'] == info['tags']
        previous_image = pinned_file.read_bytes();pinned_file.unlink()
        assert 'error' in rpc('runtime.prepare_harness', {'id':defaults['harness_ref']})
        assert rpc('query.configuration_defaults')['result']['materializations'] == materialized
        pinned_file.write_bytes(previous_image)
        calls = [json.loads(line) for line in (root/'calls').read_text().splitlines()]
        assert sum(c[0] == 'build' for c in calls) == 1
        assert sum(c[:2] == ['image','tag'] and c[-1].startswith('devdesk/prepared-') for c in calls) == 1, 'only the fixture creates a legacy tag'
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
        if sys.platform.startswith('linux'):
            child.terminate()
            child.communicate(timeout=20)
            assert child.returncode == 0
            (root/'require-github-token').touch()
            child, url = launch('data-auth')
            status = rpc('github.auth_status')['result']
            assert status['status'] == 'unconfigured', status
            opened = rpc('project.open_github', {'command_id':'github-open-auth', 'payload':{'repository':'fixture/repository'}})
            github_id = opened['result']['id']
            deadline = time.monotonic() + 12
            while time.monotonic() < deadline:
                op = rpc('query.repository', {'project_id':github_id})['result']['operations'][0]
                if op['status'] == 'needs_reconciliation': break
                time.sleep(.1)
            assert op['reason'].startswith('github_auth_required'), op
            original_id = op['id']
            assert (root/'clones').read_text().splitlines() == ['clone','clone']
            rejected = rpc('github.configure_auth', {'expected_revision':status['revision'],'token':'fixture-invalid-token'})
            assert rejected['error']['data']['code'] == 'github_auth_invalid', rejected
            assert rpc('github.auth_status')['result'] == status
            saved = rpc('github.configure_auth', {'expected_revision':status['revision'],'token':'fixture-valid-token'})
            assert 'fixture-valid-token' not in json.dumps(saved)
            assert saved['result']['source'] == 'devdesk' and saved['result']['login'] == 'fixture-account', saved
            stale = rpc('github.configure_auth', {'expected_revision':status['revision'],'token':'fixture-valid-token'})
            assert stale['error']['data']['code'] == 'revision_conflict', stale
            assert rpc('github.auth_status')['result'] == saved['result']
            reply = rpc('repository.reconcile', {'project_id':github_id,'command_id':'resume-after-auth','target':op['id'],'expected_revision':op['revision'],'payload':{}})
            assert 'result' in reply, reply
            deadline = time.monotonic() + 12
            while time.monotonic() < deadline:
                state = rpc('query.repository', {'project_id':github_id})['result']
                op = state['operations'][0]
                if op['status'] == 'completed' or op['reason'] != 'reconcile_requested': break
                time.sleep(.1)
            assert len(state['operations']) == 1 and op['id'] == original_id and op['status'] == 'completed', state
            assert rpc('query.workspace', {'project_id':github_id})['result']['repository_ready'] is True
            assert (root/'clones').read_text().splitlines() == ['clone','clone','clone']
            print('PASS: GitHub Token validation, replacement revision, credential reuse, same-operation recovery')
        print('PASS: interrupted clone remains unknown without refetch; fresh clone/worktree/reopen, lazy per-Harness preparation, concurrent reuse, failure/retry, daemon failure, standalone embedded recipes')
    finally:
        child.terminate()
        try: child.wait(timeout=10)
        except subprocess.TimeoutExpired: child.kill(); child.wait()
        if sys.exc_info()[0] is not None:
            (root/'host-failure.log').write_text(child.stderr.read())
            retained = Path(tempfile.mkdtemp(prefix='devdesk-preparation-failure-'))
            shutil.copytree(root, retained, dirs_exist_ok=True)
            print('Retained failure:', retained, file=sys.stderr)
