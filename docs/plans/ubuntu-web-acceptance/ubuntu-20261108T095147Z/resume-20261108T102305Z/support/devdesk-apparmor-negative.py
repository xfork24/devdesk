import json,subprocess,uuid
from pathlib import Path
ctx=json.loads(Path('/tmp/devdesk-ubuntu-acceptance-context.json').read_text());r=Path(ctx['reports']);name='devdesk-aa-negative-'+uuid.uuid4().hex;report={'passed':False,'name':name,'scope':'non-root outer capabilities and rejection of unlisted mounts in a child user namespace'}
image=json.loads(subprocess.check_output(['docker','image','inspect','devdesk/dsh:0.2.2-rc.1'],text=True))[0]['Id']
command=['docker','create','--name',name,'--label','devdesk.probe=apparmor-negative','--network=none','--read-only','--user','1000:1000','--cap-drop=ALL','--security-opt=no-new-privileges','--security-opt','apparmor=devdesk-harness-v1','--security-opt','seccomp='+str(Path('containers/security/nested-sandbox.json').resolve()),'--pids-limit=128','--memory=256m','--tmpfs','/tmp:rw,noexec,nosuid,size=32m','--entrypoint','/bin/sh',image,'-c', '''grep -q '^CapEff:[[:space:]]*0000000000000000$' /proc/self/status && unshare -Ur -m --propagation unchanged /bin/sh -c 'mkdir -p /tmp/unapproved; if mount -t tmpfs tmpfs /tmp/unapproved; then exit 1; else echo UNLISTED_MOUNT_DENIED; fi' ''']
cid=None
try:
 cid=subprocess.check_output(command,text=True).strip();assert len(cid)==64;report['container']=cid
 process=subprocess.run(['docker','start','-a',cid],text=True,capture_output=True,timeout=30);report['exit_code']=process.returncode;report['stdout']=process.stdout;report['stderr']=process.stderr;inspection=json.loads(subprocess.check_output(['docker','inspect',cid],text=True))[0];report['profile']=inspection['AppArmorProfile'];report['cap_drop']=inspection['HostConfig']['CapDrop'];report['passed']=process.returncode==0 and 'UNLISTED_MOUNT_DENIED' in process.stdout and report['profile']=='devdesk-harness-v1'
finally:
 if cid:subprocess.run(['docker','rm','-f',cid],stdout=subprocess.DEVNULL,check=True)
 report['removed']=cid is not None and subprocess.run(['docker','inspect',cid],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode!=0
 (r/'apparmor-negative.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2));raise SystemExit(0 if report['passed'] else 1)
