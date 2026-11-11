"""Corroborate native engineering facts, raw files and exact publication versions."""
import hashlib,json,os,re,sqlite3,subprocess
from pathlib import PurePosixPath

def verify_evidence(client, rows, output):
 views={row['id']:client.rpc('query.workflow_instance',{'id':row['id']}) for row in rows}
 root=next(view for view in views.values() if view['runtime']['parent_execution_id'] is None)
 children=[view for view in views.values() if view['runtime']['parent_execution_id'] is not None]
 assert len(children)==1, 'Expected one actual Task invocation'
 database=output/'data/projects'/root['runtime']['project_id']/'engineering.sqlite3'
 with sqlite3.connect(database.resolve().as_uri()+'?mode=ro',uri=True) as connection:
  assert connection.execute("SELECT count(*) FROM governance_records WHERE kind='workflow_waiver'").fetchone()[0]==0, 'Engineering acceptance must retain every gate'
  confirmations={key:json.loads(data) for key,data in connection.execute("SELECT definition_id,data FROM governance_records WHERE kind='workflow_confirmation'")}
 human_confirmations=0
 for view in views.values():
  assert view['runtime']['status']=='completed' and all(step['status']=='completed' for step in view['runtime']['steps'])
  assert not any(item['data'].get('bypassed') for item in view['history'] if item['kind']=='workflow_change')
  for definition in view['plan']['steps']:
   if definition['confirmation']!='human':continue
   human_confirmations+=1
   step=next(step for step in view['runtime']['steps'] if step['step']==definition['key']);confirmation=confirmations[step['id']]
   assert confirmation['input_digest']==step['input_digest'] and confirmation['outputs']==step['outputs'] and not confirmation.get('override_completion')
 def receipt(view, key):
  step=next(step for step in view['runtime']['steps'] if step['step']==key)
  assert step['status']=='completed', (key,step['status'])
  return view['outputs'][step['outputs']['result']]['value']
 red=receipt(children[0],'red');assert red['expected_failure'] and red['assertion_failed'] and red['counts']['failed']>0 and red['container_id']
 diagnostic_red=receipt(children[0],'red_diagnostics') if any(step['step']=='red_diagnostics' for step in children[0]['runtime']['steps']) else red
 assert diagnostic_red['expected_failure'] and diagnostic_red['candidate']==red['candidate'] and diagnostic_red['image_digest']==red['image_digest'] and diagnostic_red['recipe']==red['recipe'] and diagnostic_red['execution']['files']==red['execution']['files']
 if diagnostic_red is not red:
  for key in ['review','candidate_review']:assert next(step for step in children[0]['runtime']['steps'] if step['step']==key)['inputs']['red_diagnostics']==diagnostic_red
  assert receipt(children[0],'handoff')['inputs']['red_diagnostics']==diagnostic_red
 failures=[json.loads(line) for line in diagnostic_red['report'].splitlines() if json.loads(line).get('event')=='fail' and json.loads(line).get('classification')=='assertion']
 assert any('ERR_MODULE_NOT_FOUND' in json.dumps(record.get('diagnostic')) and 'greeting.mjs' in json.dumps(record.get('diagnostic')) for record in failures), 'The actual assertion diagnostic must corroborate the missing implementation, beyond its failure classification'
 green=receipt(children[0],'green');assert green['status']=='succeeded' and not green['expected_failure'] and green['exit_code']==0 and green['counts']['passed']>0
 verified=receipt(children[0],'verify');assert verified['status']=='succeeded' and verified['candidate']==green['candidate'] and verified['exit_code']==0 and verified['container_id']
 completed=receipt(children[0],'complete');assert completed['status']=='completed' and completed['task_id']==children[0]['runtime']['inputs']['item']['id']
 implementation=next(step for step in children[0]['runtime']['steps'] if step['step']=='implementation')
 evidence=children[0]['outputs'][implementation['outputs']['evidence']]['value'];runs=evidence['runs']
 assert len(runs)>=2 and [r['sequence'] for r in runs]==sorted({r['sequence'] for r in runs})
 reds=[i for i,r in enumerate(runs) if r['phase']=='red'];greens=[i for i,r in enumerate(runs) if r['phase']=='green' and r['exit_code']==0]
 assert reds and greens and min(reds)<max(greens) and runs[reds[0]]['exit_code']!=0
 for run in runs:
  assert run['command'] and run['cwd'] and isinstance(run['stdout'],str) and isinstance(run['stderr'],str)
  for file in run['files']:assert hashlib.sha256(file['content'].encode()).hexdigest()==file['sha256'],file['path']
 def test_files(run):return {f['path'].removeprefix('/work/').removeprefix('./'):f['sha256'] for f in run['files'] if f['path'].startswith('tests/') or '/tests/' in f['path']}
 red_files=test_files(runs[reds[0]]);green_files=test_files(runs[greens[-1]])
 assert red_files and red_files.items()<=green_files.items(), 'Raw Red and Green must retain the same independent tests'
 for key,value in red['execution'].items():assert runs[reds[0]][key]==value, 'Raw Red differs from its actual platform execution: '+key
 repo=output/'data/projects'/root['runtime']['project_id']/'repository'
 assert repo.is_dir() and repo.resolve().is_relative_to((output/'data/projects').resolve())
 environment={k:v for k,v in os.environ.items() if not k.startswith('GIT_')}
 environment.update(GIT_CONFIG_GLOBAL='/dev/null',GIT_CONFIG_NOSYSTEM='1',GIT_TERMINAL_PROMPT='0')
 for run,revision in [(runs[reds[0]],red['candidate']),(runs[greens[-1]],green['candidate'])]:
  assert re.fullmatch('[0-9a-f]{40,64}',revision)
  for file in run['files']:
   path=PurePosixPath(file['path'].removeprefix('/work/').removeprefix('./'))
   assert not path.is_absolute() and '..' not in path.parts
   blob=subprocess.run(['git','-C',str(repo),'-c','core.fsmonitor=false','show','--no-textconv',revision+':'+str(path)],check=True,capture_output=True,timeout=10,env=environment).stdout
   assert blob==file['content'].encode(), 'Evidence content differs from actual fixed Git blob: '+str(path)
 validation={'passed':True,'runs':len(runs),'unchanged_test_files':sorted(red_files),'red_execution_matches_platform':True,'files_match_actual_git_blobs':True,'no_bypass_or_waivers':True,'human_confirmations_match_exact_results':True,'human_confirmation_count':human_confirmations,'diagnostic_operation_id':diagnostic_red['operation_id'],'diagnostic_candidate_matches_original_red':True,'red_revision':red['candidate'],'green_revision':green['candidate']}
 publications=[]
 for view in [children[0],root]:
  candidate=receipt(view,'candidate');tests=receipt(view,'candidate_tests');published=receipt(view,'publish')
  assert candidate['parents']==[candidate['target_revision']] and candidate['diff']
  assert tests['status']=='succeeded' and tests['candidate']==candidate['revision'] and tests['exit_code']==0 and tests['container_id']
  expected_files=set(red_files)
  if view is root:expected_files.add('tests/workflow-v5-'+json.loads((output/'context.json').read_text())['suffix']+'-feature/behavior.test.mjs')
  assert set(tests['recipe']['files'])==expected_files and {file['path'] for file in tests['execution']['files']}==expected_files
  assert tests['image_digest']==red['image_digest']
  allowed_files={file['path'].removeprefix('/work/').removeprefix('./') for file in runs[greens[-1]]['files']}|expected_files|{'README.md'}
  changed=subprocess.run(['git','-C',str(repo),'-c','core.fsmonitor=false','diff','--name-only',candidate['target_revision'],candidate['revision'],'--'],check=True,capture_output=True,timeout=10,env=environment).stdout.decode().splitlines()
  assert changed and set(changed)<=allowed_files, 'Candidate changes files outside the agreed acceptance scope'
  for file in tests['execution']['files']+runs[greens[-1]]['files']:
   assert hashlib.sha256(file['content'].encode()).hexdigest()==file['sha256']
   path=file['path'].removeprefix('/work/').removeprefix('./');assert path and not PurePosixPath(path).is_absolute() and '..' not in PurePosixPath(path).parts
   blob=subprocess.run(['git','-C',str(repo),'-c','core.fsmonitor=false','show','--no-textconv',candidate['revision']+':'+path],check=True,capture_output=True,timeout=10,env=environment).stdout
   assert blob==file['content'].encode(), 'Final candidate changed a tested or evidenced file: '+path
  pr=published['pull_request'];assert pr['merged'] and pr['merge_commit']==candidate['revision'] and pr['head']==candidate['revision']
  publications.append(published)
 for key,view in views.items():(output/(key+'.json')).write_text(json.dumps(view,ensure_ascii=False,indent=2))
 (output/'publication-evidence.json').write_text(json.dumps(publications,ensure_ascii=False,indent=2))
 validation.update(final_candidate_files_match_evidence=True,independent_feature_test_included=True)
 (output/'raw-evidence-validation.json').write_text(json.dumps(validation,ensure_ascii=False,indent=2))
 return publications
