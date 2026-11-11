#!/usr/bin/env python3
"""Read-only source binaries; all dataset changes stay in the report directory."""
import json,shutil,subprocess,sys,time,uuid,signal,hashlib
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from client import Client
import argparse,tempfile
ROOT=Path(__file__).resolve().parents[2]
parser=argparse.ArgumentParser(description="Upgrade a complete factory dataset produced by an actual Workflow v4 Host.")
parser.add_argument('--legacy-binary',type=Path,required=True)
parser.add_argument('--binary',type=Path,required=True)
parser.add_argument('--report-directory',type=Path)
args=parser.parse_args()
out=(args.report_directory or Path(tempfile.mkdtemp(prefix='devdesk-genuine-v4-upgrade-'))).resolve();out.mkdir(exist_ok=True,parents=True)
legacy=out/'devdesk-v4';shutil.copy2(args.legacy_binary.resolve(),legacy)
modern=out/'devdesk-v5';shutil.copy2(args.binary.resolve(),modern)
data=out/'data'
def boot(binary):
 log=out/(binary.name+'-'+str(time.time_ns())+'.log');p=subprocess.Popen([str(binary),'--data',str(data),'--port','0'],stdout=log.open('w'),stderr=subprocess.STDOUT,cwd=out)
 for _ in range(200):
  lines=[line for line in log.read_text().splitlines() if line.startswith('http://')]
  if lines:return p,Client(lines[0])
  if p.poll() is not None:raise RuntimeError(log.read_text())
  time.sleep(.1)
 raise TimeoutError('boot')
def stop(p):
 if p.poll() is None:p.send_signal(signal.SIGINT);p.wait(timeout=30)
p,c=boot(legacy)
try:
 d=c.rpc('query.configuration_defaults');assert next(r for r in d['records'] if r['id']==d['workflow_ref'])['data']['schema_version']==4
 c.command('delivery_type.create',payload={'name':'旧自定义交付','data':{'format':'markdown','body':'真实旧定义'}})
 preview=c.rpc('query.template_reset_preview');c.command('configuration.templates.reset',payload={'expected_catalog_generation':preview['generation'],'catalog_fingerprint':preview['catalog_fingerprint'],'bundle_hash':preview['bundle_hash']})
 assert c.rpc('query.template_catalog')['generation']==2
 project=c.command('project.create',payload={'title':'真正旧程序创建的项目'})['id'];c.project=project
 doc={'schema_version':4,'stages':[{'key':'work','name':'保留阶段'}],'steps':[{'key':'gate','name':'原人工门禁','stage':'work','executor':{'type':'control'},'confirmation':'human'},{'key':'next','name':'后续工作','stage':'work','executor':{'type':'control'},'dependencies':['gate']}]}
 def command(method,target=None,revision=None,payload=None):return c.rpc(method,{'command_id':str(uuid.uuid4()),'target':target,'expected_revision':revision,'expected_catalog_generation':2,'payload':payload or {}})
 draft=command('workflow.create',payload={'name':'旧真实流程','document':doc});wf=command('workflow.publish',draft['id'],draft['revision'])
 pr=c.rpc('query.workspace')['active_project'];d=c.rpc('query.configuration_defaults');run=command('workflow.start',project,pr['revision'],{'workflow_ref':wf['id'],'role_agent_bindings':d['role_agent_bindings']})['id']
 for _ in range(60):
  old=c.rpc('query.workflow_instance',{'id':run})
  if old['runtime']['steps'][0]['status']=='waiting_confirmation':break
  time.sleep(.1)
 else:raise TimeoutError('legacy gate')
 (out/'before.json').write_text(json.dumps(old,ensure_ascii=False,indent=2));(out/'context.json').write_text(json.dumps({'project':project,'run':run}))
finally:stop(p)
sentinel=data/'credentials/retained.txt';sentinel.parent.mkdir(exist_ok=True);sentinel.write_text('真实旧数据的凭据哨兵')
args=[sys.executable,str(ROOT/'scripts/upgrade/harness-v5.py'),str(data),'--binary',str(modern)]
subprocess.run(args,check=True);subprocess.run(args+['--apply','--backup-directory',str(out/'backup')],check=True)
p,c=boot(modern)
try:
 c.open(project);catalog=c.rpc('query.template_catalog');assert catalog['generation']==2
 d=c.rpc('query.configuration_defaults');(out/'defaults.json').write_text(json.dumps(d,ensure_ascii=False,indent=2))
 listed=c.rpc('query.configurations',{'limit':1000,'include_history':False});visible={r['id'] for r in listed['records']}
 assert d['workflow_ref'] in visible,('default workflow hidden',d['workflow_ref'])
 assert d['role_agent_bindings'][d['orchestrator_role_ref']] in visible,('manager hidden',d['role_agent_bindings'][d['orchestrator_role_ref']])
 view=c.rpc('query.workflow_instance',{'id':run});assert view['runtime']['schema_version']==5;assert view['runtime']['steps'][0]['status']=='waiting_confirmation'
 assert view['runtime']['steps'][1]['status']=='queued'
 for key in ('gate','next'):
  view=c.rpc('query.workflow_instance',{'id':run});row=view['record'];ex=next(s for s in view['runtime']['steps'] if s['step']==key)
  payload={'plan_revision_id':view['runtime']['active_plan_revision_id'],'control_version':view['runtime']['control_version'],'step':key}
  if key=='gate':c.command('workflow.step.confirm',run,row['revision'],{**payload,'input_digest':ex['input_digest'],'outputs':ex['outputs']})
  else:c.command('workflow.step.dispatch',run,row['revision'],payload)
  view=c.rpc('query.workflow_instance',{'id':run});ex=next(s for s in view['runtime']['steps'] if s['step']==key)
  c.command('workflow.step.accept',run,view['record']['revision'],{'plan_revision_id':view['runtime']['active_plan_revision_id'],'step':key,'execution_id':ex['id'],'input_digest':ex['input_digest'],'outputs':ex['outputs'],'reason':'真实旧实例继续接纳'})
 view=c.rpc('query.workflow_instance',{'id':run});c.command('workflow.complete',run,view['record']['revision'],{'plan_revision_id':view['runtime']['active_plan_revision_id'],'reason':'旧实例显式完成'})
 (out/'after.json').write_text(json.dumps(c.rpc('query.workflow_instance',{'id':run}),ensure_ascii=False,indent=2))
 assert sentinel.read_text()=='真实旧数据的凭据哨兵'
 (out/'report.md').write_text('# Genuine v4 dataset upgrade\n\nPassed full factory generation 2, custom workflow waiting Human confirmation, immutable archive, retained sentinel, explicit continuation and completion.\n')
 print('PASS genuine v4 factory dataset upgrade',out,flush=True)
finally:stop(p)
