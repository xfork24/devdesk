#!/usr/bin/env python3
"""Real application repository registration and feature worktree creation; no remote writes."""
import argparse,http.cookiejar,json,time,urllib.request,uuid,subprocess
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--url',default='http://127.0.0.1:51751');p.add_argument('--output',type=Path,default=Path('docs/plans/real-engineering-integration/R4-repository.json'));p.add_argument('--reports',type=Path,default=Path('docs/plans/real-engineering-integration'));p.add_argument('--github-repo',default='xfork24/devdesk-integration-test');a=p.parse_args();a.output=a.reports/a.output.name if a.output.parent==Path('docs/plans/real-engineering-integration') else a.output
c=urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()));c.open(a.url+'/').close()
def rpc(method,params=None):
 request=urllib.request.Request(a.url+'/rpc',data=json.dumps({'jsonrpc':'2.0','id':1,'method':method,'params':params or {}}).encode(),headers={'Content-Type':'application/json'});result=json.load(c.open(request,timeout=40))
 if 'error' in result:raise RuntimeError(result['error']['data']['code'])
 return result['result']
def command(method,target=None,revision=None,payload=None):return rpc(method,{'project_id':project,'command_id':str(uuid.uuid4()),'target':target,'expected_revision':revision,'payload':payload or {}})
report={'passed':False,'remote_writes':False,'stage':'github_policy'}
try:
 policy=rpc('github.diagnose',{'repository':a.github_repo});assert policy['compatible'];report['compatible_repository']=policy
 protected=rpc('github.diagnose',{'repository':'openai/codex'});assert not protected['compatible'] and protected['active_branch_rulesets'];report['incompatible_policy_probe']={'repository':protected['repository'],'compatible':False,'active_branch_rulesets':protected['active_branch_rulesets'],'read_only':True}
 source=json.loads((a.reports/'R5-browser-proposal.json').read_text());project=source['project_id'];report['project_id']=project
 command('project.open',project);view=rpc('query.workspace',{'project_id':project});state=rpc('query.repository',{'project_id':project})
 if not state['operations']:
  receipt=command('repository.connect',project,view['active_project']['revision'],{'repository':a.github_repo,'mainline':'main'});report['operation_id']=receipt['id']
 report['stage']='repository_connect';deadline=time.monotonic()+90
 while time.monotonic()<deadline:
  state=rpc('query.repository',{'project_id':project});op=next(op for op in state['operations'] if op['action']['kind']=='connect')
  if op['status'] in ['completed','needs_reconciliation']:break
  time.sleep(.5)
 assert op['status']=='completed',op['reason'];report['operation_id']=op['id'];assert state['project']['execution_mode']=='live';assert op['receipt']['policy']['compatible']
 assert len(state['worktrees'])==1
 report['stage']='feature_worktree';feature=command('feature.create',payload={'project_id':project,'title':'Real arithmetic feature','source_requirement':source['requirement_id']})['id'];report['feature_id']=feature
 deadline=time.monotonic()+40
 while time.monotonic()<deadline:
  state=rpc('query.repository',{'project_id':project});work=next((w for w in state['worktrees'] if w['owner_id']==feature),None)
  if work:break
  time.sleep(.5)
 assert work
 branch=subprocess.check_output(['git','-C',work['path'],'symbolic-ref','HEAD'],text=True).strip();assert branch=='refs/heads/'+work['branch']
 head=subprocess.check_output(['git','-C',work['path'],'rev-parse','HEAD'],text=True).strip();assert head==work['base']==op['receipt']['head']
 report['worktrees']=[{'kind':w['kind'],'owner':w['owner_id'],'branch':w['branch'],'base':w['base']} for w in state['worktrees']];report['passed']=True;report['stage']='finished'
except Exception as e:report['failure_type']=type(e).__name__;report['failure_code']=str(e) if isinstance(e,(RuntimeError,AssertionError)) else 'local diagnostic'
a.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2));raise SystemExit(0 if report['passed'] else 1)
