#!/usr/bin/env python3
"""Explicit real Codex/Git/Docker/GitHub acceptance of the explicit detailed engineering graph in an isolated dataset."""
import argparse,json,os,shutil,subprocess,tempfile,time,uuid,sys,signal,hashlib
from pathlib import Path
from client import Client

from engineering_evidence import verify_evidence
from summarize_engineering_scene import summarize
from export_evidence import export as export_project_evidence
from workflow_observer import manager_activity, read_workflow_tree, preserve_human_request, check_human_request
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'performance'))
from feature_frontend_load import FeatureFrontendLoad

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--detailed',action='store_true',required=True,help='Explicit opt-in to the preserved detailed suite; streamlined defaults use probe_streamlined_workflow.py')
parser.add_argument('--binary',type=Path,required=True)
parser.add_argument('--codex-auth-source',type=Path,required=True)
parser.add_argument('--github-repo',required=True)
parser.add_argument('--dedicated-repository',action='store_true',help='Explicitly designate this repository for test branches, commits and PRs')
parser.add_argument('--report-directory',type=Path)
parser.add_argument('--resume',action='store_true')
parser.add_argument('--driver',choices=['agent','rust'],default='agent')
parser.add_argument('--unattended',action='store_true',help='Fresh Agent run: fail on supplemental input or any Human gate; no probe intervention')
parser.add_argument('--guidance',default='')
parser.add_argument('--timeout-seconds',type=int,default=10800)
parser.add_argument('--manager-profile-source',type=Path,help='Explicit immutable Harness Profile source to adopt during stopped-controller recovery')
parser.add_argument('--requirement-file',type=Path,help='Initial business requirement; $IMAGE and $SUFFIX are replaced before the single start')
parser.add_argument('--module-path',help='Existing module changed by the requirement, for exact candidate evidence')
parser.add_argument('--frontends',type=int,choices=[0,100],default=0,help='Attach independent HTTP/SSE traffic; never controls the manager')
args=parser.parse_args()
assert args.manager_profile_source is None or args.resume, 'manager context adoption is a resume operation'
assert args.timeout_seconds>0
assert not args.guidance, 'Autonomous acceptance cannot receive execution guidance'
assert not (args.resume and args.unattended), 'unattended acceptance requires a fresh dataset'
assert not (args.driver=='rust' and args.unattended), 'Rust actor verification is separate from the unattended Agent controller contract'
assert args.dedicated_repository or 'test' in args.github_repo.lower(), 'designate the dedicated test repository explicitly'

OUT=args.report_directory or Path(tempfile.mkdtemp(prefix='devdesk-engineering-v5-'));OUT.mkdir(parents=True,exist_ok=True);OUT=OUT.resolve();print('Evidence:',OUT,flush=True)
assert args.resume == (OUT/'context.json').exists(), 'select --resume for an existing acceptance dataset'
if (OUT/'failure.txt').exists():(OUT/'failure.txt').rename(OUT/('failure-before-launch-'+str(time.time_ns())+'.txt'))
private_binary=OUT/'devdesk-host';shutil.copy2(args.binary.resolve(),private_binary)
binary_digest=hashlib.sha256(private_binary.read_bytes()).hexdigest()
binary_archive=OUT/'binaries'/binary_digest/'devdesk-host'
if binary_archive.exists():
 assert hashlib.sha256(binary_archive.read_bytes()).hexdigest()==binary_digest, 'Preserved acceptance binary changed'
else:
 binary_archive.parent.mkdir(parents=True)
 shutil.copy2(private_binary,binary_archive)
