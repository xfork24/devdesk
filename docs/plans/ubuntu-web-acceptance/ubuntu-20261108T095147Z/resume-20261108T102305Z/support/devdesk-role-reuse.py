import json,sys,time
from pathlib import Path
ctx=json.loads(Path('/tmp/devdesk-ubuntu-acceptance-context.json').read_text());r=Path(ctx['reports']);sys.path.insert(0,'tests/live');from client import Client
c=Client(ctx['url']);sources={provider:json.loads((r/('R3-'+provider+'-application.json')).read_text()) for provider in ['codex','dsh']};report={'passed':False,'scope':'actual native reviewer discussions in a different project, independent sessions, shared installations','sessions':[]}
try:
 for provider,other in [('codex','dsh'),('dsh','codex')]:
  owner=sources[other]['project_id'];c.open(owner);revision=c.rpc('query.workspace')['active_project']['revision'];sid,tid,state=c.discuss(owner,revision,'reviewer',provider,'Do not use tools or modify files. Return only ROLE_ISOLATION_OK. This is an independent reviewer discussion, not an engineering review decision.')
  native=state['session']['external_session'];assert native and native!=sources[provider]['native_session'];assert all(x['writer_revoked'] and x['status']=='released' and x['protocol_artifact'] for x in state['executions']);binding=state['session']['runtime_binding'];report['sessions'].append({'provider':provider,'project':owner,'session':sid,'turn':tid,'native_session':native,'installation':binding['installation']['id'],'execution_ids':[x['id'] for x in state['executions']],'released':True})
 report['passed']=True
finally:(r/'role-reuse.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
