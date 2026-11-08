#!/usr/bin/env python3
"""Real concurrent registration and Docker layer reuse, independent of model output."""
from client import Client
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import argparse,json,sqlite3,subprocess
p=argparse.ArgumentParser();p.add_argument('--url',default='http://127.0.0.1:51751');p.add_argument('--root',type=Path,required=True);p.add_argument('--reports',type=Path,default=Path('docs/plans/real-engineering-integration'));a=p.parse_args();c=Client(a.url);r={'passed':False,'installations':[]}
try:
 rows=c.rpc('query.runtime_installations')['installations'];images=[x for x in rows if x['artifact']['kind']=='docker_image'];assert {x['key']['provider']for x in images}=={'codex','dsh'}
 layers=[]
 for installation in images:
  registration={k:v for k,v in installation.items() if k!='id'}
  def register(_):return Client(a.url).rpc('runtime.register',registration)['id']
  with ThreadPoolExecutor(max_workers=4) as pool:ids=list(pool.map(register,range(4)))
  assert ids==[installation['id']]*4
  identity=installation['artifact']['image_id'];actual=json.loads(subprocess.check_output(['docker','image','inspect',identity],text=True))[0];assert actual['Id']==identity;layers.append(set(actual['RootFS']['Layers']))
  references=[]
  for db in a.root.resolve().glob('projects/*/engineering.sqlite3'):
   conn=sqlite3.connect('file:'+str(db)+'?mode=ro',uri=True)
   for sid,agent,binding in conn.execute('select id,agent,runtime_binding from sessions where runtime_binding is not null'):
    if json.loads(binding)['installation']['id']==installation['id']:references.append({'project':db.parent.name,'session':sid,'agent':agent})
   conn.close()
  assert len({x['project']for x in references})>1 and len({x['agent']for x in references})>1
  r['installations'].append({'id':installation['id'],'provider':installation['key']['provider'],'platform':installation['key']['os']+'/'+installation['key']['arch'],'image':identity,'concurrent_registrations':len(ids),'references':references})
 assert len(c.rpc('query.runtime_installations')['installations'])==len(rows);shared=set.intersection(*layers);assert shared;r['shared_layers']=sorted(shared);r['passed']=True
finally:
 a.reports.mkdir(parents=True,exist_ok=True);(a.reports/'R8-installations.json').write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps({'passed':r['passed'],'installations':len(r['installations']),'report':str(a.reports/'R8-installations.json')}))
