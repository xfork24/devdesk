#!/usr/bin/env python3
"""Recheck a completed isolated engineering report without credentials or models."""
import argparse,json,re
from pathlib import Path
from engineering_evidence import verify_evidence

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('report_directory',type=Path)
args=parser.parse_args();root=args.report_directory.resolve()
views={path.stem:json.loads(path.read_text()) for path in root.glob('*.json') if re.fullmatch('[0-9a-f-]{36}',path.stem)}
assert views and all(view['runtime']['status']=='completed' for view in views.values())
class SavedViews:
    def rpc(self,method,params):
        assert method=='query.workflow_instance'
        return views[params['id']]
verify_evidence(SavedViews(),[{'id':ident} for ident in views],root)
print('PASS saved native engineering facts, raw execution, fixed Git blobs and exact PR versions')
