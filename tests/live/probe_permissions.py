#!/usr/bin/env python3
"""Two-provider Permission acceptance with a controlled HTTP canary; no secrets collected."""
from client import Client
from pathlib import Path
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import argparse,hashlib,json,threading,time,uuid
p=argparse.ArgumentParser();p.add_argument('--url',default='http://127.0.0.1:51761');p.add_argument('--reports',type=Path,required=True);a=p.parse_args();a.reports.mkdir(parents=True,exist_ok=True)
c=Client(a.url);context=c.rpc('query.workspace');original=context.get('active_project_id');r={'passed':False,'platform':'macOS','original_project':original,'providers':[],'started':time.time()};output=a.reports/'permission-native-tools.json';hits=[]
if output.exists():output.rename(a.reports/('permission-native-tools-before-'+uuid.uuid4().hex[:8]+'.json'))
class Probe(BaseHTTPRequestHandler):
 def do_GET(self):
  hits.append(self.path);body=b'PERMISSION_NETWORK_OK';self.send_response(200);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
 def log_message(self,*args):pass
server=ThreadingHTTPServer(('0.0.0.0',0),Probe);threading.Thread(target=server.serve_forever,daemon=True).start()
def tools_turn(project,provider,prompt,overrides):
 before=c.rpc('query.workspace')['active_project']
 sid=c.command('session.open',project,before['revision'],{'agent':'builtin-agent-analyst-v1','provider':provider,'permission_overrides':overrides})['id']
 state=c.rpc('query.session',{'id':sid});tid=c.command('conversation.send',sid,state['session']['revision'],{'client_message_id':str(uuid.uuid4()),'input':prompt})['id']
 state=c.wait(lambda:c.rpc('query.session',{'id':sid,'limit':100}),lambda s:next(t for t in s['turns'] if t['id']==tid)['status'] in ['completed','failed','cancelled','needs_reconciliation'],360)
 assert next(t for t in state['turns'] if t['id']==tid)['status']=='completed'
 return sid,tid,state
try:
 settings=c.rpc('query.permissions')['configuration'];r['previous_configuration']=settings
 for provider in ['codex','dsh']:settings['harnesses'][provider]={'permission':{'revision_id':'permission-container-full-access-v1'},'overrides':{}}
 settings['sandbox']={'permission':{'revision_id':'permission-container-full-access-v1'},'overrides':{}};settings['approval']='never'
 r['configuration']=c.command('permission.bind','permission-configuration',settings['revision'],settings)
 project=c.command('project.create',payload={'title':'Permission native acceptance '+uuid.uuid4().hex[:8]})['id'];c.project=project;r['project_id']=project
 for provider in ['codex','dsh']:
  before=c.rpc('query.workspace')['active_project'];nonce=uuid.uuid4().hex;endpoint=f'http://host.docker.internal:{server.server_port}/{provider}/{nonce}'
  js=f"const fs=require('fs');fs.writeFileSync('/tmp/devdesk-permission-{nonce}','local-write-ok');fetch('{endpoint}').then(r=>r.text()).then(t=>console.log(t)).catch(e=>{{console.error(e.message);process.exit(1)}})"
  prompt='Use your shell execution tool once to execute this exact command: node -e '+__import__('shlex').quote(js)+'. Then reply PERMISSION_COMPLETE. Use only this requested tool, do not delegate, and do not inspect credentials or runtime state.'
  sid,tid,state=c.discuss(project,before['revision'],'analyst',provider,prompt,timeout=360)
  binding=state['session']['runtime_binding'];plan=binding['permissions'];assert plan['harness']['policy']['filesystem']['mode']=='full_access';assert not plan['isolation']['nested_sandbox'];assert plan['approval']=='never'
  r['current_session_id']=sid;r['current_turn_id']=tid
  assert any(nonce in hit for hit in hits),'model completed without the actual HTTP canary request'
  executions=state['executions'];assert executions and all(e['image_digest'].startswith('sha256:') for e in executions)
  row={'provider':provider,'session_id':sid,'turn_id':tid,'configuration_digest':binding['configuration_digest'],'permissions':plan,'canary_received':True,'executions':[{k:e.get(k) for k in ['id','container_id','image_digest','status']} for e in executions]};r['providers'].append(row);output.write_text(json.dumps(r,indent=2)+'\n');print(provider+' native local write + tool network passed',flush=True)
 r['restricted_providers']=[]
 for provider in ['codex','dsh']:
  nonce=uuid.uuid4().hex;endpoint=f'http://host.docker.internal:{server.server_port}/restricted/{provider}/{nonce}'
  js=f"const fs=require('fs');let denied=false;try{{fs.writeFileSync('/work/permission-denied-{nonce}','bad')}}catch{{denied=true}}fetch('{endpoint}',{{signal:AbortSignal.timeout(2000)}}).then(()=>process.exit(3)).catch(()=>{{if(!denied)process.exit(4);console.log('PERMISSION_DENIED_OK')}})"
  prompt='Use your shell execution tool once to execute exactly: node -e '+__import__('shlex').quote(js)+'. This is a controlled permission denial probe. Do not inspect credentials or runtime state. After the tool completes, reply PERMISSION_COMPLETE.'
  policy={'filesystem':{'mode':'read_only'},'network':{'mode':'disabled'}}
  if provider=='dsh':policy['tools']={'mode':'restricted','allow':['bash','ask_user_question'],'deny':[]}
  sid,tid,state=tools_turn(project,provider,prompt,{'harness':{'overrides':policy}})
  assert not any(nonce in hit for hit in hits),'restricted tool reached the HTTP canary'
  results=[e for e in state['events'] if e['turn_id']==tid and e['kind']=='tool' and (str(e.get('provider_item','')).startswith('tool/result:') or e['deduplication_key'].endswith(':complete'))]
  proven=False
  for e in results:
   result=json.loads(e['content'])
   if provider=='codex':proven|=result.get('exitCode')==0 and 'PERMISSION_DENIED_OK' in result.get('aggregatedOutput','')
   else:proven|=not result.get('message',{}).get('isError',True) and 'PERMISSION_DENIED_OK' in json.dumps(result.get('message',{}).get('content',[]))
  assert proven,'no actual successful denial-tool output'
  plan=state['session']['runtime_binding']['permissions'];assert plan['harness']['policy']['network']['mode']=='disabled' and plan['sandbox']['policy']['network']['mode']=='enabled'
  assert all(e['writer_revoked'] and e['permissions']==plan for e in state['executions'])
  r['restricted_providers'].append({'provider':provider,'session_id':sid,'turn_id':tid,'write_denied':True,'tool_network_denied':True,'model_transport_succeeded':True,'permissions':plan});output.write_text(json.dumps(r,indent=2)+'\n');print(provider+' read-only + tool-network-disabled model turn passed',flush=True)
 r['passed']=True
except Exception as e:
 r['error']=str(e);raise
finally:
 r['finished']=time.time();output.write_text(json.dumps(r,indent=2)+'\n');server.shutdown()
 if original:
  try:c.project=None;c.command('project.open',original)
  except Exception:pass