attempt=OUT/'attempts'/str(time.time_ns());attempt.mkdir(parents=True)
for name in [Path(__file__).name,'client.py','engineering_evidence.py','autonomous_orchestration.py','export_evidence.py','summarize_engineering_scene.py','workflow_observer.py']:shutil.copy2(Path(__file__).parent/name,attempt/name)
for name in ['native_frontend_load.py','feature_frontend_load.py']:shutil.copy2(Path(__file__).resolve().parents[1]/'performance'/name,attempt/name)
(attempt/'ATTEMPT.json').write_text(json.dumps({'resume':args.resume,'unattended':args.unattended,'timeout_seconds':args.timeout_seconds,'binary_sha256':binary_digest,'binary_archive':str(binary_archive.relative_to(OUT)),'guidance':args.guidance,'driver':args.driver,'frontends':args.frontends},ensure_ascii=False,indent=2))
def verify_rust_actor(project):
 import sqlite3
 with sqlite3.connect(OUT/'data/projects'/project/'engineering.sqlite3') as db:
  assert db.execute("SELECT count(*) FROM governance_records WHERE type='workflow_controller'").fetchone()[0]==0
  decisions=[json.loads(row[0]) for row in db.execute("SELECT data FROM governance_records WHERE type='workflow_decision'")]
  actual=[d for d in decisions if d['operation'] in ['dispatch','accept','complete']]
  assert actual and all(d['actor']['kind']=='driver' for d in actual), 'Business advancement did not belong to the Rust driver'
 (OUT/'driver-actor-validation.json').write_text(json.dumps({'passed':True,'workflow_controller_count':0,'business_actions':len(actual),'actor_kind':'driver'},indent=2)+'\n')

