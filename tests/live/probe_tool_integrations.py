"""Creates real tool integrations and runs two Codex turns, including restart. Requires Docker, Node and a signed Host."""
import sys,json,time,subprocess,tempfile,pathlib,shutil,uuid,threading,http.server,urllib.request,urllib.parse
from client import Client
import argparse,socket
parser=argparse.ArgumentParser(description="Real Codex-only HTTP/MCP/Skill acceptance in a new isolated dataset; never calls DSH.")
parser.add_argument('--host',type=pathlib.Path,default=pathlib.Path(__file__).resolve().parents[2]/'target/debug/devdesk-host')
parser.add_argument('--codex-auth-source',type=pathlib.Path,required=True,help='Existing dedicated Codex OAuth auth.json; copied privately, never printed or changed')
parser.add_argument('--report',type=pathlib.Path,default=pathlib.Path('/tmp/devdesk-tool-integrations-native.json'))
args=parser.parse_args()
REPO=pathlib.Path(__file__).resolve().parents[2];ROOT=pathlib.Path(tempfile.mkdtemp(prefix='devdesk-tool-integrations-native-',dir='/tmp')).resolve()
with socket.socket() as listener:listener.bind(('127.0.0.1',0));PORT=listener.getsockname()[1]
# Copy the existing dedicated Codex OAuth file into this isolated dataset only.
auth=args.codex_auth_source.resolve();assert auth.is_file(),'Missing explicit Codex OAuth source'
marker='HTTP-CANARY-'+uuid.uuid4().hex;skill_marker='SKILL-CANARY-'+uuid.uuid4().hex
counts={'http':0}
class Fixture(http.server.BaseHTTPRequestHandler):
 def do_GET(self):
  counts['http']+=1;body=json.dumps({'marker':marker}).encode();self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
 def log_message(self,*a):pass
