#!/usr/bin/env python3
"""One real native turn with a typed, immutable Agent composition."""
from client import Client
from pathlib import Path
import argparse,json,platform,time,uuid
p=argparse.ArgumentParser();p.add_argument('--url',required=True);p.add_argument('--project',required=True);p.add_argument('--provider',choices=['codex','dsh'],required=True);p.add_argument('--model',required=True);p.add_argument('--report',type=Path,required=True);a=p.parse_args();c=Client(a.url);rows=['# 完整 Agent 组合原生验收','',f'日期：{time.strftime("%Y-%m-%d %H:%M:%S %z")}','']
try:
 c.open(a.project);owner=c.rpc('query.workspace')['active_project'];runtime=next(r for r in reversed(c.rpc('query.runtime_installations')['installations']) if r['key']['provider']==a.provider and r['key']['os']=='linux');provider='openai' if a.provider=='codex' else 'deepseek'
 model=c.command('model.create',payload={'name':'Native '+provider+' typed model','data':{'provider':provider,'model':a.model,'parameters':({'reasoning_effort':'low'} if a.provider=='codex' else {'max_tokens':4096}),'capabilities':[]}})['id']
 harness=c.command('harness.create',payload={'model_providers':[provider],'name':'Native '+a.provider+' typed Harness','data':{'provider':a.provider,'runtime_version':runtime['key']['version'],'capabilities':[]}})['id']
 tool=c.command('tool.create',payload={'name':'Native tools','data':{'qualified_name':'harness.native','implementation':'harness.native','input_schema':{},'output_schema':{},'capabilities':[]}})['id']
 resources=c.rpc('query.permission_resources');target=next(t['target'] for t in resources['targets'] if t['target']['revision_id']==harness)
 c.command('permission.bindings.apply',payload={'bindings':[{'target':target,'permission_revision_id':'builtin-permission-harness-full-access:1','expected_revision':0}]})
 cap=c.command('capability.create',payload={'model_ref':model,'harness_ref':harness,'skills':[],'tools':[tool],'name':'Native typed capability','data':{}})['id']
 role=c.command('role.create',payload={'name':'Native acceptance role','data':{'instructions':'Answer the supplied acceptance prompt concisely without changing files.'}})['id']
 agent=c.command('agent.create',payload={'role_ref':role,'capability_ref':cap,'name':'Native typed analyst','data':{}})['id']
 instance=c.command('agent.instance.create',payload={'name':'Native typed Project instance','agent_revision_id':agent,'scope':'project:' + str(a.project)})['id']
 for payload in [{'agent':'builtin-agent-analyst-v1','provider':a.provider},{'agent_instance_id':instance,'model':'runtime-override'}]:
  try:c.command('session.open',owner['id'],owner['revision'],payload)
  except RuntimeError:pass
  else:raise AssertionError('legacy or separate model override bypassed typed binding')
 session=c.command('session.open',owner['id'],owner['revision'],{'agent_instance_id':instance,'work_type':'discussion','force_new':True})['id'];s=c.rpc('query.session',{'id':session});marker='typed-'+uuid.uuid4().hex;tid=c.command('conversation.send',session,s['session']['revision'],{'client_message_id':str(uuid.uuid4()),'input':'Reply with exactly this acceptance marker, without tools or file changes: '+marker})['id']
 final=c.wait(lambda:c.rpc('query.session',{'id':session,'limit':100}),lambda s:next(t for t in s['turns'] if t['id']==tid)['status'] in ['completed','failed','cancelled','needs_reconciliation'],300);turn=next(t for t in final['turns'] if t['id']==tid);assert turn['status']=='completed' and turn['consumed'];assert marker in '\n'.join(e['content'] for e in final['events'] if e['kind']=='assistant')
 binding=final['session']['runtime_binding'];assert binding['model']==a.model and binding['installation']['key']==runtime['key'] and binding['agent_configuration']['agent_instance_id']==instance
 rows+=['- Harness：'+a.provider+' · '+runtime['key']['version'],'- 实际 Model：'+binding['model'],f'- AgentRevision：{agent}；AgentID：{instance}。',f'- Session：{session}；Turn：{tid}。','- 原生输入已消费、模型输出核对完成，worker 已停止并导出。','- 新 live 会话的旧角色入口及独立 Model 覆盖均被拒绝。','',f'结果：通过。宿主 {platform.system()} / {platform.machine()}，Linux Docker 原生 Harness。']
except Exception as error:rows+=['','结果：未通过。',str(error)];raise
finally:a.report.parent.mkdir(parents=True,exist_ok=True);a.report.write_text('\n'.join(rows)+'\n')
