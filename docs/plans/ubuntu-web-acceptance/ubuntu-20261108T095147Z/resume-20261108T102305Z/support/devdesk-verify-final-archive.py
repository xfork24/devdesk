import hashlib,json,sqlite3,subprocess,sys,tarfile,tempfile,time
from pathlib import Path
ctx=json.load(open('/tmp/devdesk-ubuntu-acceptance-context.json'));r=Path(ctx['reports']);root=Path(ctx['data']);old=json.load(open(r/'package-before-final-documentation.json'));archive=Path(old['package']);prior=Path('dist/ubuntu-20261108T095147Z/before-final-documentation')/archive.name
runtime=lambda n:n.endswith('/devdesk-host') or '/containers/' in n or '/scripts/' in n
with tarfile.open(prior) as a,tarfile.open(archive) as b:
 def runtime_hashes(tar):return {m.name:hashlib.sha256(tar.extractfile(m).read()).hexdigest() for m in tar.getmembers() if m.isfile() and runtime(m.name)}
 before,after=runtime_hashes(a),runtime_hashes(b);assert before==after,'runtime payload drifted'
 members=b.getmembers();assert all(not m.issym() and not m.islnk() and not Path(m.name).is_absolute() and '..' not in Path(m.name).parts for m in members)
 assert len([m for m in members if m.name.endswith('/devdesk-host')])==1
 forbidden=[m.name for m in members if '/credential-helper/' in m.name or '/data/' in m.name or m.name.endswith(('/auth.json','.p12','.pfx','.key'))];assert not forbidden
 # Read only this dataset's actual credentials into memory to scan the archive.
 sys.path.insert(0,str(Path('tests/live').resolve()));from client import Client
 c=Client(ctx['url']);reference=c.rpc('credentials.status')['reference'];service='devdesk.'+hashlib.sha256(str(root).encode()).hexdigest();assert reference.startswith(service+':')
 key=subprocess.run(['secret-tool','lookup','service',service,'account',reference[len(service)+1:]],capture_output=True,timeout=20);assert key.returncode==0;secrets={key.stdout.rstrip(b'\r\n')};assert all(len(s)>20 for s in secrets)
 for auth in (root/'credentials').rglob('auth.json'):
  for name in ['access_token','refresh_token','id_token']:
   v=json.loads(auth.read_text()).get('tokens',{}).get(name)
   if isinstance(v,str) and v:secrets.add(v.encode())
 count=0
 for m in members:
  if not m.isfile():continue
  data=b.extractfile(m).read();assert not any(secret in data for secret in secrets),'archive contains actual secret';count+=1
 extract=Path(tempfile.mkdtemp(prefix='devdesk-ubuntu-final-docs-'));b.extractall(extract,filter='data')
host=next(extract.glob('*/devdesk-host'));host_hash=hashlib.sha256(host.read_bytes()).hexdigest();assert host_hash==old['host_sha256']
sha=hashlib.sha256(archive.read_bytes()).hexdigest();sidecar=archive.with_suffix(archive.suffix+'.sha256').read_text().split()[0];assert sidecar==sha
report={'passed':True,'package':str(archive.resolve()),'sha256':sha,'host':str(host),'host_sha256':host_hash,'target':'x86_64-unknown-linux-gnu','extract_root':str(extract),'members':[m.name for m in members],'documentation_only_repackage':True,'model_tested_archive':str(prior.resolve()),'model_tested_archive_sha256':old['sha256'],'model_tested_host':old['host'],'identical_runtime_files':before,'secret_scan':{'passed':True,'archive_regular_files_checked':count,'actual_dataset_credentials_memory_only':True},'single_host':True,'forbidden_payloads':forbidden,'observed_at':time.time()}
(r/'package.json').write_text(json.dumps(report,indent=2)+'\n');Path('/tmp/devdesk-ubuntu-current-package.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'passed':True,'sha256':sha,'host_sha256':host_hash,'identical_runtime_files':len(before),'host':str(host),'archive_files_scanned':count}))
