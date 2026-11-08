import json,sys,sqlite3,subprocess,time,urllib.request
from pathlib import Path
ctx=json.loads(Path('/tmp/devdesk-ubuntu-acceptance-context.json').read_text());r=Path(ctx['reports']);label=sys.argv[1];binary=Path(sys.argv[2]).resolve() if len(sys.argv)>2 else Path('target/debug/devdesk-host').resolve();assert binary.name=='devdesk-host' and binary.is_file();sys.path.insert(0,str(Path('tests/live').resolve()));from client import Client
c=Client(ctx['url']);assert Path(c.rpc('query.workspace')['data_root']).resolve()==Path(ctx['data']);port=ctx['url'].rsplit(':',1)[1];matches=[]
for entry in Path('/proc').iterdir():
 if not entry.name.isdigit():continue
 try:
  args=[x.decode() for x in (entry/'cmdline').read_bytes().split(b'\0') if x]
  if not args or Path(args[0]).name!='devdesk-host' or '--data' not in args or '--port' not in args:continue
  if Path(args[args.index('--data')+1]).resolve()==Path(ctx['data']) and args[args.index('--port')+1]==port:matches.append(int(entry.name))
 except (OSError,UnicodeError,IndexError):pass
assert len(matches)==1,'no unique owned host';oldpid=matches[0]
for db in Path(ctx['data']).glob('projects/*/engineering.sqlite3'):
 with sqlite3.connect(db) as conn:assert conn.execute("select count(*) from executions where status != 'released'").fetchone()[0]==0
c.http.open(urllib.request.Request(ctx['url']+'/shutdown',data=b'{}',headers={'Content-Type':'application/json'}),timeout=10).close()
for _ in range(300):
 path=Path('/proc',str(oldpid))
 if not path.exists() or '\nState:\tZ' in (path/'status').read_text():break
 time.sleep(.05)
else:raise RuntimeError('owned host still stopping')
argv=[str(binary),'--data',ctx['data'],'--port',port];cwd=str(binary.parent) if len(sys.argv)>2 else str(Path.cwd())
with (r/(label+'.log')).open('w') as log:p=subprocess.Popen(argv,cwd=cwd,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
c=Client(ctx['url']);assert c.rpc('query.workspace')['data_root']==ctx['data'];receipt={'pid':p.pid,'argv':argv,'owned':True,'data':ctx['data'],'url':ctx['url'],'cwd':cwd,'previous_owned_pid':oldpid};(r/(label+'-ownership.json')).write_text(json.dumps(receipt,indent=2)+'\n');Path('/tmp/devdesk-current-host.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt))