service=http.server.ThreadingHTTPServer(('0.0.0.0',0),Fixture);threading.Thread(target=service.serve_forever,daemon=True).start();origin=f'http://host.docker.internal:{service.server_port}'
mcp=subprocess.Popen(['node',str(REPO/'tests/fixtures/tool-integrations/mcp-service.mjs'),'--http'],stdout=subprocess.PIPE,text=True);mcp_port=json.loads(mcp.stdout.readline())['port']
log=open(ROOT.parent/(ROOT.name+'.host.log'),'w');host=subprocess.Popen([str(args.host.resolve()),'--data',str(ROOT),'--port',str(PORT)],stdout=log,stderr=subprocess.STDOUT)
report={'dataset':str(ROOT),'real_models':['codex/gpt-6.1-sol'],'dsh_calls':0};args.report.parent.mkdir(parents=True,exist_ok=True)
try:
 c=Client(f'http://127.0.0.1:{PORT}');shutil.copyfile(auth,ROOT/'credentials/codex/auth.json');(ROOT/'credentials/codex/auth.json').chmod(0o600);assert c.rpc('codex.auth.status')['authenticated']
 pid=c.command('project.create',payload={'title':'External tools and Skill acceptance'})['id'];c.project=pid;scope={'kind':'project','project_id':pid};defaults=c.rpc('query.configuration_defaults')
 def create(method,name,data):return c.command(method,payload={'name':name,'scope':scope,'data':data})
 resources=c.rpc('query.permission_resources');changes=[]
 for resource in resources['targets']:
  if resource['target']['revision_id'] in [defaults['harness_ref'],'builtin-permission-sandbox-docker-v1']:
   changes.append({'target':resource['target'],'permission_revision_id':'permission-harness-full-access-v2-r1' if resource['target']['kind']=='harness' else 'permission-sandbox-full-access-v2-r1','expected_revision':resource.get('active',{}).get('revision',0) if resource.get('active') else 0})
 c.command('permission.bindings.apply',payload={'bindings':changes})
 api=create('integration.create','Local canary API',{'source':{'kind':'http_api','base_url':origin}})
 t=create('integration.tool.create','lookup_http_canary',{'schema_version':2,'qualified_name':'lookup_http_canary','description':'Read the canary marker from the explicitly bound local acceptance service.','integration_ref':api['id'],'implementation':{'kind':'http_api','method':'GET','path':'/canary'},'input_schema':{'type':'object','properties':{},'additionalProperties':False},'output_schema':{'type':'object','properties':{'marker':{'type':'string'}},'required':['marker']},'effect':'read_only'})
 def bind(source,tools):return create('integration.binding.create','Acceptance account '+source['name'],{'integration_ref':source['id'],'harness_ref':defaults['harness_ref'],'sandbox_ref':'builtin-permission-sandbox-docker-v1','allowed_origins':[source['data']['source'].get('base_url') or urllib.parse.urlsplit(source['data']['source']['url']).scheme+'://'+urllib.parse.urlsplit(source['data']['source']['url']).netloc],'allow_private_network':True,'allowed_tools':tools,'timeout_ms':15000,'max_result_bytes':1048576})
 b=bind(api,[t['id']]);remote=create('integration.create','Local real MCP',{'source':{'kind':'mcp','transport':'streamable_http','url':f'http://host.docker.internal:{mcp_port}/mcp','command':[],'protocol_version':'2026-07-28'}});mb=bind(remote,[])
 discovery=c.command('integration.discover',payload={'binding_ref':mb['id']});published=c.command('integration.publish_tools',payload={'discovery_ref':discovery['id'],'names':['sum'],'aliases':{'sum':'lookup_mcp_sum'},'effects':{'sum':'read_only'},'scope':scope})['records'][0]
 updated=dict(mb['data']);updated['allowed_tools']=[published['id']];mb=c.command('integration.binding.revise',mb['id'],mb['revision'],{'name':mb['name'],'scope':scope,'data':updated})
 c.command('integration.project.bind',payload={'project_id':pid,'bindings':{api['id']:b['id'],remote['id']:mb['id']}})
 # Actual static package import uses the same validated core adoption contract as ZIP/Git.
 body='---\nname: integration-canary\ndescription: Verify selected integrations with frozen evidence.\n---\n\n# Integration verification\n\nRead [the reference](references/canary.md), then invoke both selected external tools. Do not install packages.'
 skill=c.command('skill.import',payload={'name':'Integration canary Skill','scope':scope,'package':{'body':body,'description':'Read the selected reference and call the selected tools','files':[{'path':'references/canary.md','content':skill_marker+'\n'}],'requires_tools':[t['id'],published['id']],'source':'manual acceptance package','scripts_disabled':False}})
 native='builtin-tool-codex-native-v1';cap=create('capability.create','Codex external tools verification',{'model_ref':defaults['model_ref'],'harness_ref':defaults['harness_ref'],'tools':[native,t['id'],published['id']],'skills':[skill['id']]})
 roles=c.rpc('query.configurations',{'kind':'role','include_history':True})['records'];role=max((r for r in roles if r['definition_id']=='builtin-role-analyst'),key=lambda r:r['version'])
 agent=create('agent.create','Codex integration verifier',{'role_ref':role['id'],'capability_ref':cap['id'],'permission_selection':{'sandbox_ref':'builtin-permission-sandbox-docker-v1','harness':{'mode':'follow_target'},'sandbox':{'mode':'follow_target'}}})
 bindings=dict(defaults['role_agent_bindings']);bindings[role['definition_id']]=agent['id'];workspace=c.rpc('query.workspace');c.command('project.configuration.update',pid,workspace['active_project']['revision'],{'workflow_ref':defaults['workflow_ref'],'role_agent_bindings':bindings})
 workspace=c.rpc('query.workspace');sid=c.command('session.open',pid,workspace['active_project']['revision'],{'agent':agent['id']})['id'];state=c.rpc('query.session',{'id':sid});lock=state['session']['runtime_binding']['agent_configuration']['integration_lock'];assert len(lock['tools'])==2 and list(lock['skills'])==[skill['id']]
 prompt='This is an authorized integration acceptance. Read the selected Integration canary Skill and its references/canary.md file using the native shell. Invoke lookup_http_canary once with {}. Invoke lookup_mcp_sum once with {"a":7,"b":8}. Do not call the services using shell networking. Reply with the exact HTTP marker, exact Skill reference marker and the MCP sum. Do not install anything or modify project files.'
 tid=c.command('conversation.send',sid,state['session']['revision'],{'client_message_id':str(uuid.uuid4()),'input':prompt})['id'];print('Native Codex verification started',pid,sid,tid,flush=True)
 def read():return c.rpc('query.session',{'id':sid,'limit':100})
 state=c.wait(read,lambda s:next(t for t in s['turns'] if t['id']==tid)['status'] in ['completed','failed','cancelled','needs_reconciliation'],360)
 turn=next(t for t in state['turns'] if t['id']==tid);assert turn['status']=='completed',{'turn':turn,'last_events':state['events'][-4:]}
 text='\n'.join(e['content'] for e in state['events'] if e['kind']=='assistant');assert marker in text and skill_marker in text and '15' in text,text
 calls=c.rpc('query.tool_calls',{'local':True,'session_id':sid})['records'];assert {r['name'] for r in calls}=={'lookup_http_canary','lookup_mcp_sum'},calls;assert all(r['data']['status']=='succeeded' and r['data']['receipt']['origin']=='external' and r['data']['reply_sent'] for r in calls);assert counts['http']==1,counts
 report.update(project=pid,session=sid,turn=tid,calls=[r['id'] for r in calls],call_origins=[r['data']['receipt']['origin'] for r in calls],http_request_count=counts['http'],skill_ref=skill['id'],status='first_turn_passed')
 args.report.write_text(json.dumps(report,indent=2));(ROOT/'model-output.txt').write_text(text)
 print('Codex consumed real HTTP, MCP and selected Skill results',flush=True)
 # Restart and ask a second real Codex turn to invoke the same frozen contracts.
 host.terminate();host.wait(timeout=10);host=subprocess.Popen([str(args.host.resolve()),'--data',str(ROOT),'--port',str(PORT)],stdout=log,stderr=subprocess.STDOUT);c=Client(f'http://127.0.0.1:{PORT}',pid)
 restored=c.rpc('query.session',{'id':sid,'limit':100});assert restored['session']['runtime_binding']['agent_configuration']['integration_lock']==lock
 next_turn=c.command('conversation.send',sid,restored['session']['revision'],{'client_message_id':str(uuid.uuid4()),'input':'Use lookup_mcp_sum once with a=20,b=22. Read the selected Skill reference again. Try to append a newline to its references/canary.md; this must be denied by the readonly /skills mount. Reply with the sum, the HTTP marker from the previous turn, the Skill marker and the write denial. Do not call HTTP again or change project files.'})['id'];state=c.wait(read,lambda s:next(t for t in s['turns'] if t['id']==next_turn)['status'] in ['completed','failed','cancelled','needs_reconciliation'],360);assert next(t for t in state['turns'] if t['id']==next_turn)['status']=='completed'
 text='\n'.join(e['content'] for e in state['events'] if e['kind']=='assistant' and e['turn_id']==next_turn);assert '42' in text and marker in text and skill_marker in text,text
 logs=list((ROOT/'projects'/pid).rglob('*.jsonl'));assert any('Read-only file system' in path.read_text(errors='replace') for path in logs),'Missing actual readonly Shell failure'
 calls=c.rpc('query.tool_calls',{'session_id':sid})['records'];assert len(calls)==3 and all(r['data']['status']=='succeeded' for r in calls)
 report.update(status='passed',restored_turn=next_turn,restored_http_request_count=counts['http'],skill_read_only_mount_verified=True,calls_after_restart=len(calls));assert counts['http']==1
 # Scan noncredential evidence for the actual test OAuth token values without printing them.
 secrets=[]
 for file in ROOT.rglob('auth.json'):
  value=json.loads(file.read_text());secrets.extend(v.encode() for v in value.get('tokens',{}).values() if isinstance(v,str) and len(v)>40)
 for file in ROOT.rglob('*'):
  if file.is_file() and file.name!='auth.json':
   data=file.read_bytes();assert not any(secret in data for secret in secrets),'Credential appeared in noncredential evidence: '+str(file.relative_to(ROOT))
 report['token_scan']='passed'
 print('Native restart recovery passed',flush=True)
finally:
 args.report.write_text(json.dumps(report,indent=2));host.terminate();host.wait(timeout=15);mcp.terminate();mcp.wait(timeout=5);service.shutdown();log.close()
 for secret in ROOT.rglob('auth.json'):secret.unlink()
 print('Acceptance dataset:',ROOT,'Report:',args.report,flush=True)