if (OUT/'host.log').exists():(OUT/'host.log').rename(OUT/('host-before-launch-'+str(time.time_ns())+'.log'))
child=subprocess.Popen([str(private_binary),'--data',str(OUT/'data'),'--port','0'],stdout=(OUT/'host.log').open('w'),stderr=subprocess.STDOUT)
frontend_load=None
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
  context=json.loads((OUT/'context.json').read_text());project=context['project'];feature=context['feature'];goal=context['goal'];c.project=project
  assert context.get('driver',args.driver)==args.driver, 'Select the driver recorded by the preserved scene'
  root_view=c.rpc('query.workflow_instance',{'owner_id':feature});root_id=root_view['record']['id']
  if root_view['runtime']['status']=='completed':
   # Completed scenes are evidence-only. No controller lookup/takeover, model
   # dispatch, plan edit, result acceptance or publication is performed here.
   completed=c.rpc('query.workspace',{'feature_id':feature})
   assert completed['detail']['feature']['status']=='completed' and len(completed['detail']['tasks'])==1 and completed['detail']['tasks'][0]['status']=='completed'
   rows=[r for r in completed['configurations'] if r['type']=='workflow_instance' and r['data']['root_instance_id']==root_id]
   publications=verify_evidence(c,rows,OUT)
   if args.driver=='rust':verify_rust_actor(project)
   context.setdefault('evidence_revalidations',[]).append({'time_ns':time.time_ns(),'binary_sha256':binary_digest,'classification':'completed scene; evidence-only revalidation; no business resumption'})
   (OUT/'context.json').write_text(json.dumps(context,ensure_ascii=False,indent=2))
   (OUT/'report.md').write_text('# Native default engineering Workflow\n\nPassed actual Codex, managed Git, Docker Red/Green, independent reviews, the frozen confirmation policy and explicit completion. Both GitHub PRs merged at their exact tested candidate SHA. This completed-scene invocation only revalidated preserved facts; it did not take over, dispatch, accept or republish. Original verifier failures remain preserved.\n\nDriver: '+args.driver+'\nRoot: '+root_id+'\nFeature: '+feature+'\nPRs: '+', '.join(str(publication['pull_request']['number']) for publication in publications)+'\n')
   print('PASS completed native default engineering Workflow evidence; no takeover or redispatch',flush=True)
   sys.exit(0)
  assert args.driver=='agent', 'Interrupted Rust recovery requires explicit effect reconciliation; --resume only revalidates a completed Rust scene'
  c.open(project)
  context.setdefault('initially_unattended',context.get('unattended',False));context['unattended']=False
  context.setdefault('resumptions',[]).append({'time_ns':time.time_ns(),'binary_sha256':binary_digest,'classification':'same-dataset recovery; not an uninterrupted unattended run'})
  (OUT/'context.json').write_text(json.dumps(context,ensure_ascii=False,indent=2))
  manager=c.rpc('query.orchestration',{'root_instance_id':root_id})['record']['id']
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
  if args.manager_profile_source:
   source=json.loads(args.manager_profile_source.read_text());assert source['type']=='permission_profile'
   selected_context=source['identity']+':'+str(source['revision'])
   catalog=c.rpc('query.configurations',{'local':True,'kind':'permission_profile','include_history':True,'limit':100})['records'];existing=next((r for r in catalog if r['id']==selected_context),None)
   if existing:
    assert existing['data']==source['data'] and existing['scope']==source['scope'],'Existing context differs from the explicit source'
   else:
    change={'operations':[{'type':'create','source':source}]};preview=c.rpc('configuration.change.preview',{'changeset':change});applied=c.command('configuration.change.apply',payload={'changeset':change,'preview_hash':preview['preview_hash']});(attempt/'manager-context-import.json').write_text(json.dumps({'source':source,'preview':preview,'receipt':applied},ensure_ascii=False,indent=2))
   shutil.copy2(args.manager_profile_source,attempt/'manager-context-source.json')
  manager_row=c.rpc('query.orchestration',{'id':manager})['record'];resumed=c.command('orchestration.resume',manager,manager_row['revision'],{'epoch':manager_row['data']['epoch'],**({'harness_permission_ref':selected_context} if selected_context else {})});manager=resumed['id']
  context['resumptions'][-1]['controller_id']=manager;context['resumptions'][-1]['harness_permission_ref']=resumed['controller']['harness_permission_ref'];(OUT/'context.json').write_text(json.dumps(context,ensure_ascii=False,indent=2))
  m=c.rpc('query.orchestration',{'id':manager})['record'];print('Resumed preserved root:',root_id,flush=True)
 else:
  project=c.command('project.open_github',payload={'repository':args.github_repo,'mainline':'main','title':'Core engineering acceptance'})['id'];c.project=project
  c.wait(lambda:c.rpc('query.workspace'),lambda v:v['repository_ready'],300)
  defaults=c.rpc('query.configuration_defaults');defaults.update({'workflow_ref':'builtin-workflow-feature-sol-high:11','task_workflow_ref':'builtin-workflow-task-sol-high:10','standard_ref':'builtin-standard-engineering:9','role_agent_bindings':{f'builtin-role-{role}:1':f'builtin-agent-sol-high-{role}:{13 if role=="project-manager" else 6}' for role in ['analyst','architect','project-manager','developer','tester','reviewer']}})
  selected_project=c.rpc('query.workspace')['active_project'];c.command('project.configuration.update',project,selected_project['revision'],{'workflow_ref':defaults['workflow_ref'],'standard_ref':defaults['standard_ref'],'sandbox_ref':defaults['sandbox_ref'],'role_agent_bindings':defaults['role_agent_bindings'],'orchestrator_role_ref':defaults['orchestrator_role_ref'],'harness_permission_ref':defaults['harness_permission_ref'],'orchestration':{'mode':args.driver}})
  runtime=c.rpc('runtime.prepare',{'provider':'codex'});image=runtime['artifact']['image_id']
  workflow=c.rpc('query.workflow',{'id':defaults['workflow_ref']})['data'];suffix=uuid.uuid4().hex[:8];module=f'src/workflow-v5-{suffix}/greeting.mjs';task_test=f'tests/workflow-v5-{suffix}-task/behavior.test.mjs';feature_test=f'tests/workflow-v5-{suffix}-feature/behavior.test.mjs';increment_test=f'test/workflow-v5-{suffix}-increments.test.mjs'
  goal=f'''在已有仓库新增无外部依赖的 ESM 模块 {module}，export greeting(name)。去掉字符串首尾空白，返回 Hello, <trimmed>!；空白字符串或非字符串参数抛出 TypeError。一个 Task 即可完成，稳定 key greeting，依赖为空。验收 AC-GREET：正常及带首尾空白输入返回问候；AC-INVALID：空白与非字符串输入抛出 TypeError。允许新增 {module}、Task 独立测试 {task_test}、Feature 独立测试 {feature_test} 及开发职责自有的增量测试 {increment_test}；可调整 README，禁止修改既有无关文件或增加辅助仓库文档。使用 node:test、node:assert/strict，不安装依赖、不联网。测试 import 为 ../../{module}。按两个功能增量交付：问候与 trim，以及无效输入拒绝；分别保留真实 TDD Red/Green 证据和测试源码快照。Task 配方运行 {task_test}；Feature 及最终候选配方运行 {task_test} 和 {feature_test}。固定 Node TestRecipe runner=node_test，image={image}，timeout_seconds=60。README 修改可选，不属于必填交付物。'''
  if args.requirement_file:goal=args.requirement_file.read_text().replace('$IMAGE',image).replace('$SUFFIX',suffix)
  if args.module_path:module=args.module_path
  assert {step['key'] for step in workflow['steps'] if step.get('confirmation')=='human'}=={'requirements_review','design_review','plan_review'}
  published={'id':defaults['workflow_ref']};p=c.rpc('query.workspace')['active_project']
  configuration=c.command('project.configuration.update',project,p['revision'],{'workflow_ref':published['id'],'standard_ref':defaults['standard_ref'],'sandbox_ref':defaults['sandbox_ref'],'role_agent_bindings':defaults['role_agent_bindings'],'orchestrator_role_ref':defaults['orchestrator_role_ref'],'harness_permission_ref':defaults['harness_permission_ref'],'orchestration':{'mode':args.driver}})
  feature=c.command('feature.create',payload={'project_id':project,'title':'Core greeting '+suffix,'initial_requirement':goal,'start_workflow':False,'configuration_revision_id':configuration['id']})['id']
  (OUT/'context.json').write_text(json.dumps({'url':urls[0],'project':project,'feature':feature,'workflow':published['id'],'suffix':suffix,'module_path':module,'feature_test':feature_test,'goal':goal,'default_automatic_confirmations':False,'expected_human_checkpoints':['requirements_review','design_review','plan_review'],'required_increment_pairs':2,'unattended':args.unattended,'autonomous':args.driver=='agent','driver':args.driver},ensure_ascii=False,indent=2));print('Feature:',feature,flush=True)
  c.command('workflow.start',feature,c.feature(feature)['feature']['revision'])
  initial=c.rpc('query.workflow_instance',{'owner_id':feature});root_id=initial['record']['id']
  manager=c.rpc('query.orchestration',{'root_instance_id':root_id})['record']['id'] if args.driver=='agent' else None
 if args.frontends:
  frontend_load=FeatureFrontendLoad(Client,urls[0],OUT/'frontend-load',args.frontends)
  frontend_load.attach(project,root_id)
  (OUT/'frontend-load/attachment.json').write_text(json.dumps({'started_at_ms':time.time()*1000,'native_project':project,'feature':feature,'root':root_id,'native_controls':0,'scope':'Independent Draft/Context owners; only read queries and foreign context.update. Finish and committed readbacks precede private Host shutdown.'},indent=2))
  frontend_load.begin(project)
 last=None;confirmed=set();shown_requests=set();inputs=set();deadline=time.monotonic()+args.timeout_seconds;manager_progress=None;last_manager_progress=time.monotonic()
 while time.monotonic()<deadline:
  tree=read_workflow_tree(c,root_id);rows=[v['record'] for v in tree.values()];root=tree[root_id]['record']
  status=[(r['id'],s['path'],s['status'],s['reason']) for r in rows for s in r['data']['steps'] if s['status'] not in ['completed','queued','skipped']]
  if status!=last:print(status,flush=True);last=status
  if manager:
   manager_view=c.rpc('query.orchestration',{'id':manager});(OUT/'latest-manager.json').write_text(json.dumps(manager_view,ensure_ascii=False,indent=2))
   progress=manager_activity(manager_view)
   if progress!=manager_progress:manager_progress=progress;last_manager_progress=time.monotonic()
   if manager_view['record']['data']['status'] in ['failed','needs_reconciliation','budget_exhausted'] or manager_view['record']['data'].get('run_state')=='needs_attention':raise RuntimeError(manager_view['record']['data'])
   if args.unattended:
    assert manager_view['record']['data']['status'] in ['active','closed'], 'Unattended controller paused for intervention'
    assert not any(r['type']=='workflow_pending_decision' and r['data']['status']=='pending' for r in manager_view['decisions']), 'Unattended workflow requested a Human decision'
  else:
   (OUT/'latest-driver.json').write_text(json.dumps(c.rpc('query.workflow_driver',{'id':root_id}),ensure_ascii=False,indent=2))
  (OUT/'latest.json').write_text(json.dumps({'configurations':rows},ensure_ascii=False,indent=2))
  for row in rows:
   for step in row['data']['steps']:
    if step['status'] in ['failed','needs_reconciliation','cancelled']:
     # Unknown/cancelled child executions also require the actual manager to
     # stop/reconcile before retrying. Do not interrupt it at that intermediate
     # state. The existing manager failure, inactivity and overall deadlines
     # still apply; the observer never issues any recovery business command.
     if step['id'] not in failure_observed:
      with (OUT/'observed-step-failures.jsonl').open('a') as stream:stream.write(json.dumps({'observed_at_ms':time.time()*1000,'instance_id':row['id'],'execution_id':step['id'],'status':step['status'],'failure':step.get('failure'),'reason':step.get('reason')},ensure_ascii=False)+'\n')
     first=failure_observed.setdefault(step['id'],time.monotonic())
     if manager and time.monotonic()-max(first,last_manager_progress)<600:continue
     detail=c.rpc('query.workflow_instance',{'id':row['id']});(OUT/'failed-instance.json').write_text(json.dumps(detail,ensure_ascii=False,indent=2));raise RuntimeError((step['path'],step['status'],step['reason']))
    if step['status']=='waiting_input':
     if args.unattended:raise RuntimeError('Workflow needs an actual human answer: '+step['path'])
     pending=tree[row['id']];(OUT/'pending-input.json').write_text(json.dumps(pending,ensure_ascii=False,indent=2))
     if manager:
      # The manager prepares the question and its materials; a real answer is
      # submitted through the manager page. This observer never speaks for it.
      continue
     answer=input('Human clarification required for '+step['path']+'\n'+step['reason']+'\nAnswer (empty stops acceptance): ').strip()
     if not answer:raise RuntimeError('Human answer was not provided')
     manager_record=c.rpc('query.orchestration',{'id':manager})['record']
     c.command('orchestration.input',manager,manager_record['revision'],{'epoch':manager_record['data']['epoch'],'input':answer});inputs.add((step['id'],step['sequence']));break
    if step['status']=='waiting_confirmation' and step['id'] not in confirmed:
     pending=tree[row['id']];(OUT/'pending-confirmation.json').write_text(json.dumps(pending,ensure_ascii=False,indent=2))
     if args.unattended or json.loads((OUT/'context.json').read_text()).get('default_automatic_confirmations'):raise RuntimeError('Workflow requires actual Human confirmation: '+step['path'])
     manager_view=c.rpc('query.orchestration',{'id':manager}) if manager else {'decisions':[]}
     requests=[r for r in manager_view['decisions'] if r['definition_id']==row['id'] and r['data']['status']=='pending' and r['data']['plan_revision_id']==pending['runtime']['active_plan_revision_id'] and r['data']['request'].get('operation') in ('workflow.step.accept','workflow.step.confirm') and r['data']['request'].get('arguments',{}).get('execution_id')==step['id']]
     if manager and not requests:continue
     for request in requests:
      packet=request['data']['request']['interaction']
      if request['id'] not in shown_requests:
       print('MANAGER HUMAN REQUEST '+request['id']+'\n'+json.dumps(packet,ensure_ascii=False,indent=2),flush=True)
       manager_url=urls[0]+'/?tab=execution&detail=manager&project_id='+project+'&feature_id='+feature
       print('Human reviews and decides in the manager page: '+manager_url,flush=True)
       preserve_human_request(OUT,request,manager_url,lambda saved:check_human_request(c,tree,saved))
       shown_requests.add(request['id'])
     if manager and not pending['completion_status'][step['id']]['confirmation']:continue
     if not manager:
      if input('Human decision for Rust workflow (type confirm): ').strip()!='confirm':raise RuntimeError('Human did not confirm')
      c.command('workflow.step.confirm',row['id'],pending['record']['revision'],{'plan_revision_id':pending['runtime']['active_plan_revision_id'],'step':step['step'],'execution_id':step['id'],'input_digest':step['input_digest'],'outputs':step['outputs'],'reason':'真实人类确认当前版本'})
     confirmed.add(step['id']);print('Confirmed:',step['path'],flush=True);break
  if root['data']['status']=='completed':
   view=c.rpc('query.execution_workspace',{'feature_id':feature})
   assert view['detail']['feature']['status']=='completed';assert len(view['detail']['tasks'])==1 and view['detail']['tasks'][0]['status']=='completed'
   # The controller may still be finishing the native tool call that completed
   # the root. Let that turn settle before SIGINT can mark it unknown.
   settled=c.wait(lambda:c.rpc('query.orchestration',{'id':manager}),lambda state:state['record']['data']['status']=='closed',120) if manager else c.rpc('query.workflow_driver',{'id':root_id})
   (OUT/'settled-manager.json').write_text(json.dumps(settled,ensure_ascii=False,indent=2))
   publications=verify_evidence(c,rows,OUT)
   if not manager:verify_rust_actor(project)
   print('PASS native default engineering Workflow',root['id'],flush=True)
   (OUT/'report.md').write_text('# Native default engineering Workflow\n\nPassed through actual Codex, managed Git, Docker Red/Green and candidate tests; both GitHub PRs merged at their exact candidate SHA.\n\nRoot: '+root['id']+'\nFeature: '+feature+'\nNew confirmations in this invocation: '+str(len(confirmed))+'\nPRs: '+', '.join(str(publication['pull_request']['number']) for publication in publications)+'\n');break
  time.sleep(5)
 else:raise TimeoutError('Default engineering workflow')
except SystemExit:
 raise
except BaseException as error:
 (OUT/'failure.txt').write_text(str(error));raise
finally:
 active_error=sys.exc_info()[1]
 prior_error=active_error is not None and not (isinstance(active_error,SystemExit) and active_error.code in (None,0))
 frontend_error=None
 if frontend_load:
  try:
   load_summary=frontend_load.finish()
   if load_summary['errors'] or load_summary['failures']:raise RuntimeError('Actual Feature frontend load failed; retained all original RPCs/readbacks')
  except BaseException as error:frontend_error=error;(OUT/'frontend-load/failure.txt').write_text(str(error))
  attachment=OUT/'frontend-load/attachment.json'
  if attachment.exists():
   metadata=json.loads(attachment.read_text());metadata.update(stopped_at_ms=time.time()*1000,stop_reason='workflow_failed' if prior_error else 'workflow_completed');attachment.write_text(json.dumps(metadata,indent=2))
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
   summarize(archive,OUT)
  except Exception as archive_error:
   (OUT/'archive-failure.txt').write_text(str(archive_error)+'\n')
   if not prior_error:raise
 if frontend_error and not prior_error:raise frontend_error
