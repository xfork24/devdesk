#!/usr/bin/env python3
"""Actual provider tool invocation must work while private state remains inaccessible."""
from client import Client
from pathlib import Path
import json,argparse
p=argparse.ArgumentParser();p.add_argument('--provider',required=True,choices=['codex','dsh']);a=p.parse_args();r=json.loads(Path('docs/reports/real-integration/R5-planning.json').read_text());c=Client();c.open(r['project_id']);v=c.feature(r['feature_id'])
report={'provider':a.provider,'passed':False,'project_id':r['project_id']}
try:
 prompt='Use the confined shell tool once to run exactly: printf "DEVDESK_TOOL_EXECUTED\\n"; pwd . Do not request broader permissions or modify files. Return ONLY JSON {"tool_ran":boolean} based on actual tool execution, false if unavailable.'

 sid,tid,state=c.discuss(r['feature_id'],v['feature']['revision'],'analyst',a.provider,prompt)
 events=[e for e in state['events'] if e['turn_id']==tid and e['mode']=='full'];answer=next(e['content'] for e in reversed(events) if e['kind']=='assistant').strip().removeprefix('```json').removesuffix('```').strip();value=json.loads(answer)
 tool=[e for e in events if e['kind']=='tool' and 'DEVDESK_TOOL_EXECUTED' in e['content'] and '/work' in e['content'] and json.loads(e['content']).get('type') not in ['userMessage','reasoning']]
 assert value['tool_ran'] and tool,'Actual confined tool did not prove hidden state'
 report.update(passed=True,session=sid,turn=tid,tool_events=[e['id'] for e in tool],protocol_artifacts=[e['protocol_artifact'] for e in state['executions'] if e['protocol_artifact']])
except Exception as e:report.update(failure_type=type(e).__name__,failure=str(e));raise
finally:
 Path('docs/reports/real-integration/R3-'+a.provider+'-tools.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
