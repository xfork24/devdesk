"""Corroborate frozen contracts, incremental evidence and exact publications."""
import hashlib,json,os,re,sqlite3,subprocess
from pathlib import PurePosixPath

def verify_evidence(client, rows, output):
 views={row['id']:client.rpc('query.workflow_instance',{'id':row['id']}) for row in rows}
 root=next(v for v in views.values() if v['runtime']['parent_execution_id'] is None)
 children=[v for v in views.values() if v['runtime']['parent_execution_id'] is not None]
 assert len(children)==1, 'Expected one actual Task invocation'
 child=children[0];context=json.loads((output/'context.json').read_text());task_snapshots={}
 database=output/'data/projects'/root['runtime']['project_id']/'engineering.sqlite3'
 with sqlite3.connect(database.resolve().as_uri()+'?mode=ro',uri=True) as connection:
  assert connection.execute("SELECT count(*) FROM governance_records WHERE kind='workflow_waiver'").fetchone()[0]==0
  confirmations={key:json.loads(data) for key,data in connection.execute("SELECT definition_id,data FROM governance_records WHERE kind='workflow_confirmation'")}
 if context.get('unattended'):
  with sqlite3.connect(database.resolve().as_uri()+'?mode=ro',uri=True) as connection:
   assert connection.execute("SELECT count(*) FROM governance_records WHERE kind='controller_input'").fetchone()[0]==1, 'Unattended run received additional human steering'
 human_confirmations=0
 for view in views.values():
  assert view['runtime']['status']=='completed' and all(s['status']=='completed' for s in view['runtime']['steps'])
  assert not any(item['data'].get('bypassed') for item in view['history'] if item['kind']=='workflow_change')
  # Runtime workflow refs change; follow immutable revision ancestry rather than
  # trusting the mutable catalog or treating the revised graph as its own baseline.
  revision=view['runtime']['active_plan_revision_id'];seen=set()
  with sqlite3.connect(database.resolve().as_uri()+'?mode=ro',uri=True) as connection:
   while revision:
    assert revision not in seen,'Plan revision ancestry cycle'
    seen.add(revision)
    original=connection.execute("SELECT data FROM governance_records WHERE kind='workflow_plan_revision' AND id=?",(revision,)).fetchone()
    assert original is not None,'Original frozen plan must remain available'
    frozen=json.loads(original[0]);assert frozen['instance_id']==view['record']['id']
    source=frozen['plan']['document'];revision=frozen.get('parent_revision_id')
  for action,requirements in source.get('action_requirements',{}).items():
   assert set(requirements)<=set(view['plan']['action_requirements'].get(action,[]))
  current={s['key']:s for s in view['plan']['steps']}
  for original in source['steps']:
   assert original['key'] in current,'A mandatory source node was removed'
   actual=current[original['key']]
   assert set(original.get('dependencies',[]))<=set(actual['dependencies'])
   assert original.get('completion',{'op':'always'})==actual['completion']
   assert original.get('confirmation','automatic')==actual['confirmation']
   assert not actual.get('allow_completion_override',False)
   assert set(original.get('spec_refs',[]))<=set(actual['spec_refs'])
   assert set(original.get('required_skill_refs',[]))<=set(actual['required_skill_refs'])
   for key in ['type','tool_ref','role_ref','workflow_ref']:
    if key in original['executor']:assert original['executor'][key]==actual['executor'][key]
   for name,port in original.get('outputs',{}).items():
    assert name in actual['outputs'] and actual['outputs'][name]['delivery_ref']==port['delivery_ref']
    assert actual['outputs'][name]['required']==port.get('required',True)
    assert actual['outputs'][name]['content_checks']==port.get('content_checks',False)
  for definition in view['plan']['steps']:
   if definition['confirmation']!='human':continue
   human_confirmations+=1
   step=next(s for s in view['runtime']['steps'] if s['step']==definition['key']);decision=confirmations[step['id']]
   assert decision['input_digest']==step['input_digest'] and decision['outputs']==step['outputs'] and not decision.get('override_completion')
 if context.get('default_automatic_confirmations'):
  assert human_confirmations==0 and not confirmations, 'Default automatic policy must not create Human approvals'
  with sqlite3.connect(database.resolve().as_uri()+'?mode=ro',uri=True) as connection:
   assert connection.execute("SELECT count(*) FROM governance_records WHERE kind='workflow_human_decision' AND json_extract(data,'$.approved')=1").fetchone()[0]==0, 'No manual decision approval may advance the default workflow'
 def step(view,key):return next(s for s in view['runtime']['steps'] if s['step']==key)
 def receipt(view,key):
  current=step(view,key);assert current['status']=='completed'
  return view['outputs'][current['outputs']['result']]['value']
 def task(view,key):
  current=step(view,key);ident=current['turn_id']
  result=client.rpc('query.execution_task',{'turn_id':ident});task_snapshots[ident]=result
  return result['task']['task']
 def normalized(path,cwd='/work'):
  if path.startswith(cwd.rstrip('/')+'/'):path=path[len(cwd.rstrip('/'))+1:]
  path=path.removeprefix('/work/').removeprefix('./');value=PurePosixPath(path)
  assert path and not value.is_absolute() and '..' not in value.parts,path
  return str(value)
 repo=output/'data/projects'/root['runtime']['project_id']/'repository'
 assert repo.is_dir() and repo.resolve().is_relative_to((output/'data/projects').resolve())
 environment={k:v for k,v in os.environ.items() if not k.startswith('GIT_')};environment.update(GIT_CONFIG_GLOBAL='/dev/null',GIT_CONFIG_NOSYSTEM='1',GIT_TERMINAL_PROMPT='0')
 def blob(revision,path):
  assert re.fullmatch('[0-9a-f]{40,64}',revision)
  return subprocess.run(['git','-C',str(repo),'-c','core.fsmonitor=false','show','--no-textconv',revision+':'+normalized(path)],check=True,capture_output=True,timeout=10,env=environment).stdout
 red=receipt(child,'red');green=receipt(child,'green');verified=receipt(child,'verify')
 assert red['status']=='succeeded' and red['expected_failure'] and red['assertion_failed'] and red['counts']['failed']>0 and red['container_id']
 for actual in [green,verified]:assert actual['status']=='succeeded' and actual['exit_code']==0 and actual['container_id'] and not actual['expected_failure']
 assert verified['candidate']==green['candidate']
 red_files={f['path']:f for f in red['execution']['files']};green_files={f['path']:f for f in green['execution']['files']}
 assert red_files and red_files==green_files,'Independent tests changed between platform Red and Green'
 for f in red_files.values():assert blob(red['candidate'],f['path'])==f['content'].encode() and blob(green['candidate'],f['path'])==f['content'].encode()
 completed=receipt(child,'complete');assert completed['status']=='completed' and completed['task_id']==child['runtime']['inputs']['item']['id']
 implementation=step(child,'implementation');evidence_id=implementation['outputs']['evidence'];evidence=child['outputs'][evidence_id]['value'];runs=evidence['runs']
 definition=next(s for s in child['plan']['steps'] if s['key']=='implementation');delivery=definition['outputs']['evidence']['delivery_ref']
 required=child['runtime']['plan']['definitions'][delivery]['data']['content_schema']['properties']['runs']['items']['required']
 assert len(runs)>=2 and [r['sequence'] for r in runs]==sorted({r['sequence'] for r in runs})
 reds=[i for i,r in enumerate(runs) if r['phase']=='red'];greens=[i for i,r in enumerate(runs) if r['phase']=='green' and r['exit_code']==0]
 assert reds and greens and min(reds)<max(greens)
 for run in runs:
  assert set(required)<=set(run) and run['command'] and run['cwd'] and isinstance(run['stdout'],str) and isinstance(run['stderr'],str)
  if run['phase']=='red':assert run['exit_code'] not in (0,None)
  if 'environment' in required:
   assert {'runtime','version','platform','arch'}<=set(run['environment'])
   assert all(isinstance(run['environment'][k],str) and run['environment'][k] for k in ['runtime','version','platform','arch'])
   assert {'status','signal','error','timed_out','cancelled'}<=set(run['termination'])
   if run['phase']=='green':assert run['exit_code']==0 and run['termination']=={'status':'exited','signal':None,'error':None,'timed_out':False,'cancelled':False}
  for f in run['files']:
   assert hashlib.sha256(f['content'].encode()).hexdigest()==f['sha256'],f['path'];normalized(f['path'],run['cwd'])
 pairs=0;pending=False
 for run in runs:
  if run['phase']=='red':pending=True
  elif run['phase']=='green' and run['exit_code']==0 and pending:pairs+=1;pending=False
 assert pairs>=context.get('required_increment_pairs',1),'Missing separately recorded incremental Red/Green pairs'
 # Older frozen contracts explicitly copied the platform Red. Incremental
 # contracts additionally preserve local intermediate states, which need not
 # have been committed as the platform baseline.
 platform_copy=any(all(run.get(k)==v for k,v in red['execution'].items()) for run in runs if run['phase']=='red')
 if 'environment' not in required:assert platform_copy,'Original frozen platform Red execution was changed'
 else:
  diagnostic=red['report'];assert 'ERR_MODULE_NOT_FOUND' not in diagnostic,'Loading failure cannot replace business Red'
  assert blob(red['candidate'],'src/workflow-v5-'+context['suffix']+'/greeting.mjs')
 for key in ['documentation','review','candidate_review']:
  current=step(child,key);rendered=task(child,key)
  assert current['inputs']['evidence']==rendered['inputs']['evidence']==evidence
  assert rendered['input_sources']['evidence']['source_output_id']==evidence_id
 assert receipt(child,'handoff')['inputs']['evidence']==evidence
 handed_off=receipt(root,'implementation')
 def contains_evidence(value):
  if value==evidence:return True
  if isinstance(value,dict):return any(contains_evidence(v) for v in value.values())
  if isinstance(value,list):return any(contains_evidence(v) for v in value)
  return False
 assert contains_evidence(handed_off),'Task handoff dropped raw TDD evidence'
 for key in ['closure_review','candidate_review']:
  current=step(root,key);rendered=task(root,key)
  assert current['inputs']['tasks']==rendered['inputs']['tasks']==handed_off
  assert rendered['input_sources']['tasks']['source_output_id']==step(root,'implementation')['outputs']['result']
  assert contains_evidence(rendered['inputs']['tasks']),'Feature reviewer did not actually receive complete TDD evidence'
 diagnostic_red=red
 if any(s['step']=='red_diagnostics' for s in child['runtime']['steps']):
  diagnostic_red=receipt(child,'red_diagnostics')
  assert diagnostic_red['expected_failure'] and all(diagnostic_red[k]==red[k] for k in ['candidate','image_digest','recipe']) and diagnostic_red['execution']['files']==red['execution']['files']
  for key in ['review','candidate_review']:assert step(child,key)['inputs']['red_diagnostics']==diagnostic_red
  assert receipt(child,'handoff')['inputs']['red_diagnostics']==diagnostic_red
  failures=[json.loads(line) for line in diagnostic_red['report'].splitlines() if json.loads(line).get('event')=='fail']
  assert any('ERR_MODULE_NOT_FOUND' in json.dumps(r.get('diagnostic')) and 'greeting.mjs' in json.dumps(r.get('diagnostic')) for r in failures)
 publications=[];last_green=runs[greens[-1]]
 for view in [child,root]:
  candidate=receipt(view,'candidate');tests=receipt(view,'candidate_tests');published=receipt(view,'publish')
  assert candidate['parents']==[candidate['target_revision']] and candidate['diff']
  assert tests['status']=='succeeded' and tests['candidate']==candidate['revision'] and tests['exit_code']==0 and tests['container_id'] and tests['image_digest']==red['image_digest']
  expected_files=set(red_files)
  if view is root:expected_files.add('tests/workflow-v5-'+context['suffix']+'-feature/behavior.test.mjs')
  assert set(tests['recipe']['files'])==expected_files=={f['path'] for f in tests['execution']['files']}
  for f in tests['execution']['files']+last_green['files']:
   assert hashlib.sha256(f['content'].encode()).hexdigest()==f['sha256']
   path=normalized(f['path'],last_green['cwd']);assert blob(candidate['revision'],path)==f['content'].encode(),'Published candidate differs from tested final files: '+path
  allowed={normalized(f['path'],last_green['cwd']) for f in last_green['files']}|expected_files|{'README.md'}
  changed=subprocess.run(['git','-C',str(repo),'-c','core.fsmonitor=false','diff','--name-only',candidate['target_revision'],candidate['revision'],'--'],check=True,capture_output=True,timeout=10,env=environment).stdout.decode().splitlines()
  assert changed and set(changed)<=allowed,'Candidate exceeds the agreed scope'
  review_key='review' if view is child else 'closure_review';snapshot_key='implementation_snapshot' if view is child else 'tests_snapshot'
  review=step(view,review_key)
  if 'candidate' in review['inputs']:
   rendered=task(view,review_key);changes=receipt(view,snapshot_key)['changes']
   assert rendered['inputs']['candidate']==review['inputs']['candidate']==candidate
   assert rendered['inputs']['candidate_tests']==review['inputs']['candidate_tests']==tests
   assert rendered['inputs']['changes']==review['inputs']['changes']==changes and changes['revision']==candidate['source_revision']
  pr=published['pull_request'];assert pr['merged'] and pr['merge_commit']==pr['head']==candidate['revision'];publications.append(published)
 for ident,view in views.items():(output/(ident+'.json')).write_text(json.dumps(view,ensure_ascii=False,indent=2))
 (output/'execution-tasks.json').write_text(json.dumps(task_snapshots,ensure_ascii=False,indent=2))
 (output/'raw-tdd-evidence.json').write_text(json.dumps(evidence,ensure_ascii=False,indent=2))
 (output/'publication-evidence.json').write_text(json.dumps(publications,ensure_ascii=False,indent=2))
 validation={'passed':True,'runs':len(runs),'increment_pairs':pairs,'unchanged_test_files':sorted(red_files),'platform_red_copied':platform_copy,'declared_run_fields':required,'no_bypass_or_waivers':True,'human_confirmation_count':human_confirmations,'human_confirmations_match_exact_results':True,'consumer_task_provenance_matches':True,'diagnostic_operation_id':diagnostic_red['operation_id'],'final_candidate_files_match_evidence':True,'independent_feature_test_included':True,'red_revision':red['candidate'],'green_revision':green['candidate']}
 (output/'raw-evidence-validation.json').write_text(json.dumps(validation,ensure_ascii=False,indent=2));return publications
