#!/usr/bin/env python3
"""Explicit real Codex acceptance of a freely configured business workflow; uses an isolated dataset."""
import argparse,json,os,shutil,subprocess,tempfile,time,uuid,sys
from pathlib import Path
from client import Client
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--codex-auth-source',type=Path,required=True);parser.add_argument('--report-directory',type=Path);args=parser.parse_args()
OUT=args.report_directory or Path(tempfile.mkdtemp(prefix='devdesk-native-v5-'));OUT.mkdir(parents=True,exist_ok=True);OUT=OUT.resolve(); print('Evidence:',OUT,flush=True)
private_binary=OUT/'devdesk-host';shutil.copy2(args.binary.resolve(),private_binary)
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
 native_tools=next(record['data']['tools'] for record in defaults['records'] if record['id']==defaults['capability_ref'])
 capture=next(record['id'] for record in defaults['records'] if record['kind']=='tool' and record['data'].get('qualified_name')=='materials.capture')
 project=c.command('project.create',payload={'title':'自定义分析原生验收'})['id'];c.project=project
 role=c.command('role.create',payload={'name':'数据研究员','data':{'instructions':'调查明确提供的数字输入，保留可复核的结果。'}})
 skill=c.command('skill.create',payload={'name':'顺序分析法','data':{'body':'将输入的整数按降序排序，然后计算其总和。保留重复值。不猜测缺失数据。'}})
 capability=c.command('capability.create',payload={'name':'实际研究能力','data':{'model_ref':defaults['model_ref'],'harness_ref':defaults['harness_ref'],'skills':[skill['id']],'tools':list(dict.fromkeys(native_tools+[capture]))}})
 agent=c.command('agent.create',payload={'name':'研究 Agent','data':{'role_ref':role['id'],'capability_ref':capability['id']}})
 spec=c.command("spec.create",payload={"name":"排序分析内容规范","data":{"schema_version":3,"body":"输入整数按降序排列，保留重复值；sum 等于所有输入之和。结果可追溯当前 numbers 输入。","checks":[{"key":"calculation","name":"计算结果","instructions":"逐项核对降序、重复值、总和与实际输入一致。"}]}})
 delivery=c.command('delivery_type.create',payload={'name':'排序分析结果','data':{'spec_refs':[spec['id']],'body':'输入整数的计算结果','format':'json','content_schema':{'type':'object','required':['ordered','sum'],'properties':{'ordered':{'type':'array','items':{'type':'integer'}},'sum':{'type':'integer'}},'additionalProperties':False}}})
 document={'schema_version':5,'stages':[{'key':'study','name':'独立数据研究','default_role_ref':role['id']}],'steps':[
  {'key':'investigate','name':'按技能分析','stage':'study','executor':{'type':'agent'},'required_skill_refs':[skill['id']],'task_instructions':'按本次加载的顺序分析 Skill 处理 numbers 输入。先实际调用 devdesk_materials_capture 工具，参数为计算结果对象 ordered 与 sum；收到成功回执后，再只输出当前 output_contract 要求的单个 JSON 对象，分析结果写入 result 端口。','inputs':{'numbers':{'type':'constant','value':[3,1,2,3]}},'outputs':{'result':{'delivery_ref':delivery['id']}},'budget':{'max_turns':2,'timeout_seconds':300,'retry_limit':1}},
  {'key':'consume','name':'消费确切材料','stage':'study','executor':{'type':'control'},'dependencies':['investigate'],'inputs':{'analysis':{'type':'source','source':{'step':'investigate','port':'result'}}}}
 ]}
 draft=c.command('workflow.create',payload={'name':'自定义数字研究','document':document});workflow=c.command('workflow.publish',draft['id'],draft['revision'])
 context=next(r for r in defaults['records'] if r['id']==defaults['permission_context_ref'])['data']
 permission=c.command('permission_context.create',payload={'name':'Native research authorization','data':{**context,'step_grants':[{'workflow_ref':workflow['id'],'step':'investigate','tool_refs':[capture]}]}})
 p=c.rpc('query.workspace')['active_project'];c.command('project.configuration.update',project,p['revision'],{'workflow_ref':workflow['id'],'role_agent_bindings':{**defaults['role_agent_bindings'],role['id']:agent['id']},'permission_context_ref':permission['id']})
 p=c.rpc('query.workspace')['active_project'];run=c.command('workflow.start',project,p['revision'],{'workflow_ref':workflow['id'],'role_agent_bindings':{role['id']:agent['id']}})
 manager=c.command('orchestration.open',run['id'],payload={'role_ref':defaults['orchestrator_role_ref'],'agent_ref':defaults['role_agent_bindings'][defaults['orchestrator_role_ref']],'permission_context_ref':permission['id']})['id']
 manager_row=c.rpc('query.orchestration',{'id':manager})['record']
 c.command('orchestration.input',manager,manager_row['revision'],{'input':'按当前调查流程派发、核对、接纳每步并完成。必须使用真实业务 Agent 和工具，不更改或绕过计划。'})
 last=None
 for _ in range(900):
  rows=c.rpc('query.workspace')['configurations'];row=next(r for r in rows if r['id']==run['id']);steps=row['data']['steps'];status=[(r['step'],r['status'],r['reason']) for r in steps]
  if status!=last:print(status,flush=True);last=status
  if row['data']['status']=='completed':break
  if any(r['status'] in ['failed','needs_reconciliation','cancelled'] for r in steps):raise RuntimeError(status)
  time.sleep(1)
 else:raise TimeoutError('Free business workflow')
 view=c.rpc('query.workflow_instance',{'id':run['id']});(OUT/'free-workflow.json').write_text(json.dumps(view,ensure_ascii=False,indent=2))
 output=view['outputs'][steps[0]['outputs']['result']]['value'];assert output=={'ordered':[3,3,2,1],'sum':9},output
 assert steps[1]['inputs']['analysis']==output
 task=c.rpc('query.execution_task',{'turn_id':steps[0]['turn_id']});(OUT/'free-task.json').write_text(json.dumps(task,ensure_ascii=False,indent=2));assert task['configured'] and task['task']['renderer_version']=='agent-task/v6'
 calls=c.rpc('query.tool_calls',{'turn_id':steps[0]['turn_id']})['records'];actual=next(call for call in calls if call['kind']=='platform_call' and call['data']['status']=='succeeded');assert actual['data']['platform_receipt']['inputs']==output
 (OUT/'native-platform-call.json').write_text(json.dumps(actual,ensure_ascii=False,indent=2))
 print('PASS native custom business workflow',run['id'],flush=True)
 (OUT/'report.md').write_text('# Native Workflow v6\n\nFree business workflow: passed through actual Codex, SQLite and Artifact.\n\nWorkflow: '+run['id']+'\n')
except BaseException as error:
 (OUT/'failure.txt').write_text(str(error));raise
finally:
 if child.poll() is None:child.terminate();child.wait(timeout=20)
 shutil.rmtree(OUT/'data/credentials',ignore_errors=True)
 for private_auth in (OUT/'data').rglob('auth.json'):private_auth.unlink(missing_ok=True)
 print('Evidence:',OUT,flush=True)
