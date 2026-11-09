#!/usr/bin/env python3
"""Validate policy assets without credentials, Docker or model calls."""
import hashlib,json
from pathlib import Path
root=Path(__file__).resolve().parents[2]
manifest=json.loads((root/'permissions/backends/manifest.json').read_text())
assert manifest['schema_version']==1
for name,entry in manifest['assets'].items():
 path=root/'permissions/backends'/name
 assert not path.is_symlink() and path.is_file(),name
 assert hashlib.sha256(path.read_bytes()).hexdigest()==entry['sha256'],name
for backend in manifest['backends']:
 for name in backend['assets']:assert name in manifest['assets'],name
presets=[json.loads(p.read_text()) for p in sorted((root/'permissions/presets').glob('*.json'))]
assert len({p['id'] for p in presets})==len(presets)
for p in presets:
 assert p['schema_version'] in [1,2] and p['version']>=1
 assert set(p['policy'])==({'filesystem','network','tools'} if p['schema_version']==1 else {'filesystem','network','tools','approval'} if p['target_kind']=='harness' else {'filesystem','network'})
 assert p['policy']['filesystem']['mode'] in ['full_access','read_only','scoped']
 assert p['policy']['network']['mode'] in ['enabled','disabled','allowlist']
try:
 import jsonschema
 for p in presets:jsonschema.validate(p,json.loads((root/('schemas/permission.schema.json' if p['schema_version']==1 else 'schemas/permission-profile.schema.json')).read_text()))
 jsonschema.validate(manifest,json.loads((root/'schemas/permission-backend.schema.json').read_text()))
except ImportError:
 print('JSON Schema validation skipped: optional Python jsonschema package is not installed')
print(f'PASS Permission assets: {len(presets)} presets, {len(manifest["backends"])} backends, {len(manifest["assets"])} immutable files')
