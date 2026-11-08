import hashlib,json,pathlib,shutil,subprocess,tarfile,tempfile
ctx=json.load(open('/tmp/devdesk-ubuntu-acceptance-context.json'));r=pathlib.Path(ctx['reports']);prior=json.load(open(r/'package-before-final-documentation.json'));archive=pathlib.Path(prior['package']);verified=pathlib.Path(prior['host']);assert hashlib.sha256(verified.read_bytes()).hexdigest()==prior['host_sha256']
backup=pathlib.Path('dist/ubuntu-20261108T095147Z/rebuilt-before-verified-host-reuse');backup.mkdir(parents=True,exist_ok=True);shutil.copy2(archive,backup/archive.name);shutil.copy2(str(archive)+'.sha256',backup/(archive.name+'.sha256'))
with tempfile.TemporaryDirectory(prefix='devdesk-docs-repack-') as tmp:
 staging=pathlib.Path(tmp)
 with tarfile.open(archive) as tar:tar.extractall(staging,filter='data')
 directory=next(staging.iterdir());rebuilt=hashlib.sha256((directory/'devdesk-host').read_bytes()).hexdigest();shutil.copy2(verified,directory/'devdesk-host')
 subprocess.run(['tar','-czf',str(archive),'-C',str(staging),directory.name],check=True)
sha=hashlib.sha256(archive.read_bytes()).hexdigest();pathlib.Path(str(archive)+'.sha256').write_text(sha+'  '+archive.name+'\n')
receipt={'passed':True,'reason':'documentation-only repack explicitly reuses already model-tested Host bytes; a rebuild produced a different binary digest, so equivalence was not assumed','rebuilt_host_sha256':rebuilt,'chosen_verified_host_sha256':prior['host_sha256'],'verified_host_source':str(verified),'unverified_rebuild_preserved':str((backup/archive.name).resolve()),'final_archive_sha256':sha}
(r/'verified-host-repack.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt))
