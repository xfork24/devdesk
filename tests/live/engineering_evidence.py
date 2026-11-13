"""Corroborate frozen contracts, incremental evidence and exact publications."""
import hashlib,json,os,re,sqlite3,subprocess
from pathlib import PurePosixPath

def frozen_document(connection, frozen):
 """Resolve the immutable Workflow revision selected by the stored plan codec."""
 selection=frozen['plan']
 assert selection['$source']=='workflow_plan', 'Expected the current source-backed plan encoding'
 reference=selection['workflow_ref']
 assert reference==frozen['workflow_ref'], 'Frozen workflow selection does not match its plan'
 source=connection.execute("SELECT data FROM source_definitions WHERE reference=? AND type='workflow'",(reference,)).fetchone()
 assert source is not None, 'Original frozen Workflow source must remain available'
 return json.loads(source[0])

def verify_chunk_reads(reference, expected, output_id, current, calls, wire_items):
 """Reconstruct a large input from receipts delivered to this exact native turn."""
 assert reference['read_tool']=='devdesk_workflow_outputs_read' and reference['complete'] is False
 assert reference['source_output_id']==output_id and reference['pointer']==''
 assert reference['arguments']=={'ids':[output_id],'max_bytes':32768,'offset':0,'pointer':''}
 total=reference['total_bytes'];digest=reference['value_digest'];chunks={}
 assert total>32768 and re.fullmatch('[0-9a-f]{64}',digest)
 for call in calls:
  request=call['request'];caller=request.get('caller') or {};arguments=request.get('arguments',{})
  if (caller.get('turn_id'),caller.get('session_id'),caller.get('execution_id'))!=(current['turn_id'],current['session_id'],current['id']):continue
  if request.get('definition',{}).get('qualified_name')!='workflow.outputs.read' or arguments.get('ids')!=[output_id] or arguments.get('pointer','')!='':continue
  assert call['status']=='succeeded','Large input read did not succeed'
  receipt=call['receipt'];observed=None
  for wire in wire_items:
   item=wire['item']
   if item.get('id')!=caller['call_id']:continue
   assert item['type']=='dynamicToolCall' and item['tool']==reference['read_tool'] and item['status']=='completed' and item['success'] is True
   assert item['arguments']==arguments,'Native read arguments differ from durable request'
   bodies=[]
   for part in item.get('contentItems') or []:
    if part.get('type')!='inputText':continue
    try:bodies.append(json.loads(part['text']))
    except (ValueError,KeyError):continue
   assert receipt in bodies,'Complete read receipt absent from actual native tool response'
   observed=wire;break
  assert observed is not None,'Large input read absent from native journal'
  assert len(receipt['outputs'])==1
  chunk=receipt['outputs'][0];offset=chunk['offset'];body=chunk['value_chunk']
  assert chunk['id']==output_id and chunk['total_bytes']==total and offset==arguments.get('offset',0)
  assert chunk['data']['provenance']['value_digest']==digest,'Input provenance digest differs'
  end=offset+len(body.encode())
  assert body and 0<=offset<end<=total
  assert chunk['next_offset']==(end if end<total else None),'Invalid next chunk boundary'
  if offset in chunks:assert chunks[offset]['body']==body,'Repeated read changed its content'
  chunks[offset]={'body':body,'proof':{'offset':offset,'next_offset':chunk['next_offset'],'utf8_bytes':len(body.encode()),'call_id':caller['call_id'],'journal':observed['journal']}}
 offset=0;parts=[];proof=[]
 while offset<total:
  assert offset in chunks,'Large input read has a missing chunk at '+str(offset)
  chunk=chunks[offset];parts.append(chunk['body']);proof.append(chunk['proof']);offset+=chunk['proof']['utf8_bytes']
 body=''.join(parts)
 assert len(body.encode())==total and hashlib.sha256(body.encode()).hexdigest()==digest,'Large input content digest differs'
 assert json.loads(body)==expected,'Read input differs from sealed handoff'
 return {'execution':current['id'],'turn':current['turn_id'],'session':current['session_id'],'source_output_id':output_id,'total_bytes':total,'value_digest':digest,'chunks':proof,'native_delivery_verified':True}

