#!/usr/bin/env python3
"""Resumeable real role discussion -> adopted plan -> native planning Runs/Reviews -> two worktrees."""
from client import Client
import argparse,json,time
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--url',default='http://127.0.0.1:51751');p.add_argument('--provider',default='codex',choices=['codex','dsh']);p.add_argument('--output',type=Path,default=Path('docs/plans/real-engineering-integration/R5-planning.json'));p.add_argument('--reports',type=Path,default=Path('docs/plans/real-engineering-integration'));p.add_argument('--case-prefix',default='');a=p.parse_args();a.output=a.reports/a.output.name if a.output.parent==Path('docs/plans/real-engineering-integration') else a.output;assert len(a.case_prefix)<33 and all(ch.isalnum() or ch in '-_' for ch in a.case_prefix)
source=json.loads((a.reports/'R4-repository.json').read_text());project=source['project_id'];feature=source['feature_id'];c=Client(a.url);c.open(project)
report=json.loads(a.output.read_text()) if a.output.exists() else {'project_id':project,'feature_id':feature,'proposals':{},'passed':False,'case_prefix':a.case_prefix}
def save(stage):report['stage']=stage;a.output.write_text(json.dumps(report,indent=2,ensure_ascii=False)+'\n');print(stage,flush=True)
def view():return c.feature(feature)
def revision():return view()['feature']['revision']
def proposal(key,role,content):
 if key in report['proposals']:return report['proposals'][key]['document']
 save('discussion_'+key)
 if report.get('case_prefix'):
  content=json.loads(json.dumps(content,ensure_ascii=False).replace('src/add.mjs','src/'+report['case_prefix']+'add.mjs').replace('src/multiply.mjs','src/'+report['case_prefix']+'multiply.mjs').replace('tests/add/','tests/'+report['case_prefix']+'add/').replace('tests/multiply/','tests/'+report['case_prefix']+'multiply/'))
 prompt='Discuss and produce the following agreed engineering proposal. Return ONLY a complete ProposalContent JSON object (no markdown), preserving exact reference IDs, AC keys, module paths, recipe files/image/timeout, and independent task keys. Improve prose if needed. Do not use tools or modify files. '+json.dumps(content,ensure_ascii=False)
 sid,tid,state=c.discuss(feature,revision(),role,a.provider,prompt,work_type='design' if role=='architect' else 'discussion')
 draft=c.command('proposal.extract',payload={'turn_id':tid})['id'];adopt=c.command('proposal.adopt',draft,1)
 # adopted object is the formal document; proposal adoption cannot create Runs or assert Review.
 doc=adopt['objects'][0] if 'objects' in adopt else adopt.get('id')
 doc=adopt['objects'][0]
 report['proposals'][key]={'id':draft,'document':doc,'session':sid,'turn':tid};save('adopted_'+key);return doc
