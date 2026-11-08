"""Small local acceptance client. Project pinning and unique commands are explicit."""
import http.cookiejar,json,time,urllib.request,uuid
class Client:
 def __init__(self,url='http://127.0.0.1:51751',project=None):
  self.url=url;self.project=project;self.http=urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()));deadline=time.monotonic()+15
  while True:
   try:self.http.open(url+'/',timeout=3).close();break
   except urllib.error.URLError:
    if time.monotonic()>=deadline:raise
    time.sleep(.2)
 def rpc(self,method,params=None):
  params=dict(params or {})
  if self.project:params.setdefault('project_id',self.project)
  req=urllib.request.Request(self.url+'/rpc',data=json.dumps({'jsonrpc':'2.0','id':1,'method':method,'params':params}).encode(),headers={'Content-Type':'application/json'})
  value=json.load(self.http.open(req,timeout=40))
  if 'error' in value:raise RuntimeError(method+': '+value['error']['data']['code']+': '+value['error']['message'])
  return value['result']
 def command(self,method,target=None,revision=None,payload=None,command_id=None):
  return self.rpc(method,{'command_id':command_id or str(uuid.uuid4()),'target':target,'expected_revision':revision,'payload':payload or {}})
 def open(self,project):
  self.command('project.open',project);self.project=project
 def feature(self,feature):return self.rpc('query.workspace',{'feature_id':feature,'limit':100})['detail']
 def discuss(self,owner,revision,role,provider,prompt,timeout=240,work_type='discussion'):
  sid=self.command('session.open',owner,revision,{'agent':f'builtin-agent-{role}-v1','provider':provider,'work_type':work_type})['id']
  state=self.rpc('query.session',{'id':sid});tid=self.command('conversation.send',sid,state['session']['revision'],{'client_message_id':str(uuid.uuid4()),'input':prompt})['id'];end=time.monotonic()+timeout
  while time.monotonic()<end:
   state=self.rpc('query.session',{'id':sid,'limit':100});turn=next(t for t in state['turns'] if t['id']==tid)
   if turn['status'] in ['failed','cancelled','needs_reconciliation']:raise AssertionError('turn '+tid+': '+turn['status'])
   if turn['status']=='completed':return sid,tid,state
   time.sleep(.5)
  raise TimeoutError('turn deadline '+tid)
 def wait(self,read,predicate,timeout=300):
  end=time.monotonic()+timeout
  while time.monotonic()<end:
   value=read()
   if predicate(value):return value
   time.sleep(.5)
  raise TimeoutError('acceptance deadline')
