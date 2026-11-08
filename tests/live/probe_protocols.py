#!/usr/bin/env python3
"""Bounded, keyless protocol handshake. Never prints login payloads or raw stderr."""
import argparse,json,os,selectors,subprocess,tempfile,time
from pathlib import Path

def probe(argv,init,env,notify=False,account=False):
    process=subprocess.Popen(argv,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,env=env,start_new_session=True)
    select=selectors.DefaultSelector()
    select.register(process.stdout,selectors.EVENT_READ)
    def request(message):
        process.stdin.write((json.dumps(message)+'\n').encode());process.stdin.flush()
    buffer=b''
    pending=[]
    def next_frame(deadline):
        nonlocal buffer
        while time.monotonic()<deadline:
            if pending:return pending.pop(0)
            if not select.select(.2):continue
            chunk=os.read(process.stdout.fileno(),65536)
            if not chunk:return None
            buffer+=chunk
            if len(buffer)>1024*1024:raise ValueError('protocol frame too large')
            while b'\n' in buffer:
                line,buffer=buffer.split(b'\n',1)
                try:pending.append(json.loads(line))
                except ValueError:continue
        return None
    result={'handshake':False,'inference_tested':False}
    try:
        request({'id':1,'method':'initialize','params':init})
        deadline=time.monotonic()+25
        while time.monotonic()<deadline:
            frame=next_frame(deadline)
            if frame is None:break
            if frame.get('id')!=1:continue
            if 'error' in frame:
                result['error_code']=frame['error'].get('code');break
            result['handshake']=True
            result['server']=frame.get('result',{}).get('serverInfo',{})
            if notify:request({'method':'initialized','params':{}})
            if account:
                request({'id':2,'method':'account/read','params':{'refreshToken':False}})
                while time.monotonic()<deadline:
                    f=next_frame(deadline)
                    if f is None:break
                    if f.get('id')==2:
                        a=f.get('result',{}).get('account')
                        result['account_present']=a is not None
                        result['account_type']=a.get('type') if isinstance(a,dict) else None
                        break
            break
        return result
    finally:
        try:process.stdin.close()
        except OSError:pass
        try:process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            import signal
            os.killpg(process.pid,signal.SIGTERM)
            try:process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid,signal.SIGKILL);process.wait(timeout=3)
        select.close();process.stdout.close()

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--dsh-root',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--dsh-image', help='Immutable local DSH Docker image ID')
    parser.add_argument('--codex-image', help='Immutable local Docker image ID for a credential-free Linux handshake')
    args=parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='devdesk-probe-') as temp:
        env=os.environ.copy();env['DSH_HOME']=str(Path(temp)/'dsh')
        codex=probe(['codex','app-server'],{'clientInfo':{'name':'devdesk_probe','title':'DevDesk compatibility probe','version':'0.2.0'}},os.environ.copy(),True,True)
        env['DSH_MAX_TOKENS_AS_SUCCESS']='false'
        dsh=probe(['node',str(args.dsh_root/'apps/cli/lib/bin.js'),'--profile','sdk'],{'cwd':temp,'provider':'deepseek-official','model':'deepseek-v4-pro','approvalRequests':True,'capabilities':{'userQuestions':True}},env)
        if args.codex_image:
            state=Path(temp)/'state';state.mkdir();state.chmod(0o777)
            home=state/'codex';home.mkdir();home.chmod(0o777)
            work=Path(temp)/'work';work.mkdir();work.chmod(0o777)
            image=args.codex_image
            if not image.startswith('sha256:') or len(image)!=71:raise ValueError('Expected immutable image ID')
            codex_docker=probe(['docker','run','--rm','-i','--network=none','--read-only','--cap-drop=ALL','--security-opt=no-new-privileges','--pids-limit=128','--memory=512m','--cpus=1','--tmpfs','/tmp:rw,noexec,nosuid,size=64m','--mount',f'type=bind,source={state},target=/state','--mount',f'type=bind,source={work},target=/work,readonly',image],{'clientInfo':{'name':'devdesk_probe','version':'0.2.0'}},os.environ.copy(),True,True)
        else:codex_docker={'handshake':False,'not_run':True}
        if args.dsh_image:
            state=Path(temp)/'dsh-state';state.mkdir();state.chmod(0o777)
            work=Path(temp)/'dsh-work';work.mkdir();work.chmod(0o777)
            image=args.dsh_image
            if not image.startswith('sha256:') or len(image)!=71:raise ValueError('Expected immutable image ID')
            dsh_docker=probe(['docker','run','--rm','-i','--network=none','--read-only','--cap-drop=ALL','--security-opt=no-new-privileges','--pids-limit=256','--memory=1g','--tmpfs','/tmp:rw,noexec,nosuid,size=128m','--mount',f'type=bind,source={state},target=/state','--mount',f'type=bind,source={work},target=/work,readonly',image],{'cwd':'/work','provider':'deepseek-official','model':'deepseek-v4-pro','approvalRequests':True,'capabilities':{'userQuestions':True}},os.environ.copy())
        else:dsh_docker={'handshake':False,'not_run':True}
        report={'dsh_docker':dsh_docker,'codex_docker':codex_docker,'codex':codex,'dsh':dsh,'mode':'handshake_only','real_turns':'not_run'}
        args.output.write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps(report,indent=2))
        required=[codex,dsh]+([codex_docker] if args.codex_image else [])+([dsh_docker] if args.dsh_image else [])
        raise SystemExit(0 if all(item['handshake'] for item in required) else 1)
