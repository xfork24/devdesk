#!/usr/bin/env python3
"""Real native PM drives two child workflows and requests Human signatures in the root UI. Observer never schedules."""
import argparse,json,os,shutil,subprocess,tempfile,time,uuid,sys,signal,hashlib
from pathlib import Path
from client import Client
from workflow_observer import read_workflow_tree, observe_human_request, check_human_request
from autonomous_orchestration import verify as verify_autonomy
import sqlite3
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--codex-auth-source',type=Path,required=True);parser.add_argument('--report-directory',type=Path);args=parser.parse_args()
OUT=args.report_directory or Path(tempfile.mkdtemp(prefix='devdesk-native-v5-'));OUT.mkdir(parents=True,exist_ok=True);OUT=OUT.resolve(); print('Evidence:',OUT,flush=True)
private_binary=OUT/'devdesk-host';shutil.copy2(args.binary.resolve(),private_binary)
tooling={}
for name in [Path(__file__).name,'client.py','workflow_observer.py','autonomous_orchestration.py','verify_manager_page.mjs']:
 source=Path(__file__).parent/name;target=OUT/'tooling'/name;target.parent.mkdir(exist_ok=True);shutil.copy2(source,target);tooling[name]=hashlib.sha256(source.read_bytes()).hexdigest()
