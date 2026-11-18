"""Real HTTP/SSE traffic alongside a native PM; never issues PM controls."""
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import math
from pathlib import Path
import shutil
import threading
import time
import urllib.parse
import urllib.request
import uuid


def stats(values):
    values = sorted(values)
    def p(q):
        return values[max(0, math.ceil(len(values)*q)-1)] if values else None
    return {'count':len(values),'p50_ms':p(.5),'p95_ms':p(.95),'p99_ms':p(.99),'max_ms':values[-1] if values else None}


class NativeFrontendLoad:
    def __init__(self, client_type, origin, output, count=100):
        self.output=Path(output);self.output.mkdir()
        shutil.copy2(__file__,self.output/Path(__file__).name)
        self.origin=origin;self.count=count;self.target=None
        self.stop=threading.Event();self.streams=[];self.rows=[];self.errors=[];self.events=0;self.guard=threading.Lock()
        c=client_type(origin)
        self.project=c.command('project.create',payload={'title':'Independent HTTP load owners'})['id'];c.project=self.project
        defaults=c.rpc('query.configuration_defaults')
        project=c.rpc('query.execution_workspace')['active_project']
        c.command('project.configuration.update',self.project,project['revision'],{'workflow_ref':defaults['workflow_ref'],'role_agent_bindings':defaults['role_agent_bindings'],'orchestrator_role_ref':defaults['orchestrator_role_ref'],'harness_permission_ref':defaults['harness_permission_ref'],'orchestration':{'mode':'manual'}})
        self.contexts=[]
        for n in range(count):
            feature=c.command('feature.create',payload={'project_id':self.project,'title':f'Independent browser load {n}','initial_requirement':'Performance context; no workflow or model may start','start_workflow':False})
            detail=c.rpc('query.workspace',{'feature_id':feature['id']})['detail']
            context=next(row for row in detail['contexts'] if row['owner_id']==feature['id'])
            self.contexts.append({'project_id':self.project,'owner_id':feature['id'],'id':context['id'],'revision':context['revision']})
        with urllib.request.urlopen(origin,timeout=10) as response:self.cookie=response.headers['Set-Cookie'].split(';',1)[0]
        (self.output/'fixture.json').write_text(json.dumps({'clients':count,'project':self.project,'contexts':self.contexts,'factory_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'native_controls':0,'history':'independent Draft Features; no fabricated turns/events'},indent=2))

    def request(self, method, params, kind, scheduled, client):
        sent=time.monotonic();row={'method':method,'kind':kind,'client':client,'scheduled':scheduled,'sent':sent,'generator_queue_ms':(sent-scheduled)*1000}
        try:
            request=urllib.request.Request(self.origin+'/rpc',json.dumps({'jsonrpc':'2.0','id':str(uuid.uuid4()),'method':method,'params':params}).encode(),{'Cookie':self.cookie,'Content-Type':'application/json'})
            with urllib.request.urlopen(request,timeout=15) as response:body=response.read();value=json.loads(body)
            row['bytes']=len(body)
            if value.get('error'):row['error']=value['error']
            return value.get('result')
        except Exception as error:
            row['error']=repr(error)
            return None
        finally:
            done=time.monotonic();row.update(done=done,ms=(done-sent)*1000,scheduled_ms=(done-scheduled)*1000)
            with self.guard:
                self.rows.append(row)
                with (self.output/'rpc.jsonl').open('a') as stream:stream.write(json.dumps(row)+'\n')

    def subscribe(self, n):
        project=self.native_project if n%2==0 else self.project
        try:
            response=urllib.request.urlopen(urllib.request.Request(self.origin+'/events?'+urllib.parse.urlencode({'project_id':project,'tail':'true'}),headers={'Cookie':self.cookie}),timeout=30)
            with self.guard:self.streams.append(response)
            while not self.stop.is_set():
                line=response.readline()
                if not line:break
                if line.startswith(b'id:'):
                    assert line.decode().split(':',1)[1].strip().startswith(project+':')
                    with self.guard:self.events+=1
        except Exception as error:
            if not self.stop.is_set():self.errors.append({'client':n,'sse_error':repr(error)})

    def begin(self, native_project):
        self.native_project=native_project
        for n in range(self.count):threading.Thread(target=self.subscribe,args=(n,),daemon=True).start()
        deadline=time.monotonic()+20
        while len(self.streams)<self.count and time.monotonic()<deadline:time.sleep(.02)
        if len(self.streams)!=self.count:raise RuntimeError('Native load SSE subscriptions incomplete')
        self.started=time.monotonic()
        self.thread=threading.Thread(target=self.generate,daemon=True);self.thread.start()

    def attach(self, project, root):
        self.target={'project_id':project,'id':root}

    def generate(self):
        with ThreadPoolExecutor(max_workers=64) as pool:
            index=0
            while not self.stop.is_set():
                at=self.started+index/self.count
                if self.stop.wait(max(0,at-time.monotonic())):break
                n=index%self.count;target=self.target
                method=['query.execution_workspace','query.workflow_progress','query.workflow_driver','query.history','query.tool_calls'][index%5]
                if not target or method=='query.execution_workspace':params={'project_id':self.project}
                elif method in ['query.history','query.tool_calls']:params={'project_id':target['project_id'],'limit':20,'view':'summary'}
                else:params={**target,'view':'summary'} if method=='query.workflow_driver' else target
                if not target and method!='query.execution_workspace':method='query.execution_workspace'
                pool.submit(self.request,method,params,'read',at,n)
                if index%10==0:pool.submit(self.write,index//10,at)
                index+=1
            self.scheduled_reads=index
            self.scheduled_writes=(index+9)//10

    def write(self, n, at):
        client=n%self.count;context=self.contexts[client]
        # At 10 writes/s each independent owner is touched once per ten seconds.
        # There is no automatic version rebasing or command retry.
        result=self.request('context.update',{'project_id':self.project,'command_id':str(uuid.uuid4()),'target':context['id'],'expected_revision':context['revision'],'payload':{'content':f'Native-load client {client}, write {n}'}},'write',at,client)
        if result:context['revision']+=1

    def finish(self):
        self.stop.set()
        if hasattr(self,'thread'):
            self.thread.join(timeout=45)
            if self.thread.is_alive():self.errors.append({'generator_did_not_stop':True})
        for n, context in enumerate(self.contexts):
            value=self.request('query.workflow_contexts',{'project_id':self.project,'owner_id':context['owner_id'],'context_id':context['id']},'verify',time.monotonic(),n)
            if value is None or value.get('context',{}).get('revision')!=context['revision']:
                self.errors.append({'client':n,'write_read_back_failed':True})
        active=self.count if hasattr(self,'thread') else 0
        elapsed=time.monotonic()-self.started if hasattr(self,'started') else 0
        summary={'clients':active,'prepared_targets':len(self.contexts),'offered_read_rps':active,'offered_write_rps':active/10,'sse_connections':len(self.streams),'events':self.events,'native_controls':0,'errors':self.errors,'scheduled_reads':getattr(self,'scheduled_reads',0),'scheduled_writes':getattr(self,'scheduled_writes',0),'elapsed_seconds':elapsed,'failures':[r for r in self.rows if r.get('error')],'local_only':True,'formal_p6':False}
        for kind in ['read','write']:
            selected=[r for r in self.rows if r['kind']==kind]
            summary[kind]={**stats([r['ms']for r in selected]),'generator_queue':stats([r['generator_queue_ms']for r in selected])}
        (self.output/'summary.json').write_text(json.dumps(summary,indent=2))
        self.summary=summary
        return summary
