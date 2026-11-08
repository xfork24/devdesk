#!/usr/bin/env python3
"""Exercise the real Codex child sandbox using harmless credential canaries, no model calls."""
import argparse,hashlib,json,os,subprocess,tempfile,uuid
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--apparmor-profile',choices=['devdesk-harness-v1']);args=p.parse_args()
report={'passed':False,'inference_tested':False,'credential_canary_only':True,'seccomp':'moby-default + nested namespace/mount syscalls'}
reference=os.environ.get('DEVDESK_TEST_CODEX_IMAGE','devdesk/codex:0.167.0')
args.output.parent.mkdir(parents=True,exist_ok=True)
try:
 inspected=subprocess.run(['docker','image','inspect',reference],capture_output=True,text=True,timeout=15)
 assert inspected.returncode==0
 image=json.loads(inspected.stdout)[0]
 assert image['Os']=='linux' and image['Architecture'] in ['amd64','arm64']
 identity=image['Id'];assert identity.startswith('sha256:') and len(identity)==71
except (OSError,subprocess.TimeoutExpired,ValueError,KeyError,IndexError,AssertionError) as error:
 report.update(stage='image_resolution',failure_type=type(error).__name__)
 args.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2));raise SystemExit(1)
report.update(image=identity,image_reference=reference,os=image['Os'],architecture=image['Architecture'])
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
 name='devdesk-nested-probe-'+uuid.uuid4().hex
 command=['docker','create','--name',name,'--label','devdesk.probe=nested-sandbox','--network=none','--read-only','--user',f'{os.getuid()}:{os.getgid()}',
          '--cap-drop=ALL','--security-opt=no-new-privileges','--security-opt',f'seccomp={ROOT}/permissions/backends/docker/seccomp/nested-v1.json',
          '--pids-limit=128','--memory=512m','--tmpfs','/tmp:rw,noexec,nosuid,size=64m',
          '--mount',f'type=bind,source={state},target=/state','--mount',f'type=bind,source={work},target=/work',
          '--entrypoint','/bin/sh',identity,'-c',
          'printf "outer_apparmor="; cat /proc/self/attr/current; exec codex sandbox -- /bin/sh -c "$1"','probe',
          '(! cat /state/codex/auth.json 2>/dev/null) && printf native-sandbox-ok > /work/proof']
 try:
  if args.apparmor_profile:
   report['stage']='apparmor_policy'
   expected=(ROOT/'permissions/backends/docker/apparmor/devdesk-harness-v1.apparmor').read_bytes()
   installed=Path('/etc/apparmor.d/devdesk-harness-v1')
   assert installed.read_bytes()==expected,'installed AppArmor policy differs from reviewed source'
   report['apparmor_policy_sha256']=hashlib.sha256(expected).hexdigest()
   command[2:2]=['--security-opt','apparmor='+args.apparmor_profile]
  report['stage']='container'
  created=subprocess.run(command,capture_output=True,text=True,timeout=30)
  assert created.returncode==0,'dedicated diagnostic container creation failed'
  receipt=created.stdout.strip();assert len(receipt)==64 and all(c in '0123456789abcdef' for c in receipt)
  report['container_id']=receipt;report['container_name']=name
  container=json.loads(subprocess.check_output(['docker','inspect',name],text=True))[0]
  assert container['Id']==receipt and container['Config']['Labels']['devdesk.probe']=='nested-sandbox'
  report['security']={'user':container['Config']['User'],'cap_drop':container['HostConfig']['CapDrop'],'no_new_privileges':'no-new-privileges' in container['HostConfig']['SecurityOpt'],'seccomp_sha256':hashlib.sha256((ROOT/'permissions/backends/docker/seccomp/nested-v1.json').read_bytes()).hexdigest(),'read_only_root':container['HostConfig']['ReadonlyRootfs'],'privileged':container['HostConfig']['Privileged'],'network':container['HostConfig']['NetworkMode'],'mount_targets':[m['Destination'] for m in container['Mounts']]}
  result=subprocess.run(['docker','start','-a',name],capture_output=True,text=True,timeout=30)
  container=json.loads(subprocess.check_output(['docker','inspect',name],text=True))[0]
  report['apparmor_profile']=container['AppArmorProfile']
  report['process_policy']=result.stdout.strip()
  report['exit_code']=result.returncode
  # No real credentials exist in this diagnostic container.
  if result.returncode:report['diagnostic']=result.stderr[-1800:]
  report['passed']=result.returncode==0 and (work/'proof').read_text()=='native-sandbox-ok'
  report['work_write_proven']=report['passed'];report['credential_read_denied']=report['passed']
  report['stage']='finished'
 except Exception as e:report['failure_type']=type(e).__name__
 finally:
  # Only this probe's receipt proves ownership; never select by an image/tag.
  if report.get('container_id'):
   removed=subprocess.run(['docker','rm','-f',report['container_id']],capture_output=True,timeout=30)
   report['cleanup_verified']=removed.returncode==0 and subprocess.run(['docker','inspect',report['container_id']],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode!=0
   report['passed']=report['passed'] and report['cleanup_verified']
args.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2));raise SystemExit(0 if report['passed'] else 1)