(OUT/'identity.json').write_text(json.dumps({'binary_sha256':hashlib.sha256(private_binary.read_bytes()).hexdigest(),'tooling_sha256':tooling},indent=2)+'\n')
child=subprocess.Popen([str(private_binary),'--data',str(OUT/'data'),'--port','0'],stdout=(OUT/'host.log').open('w'),stderr=subprocess.STDOUT)
try:
 for _ in range(100):
  lines=(OUT/'host.log').read_text().splitlines(); urls=[r for r in lines if r.startswith('http://')]
  if urls:break
  if child.poll() is not None:raise RuntimeError('Host startup failed')
  time.sleep(.1)
 else:raise TimeoutError('Host startup')
 c=Client(urls[0]);auth=OUT/'data/credentials/codex/auth.json';auth.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(args.codex_auth_source.resolve(),auth);auth.chmod(0o600)
 c.rpc('runtime.prepare',{'provider':'codex'})
 defaults=c.rpc('query.configuration_defaults')
 native_tools=next(record['tools'] for record in defaults['records'] if record['id']==defaults['capability_ref'])
 capture=next(record['id'] for record in defaults['records'] if record['type']=='tool' and record['data'].get('qualified_name')=='materials.capture')
 project=c.command('project.create',payload={'title':'真实子流程审批联合验收'})['id'];c.project=project
 role=c.command('role.create',payload={'name':'数据研究员','data':{'instructions':'调查明确提供的数字输入，保留可复核的结果。'}})
 skill=c.command('skill.create',payload={'name':'顺序分析法','data':{'body':'将输入的整数按降序排序，然后计算其总和。保留重复值。不猜测缺失数据。'}})
 capability=c.command('capability.create',payload={'model_ref':defaults['model_ref'],'harness_ref':defaults['harness_ref'],'skills':[skill['id']],'tools':list(dict.fromkeys(native_tools+[capture])),'name':'实际研究能力','data':{}})
 agent=c.command('agent.create',payload={'role_ref':role['id'],'capability_ref':capability['id'],'name':'研究 Agent','data':{}})
 spec=c.command("spec.create",payload={"name":"排序分析内容规范","data":{"schema_version":3,"body":"输入整数按降序排列，保留重复值；sum 等于所有输入之和。结果可追溯当前 numbers 输入。","checks":[{"key":"calculation","name":"计算结果","instructions":"逐项核对降序、重复值、总和与实际输入一致。"}]}})
 delivery=c.command('delivery_type.create',payload={'spec_refs':[spec['id']],'name':'排序分析结果','data':{'body':'输入整数的计算结果','format':'json','content_schema':{'type':'object','required':['ordered','sum'],'properties':{'ordered':{'type':'array','items':{'type':'integer'}},'sum':{'type':'integer'}},'additionalProperties':False}}})
 document={'schema_version':6,'stages':[{'key':'study','name':'独立数据研究','default_role_ref':role['id']}],'steps':[
  {'key':'investigate','name':'按技能分析','stage':'study','executor':{'type':'agent'},'required_skill_refs':[skill['id']],'task_instructions':'按本次加载的顺序分析 Skill 处理 numbers 输入。先实际调用 devdesk_materials_capture 工具，参数为计算结果对象 ordered 与 sum；收到成功回执后，再只输出当前 output_contract 要求的单个 JSON 对象，分析结果写入 result 端口。','inputs':{'numbers':{'type':'constant','value':[3,1,2,3]}},'outputs':{'result':{'delivery_ref':delivery['id']}},'budget':{'max_turns':2,'timeout_seconds':300,'retry_limit':1}},
  {'key':'consume','name':'人工验收确切分析','stage':'study','executor':{'type':'control'},'confirmation':'human','dependencies':['investigate'],'inputs':{'analysis':{'type':'source','source':{'step':'investigate','port':'result'}}}}
 ]}
 profiles=c.rpc('query.permission_resources')['profiles']
 base_profile=next(r for r in profiles if r['identity']=='builtin-permission-harness-read-only' and r['revision']==1)
 policy={**base_profile['data']['policy'],'tool_refs':[capture]}
 permission=c.command('permission.profile.create',payload={'name':'Native research Profile','identity':'native-step-'+project,'target_kind':'harness','policy':policy})
 profile_ref=f"{permission['identity']}:{permission['revision']}"
 next(step for step in document['steps'] if step['key']=='investigate')['harness_permission_ref']=profile_ref
 draft=c.command('workflow.create',payload={'name':'自定义数字研究','document':document});workflow=c.command('workflow.publish',draft['id'],draft['revision'])
 leaf=workflow
 root_document={'schema_version':6,'scheduling':{'max_concurrency':2,'max_actions_per_tick':8,'lease_seconds':60},'stages':[{'key':'children','name':'两个独立分析子流程'}],'steps':[{'key':key,'name':name,'stage':'children','executor':{'type':'workflow','workflow_ref':leaf['id']}} for key,name in [('left','左侧独立分析与人类验收'),('right','右侧独立分析与人类验收')]]}
 root_draft=c.command('workflow.create',payload={'name':'项目经理自主驱动的两个子流程审批','document':root_document});workflow=c.command('workflow.publish',root_draft['id'],root_draft['revision'])
 p=c.rpc('query.execution_workspace')['active_project'];c.command('project.configuration.update',project,p['revision'],{'workflow_ref':workflow['id'],'role_agent_bindings':{**defaults['role_agent_bindings'],role['id']:agent['id']},'orchestrator_role_ref':defaults['orchestrator_role_ref'],'harness_permission_ref':defaults['harness_permission_ref'],'orchestration':{'mode':'agent'}})
 p=c.rpc('query.execution_workspace')['active_project'];run=c.command('workflow.start',project,p['revision'],{'workflow_ref':workflow['id'],'role_agent_bindings':{**defaults['role_agent_bindings'],role['id']:agent['id']},'orchestrator_role_ref':defaults['orchestrator_role_ref'],'harness_permission_ref':defaults['harness_permission_ref'],'orchestration':{'mode':'agent'}})
 manager=c.rpc('query.orchestration',{'root_instance_id':run['id']})['record']['id']
 (OUT/'context.json').write_text(json.dumps({'url':urls[0],'project':project,'root':run['id'],'leaf_workflow':leaf['id']},ensure_ascii=False,indent=2))
 last=None;shown=set();observed_failures=set();deadline=time.monotonic()+2700
 while time.monotonic()<deadline:
  tree=read_workflow_tree(c,run['id']);status=[(identity,step['step'],step['status']) for identity,view in tree.items() for step in view['runtime']['steps']]
  if status!=last:print(status,flush=True);last=status
  manager_view=c.rpc('query.orchestration',{'id':manager});(OUT/'latest-manager.json').write_text(json.dumps(manager_view,ensure_ascii=False,indent=2));(OUT/'latest-tree.json').write_text(json.dumps(tree,ensure_ascii=False,indent=2))
  if manager_view['record']['data']['status'] in ['needs_reconciliation','failed','budget_exhausted']:raise RuntimeError(manager_view['record']['data'])
  for request in manager_view['decisions']:
   if request['data']['status']!='pending' or request['id'] in shown:continue
   manager_url=urls[0]+'/?tab=execution&detail=manager&project_id='+project+'&instance_id='+run['id']
   observation=observe_human_request(OUT,request,manager_url,lambda saved:check_human_request(c,tree,saved,[run['id']]))
   if not observation['diagnostic_passed']:print('Human review diagnostic:',observation['diagnostic'],flush=True)
   shown.add(request['id']);print('CHILD HUMAN REQUEST '+request['id']+' '+request['definition_id']+' '+manager_url,flush=True)
  # A child failure is a fact for the actual manager to handle, not permission
  # for this read-only observer to stop its Host before it can reconcile/retry.
  # The manager's own terminal failure and the unchanged overall deadline still
  # fail this run; every intermediate failed execution remains in the evidence.
  for identity,view in tree.items():
   for step in view['runtime']['steps']:
    key=(step['id'],step['status'])
    if step['status'] in ['failed','needs_reconciliation','cancelled'] and key not in observed_failures:
     observed_failures.add(key)
     with (OUT/'observed-step-failures.jsonl').open('a') as stream:stream.write(json.dumps({'observed_at_ms':time.time()*1000,'instance_id':identity,'execution_id':step['id'],'status':step['status'],'failure':step.get('failure'),'reason':step.get('reason')},ensure_ascii=False)+'\n')
     print('Observed child failure; awaiting autonomous manager handling',identity,step['id'],step['status'],flush=True)
  if manager_view['record']['data']['status']=='closed' and tree[run['id']]['runtime']['status']!='completed':raise RuntimeError('Manager closed before the child workflow root completed')
  if tree[run['id']]['runtime']['status']=='completed':break
  time.sleep(1)
 else:raise TimeoutError('Autonomous child approval workflow')
 settled=c.wait(lambda:c.rpc('query.orchestration',{'id':manager}),lambda v:v['record']['data']['status']=='closed',120)
 full={identity:c.rpc('query.workflow_instance',{'id':identity}) for identity in tree};assert len(full)==3
 for identity,view in full.items():
  if identity==run['id']:continue
  first=next(step for step in view['runtime']['steps'] if step['step']=='investigate');gate=next(step for step in view['runtime']['steps'] if step['step']=='consume');assert view['outputs'][first['outputs']['result']]['value']=={'ordered':[3,3,2,1],'sum':9};assert gate['inputs']['analysis']=={'ordered':[3,3,2,1],'sum':9};assert view['completion_status'][gate['id']]['confirmation']['actor']['kind']=='human'
 with sqlite3.connect(OUT/'data/projects'/project/'engineering.sqlite3') as connection:
  autonomy=verify_autonomy(connection,run['id']);decisions=[json.loads(row[0]) for row in connection.execute("SELECT data FROM governance_records WHERE type='workflow_pending_decision'")];assert len([d for d in decisions if d['status']=='approved'])==2
 (OUT/'autonomous-orchestration-validation.json').write_text(json.dumps(autonomy,ensure_ascii=False,indent=2));(OUT/'completed-tree.json').write_text(json.dumps(full,ensure_ascii=False,indent=2));(OUT/'settled-manager.json').write_text(json.dumps(settled,ensure_ascii=False,indent=2));print('PASS real autonomous child approvals',run['id'],flush=True)

except BaseException as error:
 (OUT/'failure.txt').write_text(str(error));raise
finally:
 if child.poll() is None:child.send_signal(signal.SIGINT);child.wait(timeout=60)
 shutil.rmtree(OUT/'data/credentials',ignore_errors=True)
 for private_auth in (OUT/'data').rglob('auth.json'):private_auth.unlink(missing_ok=True)
 print('Evidence:',OUT,flush=True)
