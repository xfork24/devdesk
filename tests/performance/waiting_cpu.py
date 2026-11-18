import argparse,pathlib,subprocess,time,json,uuid,urllib.request,select,os,signal,shutil,hashlib
p=argparse.ArgumentParser();p.add_argument('--binary',type=pathlib.Path,required=True);p.add_argument('--output',type=pathlib.Path,required=True);p.add_argument('--seconds',type=int,default=60);a=p.parse_args();a.output.mkdir(exist_ok=False);a.output=a.output.resolve();shutil.copy2(__file__,a.output/'probe.py');(a.output/'identity.json').write_text(json.dumps({'binary_sha256':hashlib.sha256(a.binary.read_bytes()).hexdigest(),'seconds':a.seconds,'scene':'Rust driver waiting at one real Human confirmation; no native PM'},indent=2))
log=(a.output/'host.log').open('w');host=subprocess.Popen([str(a.binary),'--data',str(a.output/'dataset'),'--port','0'],stdout=subprocess.PIPE,stderr=log)
try:
 pending='';origin=None;until=time.monotonic()+30
 while time.monotonic()<until and host.poll() is None:
  if not select.select([host.stdout],[],[],.1)[0]:continue
  pending+=os.read(host.stdout.fileno(),65536).decode()
  if any(line.startswith('http://') for line in pending.splitlines()):origin=next(line for line in pending.splitlines() if line.startswith('http://'));break
 assert origin,'Host unavailable'
 with urllib.request.urlopen(origin) as response:cookie=response.headers['Set-Cookie'].split(';')[0]
 def rpc(method,params):
  request=urllib.request.Request(origin+'/rpc',json.dumps({'jsonrpc':'2.0','id':1,'method':method,'params':params}).encode(),{'Cookie':cookie,'Content-Type':'application/json'})
  with urllib.request.urlopen(request,timeout=30) as response:reply=json.load(response)
  assert 'error' not in reply,(method,reply.get('error'));return reply['result']
 def command(method,target=None,revision=None,payload=None,project=None):return rpc(method,{'project_id':project,'command_id':str(uuid.uuid4()),'target':target,'expected_revision':revision,'payload':payload or {}})
 project=command('project.create',payload={'title':'Explicit waiting CPU fixture'})['id']
 draft=command('workflow.create',payload={'name':'Waiting Human fixture','document':{'schema_version':6,'stages':[{'key':'wait','name':'Human wait'}],'steps':[{'key':'gate','name':'Human gate','stage':'wait','executor':{'type':'control'},'confirmation':'human'}]}})
 workflow=command('workflow.publish',draft['id'],draft['revision'])
 owner=rpc('query.workspace',{'project_id':project})['active_project']
 run=command('workflow.start',project,owner['revision'],{'workflow_ref':workflow['id'],'role_agent_bindings':{}},project)
 time.sleep(1.5) # Setup-only settling, excluded from the fixed 60 second CPU interval.
 header=rpc('query.workflow_management_summary',{'project_id':project,'id':run['id']})['record']
 command('workflow.driver.select',run['id'],header['revision'],{'mode':'rust','expected_driver_epoch':0,'plan_revision_id':header['data']['active_plan_revision_id'],'control_version':header['data']['control_version']},project)
 until=time.monotonic()+30
 while time.monotonic()<until:
  status=rpc('query.workflow_instance',{'project_id':project,'id':run['id'],'view':'status'})
  if status['runtime']['steps'][0]['status']=='waiting_confirmation':break
  time.sleep(.1)
 assert status['runtime']['steps'][0]['status']=='waiting_confirmation',status
 (a.output/'waiting-before.json').write_text(json.dumps(status));time.sleep(2)
 samples=[]
 for n in range(a.seconds+1):
  fields=subprocess.check_output(['ps','-p',str(host.pid),'-o','%cpu=,rss=,time='],text=True).split();samples.append({'time':time.monotonic(),'ps':fields})
  if n<a.seconds:time.sleep(1)
 (a.output/'resources.json').write_text(json.dumps(samples))
 after=rpc('query.workflow_instance',{'project_id':project,'id':run['id'],'view':'status'});assert after['runtime']['steps'][0]['status']=='waiting_confirmation';(a.output/'waiting-after.json').write_text(json.dumps(after))
 def cpu(s):
  result=0
  for part in s.split(':'):result=result*60+float(part)
  return result
 avg=100*(cpu(samples[-1]['ps'][2])-cpu(samples[0]['ps'][2]))/(samples[-1]['time']-samples[0]['time']);summary={'average_host_single_core_percent':avg,'seconds':samples[-1]['time']-samples[0]['time'],'rss_peak_kib':max(int(s['ps'][1]) for s in samples),'human_decisions':0,'goal_single_core_percent':2,'passed':avg<=2};(a.output/'summary.json').write_text(json.dumps(summary,indent=2));print(json.dumps(summary),flush=True)
finally:
 if host.poll() is None:
  host.send_signal(signal.SIGINT)
  try:host.wait(timeout=30)
  except subprocess.TimeoutExpired:host.kill();host.wait()
 log.close()
