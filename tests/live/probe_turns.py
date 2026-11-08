#!/usr/bin/env python3
"""Real bounded two-turn/RPC compatibility test. Only isolated managed credentials are read.
No raw provider error, stderr, auth frame, API key or OAuth token is printed or persisted.
"""
import argparse
import hashlib
import http.cookiejar
import json
import os
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import tempfile
import time
import uuid

IMAGES={'codex':'sha256:6d8a4432dd04977728fd94f8b77ffd6b3ee4d9e9f90128ba96250691d31246e1',
        'dsh':'sha256:009354c2ee03ddc4482823868d2cf0ccbf068c7935da2bdad3bea7d06fdf978c'}

class Protocol:
    def __init__(self,args,provider):
        self.provider=provider
        self.child=subprocess.Popen(args,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,start_new_session=True)
        self.selector=selectors.DefaultSelector();self.selector.register(self.child.stdout,selectors.EVENT_READ)
        self.buffer=b'';self.pending=[];self.sequence=0;self.events=[];self.receipts=[]
    def send(self,frame):
        if self.provider=='dsh':frame={'jsonrpc':'2.0',**frame}
        self.child.stdin.write((json.dumps(frame)+'\n').encode());self.child.stdin.flush()
    def receive(self,deadline):
        while time.monotonic()<deadline:
            if self.pending:
                frame=self.pending.pop(0)
                if frame.get('method') and 'id' in frame:
                    if self.provider=='dsh' and frame['method']=='approval/request':
                        self.send({'id':frame['id'],'result':{'outcome':'rejected'}})
                    elif self.provider=='dsh' and frame['method']=='user-question/ask':
                        self.send({'id':frame['id'],'result':{'cancelled':True}})
                    else:self.send({'id':frame['id'],'error':{'code':-32601,'message':'Compatibility probe does not execute tools'}})
                    continue
                if frame.get('method'):
                    # Auth/account notifications are deliberately excluded from artifacts.
                    if frame['method'].startswith(('session.','turn/','item/')):self.events.append(frame)
                return frame
            if not self.selector.select(.1):continue
            chunk=os.read(self.child.stdout.fileno(),65536)
            if not chunk:raise RuntimeError('worker_disconnected')
            self.buffer+=chunk
            if len(self.buffer)>4*1024*1024:raise RuntimeError('oversized_frame')
            while b'\n' in self.buffer:
                line,self.buffer=self.buffer.split(b'\n',1)
                try:self.pending.append(json.loads(line))
                except ValueError:raise RuntimeError('malformed_frame')
        raise TimeoutError('protocol_timeout')
    def request(self,method,params,seconds=30):
        self.sequence+=1;request_id=self.sequence
        self.send({'id':request_id,'method':method,'params':params})
        deadline=time.monotonic()+seconds
        while True:
            frame=self.receive(deadline)
            if frame.get('id')==request_id:
                if 'error' in frame:raise RuntimeError(f'provider_error:{method}:{frame["error"].get("code")}')
                self.receipts.append({'method':method,'id':request_id})
                return frame['result']
    def close(self):
        try:self.child.stdin.close()
        except OSError:pass
        try:self.child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(self.child.pid,signal.SIGTERM)
            try:self.child.wait(timeout=3)
            except subprocess.TimeoutExpired:os.killpg(self.child.pid,signal.SIGKILL);self.child.wait(timeout=3)
        self.selector.close();self.child.stdout.close()


def setup_credentials(provider,dataset,state):
    if provider=='codex':
        source=dataset/'credentials/codex/auth.json'
        if not source.is_file():raise RuntimeError('managed_oauth_required')
        home=state/'codex';home.mkdir(mode=0o700)
        shutil.copyfile(source,home/'auth.json');(home/'auth.json').chmod(0o600)
        return []
    service='devdesk.'+hashlib.sha256(str(dataset.resolve()).encode()).hexdigest()
    import sqlite3
    db=sqlite3.connect(f'file:{dataset}/global.sqlite3?mode=ro',uri=True)
    stored=db.execute("SELECT value FROM settings WHERE key='credentials.dsh'").fetchone();db.close()
    reference=json.loads(stored[0]) if stored else service+':deepseek-official-api-key'
    if not reference.startswith(service+':'):raise RuntimeError('credential_reference_outside_dataset')
    account=reference[len(service)+1:]
    result=subprocess.run(['security','find-generic-password','-s',service,'-a',account,'-w'],capture_output=True,timeout=15)
    if result.returncode:raise RuntimeError('keychain_read_failed')
    key=result.stdout.strip()
    if not key or b'\n' in key:raise RuntimeError('invalid_keychain_value')
    credential=state/'provider.env';credential.write_bytes(b'DEEPSEEK_API_KEY='+key+b'\n');credential.chmod(0o600)
    return ['--env-file',str(credential)]


