#!/usr/bin/env python3
"""Snapshot source changes for another platform without including runtime data/secrets."""
import hashlib,io,json,subprocess,tarfile
from pathlib import Path
root=Path(__file__).resolve().parents[2]
files=subprocess.check_output(['git','ls-files','-z','--cached','--others','--exclude-standard'],cwd=root).decode().split('\0')
entries=[]
for name in sorted(set(filter(None,files))):
 relative=Path(name)
 if relative.parts[0] in {'data','dist','target','.git','chat'} or 'node_modules' in relative.parts or '__pycache__' in relative.parts or relative.suffix=='.pyc' or relative.name in {'.DS_Store'}:continue
 path=root/relative
 if not path.exists():continue
 if path.is_symlink():raise RuntimeError('source archive refuses symlinks: '+name)
 if path.is_file():entries.append((name,hashlib.sha256(path.read_bytes()).hexdigest()))
state={'base_commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'scope':'working source snapshot; no runtime data, credentials, build artifacts or Git metadata','files':[{'path':name,'sha256':digest}for name,digest in entries]}
output=root/'dist/devdesk-worktree-source.tar.gz';output.parent.mkdir(exist_ok=True)
with tarfile.open(output,'w:gz') as archive:
 for name,_ in entries:archive.add(root/name,arcname='devdesk-source/'+name,recursive=False)
 data=(json.dumps(state,indent=2)+'\n').encode();info=tarfile.TarInfo('devdesk-source/SOURCE_STATE.json');info.size=len(data);info.mode=0o644;archive.addfile(info,io.BytesIO(data))
digest=hashlib.sha256(output.read_bytes()).hexdigest();output.with_name(output.name+'.sha256').write_text(digest+'  '+output.name+'\n');print(str(output));print('source_files='+str(len(entries)))
