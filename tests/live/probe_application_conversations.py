#!/usr/bin/env python3
"""Real host -> domain -> provider container -> persisted discussion evidence.
Requires an already running acceptance host with dedicated OAuth/Keychain credentials.
"""
import argparse,http.cookiejar,json,time,urllib.request,uuid
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--url',default='http://127.0.0.1:51751');p.add_argument('--provider',choices=['codex','dsh'],required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
c=urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()));c.open(a.url+'/').close()
def rpc(method,params=None):
 request=urllib.request.Request(a.url+'/rpc',data=json.dumps({'jsonrpc':'2.0','id':1,'method':method,'params':params or {}}).encode(),headers={'Content-Type':'application/json'})
 result=json.load(c.open(request,timeout=40))
 if 'error' in result:raise RuntimeError(result['error']['data']['code'])
 return result['result']
def cmd(method,target=None,revision=None,payload=None,project=None):return rpc(method,{'project_id':project,'command_id':str(uuid.uuid4()),'target':target,'expected_revision':revision,'payload':payload or {}})
report={'provider':a.provider,'passed':False,'turns':[],'source':'real DevDesk HTTP + managed Linux provider','stage':'configuration'}
try:
 rpc('runtime.discover_docker',{'provider':a.provider})
 project=cmd('project.create',payload={'title':f'Real {a.provider} conversation acceptance'})['id'];report['project_id']=project
 revision=rpc('query.workspace',{'project_id':project})['active_project']['revision']
 opened=cmd('session.open',project,revision,payload={'agent':'builtin-agent-analyst-v1','provider':a.provider},project=project);sid=opened['id'];report['session_id']=sid
 marker=uuid.uuid4().hex
 prompts=[f'Remember marker {marker}. Do not use tools. Return only JSON {{"marker":"{marker}","role":"analyst"}}.','Without tools return only JSON with marker from the preceding message and role analyst.']
 for index,prompt in enumerate(prompts):
  report['stage']=f'turn_{index+1}'
  state=rpc('query.session',{'project_id':project,'id':sid});session=state['session']
  payload={'client_message_id':str(uuid.uuid4()),'input':prompt}
  receipt=cmd('conversation.send',sid,session['revision'],payload,project);tid=receipt['id'];deadline=time.monotonic()+240
  while time.monotonic()<deadline:
   state=rpc('query.session',{'project_id':project,'id':sid,'limit':100});turn=next(t for t in state['turns'] if t['id']==tid)
   if turn['status'] in ['completed','failed','cancelled','needs_reconciliation']:break
   time.sleep(.5)
  assert turn['status']=='completed',turn['status']
  texts=[e['content'] for e in state['events'] if e['turn_id']==tid and e['kind']=='assistant' and e['mode']=='full'];text=texts[-1].strip().removeprefix('```json').removesuffix('```').strip();answer=json.loads(text)
  assert answer['marker']==marker and answer['role']=='analyst'
  executions=state['executions'];assert all(e['writer_revoked'] and e['status']=='released' and e['protocol_artifact'] for e in executions)
  report['turns'].append({'id':tid,'native_message':turn['external_message'],'native_turn':turn['external_turn'],'consumed':turn['consumed'],'context_retained':True,'writer_revoked':True,'protocol_artifact':executions[0]['protocol_artifact']})
  assert cmd('conversation.send',sid,session['revision'],payload,project)['id']==tid
 report['native_session']=state['session']['external_session'];report['worker_lifetimes']=len(state['executions']);assert len(state['executions'])==2
 report['passed']=True;report['stage']='finished'
except Exception as e:
 report['failure_type']=type(e).__name__;report['failure_code']=str(e) if isinstance(e,(RuntimeError,AssertionError)) else 'see local diagnostic'
a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2));raise SystemExit(0 if report['passed'] else 1)
