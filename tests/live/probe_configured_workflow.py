#!/usr/bin/env python3
"""Explicit real Codex acceptance of a freely configured business workflow; uses an isolated dataset."""
import argparse,json,os,shutil,subprocess,tempfile,time,uuid,sys,signal
from pathlib import Path
from client import Client
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--codex-auth-source',type=Path,required=True);parser.add_argument('--report-directory',type=Path);parser.add_argument('--frontends',type=int,choices=[0,100],default=0);parser.add_argument('--prepare-frontend-fixture',action='store_true');args=parser.parse_args()
OUT=args.report_directory or Path(tempfile.mkdtemp(prefix='devdesk-native-v5-'));OUT.mkdir(parents=True,exist_ok=True);OUT=OUT.resolve(); print('Evidence:',OUT,flush=True)
private_binary=OUT/'devdesk-host';shutil.copy2(args.binary.resolve(),private_binary)
child=subprocess.Popen([str(private_binary),'--data',str(OUT/'data'),'--port','0'],stdout=(OUT/'host.log').open('w'),stderr=subprocess.STDOUT)
native_load=None
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
 project=c.command('project.create',payload={'title':'自定义分析原生验收'})['id'];c.project=project
 role=c.command('role.create',payload={'name':'数据研究员','data':{'instructions':'调查明确提供的数字输入，保留可复核的结果。'}})
 skill=c.command('skill.create',payload={'name':'顺序分析法','data':{'body':'将输入的整数按降序排序，然后计算其总和。保留重复值。不猜测缺失数据。'}})
 capability=c.command('capability.create',payload={'model_ref':defaults['model_ref'],'harness_ref':defaults['harness_ref'],'skills':[skill['id']],'tools':list(dict.fromkeys(native_tools+[capture])),'name':'实际研究能力','data':{}})
 agent=c.command('agent.create',payload={'role_ref':role['id'],'capability_ref':capability['id'],'name':'研究 Agent','data':{}})
 spec=c.command("spec.create",payload={"name":"排序分析内容规范","data":{"schema_version":3,"body":"输入整数按降序排列，保留重复值；sum 等于所有输入之和。结果可追溯当前 numbers 输入。","checks":[{"key":"calculation","name":"计算结果","instructions":"逐项核对降序、重复值、总和与实际输入一致。"}]}})
 delivery=c.command('delivery_type.create',payload={'spec_refs':[spec['id']],'name':'排序分析结果','data':{'body':'输入整数的计算结果','format':'json','content_schema':{'type':'object','required':['ordered','sum'],'properties':{'ordered':{'type':'array','items':{'type':'integer'}},'sum':{'type':'integer'}},'additionalProperties':False}}})
 document={'schema_version':6,'stages':[{'key':'study','name':'独立数据研究','default_role_ref':role['id']}],'steps':[
  {'key':'investigate','name':'按技能分析','stage':'study','executor':{'type':'agent'},'required_skill_refs':[skill['id']],'task_instructions':'按本次加载的顺序分析 Skill 处理 numbers 输入。先实际调用 devdesk_materials_capture 工具，参数为计算结果对象 ordered 与 sum；收到成功回执后，再只输出当前 output_contract 要求的单个 JSON 对象，分析结果写入 result 端口。','inputs':{'numbers':{'type':'constant','value':[3,1,2,3]}},'outputs':{'result':{'delivery_ref':delivery['id']}},'budget':{'max_turns':2,'timeout_seconds':300,'retry_limit':1}},
  {'key':'consume','name':'消费确切材料','stage':'study','executor':{'type':'control'},'dependencies':['investigate'],'inputs':{'analysis':{'type':'source','source':{'step':'investigate','port':'result'}}}}
 ]}
 profiles=c.rpc('query.permission_resources')['profiles']
 base_profile=next(r for r in profiles if r['identity']=='builtin-permission-harness-read-only' and r['revision']==1)
 policy={**base_profile['data']['policy'],'tool_refs':[capture]}
 permission=c.command('permission.profile.create',payload={'name':'Native research Profile','identity':'native-step-'+project,'target_kind':'harness','policy':policy})
 profile_ref=f"{permission['identity']}:{permission['revision']}"
 next(step for step in document['steps'] if step['key']=='investigate')['harness_permission_ref']=profile_ref
 draft=c.command('workflow.create',payload={'name':'自定义数字研究','document':document});workflow=c.command('workflow.publish',draft['id'],draft['revision'])
 p=c.rpc('query.execution_workspace')['active_project'];c.command('project.configuration.update',project,p['revision'],{'workflow_ref':workflow['id'],'role_agent_bindings':{**defaults['role_agent_bindings'],role['id']:agent['id']},'orchestrator_role_ref':defaults['orchestrator_role_ref'],'harness_permission_ref':defaults['harness_permission_ref'],'orchestration':{'mode':'agent'}})
 if args.frontends or args.prepare_frontend_fixture:
  sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'performance'))
  from native_frontend_load import NativeFrontendLoad
  native_load=NativeFrontendLoad(Client,urls[0],OUT/'frontend-load',100)
  if args.frontends:native_load.begin(project)
 p=c.rpc('query.execution_workspace')['active_project'];run=c.command('workflow.start',project,p['revision'],{'workflow_ref':workflow['id'],'role_agent_bindings':{**defaults['role_agent_bindings'],role['id']:agent['id']},'orchestrator_role_ref':defaults['orchestrator_role_ref'],'harness_permission_ref':defaults['harness_permission_ref'],'orchestration':{'mode':'agent'}})
 if native_load:native_load.attach(project,run['id'])
 manager=c.rpc('query.orchestration',{'root_instance_id':run['id']})['record']['id']
 last=None
 for _ in range(900):
  view=c.rpc('query.workflow_instance',{'id':run['id'],'view':'status'});row=view['record'];steps=view['runtime']['steps'];status=[(r['step'],r['status'],r.get('reason','')) for r in steps]
  if status!=last:print(status,flush=True);last=status
  if row['data']['status']=='completed':break
  if any(r['status'] in ['failed','needs_reconciliation','cancelled'] for r in steps):raise RuntimeError(status)
  time.sleep(1)
 else:raise TimeoutError('Free business workflow')
 view=c.rpc('query.workflow_instance',{'id':run['id']});(OUT/'free-workflow.json').write_text(json.dumps(view,ensure_ascii=False,indent=2))
 investigate=next(step for step in view['runtime']['steps'] if step['step']=='investigate')
 consume=next(step for step in view['runtime']['steps'] if step['step']=='consume')
 output=view['outputs'][investigate['outputs']['result']]['value'];assert output=={'ordered':[3,3,2,1],'sum':9},output
 assert consume['inputs']['analysis']==output
 task=c.rpc('query.execution_task',{'turn_id':investigate['turn_id']});(OUT/'free-task.json').write_text(json.dumps(task,ensure_ascii=False,indent=2));assert task['configured'] and task['task']['renderer_version']=='agent-task/v6'
 calls=c.rpc('query.tool_calls',{'turn_id':investigate['turn_id']})['records'];actual=next(call for call in calls if call['type']=='platform_call' and call['data']['status']=='succeeded');assert actual['data']['platform_receipt']['inputs']==output
 (OUT/'native-platform-call.json').write_text(json.dumps(actual,ensure_ascii=False,indent=2))
 # Workflow completion can precede the manager receiving its last native reply.
 # Let that real Turn finish and the Host revoke its writer before stopping it.
 deadline=time.monotonic()+120
 while time.monotonic()<deadline:
  manager_view=c.rpc('query.orchestration',{'id':manager})
  sid=manager_view['record']['data']['session_id']
  session=c.rpc('query.session',{'id':sid})
  if not any(t['status'] in ['queued','dispatching','running','waiting_interaction','needs_reconciliation'] for t in session['turns']) and all(e['writer_revoked'] for e in session['executions']):break
  time.sleep(.5)
 else:raise TimeoutError('Completed workflow manager did not settle')
 (OUT/'native-manager.json').write_text(json.dumps(manager_view,ensure_ascii=False,indent=2))
 print('PASS native custom business workflow',run['id'],flush=True)
 (OUT/'report.md').write_text('# Native Workflow v6\n\nFree business workflow: passed through actual Codex, SQLite and Artifact.\n\nWorkflow: '+run['id']+'\n')
except BaseException as error:
 (OUT/'failure.txt').write_text(str(error));raise
finally:
 if native_load:
  load_summary=native_load.finish()
  print('Frontend load:',json.dumps(load_summary),flush=True)
 if child.poll() is None:child.send_signal(signal.SIGINT);child.wait(timeout=60)
 shutil.rmtree(OUT/'data/credentials',ignore_errors=True)
 for private_auth in (OUT/'data').rglob('auth.json'):private_auth.unlink(missing_ok=True)
 print('Evidence:',OUT,flush=True)

if native_load and (load_summary['errors'] or load_summary['failures']):raise RuntimeError('Native workflow HTTP load failed; all samples retained')
