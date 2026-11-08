#!/usr/bin/env python3
"""Exercise the real Codex child sandbox using harmless credential canaries, no model calls."""
import argparse,json,os,subprocess,tempfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);args=p.parse_args()
report={'passed':False,'inference_tested':False,'credential_canary_only':True,'seccomp':'moby-default + nested namespace/mount syscalls'}
with tempfile.TemporaryDirectory(prefix='devdesk-native-sandbox-') as d:
 root=Path(d);state=root/'state';home=state/'codex';home.mkdir(parents=True);work=root/'work';work.mkdir()
 (home/'auth.json').write_text('HARMLESS_CANARY_NOT_A_CREDENTIAL')
 (home/'config.toml').write_text('''default_permissions="devdesk"
[permissions.devdesk.filesystem]
":root"="deny"
":minimal"="read"
"/work"="write"
"/state"="deny"
[permissions.devdesk.network]
enabled=false
''')
 command=['docker','run','--rm','--network=none','--read-only','--user',f'{os.getuid()}:{os.getgid()}',
          '--cap-drop=ALL','--security-opt=no-new-privileges','--security-opt',f'seccomp={ROOT}/containers/security/nested-sandbox.json',
          '--pids-limit=128','--memory=512m','--tmpfs','/tmp:rw,noexec,nosuid,size=64m',
          '--mount',f'type=bind,source={state},target=/state','--mount',f'type=bind,source={work},target=/work',
          '--entrypoint','codex','sha256:6d8a4432dd04977728fd94f8b77ffd6b3ee4d9e9f90128ba96250691d31246e1',
          'sandbox','--','/bin/sh','-c','(! cat /state/codex/auth.json 2>/dev/null) && printf native-sandbox-ok > /work/proof']
 try:
  result=subprocess.run(command,capture_output=True,text=True,timeout=30)
  report['exit_code']=result.returncode
  # No real credentials exist in this diagnostic container.
  if result.returncode:report['diagnostic']=result.stderr[-1800:]
  report['passed']=result.returncode==0 and (work/'proof').read_text()=='native-sandbox-ok'
  report['work_write_proven']=report['passed'];report['credential_read_denied']=report['passed']
 except Exception as e:report['failure_type']=type(e).__name__
args.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2));raise SystemExit(0 if report['passed'] else 1)