try:
 req=proposal('requirement','analyst',{'kind':'requirement','title':'安全整数算术导出','body':'## 范围\n提供两个独立的 Node ESM 纯函数：src/add.mjs 导出 add(a,b)，src/multiply.mjs 导出 multiply(a,b)。不引入依赖，不修改 README 之外的现有资源。\n## 验收\nAC-ADD: 安全整数相加；AC-MUL: 安全整数相乘。支持零、负数；不允许隐式转换。\n## 失败与错误路径\n任一输入不是 Number.isSafeInteger 时抛 TypeError（含字符串、NaN、Infinity、小数、非安全整数）。结果超出安全整数范围时抛 RangeError。不得通过改动测试掩盖失败。','acceptance':[{'key':'AC-ADD','description':'add 返回安全整数之和；错误类型与溢出按失败路径处理。'},{'key':'AC-MUL','description':'multiply 返回安全整数之积；错误类型与溢出按失败路径处理。'}]})
 design=proposal('design','architect',{'kind':'design','title':'独立 ESM 算术模块设计','requirement':req,'body':'## 接口与追踪\nsrc/add.mjs named export add(a,b) 对应 AC-ADD；src/multiply.mjs named export multiply(a,b) 对应 AC-MUL。两个文件互不依赖，不要求 package.json。先对每个输入用 Number.isSafeInteger 校验，失败抛 TypeError；计算后用 Number.isSafeInteger 检查结果，失败抛 RangeError。\n## 测试与目录\n两个 Tasks 无依赖，分别仅实现各自模块；Tester 分别在 tests/add/behavior.test.mjs 与 tests/multiply/behavior.test.mjs 编写 node:test。路径通过相对 ../../src/<module>.mjs 动态导入；缺少模块时使用 assert.fail 记录真实断言失败，不能把 import 环境错误当 Red。测试覆盖正常值、负数、零、错误输入与结果溢出。Developer 无权修改 tests。\n## 失败与风险\n没有网络和依赖安装。代码和文档只经 coordinator 封存，实际测试由独立无凭据容器运行。测试配方锁定相同镜像，Red/Green/verify 结果引用具体 SHA。'})
 installs=c.rpc('query.runtime_installations')['installations'];images=[i['artifact']['image_id'] for i in installs if i['key']['provider']=='codex' and i['key']['os']=='linux' and 'image_id' in i['artifact']];assert len(images)==1,'test image must be explicit and unique';image=images[0]
 def recipe(files):return {'runner':'node_test','files':files,'image':image,'timeout_seconds':60}
 plan=proposal('task_plan','architect',{'kind':'task_plan','requirement':req,'design':design,'tasks':[{'key':'add','title':'实现 src/add.mjs 的 add 安全整数加法','acceptance':['AC-ADD'],'dependencies':[],'test_recipe':recipe(['tests/add/behavior.test.mjs'])},{'key':'multiply','title':'实现 src/multiply.mjs 的 multiply 安全整数乘法','acceptance':['AC-MUL'],'dependencies':[],'test_recipe':recipe(['tests/multiply/behavior.test.mjs'])}]})
 proposal('test_plan','tester',{'kind':'test_plan','title':'算术完整行为测试计划','requirement':req,'design':design,'body':'## 验收映射\nAC-ADD → tests/add/behavior.test.mjs，AC-MUL → tests/multiply/behavior.test.mjs。各含正数、负数、零、字符串、NaN、Infinity、小数、不安全整数与结果溢出断言。\n## 失败与错误路径\n缺实现时 dynamic import 捕获并 assert.fail，作为真正断言 Red；语法错误、启动失败、无断言等不能当预期 Red。采用 node:test + node:assert/strict，代码无依赖、网络或额外命令。Green/verify 复验同一封存 SHA，Feature 全量运行两文件。','verifies':['AC-ADD','AC-MUL'],'test_recipe':recipe(['tests/add/behavior.test.mjs','tests/multiply/behavior.test.mjs'])})
 if 'attempt' not in report:
  report['attempt']=c.command('attempt.start',feature,revision(),{'phase':'planning','provider':a.provider})['id'];save('attempt_started')
 aid=report['attempt']
 def attempt(v):return next(vv for vv in v['attempts'] if vv['id']==aid)
 for kind in ['requirement','plan']:
  save('waiting_'+kind+'_runs')
  def ready(v):
   runs=[r for r in v['runs'] if r.get('attempt_id')==aid and not r['node'].startswith('review:') and (kind!='requirement' or r['node'] in ['research','requirement'])]
   failed=[r for r in runs if r['status'] in ['failed','cancelled','interrupted']]
   if failed:raise AssertionError('native stage failed '+json.dumps([{'id':r['id'],'node':r['node'],'result':r['result']} for r in failed]))
   return all(r['status']=='succeeded' for r in runs)
  v=c.wait(view,ready,600)
  d=next((d for d in v['deliveries'] if d.get('attempt_id')==aid and d['kind']==kind),None)
  if not d:
   did=c.command('delivery.submit',aid,attempt(v)['revision'],{'kind':kind})['id'];v=view();d=next(d for d in v['deliveries'] if d['id']==did)
  if d['id'] not in v['accepted_deliveries']:
   run=next((r for r in v['runs'] if r['node']=='review:'+d['id']),None)
   if not run:
    rid=c.command('review.request',d['id'],payload={'provider':a.provider})['id']
   else:rid=run['id']
   save('review_'+kind)
   def reviewed(v):
    run=next(r for r in v['runs'] if r['id']==rid)
    if run['status'] in ['failed','cancelled','interrupted']:raise AssertionError('review run failed '+str(run['result']))
    return run['status']=='succeeded'
   v=c.wait(view,reviewed,400);assert d['id'] in v['accepted_deliveries'], 'Reviewer requested changes'
  report[kind+'_delivery']=d['id'];save('accepted_'+kind)
 row=report['proposals']['task_plan'];ps=c.rpc('query.session',{'id':row['session']});proposal_row=next(v for v in ps['proposals'] if v['id']==row['id']);tasks=c.command('proposal.instantiate',row['id'],proposal_row['revision'])['tasks'];report['tasks']=tasks
 v=c.wait(lambda:c.rpc('query.repository',{'limit':100}),lambda v:all(any(w['owner_id']==tid for w in v['worktrees']) for tid in tasks.values()),60)
 report['worktrees']=[w for w in v['worktrees'] if w['owner_id'] in tasks.values()];assert len(report['worktrees'])==2
 report['initial_snapshot']=next(s['id'] for s in view()['snapshots'] if s['kind']=='initial');report['passed']=True;report.pop('failure',None);report.pop('failure_type',None);save('finished')
except Exception as e:
 report['failure_type']=type(e).__name__;report['failure']=str(e);save('failed');raise
