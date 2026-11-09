#!/usr/bin/env python3
"""Explicit native Workflow acceptance on an existing dedicated GitHub test repository.
Only the final Markdown report is retained. Runtime evidence remains in the dataset.
"""
from client import Client
from pathlib import Path
import argparse,json,platform,subprocess,time,uuid
p=argparse.ArgumentParser();p.add_argument('--url',required=True);p.add_argument('--github-repo',required=True);p.add_argument('--provider',choices=['codex','dsh'],default='codex');p.add_argument('--report',type=Path,required=True);p.add_argument('--resume-project');p.add_argument('--resume-feature');a=p.parse_args()
assert a.github_repo!='xfork24/devdesk' and 'test' in a.github_repo.lower(),'dedicated test repository required'
c=Client(a.url);report=['# Workflow 与工作台原生验收','',f'日期：{time.strftime("%Y-%m-%d %H:%M:%S %z")}',f'宿主：{platform.system()} / {platform.machine()}',f'Harness：{a.provider}',''];feature=a.resume_feature
try:
 if not a.resume_project:
  project=c.command('project.open_github',payload={'repository':a.github_repo,'mainline':'main','title':'Workflow v0.3 native acceptance'})['id'];c.project=project
  attempts={}
  def connected():
   view=c.rpc('query.workspace')
   for op in c.rpc('query.repository')['operations']:
    if op['action']['kind']=='connect' and op['status']=='needs_reconciliation' and op.get('reason')!='reconcile_requested':
     count=attempts.get(op['id'],0)
     if count>=3:raise RuntimeError('仓库接入经三次原 operation 核对仍阻塞：'+op['reason'])
     attempts[op['id']]=count+1;c.command('repository.reconcile',op['id'],op['revision'])
   return view
  c.wait(connected,lambda v:v['repository_ready'],300)
  c.rpc('runtime.prepare',{'provider':a.provider})
  installations=c.rpc('query.runtime_installations')['installations'];runtime=next(r for r in reversed(installations) if r['key']['provider']==a.provider and r['artifact']['kind']=='docker_image')
  image=runtime['artifact']['image_id'];suffix=uuid.uuid4().hex[:8];module=f'src/workflow-{suffix}-greeting.mjs';testfile=f'tests/workflow-{suffix}/behavior.test.mjs';fulltest=f'tests/workflow-{suffix}-feature/behavior.test.mjs'
  defaults=c.rpc('query.configuration_defaults');agents=defaults['role_agent_bindings']
  if a.provider=='dsh':
   model=c.command('model.create',payload={'name':'Explicit DSH acceptance model','data':{'provider':'deepseek','model':'deepseek-v4-pro','parameters':{'max_tokens':8192},'capabilities':['structured-output']}})['id']
   harness=c.command('harness.create',payload={'name':'Explicit DSH acceptance Harness','data':{'provider':'dsh','runtime_version':runtime['key']['version'],'model_providers':['deepseek'],'capabilities':['structured-output','tool-calling']}})['id']
   for role,agent_id in list(agents.items()):
    agent=c.rpc('query.agent_revision',{'id':agent_id});data=agent['data'];cap=c.command('capability.create',payload={'name':'DSH '+role,'data':{'model_ref':model,'harness_ref':harness,'tools':data['tools'],'skills':data['skills']}})['id']
    agents[role]=c.command('agent.create',payload={'name':'DSH '+role,'data':{'role_ref':data['role_ref'],'capability_ref':cap,'permissions':data['permissions'],'approval':'never'}})['id']
  workflow=c.rpc('query.workflow',{'id':defaults['workflow_ref']})['data'];recipe=lambda f:{'runner':'node_test','files':[f],'image':image,'timeout_seconds':60}
  requirement=f'Implement one dependency-free ESM module {module}, exporting greeting(name). Trim surrounding whitespace, return "Hello, <trimmed name>!". Throw TypeError for blank or non-string input. Only this new module, its new test suites and README may change. Exactly one Task. Acceptance AC-GREET covers trimmed greeting; AC-INVALID covers blank and non-string TypeError. Tests use node:test and node:assert/strict. Test import uses dynamic import with catch and assert.fail so absent implementation gives an assertion Red, not an environment failure.'
  for step in workflow['steps']:
   if step['kind']=='agent.interact':
    step['interaction']['goal']=requirement+' Facts are fully agreed; no clarification needed. Return ONLY a complete ProposalContent JSON object. Document body is Markdown with ## headings and an explicit 错误与失败 section. Do not wrap the JSON in Markdown fences. Preserve current real document IDs. '
    if step['key']=='requirement_discussion':step['interaction']['goal']+='Output kind requirement, title/body and acceptance [{key:AC-GREET,description:trimmed greeting},{key:AC-INVALID,description:invalid input TypeError}].'
    if step['key']=='design_discussion':step['interaction']['goal']+='Output kind design; state exact module path and named export, implementation and independent tests, failure paths and no dependencies.'
    if step['key']=='task_discussion':step['interaction']['goal']+='Output kind task_plan with exactly one task key greeting, acceptance [AC-GREET,AC-INVALID], dependencies [], and test_recipe '+json.dumps(recipe(testfile))
    if step['key']=='test_discussion':step['interaction']['goal']+='Output kind test_plan, verifies [AC-GREET,AC-INVALID], feature test_recipe '+json.dumps(recipe(fulltest))+f'. The feature suite independently tests {module}; both suites import ../../{module}. It covers all acceptance and error paths.'
  # A real reusable Workflow call forwards the PRD proposal/session to the parent's adoption step.
  original=next(step for step in workflow['steps'] if step['key']=='requirement_discussion')
  child_step=json.loads(json.dumps(original));child_step['key']='prd';child_step['stage']='prd';child_step['dependencies']=[];child_step['inputs']={}
  child_doc={'schema_version':2,'process_kind':'subworkflow','stages':[{'key':'prd','name':'Reusable PRD','description':'# PRD\n需求分析师产出可采纳 PRD。'}],'steps':[child_step],'output_ports':{key:{'step':'prd','port':key} for key in ['proposal','session']}}
  child=c.command('workflow.create',payload={'name':'Reusable PRD '+suffix,'document':child_doc});child=c.command('workflow.publish',child['id'],child['revision'])
  original.update(kind='workflow.call',role_ref=None,participant_slot=None,spec_refs=[],subworkflow_ref=child['id'])
  draft=c.command('workflow.create',payload={'name':'Native greeting Workflow '+suffix,'scope':{'kind':'global'},'document':workflow});published=c.command('workflow.publish',draft['id'],draft['revision'])
  projectrow=c.rpc('query.workspace')['active_project'];configuration=c.command('project.configuration.update',project,projectrow['revision'],{'workflow_ref':published['id'],'role_agent_bindings':agents})
  feature=c.command('feature.create',payload={'project_id':project,'title':'Workflow greeting '+suffix,'require_configuration':True,'configuration_revision_id':configuration['id']})['id']
 else:
  project=a.resume_project;c.open(project)
 report+=['Project: '+project,'Feature: '+feature,'配置：v2；按阶段区分 Role、Capability、Markdown Skill 与 Spec；本次供应商：'+a.provider,''];a.report.parent.mkdir(parents=True,exist_ok=True);a.report.write_text('\n'.join(report)+'\n状态：运行中。\n')
 deadline=time.monotonic()+2400;last='';adopted=set();confirmed=set();reconciliations={}
 while time.monotonic()<deadline:
  v=c.rpc('query.workspace',{'feature_id':feature,'limit':100});instances=[r for r in v['configurations'] if r['kind']=='workflow_instance' and r['data']['feature_id']==feature and r['data']['status']!='cancelled'];root=next(r for r in instances if r['definition_id']==feature)
  repository=c.rpc('query.repository',{'limit':100})
  for operation in repository['operations']:
   if operation['status']=='needs_reconciliation' and operation['reason'] not in ['reconcile_requested','superseded_target'] and operation['action'].get('merge') in {m['id'] for m in v['detail']['merges']}:
    count=reconciliations.get(operation['id'],0)
    if count>=3:raise RuntimeError('仓库操作经三次原 ID 核对仍阻塞：'+operation['reason'])
    reconciliations[operation['id']]=count+1
    c.command('repository.reconcile',operation['id'],operation['revision']);report.append('- 按原 operation ID 核对仓库回执 '+operation['id'])
  active=[(r,s) for r in instances for s in r['data']['steps'] if s['status'] not in ['completed','pending','skipped']];state='; '.join(s['key']+':'+s['status'] for r,s in active)
  if state!=last:print(state,flush=True);last=state
  for r,step in active:
   if step['status']=='failed' or (step['status']=='blocked' and not step['reason'].startswith('等待')):
    raise AssertionError(json.dumps({'owner':r['definition_id'],'step':step,'failed_runs':[{'node':run['node'],'result':run['result']} for run in v['detail']['runs'] if run['status'] in ['failed','interrupted']]},ensure_ascii=False))
   if step['status']=='waiting_confirmation' and step['id'] not in confirmed:
    if step['key']=='merge_review' and r['definition_id']==feature:
     merge=next(m for m in v['detail']['merges'] if m['id']==step['outputs']['merge']);assert merge['status']!='completed' and merge['committed_revision'] is None,'mainline already advanced before confirmation'
    c.command('workflow.step.confirm',step['id'],step['revision'],{'input_hash':step['input_hash']});confirmed.add(step['id']);report.append('- 已在合入前确认精确候选及输入摘要。')
   if step['status']=='waiting_input' and step.get('outputs',{}).get('proposal'):
    proposal=step['outputs']['proposal']
    if proposal not in adopted:
     session=step['outputs'].get('session') or next((s['outputs'].get('session') for s in r['data']['steps'] if s['outputs'].get('proposal')==proposal and s['outputs'].get('session')),None)
     if session:
      detail=c.rpc('query.session',{'id':session});row=next(p for p in detail['proposals'] if p['id']==proposal)
      if row['status']=='draft':c.command('proposal.adopt',proposal,row['revision']);adopted.add(proposal);report.append('- 已采纳结构化提案 '+proposal)
  if root['data']['status']=='completed':
   assert v['detail']['feature']['status']=='completed';assert len(root['data']['task_members'])==1
   assert any(d['kind']=='requirement' and d['id'] in v['detail']['accepted_deliveries'] for d in v['detail']['deliveries'])
   report+=['','结果：通过。需求、方案、独立 Task TDD、候选评审、人工确认和主干合入由冻结 Workflow 驱动。','',f"完成轮次的确认数：{sum(1 for r in instances for s in r['data']['steps'] if s.get('confirmation'))}；正式提案引用数：{len({s['outputs']['proposal'] for r in instances for s in r['data']['steps'] if s['outputs'].get('proposal')})}。"];break
  time.sleep(1)
 else:raise TimeoutError('native Workflow deadline')
except BaseException as error:
 report+=['','结果：未通过。',str(error)];raise
finally:
 a.report.parent.mkdir(parents=True,exist_ok=True);a.report.write_text('\n'.join(report)+'\n')