def run(provider,dataset,output,resume=False):
    report={'provider':provider,'passed':False,'image':IMAGES[provider],'turns':[]}
    name='devdesk-turn-probe-'+uuid.uuid4().hex;report['container']=name
    nonce=uuid.uuid4().hex
    wire=None
    with tempfile.TemporaryDirectory(prefix='devdesk-live-turns-') as directory:
        root=Path(directory);state=root/'state';state.mkdir(mode=0o700);work=root/'work';work.mkdir(mode=0o700)
        try:
            report['stage']='credentials';credentials=setup_credentials(provider,dataset,state)
            patch=state/'persona.yml'
            patch.write_text('- id: system-prompt\n  config:\n    personaPrefix: "You are the DevDesk requirements analyst. For this protocol test, never call tools. Reply with the compact JSON requested by the user."\n- id: session-telemetry-otel\n  disabled: true\n- id: file-upload\n  disabled: true\n- id: tool-plugin-manager\n  disabled: true\n')
            args=['docker','run','--name',name,'-i','--network=bridge','--read-only','--user',f'{os.getuid()}:{os.getgid()}',
                  '--cap-drop=ALL','--security-opt=no-new-privileges','--pids-limit=256','--memory=1g','--cpus=2',
                  '--tmpfs','/tmp:rw,noexec,nosuid,size=128m','--mount',f'type=bind,source={state},target=/state',
                  '--mount',f'type=bind,source={work},target=/work,readonly',*credentials,IMAGES[provider]]
            if provider=='dsh':args+=['--patch','/state/persona.yml']
            report['stage']='initialize';wire=Protocol(args,provider)
            if provider=='codex':
                wire.request('initialize',{'clientInfo':{'name':'devdesk_live_probe','version':'0.2.0'}})
                wire.send({'method':'initialized','params':{}})
                account=wire.request('account/read',{'refreshToken':False});assert account['account'] is not None
                models=wire.request('model/list',{'limit':100})['data'];model=next(m for m in models if m['isDefault'])['model'];report['model']=model
                thread=wire.request('thread/start',{'cwd':'/work','model':model,'approvalPolicy':'never','sandbox':'read-only','developerInstructions':'You are the DevDesk requirements analyst. Do not use tools. Return the JSON requested by the user.'})
                session=thread['thread']['id']
            else:
                model='deepseek-v4-pro';report['model']=model
                wire.request('initialize',{'cwd':'/work','provider':'deepseek-official','model':model,'maxTokens':2048,'approvalRequests':True,'capabilities':{'userQuestions':True}})
                session='devdesk-r0-'+uuid.uuid4().hex
            report['session_id']=session
            prompts=[f'Remember marker {nonce}. Return only JSON {{"marker":"{nonce}","role":"requirements_analyst"}}.',
                     'Return only JSON with the same marker from the previous user message and role requirements_analyst. Do not use tools.']
            if resume:prompts.append('After restoring this conversation, return only JSON with the original marker and role requirements_analyst. Do not use tools.')
            for index,prompt in enumerate(prompts):
                if index==2:
                    report['stage']='restart_and_resume'
                    if provider=='dsh':wire.request('shutdown',{})
                    wire.close()
                    subprocess.run(['docker','stop','--time','3',name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=15)
                    stopped=subprocess.run(['docker','inspect','--format','{{.State.Running}}',name],capture_output=True,text=True,timeout=10)
                    report['first_writer_stopped']=stopped.returncode==0 and stopped.stdout.strip()=='false'
                    assert report['first_writer_stopped']
                    subprocess.run(['docker','rm',name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10)
                    name+='-resumed';args[args.index('--name')+1]=name
                    report['resumed_container']=name;wire=Protocol(args,provider)
                    if provider=='codex':
                        wire.request('initialize',{'clientInfo':{'name':'devdesk_live_probe','version':'0.2.0'}})
                        wire.send({'method':'initialized','params':{}})
                        restored=wire.request('thread/resume',{'threadId':session,'cwd':'/work','model':model,'approvalPolicy':'never','sandbox':'read-only'})
                        assert restored['thread']['id']==session
                    else:
                        wire.request('initialize',{'cwd':'/work','provider':'deepseek-official','model':model,'maxTokens':2048,'approvalRequests':True,'capabilities':{'userQuestions':True}})
                        wire.request('session/resume',{'sessionId':session})
                    report['native_resume']=True
                report['stage']=f'turn_{index+1}';start=len(wire.events)
                if provider=='codex':
                    receipt=wire.request('turn/start',{'threadId':session,'input':[{'type':'text','text':prompt}]})
                    turn=receipt['turn']['id'];deadline=time.monotonic()+180
                    while True:
                        frame=wire.receive(deadline)
                        if frame.get('method')=='turn/completed' and frame['params']['threadId']==session and frame['params']['turn']['id']==turn:
                            terminal=frame['params']['turn'];break
                    assert terminal['status']=='completed'
                    texts=[e['params']['item'].get('text','') for e in wire.events[start:] if e.get('method')=='item/completed' and e['params'].get('threadId')==session and e['params']['item']['type']=='agentMessage']
                    text='\n'.join(texts);detail={'turn_id':turn,'terminal':'completed'}
                else:
                    receipt=wire.request('session/prompt',{'sessionId':session,'contentBlocks':[{'type':'text','text':prompt}]})
                    wire.request('session/wait',{'sessionId':session},seconds=180)
                    events=[e['params']['event'] for e in wire.events[start:] if e.get('method')=='session.event' and e['params']['sessionId']==session]
                    admission=next(i for i,e in enumerate(events) if e['type']=='agent/inbox/spliced' and any(m['id']==receipt['messageId'] for m in e['data']['inserted']))
                    terminal=next(e for e in reversed(events[admission+1:]) if e['type']=='turn/end')
                    assert terminal['data']['reason']['kind']=='completed'
                    assistant=next(e for e in reversed(events[admission+1:]) if e['type']=='assistant/message')
                    text=''.join(b['text'] for b in assistant['data']['message']['content'] if b['type']=='text')
                    detail={'message_id':receipt['messageId'],'consumed':True,'terminal':'completed','quiescent_wait':True}
                # Validate structured output and actual retained context, without matching free prose.
                clean=text.strip();clean=clean.removeprefix('```json').removesuffix('```').strip()
                answer=json.loads(clean);assert answer['marker']==nonce and answer['role']=='requirements_analyst'
                report['turns'].append({**detail,'session_id':session,'structured_json':True,'context_retained':True,'event_count':len(wire.events)-start})
            if provider=='dsh':wire.request('shutdown',{})
            report['passed']=True;report['stage']='finished'
        except Exception as error:
            report['failure_type']=type(error).__name__
            if isinstance(error,RuntimeError):report['failure_code']=str(error)
        finally:
            if wire:wire.close()
            subprocess.run(['docker','stop','--time','3',name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=15)
            stopped=subprocess.run(['docker','inspect','--format','{{.State.Running}}',name],capture_output=True,text=True,timeout=10)
            report['writer_stopped']=stopped.returncode==0 and stopped.stdout.strip()=='false'
            subprocess.run(['docker','rm',name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=10)
    output.parent.mkdir(parents=True,exist_ok=True);output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2));return report['passed'] and report['writer_stopped']

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--provider',choices=['codex','dsh'],required=True)
    parser.add_argument('--dataset',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--resume',action='store_true')
    args=parser.parse_args();raise SystemExit(0 if run(args.provider,args.dataset.resolve(),args.output,args.resume) else 1)
