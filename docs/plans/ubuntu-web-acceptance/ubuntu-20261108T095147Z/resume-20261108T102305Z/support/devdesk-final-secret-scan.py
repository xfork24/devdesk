import hashlib,json,sqlite3,subprocess,sys,time
from pathlib import Path
ctx=json.load(open('/tmp/devdesk-ubuntu-acceptance-context.json'));root=Path(ctx['data']);reports=Path(ctx['reports']);sys.path.insert(0,str(Path('tests/live').resolve()));from client import Client
c=Client(ctx['url']);ref=c.rpc('credentials.status')['reference'];service='devdesk.'+hashlib.sha256(str(root).encode()).hexdigest();assert ref.startswith(service+':')
key=subprocess.run(['secret-tool','lookup','service',service,'account',ref[len(service)+1:]],capture_output=True,timeout=20);assert key.returncode==0
secrets={key.stdout.rstrip(b'\r\n')};assert all(len(v)>=20 for v in secrets)
auth_files=list((root/'credentials').rglob('auth.json'));assert auth_files
for f in auth_files:
 v=json.loads(f.read_text());tokens=v.get('tokens',{})
 for name in ['access_token','refresh_token','id_token']:
  token=tokens.get(name)
  if isinstance(token,str) and token:secrets.add(token.encode())
files={f.resolve() for f in reports.parent.rglob('*') if f.is_file()};files|={f.resolve() for f in root.rglob('*.sqlite3*') if f.is_file()}
for db in root.glob('projects/*/engineering.sqlite3'):
 with sqlite3.connect('file:'+str(db)+'?mode=ro',uri=True) as conn:
  for (name,) in conn.execute('select relative_path from artifacts'):files.add((db.parent/name).resolve())
leaks=[]
for f in files:
 content=f.read_bytes()
 if any(secret in content for secret in secrets):leaks.append(str(f))
# Inspect only acceptance-owned Host and descendants. Never expose argv or secrets.
host=json.load(open('/tmp/devdesk-current-host.json'));procs={}
for p in Path('/proc').iterdir():
 if not p.name.isdigit():continue
 try:
  status=(p/'status').read_text();ppid=int(next(x for x in status.splitlines() if x.startswith('PPid:')).split()[1]);procs[int(p.name)]=(ppid,(p/'cmdline').read_bytes())
 except (OSError,StopIteration,ValueError):pass
owned={host['pid']}
while True:
 added={pid for pid,(ppid,_) in procs.items() if ppid in owned}-owned
 if not added:break
 owned|=added
for pid in owned:
 if pid in procs and any(secret in procs[pid][1] for secret in secrets):leaks.append('owned-process:'+str(pid))
containers=subprocess.check_output(['docker','ps','-aq','--filter','label=devdesk.root='+str(root)],text=True).split();assert not containers,'owned workers remain'
result={'passed':not leaks,'observed_at':time.time(),'secret_sources':'actual dataset-scoped DeepSeek entry and dedicated native OAuth token fields, memory only','scanned_files':len(files),'owned_processes_checked':len(owned),'matches':leaks,'owned_containers_remaining':containers,'retained_host':host['pid'],'scope':'reports including original failures, all dataset SQLite/WAL and committed Artifacts, owned Host process tree; native private auth files are legitimate storage and excluded from output scan'}
(reports/'final-secret-and-resources.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items() if k not in ['matches']}));assert result['passed']
