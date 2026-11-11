#!/usr/bin/env python3
"""Explicit real Codex/Git/Docker/GitHub acceptance of the default engineering graph in an isolated dataset."""
import argparse,hashlib,json,os,shutil,subprocess,tempfile,time,uuid,sys
from pathlib import Path
from client import Client

def verify_evidence(client, rows, output):
 views={row['id']:client.rpc('query.workflow_instance',{'id':row['id']}) for row in rows}
 root=next(view for view in views.values() if view['runtime']['parent_execution_id'] is None)
 children=[view for view in views.values() if view['runtime']['parent_execution_id'] is not None]
 assert len(children)==1, 'Expected one actual Task invocation'
 def receipt(view, key):
  step=next(step for step in view['runtime']['steps'] if step['step']==key)
  assert step['status']=='completed', (key,step['status'])
  return view['outputs'][step['outputs']['result']]['value']
 child=children[0]
 for view in views.values():
  assert all(step.get('confirmation','automatic')=='automatic' for step in view['plan']['steps'])
  assert all(not status['confirmation'] for status in view['completion_status'].values()), 'Unexpected confirm or approve record'
 implementation=next(step for step in child['runtime']['steps'] if step['step']=='implementation')
 evidence_id=implementation['outputs']['evidence'];evidence=child['outputs'][evidence_id]['value']
 assert {'red','green'} <= {run['phase'] for run in evidence['runs']}, 'Raw evidence must include actual Red and Green'
 for run in evidence['runs']:
  assert isinstance(run['stdout'],str) and isinstance(run['stderr'],str)
  assert {'runtime','version','platform','arch'} <= set(run['environment'])
  assert {'status','signal','error','timed_out','cancelled'} <= set(run['termination'])
  if run['phase']=='green':
   assert run['exit_code']==0 and run['termination']=={'status':'exited','signal':None,'error':None,'timed_out':False,'cancelled':False}
  for source in run['files']:
   assert hashlib.sha256(source['content'].encode()).hexdigest()==source['sha256'], ('Evidence snapshot digest mismatch',run['sequence'],source['path'])
 for key in ['documentation','review','candidate_review']:
  execution=next(step for step in child['runtime']['steps'] if step['step']==key)
  task=client.rpc('query.execution_task',{'turn_id':execution['turn_id']})['task']['task']
  assert execution['inputs']['evidence']==evidence and task['inputs']['evidence']==evidence, ('Missing raw evidence at consumer',key)
  assert task['input_sources']['evidence']['source_output_id']==evidence_id, ('Wrong evidence provenance',key)
 (output/'raw-tdd-evidence.json').write_text(json.dumps(evidence,ensure_ascii=False,indent=2))
 red=receipt(children[0],'red');assert red['expected_failure'] and red['assertion_failed'] and red['counts']['failed']>0 and red['container_id']
 green=receipt(children[0],'green');assert green['status']=='succeeded' and not green['expected_failure'] and green['exit_code']==0 and green['counts']['passed']>0
 verified=receipt(children[0],'verify');assert verified['status']=='succeeded' and verified['candidate']==green['candidate'] and verified['exit_code']==0 and verified['container_id']
 completed=receipt(children[0],'complete');assert completed['status']=='completed' and completed['task_id']==children[0]['runtime']['inputs']['item']['id']
 publications=[]
 for view in [children[0],root]:
  candidate=receipt(view,'candidate');tests=receipt(view,'candidate_tests');published=receipt(view,'publish')
  review_key='review' if view is child else 'closure_review'
  snapshot_key='implementation_snapshot' if view is child else 'tests_snapshot'
  review=next(step for step in view['runtime']['steps'] if step['step']==review_key)
  task=client.rpc('query.execution_task',{'turn_id':review['turn_id']})['task']['task']
  assert task['inputs']['candidate']==candidate
  assert task['inputs']['changes']==receipt(view,snapshot_key)['changes']
  assert task['inputs']['changes']['revision']==candidate['source_revision']
  assert candidate['parents']==[candidate['target_revision']] and candidate['diff']
  assert tests['candidate']==candidate['revision'] and tests['exit_code']==0 and tests['container_id']
  pr=published['pull_request'];assert pr['merged'] and pr['merge_commit']==candidate['revision'] and pr['head']==candidate['revision']
  publications.append(published)
 for key,view in views.items():(output/(key+'.json')).write_text(json.dumps(view,ensure_ascii=False,indent=2))
 (output/'publication-evidence.json').write_text(json.dumps(publications,ensure_ascii=False,indent=2))
 return publications

parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--codex-auth-source',type=Path,required=True);parser.add_argument('--github-repo',required=True);parser.add_argument('--report-directory',type=Path);args=parser.parse_args();assert 'test' in args.github_repo.lower(), 'use a dedicated test repository'
OUT=args.report_directory or Path(tempfile.mkdtemp(prefix='devdesk-engineering-v4-'));OUT.mkdir(parents=True,exist_ok=True);OUT=OUT.resolve();print('Evidence:',OUT,flush=True)
private_binary=OUT/'devdesk-host';shutil.copy2(args.binary.resolve(),private_binary)
child=subprocess.Popen([str(private_binary),'--data',str(OUT/'data'),'--port','0'],stdout=(OUT/'host.log').open('w'),stderr=subprocess.STDOUT)
try:
 for _ in range(150):
  urls=[r for r in (OUT/'host.log').read_text().splitlines() if r.startswith('http://')]
  if urls:break
  if child.poll() is not None:raise RuntimeError('Host startup')
  time.sleep(.1)
 else:raise TimeoutError('Host startup')
 c=Client(urls[0]);auth=OUT/'data/credentials/codex/auth.json';auth.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(args.codex_auth_source.resolve(),auth);auth.chmod(0o600)
 project=c.command('project.open_github',payload={'repository':args.github_repo,'mainline':'main','title':'Workflow v4 engineering acceptance'})['id'];c.project=project
 c.wait(lambda:c.rpc('query.workspace'),lambda v:v['repository_ready'],300)
 runtime=c.rpc('runtime.prepare',{'provider':'codex'});image=runtime['artifact']['image_id']
 defaults=c.rpc('query.configuration_defaults');workflow=c.rpc('query.workflow',{'id':defaults['workflow_ref']})['data'];suffix=uuid.uuid4().hex[:8];module=f'src/workflow-v4-{suffix}/greeting.mjs';task_test=f'tests/workflow-v4-{suffix}-task/behavior.test.mjs';feature_test=f'tests/workflow-v4-{suffix}-feature/behavior.test.mjs';increment_test=f'test/workflow-v4-{suffix}-increments.test.mjs'
 goal=f'''本次验收事实已经全部确定，无需等待用户补充：在已有仓库新增依赖免费的 ESM 模块 {module}，export greeting(name)。去掉字符串首尾空白，返回 `Hello, <trimmed>!`；空白字符串或非字符串参数抛出 TypeError。一个 Task 即可完成，稳定 key greeting，依赖为空。验收 AC-GREET：正常及带首尾空白输入返回问候；AC-INVALID：空白与非字符串输入抛出 TypeError。允许新增 {module}、Task 独立测试 {task_test}、Feature 独立测试 {feature_test} 及开发职责自有的增量测试 {increment_test}；独立 tests 断言保持只读，自有增量测试可按冻结 Task 的各个增量逐步增加并分别采集 Red/Green；可调整 README，禁止修改既有无关文件或增加辅助仓库文档。测试使用 node:test、node:assert/strict，无 npm 安装、无联网、无外部依赖。测试 import 为 ../../{module}。模块或导出缺失属于运行前置条件错误，不使用动态 import 的 catch、assert.fail 或伪造返回值将它包装成业务 Red；由 Task Workflow 的 red_baseline 步骤准备可调用的最小接口，平台 Red 必须来自真实业务断言差异。实施逐次原始日志及测试/源码内容快照通过必需 evidence 输出交付，不能只提交临时路径或摘要。Task 配方仅运行 {task_test}；Feature 和最终候选配方运行 {task_test} 与 {feature_test} 两个文件。固定 Node TestRecipe 的 runner=node_test、image={image}、timeout_seconds=60。测试、实现、文档和独立评审必须按各自 Skill 和当前 Step 合同真实完成，不能把计划、模型自述或未运行内容当作工具回执。业务文档通过当前输出端口交付。'''
 assert all(step.get('confirmation','automatic')=='automatic' for step in workflow['steps'])
 p=c.rpc('query.workspace')['active_project']
 configuration=c.command('project.configuration.update',project,p['revision'],{'workflow_ref':defaults['workflow_ref'],'role_agent_bindings':defaults['role_agent_bindings']})
 feature=c.command('feature.create',payload={'project_id':project,'title':'Workflow v4 greeting '+suffix,'configuration_revision_id':configuration['id']})['id']
 (OUT/'context.json').write_text(json.dumps({'url':urls[0],'project':project,'feature':feature,'workflow':defaults['workflow_ref'],'suffix':suffix,'goal':goal},ensure_ascii=False,indent=2));print('Feature:',feature,flush=True)
 last=None;inputs=set();deadline=time.monotonic()+2400
 while time.monotonic()<deadline:
  view=c.rpc('query.workspace',{'feature_id':feature});rows=[r for r in view['configurations'] if r['kind']=='workflow_instance' and r['data']['owner_id']==feature];root=next(r for r in rows if r['data']['parent_execution_id'] is None)
  status=[(r['id'],s['path'],s['status'],s['reason']) for r in rows for s in r['data']['steps'] if s['status'] not in ['completed','queued','skipped']]
  if status!=last:print(status,flush=True);last=status
  (OUT/'latest.json').write_text(json.dumps(view,ensure_ascii=False,indent=2))
  assert root['data']['workflow_ref']==defaults['workflow_ref'], 'Feature did not inherit the project default'
  for row in rows:
   for step in row['data']['steps']:
    if step['status'] in ['failed','needs_reconciliation','cancelled']:
     detail=c.rpc('query.workflow_instance',{'id':row['id']});(OUT/'failed-instance.json').write_text(json.dumps(detail,ensure_ascii=False,indent=2));raise RuntimeError((step['path'],step['status'],step['reason']))
    if step['status']=='waiting_input':
     key=(step['id'],step['sequence'])
     if inputs or step['step']!='requirements' or row['id']!=root['id']:raise RuntimeError('Unexpected clarification after initial requirements: '+step['path'])
     c.command('workflow.step.input',row['id'],row['revision'],{'step':step['step'],'input':goal});inputs.add(key);break
    if step['status']=='waiting_confirmation':
     pending=c.rpc('query.workflow_instance',{'id':row['id']});(OUT/'pending-confirmation.json').write_text(json.dumps(pending,ensure_ascii=False,indent=2))
     raise RuntimeError('Default workflow must not need an external confirmation: '+step['path']+': '+step['reason'])
  if root['data']['status']=='completed':
   assert view['detail']['feature']['status']=='completed';assert len(view['detail']['tasks'])==1 and view['detail']['tasks'][0]['status']=='completed'
   publications=verify_evidence(c,rows,OUT)
   print('PASS native default engineering Workflow',root['id'],flush=True)
   (OUT/'report.md').write_text('# Native default engineering Workflow v4\n\nPassed through actual Codex, managed Git, Docker Red/Green and candidate tests; both GitHub PRs merged at their exact candidate SHA.\n\nRoot: '+root['id']+'\nFeature: '+feature+'\nConfirmations: 0 (no confirm or approve commands)\nPRs: '+', '.join(str(publication['pull_request']['number']) for publication in publications)+'\n');break
  time.sleep(1)
 else:raise TimeoutError('Default engineering workflow')
except BaseException as error:
 (OUT/'failure.txt').write_text(str(error));raise
finally:
 if child.poll() is None:child.terminate();child.wait(timeout=20)
 shutil.rmtree(OUT/'data/credentials',ignore_errors=True)
 for private_auth in (OUT/'data').rglob('auth.json'):private_auth.unlink(missing_ok=True)
 print('Evidence:',OUT,flush=True)
