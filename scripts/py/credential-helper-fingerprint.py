#!/usr/bin/env python3
"""Only helper source and its resolved dependencies affect the helper installation.
Changes to Host/Core/web or unrelated Cargo.lock packages must not rebuild it.
"""
import hashlib,json,subprocess,sys
from pathlib import Path
metadata=json.loads(subprocess.check_output(['cargo','metadata','--locked','--format-version','1'],stderr=subprocess.DEVNULL));packages={p['id']:p for p in metadata['packages']};nodes={n['id']:n for n in metadata['resolve']['nodes']};root=next(p['id'] for p in metadata['packages'] if p['name']=='devdesk-credential-helper');pending=[root];seen=set()
while pending:
 current=pending.pop()
 if current in seen:continue
 seen.add(current);pending.extend(d['pkg'] for d in nodes[current]['deps'])
identity={'certificate':sys.argv[1].lower(),'dependencies':sorted([{'name':packages[i]['name'],'version':packages[i]['version'],'source':packages[i]['source']}for i in seen],key=lambda x:x['name'])}
h=hashlib.sha256(json.dumps(identity,sort_keys=True).encode())
for file in sorted(Path('crates/credential-helper').rglob('*')):
 if file.is_file():h.update(str(file).encode());h.update(file.read_bytes())
print(h.hexdigest())