def verify_evidence(client, rows, output):
 views={row['id']:client.rpc('query.workflow_instance',{'id':row['id']}) for row in rows}
 root=next(v for v in views.values() if v['runtime']['parent_execution_id'] is None)
 children=[v for v in views.values() if v['runtime']['parent_execution_id'] is not None]
 assert len(children)==1, 'Expected one actual Task invocation'
 child=children[0];context=json.loads((output/'context.json').read_text());task_snapshots={};wire_deliveries={};input_reads=[]
 database=output/'data/projects'/root['runtime']['project_id']/'engineering.sqlite3'
 with sqlite3.connect(database.resolve().as_uri()+'?mode=ro',uri=True) as connection:
  assert connection.execute("SELECT count(*) FROM governance_records WHERE type='workflow_waiver'").fetchone()[0]==0
  confirmations={key:json.loads(data) for key,data in connection.execute("SELECT definition_id,data FROM governance_records WHERE type='workflow_confirmation'")}
 if context.get('unattended'):
  with sqlite3.connect(database.resolve().as_uri()+'?mode=ro',uri=True) as connection:
   assert connection.execute("SELECT count(*) FROM governance_records WHERE type='controller_input'").fetchone()[0]==1, 'Unattended run received additional human steering'
   controller_id,controller=connection.execute("SELECT id,data FROM governance_records WHERE type='workflow_controller' AND json_extract(data,'$.root_instance_id')=?",(root['record']['id'],)).fetchone()
   assert json.loads(controller)['epoch']==1,'Unattended controller was taken over or resumed'
   for data, in connection.execute("SELECT data FROM governance_records WHERE type='workflow_decision'"):
    actor=json.loads(data)['actor']
    if actor['kind']=='segment':
     authorization=connection.execute("SELECT data FROM governance_records WHERE type='workflow_segment' AND id=?",(actor['authorization_id'],)).fetchone()
     assert authorization is not None,'Segment lacks a durable authorization'
     actor=json.loads(authorization[0])['actor']
    assert actor['kind']=='controller' and actor['controller_id']==controller_id and actor['epoch']==1,'Workflow was advanced outside the native controller'
 human_confirmations=0
 for view in views.values():
  assert view['runtime']['status']=='completed' and all(s['status']=='completed' for s in view['runtime']['steps'])
  assert not any(item['data'].get('bypassed') for item in view['history'] if item['type']=='workflow_change')
  # Runtime workflow refs change; follow immutable revision ancestry rather than
  # trusting the mutable catalog or treating the revised graph as its own baseline.
  revision=view['runtime']['active_plan_revision_id'];seen=set()
  with sqlite3.connect(database.resolve().as_uri()+'?mode=ro',uri=True) as connection:
   while revision:
    assert revision not in seen,'Plan revision ancestry cycle'
    seen.add(revision)
    original=connection.execute("SELECT data FROM governance_records WHERE type='workflow_plan_revision' AND id=?",(revision,)).fetchone()
    assert original is not None,'Original frozen plan must remain available'
    frozen=json.loads(original[0]);assert frozen['instance_id']==view['record']['id']
    source=frozen_document(connection,frozen);revision=frozen.get('parent_revision_id')
  if context.get('unattended'):
   assert view['runtime']['role_agent_bindings']==frozen['role_agent_bindings'], 'Unattended run replaced the default role bindings'
  for action,requirements in source.get('action_requirements',{}).items():
   assert set(requirements)<=set(view['plan']['action_requirements'].get(action,[]))
  current={s['key']:s for s in view['plan']['steps']}
  for original in source['steps']:
   assert original['key'] in current,'A mandatory source node was removed'
   actual=current[original['key']]
   if context.get('unattended') and original['key'].endswith('review'):
    assert actual['task_instructions']==original.get('task_instructions'), 'Unattended run rewrote an independent review prompt'
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
   assert connection.execute("SELECT count(*) FROM governance_records WHERE type='workflow_human_decision' AND json_extract(data,'$.approved')=1").fetchone()[0]==0, 'No manual decision approval may advance the default workflow'
 def step(view,key):return next(s for s in view['runtime']['steps'] if s['step']==key)
 def receipt(view,key):
  current=step(view,key);assert current['status']=='completed'
  return view['outputs'][current['outputs']['result']]['value']
 def task(view,key):
  current=step(view,key);ident=current['turn_id']
  result=client.rpc('query.execution_task',{'turn_id':ident});task_snapshots[ident]=result
  if ident not in wire_deliveries:
   rendered=result['task']['rendered_input'];observed=None
   journals=output/'data/projects'/view['runtime']['project_id']/'execution/sessions'/current['session_id']/'coordinator'
   for journal in journals.glob('*.jsonl'):
    for line in journal.open():
     envelope=json.loads(line);frame=envelope['frame']
     if envelope['direction']=='out' and frame.get('method')=='turn/start' and any(rendered in part.get('text','') for part in frame['params']['input'] if part.get('type')=='text'):
      observed={'workflow':view['record']['id'],'step':key,'turn':ident,'session':current['session_id'],'journal':journal.name,'task_digest':result['task']['digest'],'rendered_utf8_bytes':len(rendered.encode()),'complete_task_sent':True}
   assert observed is not None,'Complete review task absent from actual native turn/start: '+key
   wire_deliveries[ident]=observed
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
 # Corroborate the delivered records against actual native command observations,
 # rather than trusting a model's final delivery as evidence of execution.
 command_observations=[];observed_ids=set()
 journals=output/'data/projects'/root['runtime']['project_id']/'execution/sessions'/implementation['session_id']/'coordinator'
 def records(value):
  if isinstance(value,dict):
   yield value
   for child_value in value.values():yield from records(child_value)
  elif isinstance(value,list):
   for child_value in value:yield from records(child_value)
 for journal in journals.glob('*.jsonl'):
  for line in journal.open():
   frame=json.loads(line)['frame'];item=frame.get('params',{}).get('item',{})
   if frame.get('method')!='item/completed' or item.get('type')!='commandExecution' or item.get('status')!='completed':continue
   body=item.get('aggregatedOutput') or ''
   for fragment in [body,*body.splitlines()]:
    try:observed=json.loads(fragment)
    except (ValueError,TypeError):continue
    for value in records(observed):
     if value in runs and (item['id'],value['sequence']) not in observed_ids:
      observed_ids.add((item['id'],value['sequence']))
      command_observations.append({'sequence':value['sequence'],'journal':journal.name,'item_id':item['id'],'command':item['command'],'tool_exit_code':item['exitCode'],'run':value})
 for run in runs:assert any(o['run']==run for o in command_observations),f"TDD run {run['sequence']} absent from actual native command output"
 (output/'native-tdd-observations.json').write_text(json.dumps(command_observations,ensure_ascii=False,indent=2))
 pairs=0;pending=None
 for run in runs:
  if run['phase']=='red':
   pending=run
   assert 'ERR_MODULE_NOT_FOUND' not in run['stdout']+run['stderr'], 'A loading failure cannot count as incremental Red'
  elif run['phase']=='green' and run['exit_code']==0 and pending is not None:
   assert run['command']==pending['command'] and run['cwd']==pending['cwd'], 'Incremental Red/Green must execute the same tests'
   tests=lambda r:{normalized(f['path'],r['cwd']):f['content'] for f in r['files'] if normalized(f['path'],r['cwd']).startswith(('test/','tests/'))}
   assert tests(run) and tests(run)==tests(pending), 'Incremental tests changed between Red and Green'
   pairs+=1;pending=None
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
  source_id=step(root,'implementation')['outputs']['result']
  assert current['inputs']['tasks']==handed_off,'Feature input differs from its sealed handoff: '+key
  assert rendered['input_sources']['tasks']['source_output_id']==source_id
  delivered=rendered['inputs']['tasks']
  if delivered!=handed_off:
   with sqlite3.connect(database.resolve().as_uri()+'?mode=ro',uri=True) as connection:
    calls=[json.loads(row[0]) for row in connection.execute("SELECT data FROM governance_records WHERE type='platform_call'")]
   wire_items=[]
   journals=output/'data/projects'/root['runtime']['project_id']/'execution/sessions'/current['session_id']/'coordinator'
   for journal in journals.glob('*.jsonl'):
    for line in journal.open():
     envelope=json.loads(line);frame=envelope['frame']
     if envelope['direction']=='in' and frame.get('method')=='item/completed':wire_items.append({'journal':journal.name,'item':frame['params']['item']})
   proof=verify_chunk_reads(delivered,handed_off,source_id,current,calls,wire_items);proof['step']=key;input_reads.append(proof)
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
 (output/'reviewer-wire-deliveries.json').write_text(json.dumps(list(wire_deliveries.values()),ensure_ascii=False,indent=2))
 (output/'reviewer-input-reads.json').write_text(json.dumps(input_reads,ensure_ascii=False,indent=2))
 (output/'raw-tdd-evidence.json').write_text(json.dumps(evidence,ensure_ascii=False,indent=2))
 (output/'publication-evidence.json').write_text(json.dumps(publications,ensure_ascii=False,indent=2))
 validation={'passed':True,'runs':len(runs),'increment_pairs':pairs,'unchanged_test_files':sorted(red_files),'platform_red_copied':platform_copy,'declared_run_fields':required,'no_bypass_or_waivers':True,'human_confirmation_count':human_confirmations,'human_confirmations_match_exact_results':True,'consumer_task_provenance_matches':True,'large_inputs_read_in_full':len(input_reads),'diagnostic_operation_id':diagnostic_red['operation_id'],'final_candidate_files_match_evidence':True,'independent_feature_test_included':True,'red_revision':red['candidate'],'green_revision':green['candidate']}
 (output/'raw-evidence-validation.json').write_text(json.dumps(validation,ensure_ascii=False,indent=2));return publications
