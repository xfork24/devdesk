#!/usr/bin/env python3
"""Explicit real Codex/Git/Docker/GitHub acceptance of the default engineering graph in an isolated dataset."""
import argparse,json,os,shutil,subprocess,tempfile,time,uuid,sys,signal,hashlib
from pathlib import Path
from client import Client

from engineering_evidence import verify_evidence
from export_evidence import export as export_project_evidence

parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--binary',type=Path,required=True);parser.add_argument('--codex-auth-source',type=Path,required=True);parser.add_argument('--github-repo',required=True);parser.add_argument('--dedicated-repository',action='store_true',help='Explicitly designate this repository for test branches, commits and PRs');parser.add_argument('--report-directory',type=Path);parser.add_argument('--resume',action='store_true');parser.add_argument('--unattended',action='store_true',help='Fresh run: fail on supplemental input or any Human gate; no probe intervention');parser.add_argument('--guidance',default='');parser.add_argument('--timeout-seconds',type=int,default=10800);parser.add_argument('--manager-context-source',type=Path,help='Explicit immutable Permission Context source to adopt during stopped-controller recovery');args=parser.parse_args();assert args.manager_context_source is None or args.resume, 'manager context adoption is a resume operation';assert args.timeout_seconds>0;assert not (args.resume and args.unattended), 'unattended acceptance requires a fresh dataset';assert args.dedicated_repository or 'test' in args.github_repo.lower(), 'designate the dedicated test repository explicitly'
OUT=args.report_directory or Path(tempfile.mkdtemp(prefix='devdesk-engineering-v5-'));OUT.mkdir(parents=True,exist_ok=True);OUT=OUT.resolve();print('Evidence:',OUT,flush=True)
assert args.resume == (OUT/'context.json').exists(), 'select --resume for an existing acceptance dataset'
if (OUT/'failure.txt').exists():(OUT/'failure.txt').rename(OUT/('failure-before-launch-'+str(time.time_ns())+'.txt'))
private_binary=OUT/'devdesk-host';shutil.copy2(args.binary.resolve(),private_binary)
attempt=OUT/'attempts'/str(time.time_ns());attempt.mkdir(parents=True)
for name in [Path(__file__).name,'client.py','engineering_evidence.py','export_evidence.py']:shutil.copy2(Path(__file__).parent/name,attempt/name)
(attempt/'ATTEMPT.json').write_text(json.dumps({'resume':args.resume,'unattended':args.unattended,'timeout_seconds':args.timeout_seconds,'binary_sha256':hashlib.sha256(private_binary.read_bytes()).hexdigest(),'guidance':args.guidance},ensure_ascii=False,indent=2))
if (OUT/'host.log').exists():(OUT/'host.log').rename(OUT/('host-before-launch-'+str(time.time_ns())+'.log'))
child=subprocess.Popen([str(private_binary),'--data',str(OUT/'data'),'--port','0'],stdout=(OUT/'host.log').open('w'),stderr=subprocess.STDOUT)
try:
 startup_started=time.monotonic()
 for _ in range(1200):
  urls=[r for r in (OUT/'host.log').read_text().splitlines() if r.startswith('http://')]
  if urls:break
  if child.poll() is not None:raise RuntimeError('Host startup')
  time.sleep(.1)
 else:raise TimeoutError('Host startup')
 print(f'Host ready in {time.monotonic()-startup_started:.1f}s',flush=True)
 c=Client(urls[0]);auth=OUT/'data/credentials/codex/auth.json';auth.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(args.codex_auth_source.resolve(),auth);auth.chmod(0o600)
 failure_observed={}
 if args.resume:
  context=json.loads((OUT/'context.json').read_text());project=context['project'];feature=context['feature'];goal=context['goal'];c.open(project)
  context.setdefault('initially_unattended',context.get('unattended',False));context['unattended']=False
  context.setdefault('resumptions',[]).append({'time_ns':time.time_ns(),'binary_sha256':hashlib.sha256(private_binary.read_bytes()).hexdigest(),'classification':'same-dataset recovery; not an uninterrupted unattended run'})
  (OUT/'context.json').write_text(json.dumps(context,ensure_ascii=False,indent=2))
  root_view=c.rpc('query.workflow_instance',{'owner_id':feature});root_id=root_view['record']['id'];manager=c.rpc('query.orchestration',{'root_instance_id':root_id})['record']['id']
  if root_view['runtime']['status']=='completed':
   # A verifier correction must not take over or reopen a successfully finished
   # controller, dispatch new work, or repeat a real publication.
   completed=c.rpc('query.workspace',{'feature_id':feature})
   assert completed['detail']['feature']['status']=='completed' and len(completed['detail']['tasks'])==1 and completed['detail']['tasks'][0]['status']=='completed'
   rows=[r for r in completed['configurations'] if r['type']=='workflow_instance' and r['data']['root_instance_id']==root_id]
   publications=verify_evidence(c,rows,OUT)
   (OUT/'report.md').write_text('# Native default engineering Workflow v6\n\nPassed actual Codex, managed Git, Docker Red/Green, independent reviews, the frozen confirmation policy and explicit completion. Both GitHub PRs merged at their exact tested candidate SHA. Completed-instance resume only revalidated saved facts; it did not take over, dispatch or republish. The documented same-dataset failures and resumptions remain preserved.\n\nRoot: '+root_id+'\nFeature: '+feature+'\nPRs: '+', '.join(str(publication['pull_request']['number']) for publication in publications)+'\n')
   print('PASS completed native default engineering Workflow evidence; no takeover or redispatch',flush=True)
   sys.exit(0)
  # This continuation is limited to stopped, interrupted Agent executions. No business
  # rejection, test result, output or external effect is rewritten by the probe.
  manager_row=c.rpc('query.orchestration',{'id':manager})['record']
  c.command('orchestration.takeover',manager,manager_row['revision'],{'epoch':manager_row['data']['epoch']})
  def manager_stopped(_):
   record=c.rpc('query.orchestration',{'id':manager})['record'];sid=record['data']['session_id']
   if not sid:return True
   session=c.rpc('query.session',{'id':sid})
   return all(t['status'] not in ['queued','dispatching','running','waiting_interaction','needs_reconciliation'] for t in session['turns']) and all(e['writer_revoked'] for e in session['executions'])
  c.wait(lambda:None,manager_stopped,120)
  state=c.rpc('query.workspace',{'feature_id':feature})
  for row in [r for r in state['configurations'] if r['type']=='workflow_instance' and r['data']['root_instance_id']==root_id]:
   for interrupted in row['data']['steps']:
    if interrupted['status'] not in ['needs_reconciliation','cancelled']:continue
    definition=next(s for s in row['data']['plan']['document']['steps'] if s['key']==interrupted['step']);assert definition['executor']['type']=='agent','external tool effects require dedicated reconciliation'
    def control(method):
     fresh=c.rpc('query.workflow_instance',{'id':row['id']});return c.command(method,row['id'],fresh['record']['revision'],{'plan_revision_id':fresh['runtime']['active_plan_revision_id'],'control_version':fresh['runtime']['control_version'],'step':interrupted['step'],'reason':'专用验收恢复：旧执行者已实际停止，保留原记录并开始新执行'})
    if interrupted['status']=='needs_reconciliation':control('workflow.execution.stop')
    c.wait(lambda:c.rpc('query.session',{'id':interrupted['session_id']}),lambda v:all(e['writer_revoked'] for e in v['executions']) and all(t['status'] not in ['queued','dispatching','running','waiting_interaction','needs_reconciliation'] for t in v['turns']),120)
    if c.rpc('query.workflow_instance',{'id':row['id']})['runtime']['steps'][row['data']['steps'].index(interrupted)]['status']=='needs_reconciliation':control('workflow.step.reconcile')
    c.wait(lambda:c.rpc('query.workflow_instance',{'id':row['id']}),lambda v:next(e for e in v['runtime']['steps'] if e['step']==interrupted['step'])['status']=='cancelled',60)
    control('workflow.step.retry')
  selected_context=None
  if args.manager_context_source:
   source=json.loads(args.manager_context_source.read_text());assert source['type']=='permission_context'
   selected_context=source['identity']+':'+str(source['revision'])
   catalog=c.rpc('query.configurations',{'kind':'permission_context','include_history':True,'limit':100})['records'];existing=next((r for r in catalog if r['id']==selected_context),None)
   if existing:
    assert existing['data']==source['data'] and existing['scope']==source['scope'],'Existing context differs from the explicit source'
   else:
    change={'operations':[{'type':'create','source':source}]};preview=c.rpc('configuration.change.preview',{'changeset':change});applied=c.command('configuration.change.apply',payload={'changeset':change,'preview_hash':preview['preview_hash']});(attempt/'manager-context-import.json').write_text(json.dumps({'source':source,'preview':preview,'receipt':applied},ensure_ascii=False,indent=2))
   shutil.copy2(args.manager_context_source,attempt/'manager-context-source.json')
  manager_row=c.rpc('query.orchestration',{'id':manager})['record'];resumed=c.command('orchestration.resume',manager,manager_row['revision'],{'epoch':manager_row['data']['epoch'],**({'permission_context_ref':selected_context} if selected_context else {})});manager=resumed['id']
  context['resumptions'][-1]['controller_id']=manager;context['resumptions'][-1]['permission_context_ref']=resumed['controller']['permission_context_ref'];(OUT/'context.json').write_text(json.dumps(context,ensure_ascii=False,indent=2))
  m=c.rpc('query.orchestration',{'id':manager})['record'];print('Resumed preserved root:',root_id,flush=True)
 else:
  project=c.command('project.open_github',payload={'repository':args.github_repo,'mainline':'main','title':'Workflow v6 engineering acceptance'})['id'];c.project=project
  c.wait(lambda:c.rpc('query.workspace'),lambda v:v['repository_ready'],300)
  runtime=c.rpc('runtime.prepare',{'provider':'codex'});image=runtime['artifact']['image_id']
  defaults=c.rpc('query.configuration_defaults');workflow=c.rpc('query.workflow',{'id':defaults['workflow_ref']})['data'];suffix=uuid.uuid4().hex[:8];module=f'src/workflow-v5-{suffix}/greeting.mjs';task_test=f'tests/workflow-v5-{suffix}-task/behavior.test.mjs';feature_test=f'tests/workflow-v5-{suffix}-feature/behavior.test.mjs';increment_test=f'test/workflow-v5-{suffix}-increments.test.mjs'
  goal=f'''本次验收事实已经全部确定，无需等待用户补充：在已有仓库新增依赖免费的 ESM 模块 {module}，export greeting(name)。去掉字符串首尾空白，返回 `Hello, <trimmed>!`；空白字符串或非字符串参数抛出 TypeError。一个 Task 即可完成，稳定 key greeting，依赖为空。验收 AC-GREET：正常及带首尾空白输入返回问候；AC-INVALID：空白与非字符串输入抛出 TypeError。允许新增 {module}、Task 独立测试 {task_test}、Feature 独立测试 {feature_test} 及开发职责自有的增量测试 {increment_test}；本次明确两个 TDD 增量：先 AC-GREET 的问候与 trim，再 AC-INVALID 的无效输入拒绝，每个增量分别保留真实 Red 和 Green。独立 tests 断言保持只读，自有增量测试按这两个冻结增量逐步增加并分别采集 Red/Green；可调整 README，禁止修改既有无关文件或增加辅助仓库文档。测试使用 node:test、node:assert/strict，无 npm 安装、无联网、无外部依赖。测试 import 为 ../../{module}。模块或导出缺失属于运行前置条件错误，不使用动态 import 的 catch、assert.fail 或伪造返回值将它包装成业务 Red；由 Task Workflow 的 red_baseline 步骤准备可调用的最小接口，平台 Red 必须来自真实业务断言差异。实施逐次原始日志及测试/源码内容快照通过必需 evidence 输出交付，不能只提交临时路径或摘要。每轮采集真实 run 后，将该轮完整结构化 run JSON 输出到原生命令工具的 stdout，保留在工具观察账本中，再将相同记录汇总到 evidence；不要只在最终模型回答中补写记录。Task 配方仅运行 {task_test}；Feature 和最终候选配方运行 {task_test} 与 {feature_test} 两个文件。固定 Node TestRecipe 的 runner=node_test、image={image}、timeout_seconds=60。测试、实现、文档和独立评审必须按各自 Skill 和当前 Step 合同真实完成，不能把计划、模型自述或未运行内容当作工具回执。业务文档通过当前输出端口交付。README 修改是可选项，不属于本次必填交付物；规划和评审不得把它升级为新增完成条件，文档节点为只读时必须通过 result 端口交付完整文档。'''
  assert all(step.get('confirmation','automatic')=='automatic' for step in workflow['steps'])
  published={'id':defaults['workflow_ref']};p=c.rpc('query.workspace')['active_project']
  configuration=c.command('project.configuration.update',project,p['revision'],{'workflow_ref':published['id'],'role_agent_bindings':defaults['role_agent_bindings'],'orchestrator_role_ref':defaults['orchestrator_role_ref'],'permission_context_ref':defaults['permission_context_ref'],'orchestration':{'mode':'agent'}})
  feature=c.command('feature.create',payload={'project_id':project,'title':'Workflow v6 greeting '+suffix,'configuration_revision_id':configuration['id']})['id']
  (OUT/'context.json').write_text(json.dumps({'url':urls[0],'project':project,'feature':feature,'workflow':published['id'],'suffix':suffix,'goal':goal,'default_automatic_confirmations':True,'required_increment_pairs':2,'unattended':args.unattended},ensure_ascii=False,indent=2));print('Feature:',feature,flush=True)
  initial=c.rpc('query.workflow_instance',{'owner_id':feature});root_id=initial['record']['id']
  manager=c.command('orchestration.open',root_id,initial['record']['revision'],{})['id']
  m=c.rpc('query.orchestration',{'id':manager})['record']
 c.command('orchestration.input',manager,m['revision'],{'epoch':m['data']['epoch'],'input':'业务 Agent 不会继承你的控制会话。派发需求前先检查输入，必要时通过 workflow.change.preview/apply 将下列完整业务事实绑定到需求节点的明确输入；不得让业务节点因缺少已经给出的事实而等待人工补充。请按当前默认 Feature 工作流组织完整交付，包括 Task 子流程，使用原生工具明确派发、接纳、完成每个实例。遵守全部测试、独立评审及当前 Plan 的确认策略，不绕过、不豁免任何条件。默认 automatic 不创建人工签署，但仍须通过 Harness 显式接纳；只有实际冻结 Plan 为 human 时由验收入口处理精确签署。可使用 workflow.segment.dispatch 有界委托当前已知的自动步骤，逐一列出确切节点；每个子流程仍需你明确 complete。碰到拒绝、人工门禁或异常时重新决策，不得跨过。只运行一个 Task。业务目标：'+goal+'\n补充执行指导：'+args.guidance})
 last=None;confirmed=set();inputs=set();deadline=time.monotonic()+args.timeout_seconds;manager_progress=None;last_manager_progress=time.monotonic()
 while time.monotonic()<deadline:
  view=c.rpc('query.workspace',{'feature_id':feature});rows=[r for r in view['configurations'] if r['type']=='workflow_instance' and r['data']['root_instance_id']==root_id];root=next(r for r in rows if r['data']['parent_execution_id'] is None)
  status=[(r['id'],s['path'],s['status'],s['reason']) for r in rows for s in r['data']['steps'] if s['status'] not in ['completed','queued','skipped']]
  if status!=last:print(status,flush=True);last=status
  manager_view=c.rpc('query.orchestration',{'id':manager});(OUT/'latest-manager.json').write_text(json.dumps(manager_view,ensure_ascii=False,indent=2))
  progress=[(r['id'],r['data'].get('status')) for r in manager_view['calls']]
  if progress!=manager_progress:manager_progress=progress;last_manager_progress=time.monotonic()
  if manager_view['record']['data']['status'] in ['failed','needs_reconciliation','budget_exhausted']:raise RuntimeError(manager_view['record']['data'])
  if args.unattended:
   assert manager_view['record']['data']['status'] in ['active','closed'], 'Unattended controller paused for intervention'
   assert not any(r['type']=='workflow_pending_decision' and r['data']['status']=='pending' for r in manager_view['decisions']), 'Unattended workflow requested a Human decision'
  (OUT/'latest.json').write_text(json.dumps(view,ensure_ascii=False,indent=2))
  for row in rows:
   for step in row['data']['steps']:
    if step['status'] in ['failed','needs_reconciliation','cancelled']:
     if step['status']=='failed':
      # Observe the actual configured manager's recovery within its budget. The
      # probe never retries, changes a result, or relaxes a completion condition.
      first=failure_observed.setdefault(step['id'],time.monotonic())
      if time.monotonic()-max(first,last_manager_progress)<600:continue
     detail=c.rpc('query.workflow_instance',{'id':row['id']});(OUT/'failed-instance.json').write_text(json.dumps(detail,ensure_ascii=False,indent=2));raise RuntimeError((step['path'],step['status'],step['reason']))
    if step['status']=='waiting_input':
     if args.unattended:raise RuntimeError('Unattended workflow requested supplemental input: '+step['path'])
     key=(step['id'],step['sequence'])
     if key in inputs or step['sequence']>3:raise RuntimeError('Repeated clarification: '+step['path'])
     c.command('workflow.step.input',row['id'],row['revision'],{'step':step['step'],'input':goal+'\n当前流程已提供并接纳的输出引用：'+json.dumps({s['step']:s['outputs'] for s in row['data']['steps'] if s['outputs']},ensure_ascii=False)+'。本次使用 Workflow 端口材料，不存在另一个未声明的提案采纳过程。'});inputs.add(key);break
    if step['status']=='waiting_confirmation' and step['id'] not in confirmed:
     pending=c.rpc('query.workflow_instance',{'id':row['id']});(OUT/'pending-confirmation.json').write_text(json.dumps(pending,ensure_ascii=False,indent=2))
     if json.loads((OUT/'context.json').read_text()).get('default_automatic_confirmations'):raise RuntimeError('Default workflow unexpectedly requires Human confirmation: '+step['path'])
     definition=next(definition for definition in pending['plan']['steps'] if definition['key']==step['step'])
     rejected=[name for name,port in definition.get('outputs',{}).items() if port.get('content_checks') and pending['outputs'][step['outputs'][name]]['value']['approve'] is False]
     if rejected:raise RuntimeError('Content review rejected '+step['path']+': '+step['reason'])
     requests=[r for r in manager_view['decisions'] if r['definition_id']==row['id'] and r['data']['status']=='pending' and r['data']['plan_revision_id']==pending['runtime']['active_plan_revision_id'] and r['data']['request'].get('operation') in ('workflow.step.accept','workflow.step.confirm') and r['data']['request'].get('arguments',{}).get('execution_id')==step['id']]
     if requests:
      for request in requests:c.command('workflow.decision.resolve',request['id'],request['revision'],{'approved':True,'reason':'专用验收：核对确切执行、原始报告与实际工具回执后提供 Human 决定，不覆盖未满足条件。'})
     elif not pending['completion_status'][step['id']]['confirmation']:
      c.command('workflow.step.confirm',row['id'],pending['record']['revision'],{'plan_revision_id':pending['runtime']['active_plan_revision_id'],'control_version':pending['runtime']['control_version'],'step':step['step'],'input_digest':step['input_digest'],'outputs':step['outputs'],'reason':'专用验收：核对当前确切输入、模型报告及实际工具回执；不覆盖未满足条件。'})
     confirmed.add(step['id']);print('Confirmed:',step['path'],flush=True);break
  if root['data']['status']=='completed':
   assert view['detail']['feature']['status']=='completed';assert len(view['detail']['tasks'])==1 and view['detail']['tasks'][0]['status']=='completed'
   # The controller may still be finishing the native tool call that completed
   # the root. Let that turn settle before SIGINT can mark it unknown.
   settled=c.wait(lambda:c.rpc('query.orchestration',{'id':manager}),lambda state:state['record']['data']['status']=='closed',120)
   (OUT/'settled-manager.json').write_text(json.dumps(settled,ensure_ascii=False,indent=2))
   publications=verify_evidence(c,rows,OUT)
   print('PASS native default engineering Workflow',root['id'],flush=True)
   (OUT/'report.md').write_text('# Native default engineering Workflow v6\n\nPassed through actual Codex, managed Git, Docker Red/Green and candidate tests; both GitHub PRs merged at their exact candidate SHA.\n\nRoot: '+root['id']+'\nFeature: '+feature+'\nNew confirmations in this invocation: '+str(len(confirmed))+'\nPRs: '+', '.join(str(publication['pull_request']['number']) for publication in publications)+'\n');break
  time.sleep(5)
 else:raise TimeoutError('Default engineering workflow')
except SystemExit:
 raise
except BaseException as error:
 (OUT/'failure.txt').write_text(str(error));raise
finally:
 prior_error=sys.exc_info()[0] is not None
 if child.poll() is None:child.send_signal(signal.SIGINT);child.wait(timeout=30)
 shutil.rmtree(OUT/'data/credentials',ignore_errors=True)
 for private_auth in (OUT/'data').rglob('auth.json'):private_auth.unlink(missing_ok=True)
 print('Evidence:',OUT,flush=True)

 if 'c' in globals() and c.project:
  try:
   archive=OUT/('project-evidence-'+str(time.time_ns()))
   manifest=export_project_evidence(OUT/'data',c.project,archive,private_binary)
   with (OUT/'evidence-index.jsonl').open('a') as index:index.write(json.dumps({'archive':str(archive),'project':c.project,'artifacts_verified':manifest['artifacts_verified']})+'\n')
   if not manifest['artifacts_verified']:raise RuntimeError('Archived artifact integrity failed')
  except Exception as archive_error:
   (OUT/'archive-failure.txt').write_text(str(archive_error)+'\n')
   if not prior_error:raise
