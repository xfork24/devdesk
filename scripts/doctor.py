#!/usr/bin/env python3
"""Read-only Web/live host preflight. Never reads credential contents."""
import argparse,json,os,platform,shutil,subprocess,sys
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--output',type=Path);p.add_argument('--binary',type=Path);a=p.parse_args();r={'platform':platform.system(),'architecture':platform.machine(),'client':'web','checks':[]}
def check(name,passed,detail):r['checks'].append({'name':name,'passed':bool(passed),'detail':detail})
def executes(argv):
 try:return subprocess.run(argv,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=12).returncode==0
 except (OSError,subprocess.TimeoutExpired):return False
check('host_platform',platform.system() in ['Darwin','Linux'],'macOS/Linux Web Host')
for command in ['git','gh','docker','codex']:
 found=shutil.which(command);check(command,found is not None,'found' if found else 'install '+command)
check('docker_daemon',executes(['docker','info']),'Docker Engine/Desktop must be running')
try:
 security=subprocess.run(['docker','info','--format','{{json .SecurityOptions}}'],capture_output=True,text=True,timeout=12)
 apparmor=security.returncode==0 and any(value.startswith('name=apparmor') for value in json.loads(security.stdout))
except (OSError,subprocess.TimeoutExpired,ValueError,TypeError):apparmor=False
if apparmor:
 installed=Path('/etc/apparmor.d/devdesk-harness-v1');expected=Path(__file__).resolve().parents[1]/'containers/security/devdesk-harness-v1.apparmor'
 try:
  metadata=installed.lstat();matches=not installed.is_symlink() and metadata.st_uid==0 and metadata.st_mode&0o022==0 and installed.read_bytes()==expected.read_bytes()
 except OSError:matches=False
 check('nested_apparmor_policy',matches,'review and install the fixed DevDesk profile on the Docker host; root file integrity only, actual sandbox execution still requires live tests')
check('github_authentication',executes(['gh','auth','status']),'gh account configured; no token is read or printed')
for provider,version in [('codex','0.167.0'),('dsh','0.2.2-rc.1')]:check(provider+'_linux_image',executes(['docker','image','inspect','devdesk/'+provider+':'+version]),'build the fixed runtime image on this Docker platform')
if platform.system()=='Linux':
 check('secret_tool',shutil.which('secret-tool') is not None,'Ubuntu: install libsecret-tools')
 check('desktop_session',bool(os.environ.get('DBUS_SESSION_BUS_ADDRESS')),'Secret Service requires a desktop/user D-Bus session; credential contents are not read')
 # A nonempty D-Bus address (including one inherited over SSH) is not proof
 # of a usable desktop vault. Read only collection metadata, never a secret
 # or an Unlock method, and bound a stalled service independently of Docker.
 try:
  metadata=subprocess.run(['busctl','--user','get-property','org.freedesktop.secrets','/org/freedesktop/secrets/aliases/default','org.freedesktop.Secret.Collection','Locked'],capture_output=True,text=True,timeout=3)
  available=metadata.returncode==0 and metadata.stdout.strip() in ['b false','b true']
  unlocked=available and metadata.stdout.strip()=='b false'
 except (OSError,subprocess.TimeoutExpired):available=unlocked=False
 check('secret_service',available,'busctl must reach the default Secret Service collection; no secret is read')
 check('default_collection_unlocked',unlocked,'unlock the desktop default collection; preflight never prompts or unlocks it')
elif platform.system()=='Darwin':
 identity=Path.home()/'Library/Application Support/DevDesk/signing/identity.txt';check('stable_signing_identity',identity.is_file() or bool(os.environ.get('DEVDESK_CODESIGN_IDENTITY')),'configure a stable code signing identity once')
 helper=Path.home()/'Library/Application Support/DevDesk/credential-helper/v1/devdesk-credential-helper'
 check('shared_credential_helper',helper.is_file() and executes(['/usr/bin/codesign','--verify','--strict',str(helper)]),'one fixed signed helper per user; Host builds reuse it')
 if a.binary:check('binary_signature',executes(['/usr/bin/codesign','--verify','--strict',str(a.binary.resolve())]),'signature integrity only; repeated-build Keychain access needs the actual credential acceptance test')
r['passed']=all(x['passed'] for x in r['checks']);value=json.dumps(r,indent=2)+'\n'
if a.output:a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(value)
print(value);raise SystemExit(0 if r['passed'] else 1)
