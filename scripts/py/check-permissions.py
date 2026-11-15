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
presets=[json.loads(p.read_text()) for p in sorted((root/'resources/configuration/permissions/profiles').glob('*.json'))]
assert len({(p['identity'],p['revision']) for p in presets})==len(presets)
for source in presets:
 assert source['type']=='permission_profile' and source['revision']>=1
 p=source['data']
 assert type(p['schema_version']) is int and p['schema_version'] >= 1
 required={'filesystem','network','tools','approval'} if p['target_kind']=='harness' else {'filesystem','network'}
 optional={'platform','tool_refs'} if p['target_kind']=='harness' else set()
 assert required <= set(p['policy']) <= required | optional
 assert p['policy']['filesystem']['mode'] in ['full_access','read_only','scoped']
 assert p['policy']['network']['mode'] in ['enabled','disabled','allowlist']
try:
 from jsonschema import Draft202012Validator
 from referencing import Registry, Resource
except ImportError as error:
 raise SystemExit('Permission Schema validation requires: python3 -m pip install -r scripts/check/requirements.txt') from error
# The default registry cannot retrieve remote resources. All references resolve
# against the checked-out schema set, independently of the caller's cwd.
schemas={p.name:json.loads(p.read_text()) for p in (root/'schemas').glob('*.schema.json')}
registry=Registry().with_resources((name,Resource.from_contents(schema)) for name,schema in schemas.items())
for name in ('permission-profile.schema.json','permission-backend.schema.json'):
 Draft202012Validator.check_schema(schemas[name])
def validate_source(value):
 Draft202012Validator(schemas['permission-profile.schema.json'],registry=registry).validate(value)
for p in presets:validate_source(p)
Draft202012Validator(schemas['permission-backend.schema.json'],registry=registry).validate(manifest)
print(f'PASS Permission assets: {len(presets)} presets, {len(manifest["backends"])} backends, {len(manifest["assets"])} immutable files')
