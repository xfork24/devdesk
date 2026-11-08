import json,sys,time
from pathlib import Path
ctx=json.load(open('/tmp/devdesk-ubuntu-acceptance-context.json'));r=Path(ctx['reports']);sys.path.insert(0,str(Path('tests/live').resolve()));from client import Client
c=Client(ctx['url']);assert c.rpc('query.workspace')['data_root']==ctx['data']
auth=c.rpc('codex.auth.status',{'installation_id':'45ef8767-7ed8-46fb-90e7-aa1d83eceb4a'});assert auth['authenticated'] and not auth['pending']
dsh=c.rpc('credentials.check');assert dsh['authorized']
conversations=[]
for provider in ['codex','dsh']:
 p=json.load(open(r/f'final-package-native-{provider}.json'));assert p['passed'];c.open(p['project_id']);s=c.rpc('query.session',{'id':p['session_id'],'limit':100});assert s['session']['external_session']==p['native_session']
 for turn in p['turns']:assert next(t for t in s['turns'] if t['id']==turn['id'])['status']=='completed'
 conversations.append({'provider':provider,'project':p['project_id'],'session':p['session_id'],'native_session':p['native_session'],'turns_preserved':len(p['turns'])})
full=json.load(open('/tmp/devdesk-ubuntu-full-context.json'));source=json.load(open(Path(full['reports'])/'R7-merges.json'));c.open(source['project_id']);v=c.feature(source['feature_id']);assert v['feature']['status']=='completed';assert set(source['final_snapshots'])<=set(s['id'] for s in v['snapshots'])
report={'passed':True,'observed_at':time.time(),'host':json.load(open('/tmp/devdesk-current-host.json')),'codex':{k:auth[k] for k in ['authenticated','pending','type']},'dsh':dsh,'package_conversations':conversations,'completed_project':source['project_id'],'completed_feature':source['feature_id'],'preserved_snapshots':source['final_snapshots']}
(r/'final-package-restart-verification.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'passed':True,'conversations':len(conversations),'snapshots':len(source['final_snapshots'])}))
