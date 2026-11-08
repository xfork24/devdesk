import hashlib,json,re,subprocess,time
from pathlib import Path
ctx=json.load(open('/tmp/devdesk-ubuntu-acceptance-context.json'));r=Path(ctx['reports']);full=Path(json.load(open('/tmp/devdesk-ubuntu-full-context.json'))['reports']);checks={}
for mode in ['full','verify-existing']:
 value=json.load(open(full/f'live-check-{mode}-git-docker-harness-github-web-workflow-recovery.json'));assert value['passed'] and all(x['passed'] for x in value['steps'])
 for step in value['steps']:
  evidence=step.get('evidence',{})
  if isinstance(evidence,dict) and 'log' in evidence:assert hashlib.sha256(Path(evidence['log']).read_bytes()).hexdigest()==evidence['sha256']
 checks[mode]={'passed':True,'steps':len(value['steps'])}
for filename in ['R6-implementation.json','R7-merges.json','R8-recovery.json','R8-installations.json']:assert json.load(open(full/filename))['passed']
for filename in ['final-package-native-codex.json','final-package-native-dsh.json','final-package-credentials.json','final-package-restart-verification.json','final-package-browser.json','package.json']:assert json.load(open(r/filename))['passed']
plan=Path('docs/plans/ubuntu-web-acceptance/PLAN.md').read_text();assert '- [ ]' not in plan;checks['plan_completed_items']=plan.count('- [x]')
original=json.load(open(r.parent/'evidence-sha256.json'))
for name,sha in original.items():assert hashlib.sha256((r.parent/name).read_bytes()).hexdigest()==sha
checks['original_files_unchanged']=len(original)
source=json.load(open(r/'source-files.json'))
for file in source['files']:assert hashlib.sha256(Path(file['path']).read_bytes()).hexdigest()==file['sha256']
checks['source_files_verified']=len(source['files'])
markdown=[Path(file['path']) for file in source['files'] if file['path'].endswith('.md')]+[r/'acceptance.md'];links=0
for p in markdown:
 for name in re.findall(r'(?<!!)\[[^\]]*\]\(([^)]+)\)',p.read_text()):
  if '://' in name or name.startswith(('#','mailto:')):continue
  name=name.split('#',1)[0]
  assert not name or (p.parent/name).exists(),(str(p),name);links+=1
checks['local_markdown_links']=links
count=0
for file in r.rglob('*.json'):json.load(open(file));count+=1
checks['valid_json_reports']=count
package=json.load(open(r/'package.json'));assert hashlib.sha256(Path(package['package']).read_bytes()).hexdigest()==package['sha256'];assert hashlib.sha256(Path(package['host']).read_bytes()).hexdigest()==package['host_sha256'];assert Path('scripts/install-linux-sandbox-policy.sh').stat().st_mode&0o111
checks['archive_sha256']=package['sha256'];checks['single_host_and_verified_runtime_files']=len(package['identical_runtime_files'])
checks['format_and_syntax']={'passed':True,'commands_observed_exit_0':['cargo fmt --all -- --check','python3 -m py_compile scripts/doctor.py tests/live/client.py tests/live/check_live.py tests/live/probe_implementation.py tests/live/probe_nested_sandbox.py','node --check tests/live/probe_linux_credentials.mjs','node --check tests/live/probe_oauth_recovery.mjs','git diff --check'],'execution':'direct tool results before documentation-only archive repack; code files unchanged since'}
report={'passed':True,'observed_at':time.time(),'checks':checks};(r/'final-evidence-check.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
