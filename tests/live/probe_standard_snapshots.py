#!/usr/bin/env python3
"""Real two-Task Standard adoption, historical switch, tests and Git in a disposable dataset."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time
import uuid
from client import Client

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--codex-auth-source', type=Path, required=True)
parser.add_argument('--github-repo', required=True)
parser.add_argument('--report-directory', type=Path)
parser.add_argument('--resume', action='store_true', help='Recover the same preserved dataset without recreating work or evidence')
parser.add_argument('--timeout-seconds', type=int, default=10800)
args = parser.parse_args()
assert 'test' in args.github_repo.lower(), 'use a dedicated test repository'
out = (args.report_directory or Path(tempfile.mkdtemp(prefix='devdesk-standard-real-'))).resolve()
out.mkdir(parents=True, exist_ok=True)
assert args.resume == (out / 'context.json').exists(), 'select --resume for an existing acceptance dataset'
private = out / 'devdesk-host'
shutil.copy2(args.binary.resolve(), private)
process = None
context = json.loads((out / 'context.json').read_text()) if args.resume else {}
project = context.get('project')
report = {'passed': False, 'models_called': True, 'repository': args.github_repo}


def boot():
    global process
    log = out / f'host-{time.time_ns()}.log'
    process = subprocess.Popen([str(private), '--data', str(out / 'data'), '--port', '0'], stdout=log.open('w'), stderr=subprocess.STDOUT)
    for _ in range(300):
        urls = [line for line in log.read_text().splitlines() if line.startswith('http://')]
        if urls:
            return Client(urls[0], project)
        if process.poll() is not None:
            raise RuntimeError('Host startup failed; inspect private host log')
        time.sleep(.1)
    raise TimeoutError('Host startup')


def control(c, method, instance, payload=None):
    view = c.rpc('query.workflow_instance', {'id': instance})
    return c.command(method, instance, view['record']['revision'], {'plan_revision_id': view['runtime']['active_plan_revision_id'], 'control_version': view['runtime']['control_version'], **(payload or {})})


def manager_input(c, manager, text):
    row = c.rpc('query.orchestration', {'id': manager})['record']
    c.command('orchestration.input', manager, row['revision'], {'epoch': row['data']['epoch'], 'input': text})


def wait(c, root, manager, predicate, phase):
    deadline = time.monotonic() + args.timeout_seconds
    previous = None
    while time.monotonic() < deadline:
        workspace = c.rpc('query.workspace', {'feature_id': feature, 'limit': 100})
        instances = [r for r in workspace['configurations'] if r['kind'] == 'workflow_instance' and r['data']['root_instance_id'] == root]
        current = next(r for r in instances if r['id'] == root)
        controller = c.rpc('query.orchestration', {'id': manager})
        state = [(r['data']['owner_id'], s['step'], s['status']) for r in instances for s in r['data']['steps'] if s['status'] not in ['queued', 'completed']]
        if state != previous:
            print(phase, state, flush=True)
            previous = state
        (out / 'latest.json').write_text(json.dumps({'workspace': workspace, 'controller': controller}, ensure_ascii=False, indent=2))
        assert controller['record']['data']['status'] not in ['failed', 'budget_exhausted', 'needs_reconciliation'], 'Controller did not finish normally'
        for row in instances:
            for step in row['data']['steps']:
                assert step['status'] not in ['failed', 'needs_reconciliation', 'cancelled', 'waiting_input', 'waiting_confirmation'], (phase, step['path'], step['status'], step.get('reason'))
        if predicate(workspace, instances, current):
            session = controller['record']['data'].get('session_id')
            if not session or not any(t['status'] in ['queued', 'dispatching', 'running', 'waiting_interaction', 'needs_reconciliation'] for t in c.rpc('query.session', {'id': session})['turns']):
                return workspace, instances, current
        time.sleep(.7)
    raise TimeoutError(phase)


def publish(c, base, document, name):
    draft = c.command('workflow.draft.create', base['id'], base['revision'], {'document': document, 'name': name})
    return c.command('workflow.publish', draft['id'], draft['revision'])


try:
    print('Evidence:', out, flush=True)
    c = boot()
    auth = out / 'data/credentials/codex/auth.json'
    auth.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(args.codex_auth_source.resolve(), auth)
    auth.chmod(0o600)
    if args.resume:
        feature, root, manager = (context[key] for key in ['feature', 'root', 'manager'])
        before = json.loads((out / 'before-switch.json').read_text())
        child_a, child_b = before['a'], before['b']
        ta, tb = ({'id': child['data']['owner_id']} for child in [child_a, child_b])
        sb = context['sb']
        original_standard = child_a['data']['plan']['standard_ref']
        old_plan = child_a['data']['active_plan_revision_id']
        old_executions = [step for step in child_a['data']['steps'] if step['started']]
        c.open(project)
        if (out / 'report.json').exists():
            (out / 'report.json').rename(out / f'report-before-resume-{time.time_ns()}.json')
        current = c.rpc('query.workflow_instance', {'id': root})
        if current['runtime']['status'] != 'completed':
            row = c.rpc('query.orchestration', {'id': manager})['record']
            c.command('orchestration.takeover', manager, row['revision'], {'epoch': row['data']['epoch']})
            def stopped(session):
                return all(execution['writer_revoked'] for execution in session['executions']) and all(turn['status'] not in ['queued', 'dispatching', 'running', 'waiting_interaction', 'needs_reconciliation'] for turn in session['turns'])
            sid = c.rpc('query.orchestration', {'id': manager})['record']['data'].get('session_id')
            if sid:
                c.wait(lambda: c.rpc('query.session', {'id': sid}), stopped, 120)
            workspace = c.rpc('query.workspace', {'feature_id': feature, 'limit': 100})
            instances = [r for r in workspace['configurations'] if r['kind'] == 'workflow_instance' and r['data']['root_instance_id'] == root]
            recovery = []
            for row in instances:
                for step in row['data']['steps']:
                    if step['status'] in ['needs_reconciliation', 'cancelled']:
                        definition = next(s for s in row['data']['plan']['document']['steps'] if s['key'] == step['step'])
                        assert definition['executor']['type'] == 'agent', 'External tool effects need dedicated reconciliation'
                        payload = {'step': step['step'], 'reason': '同数据集恢复：停止旧执行者，保留中断记录及已有真实证据，再执行未完成步骤'}
                        if step['status'] == 'needs_reconciliation':
                            control(c, 'workflow.execution.stop', row['id'], payload)
                        c.wait(lambda: c.rpc('query.session', {'id': step['session_id']}), stopped, 120)
                        fresh = c.rpc('query.workflow_instance', {'id': row['id']})
                        if next(s for s in fresh['runtime']['steps'] if s['step'] == step['step'])['status'] == 'needs_reconciliation':
                            control(c, 'workflow.step.reconcile', row['id'], payload)
                        control(c, 'workflow.step.retry', row['id'], payload)
                        recovery.append({'instance': row['id'], 'execution': step['id'], 'step': step['step'], 'previous_status': step['status'], 'action': 'stop/reconcile/retry'})
                    elif step['status'] == 'failed':
                        # This query has no external mutation to reconcile. Business/test
                        # failures are never converted to success or silently bypassed.
                        assert step['step'] == 'target' and step['reason'] == '工作空间不属于当前执行范围', step
                        control(c, 'workflow.step.retry', row['id'], {'step': step['step'], 'reason': '已修复真实 Task 读取所属 Feature 目标的权限边界，保留失败事实并重试只读查询'})
                        recovery.append({'instance': row['id'], 'execution': step['id'], 'step': step['step'], 'previous_status': 'failed', 'action': 'retry read-only query after code fix'})
            (out / f'recovery-{time.time_ns()}.json').write_text(json.dumps(recovery, ensure_ascii=False, indent=2))
            if c.rpc('query.workflow_instance', {'id': root})['runtime']['paused']:
                control(c, 'workflow.resume', root)
            row = c.rpc('query.orchestration', {'id': manager})['record']
            c.command('orchestration.resume', manager, row['revision'], {'epoch': row['data']['epoch']})
            manager_input(c, manager, '当前是同数据集恢复：真实 Task 读取所属 Feature 的 target 权限边界已修复；失败查询与中断的业务步骤已通过公开恢复命令重试，旧执行及真实证据均保留。继续已有两项 Task 及 Feature，保留已完成节点与 Standard 切换历史。中断的实现保留已有源码和真实 TDD 记录，按合同补齐证据，不伪造或重写旧记录。完成全部测试、独立评审、候选测试、Git 接纳与发布，显式 complete 子流程及根流程。禁止修改计划、豁免或跳过检查。')
    else:
        project = c.command('project.open_github', payload={'repository': args.github_repo, 'mainline': 'main', 'title': 'Standard snapshot engineering acceptance'})['id']
        c.project = project
        c.wait(lambda: c.rpc('query.workspace'), lambda value: value['repository_ready'], 300)
        image = c.rpc('runtime.prepare', {'provider': 'codex'})['artifact']['image_id']
        defaults = c.rpc('query.configuration_defaults')
        feature_source = c.rpc('query.workflow', {'id': defaults['workflow_ref']})
        task_source = c.rpc('query.workflow', {'id': defaults['task_workflow_ref']})
        feature_doc = json.loads(json.dumps(feature_source['data']))
        goal_spec=c.command('spec.create',payload={'name':'Engineering goal input','data':{'schema_version':3,'body':'输入应完整提供本次工程目标与验收事实。','checks':[{'key':'goal_input','name':'工程目标','instructions':'核对 instructions 是完整、非空的工程目标。'}]}})
        goal_delivery=c.command('delivery_type.create',payload={'name':'Engineering goal','data':{'format':'json','spec_refs':[goal_spec['id']],'content_schema':{'type':'object','required':['instructions'],'properties':{'instructions':{'type':'string','minLength':1}}}}})
        feature_doc.setdefault('inputs', {})['goal'] = {'delivery_ref': goal_delivery['id']}
        first = next(s for s in feature_doc['steps'] if s['executor']['type'] == 'agent')
        first.setdefault('inputs', {})['goal'] = {'type': 'source', 'source': {'step': '$input', 'port': 'goal'}}
        invocation = next(s for s in feature_doc['steps'] if s['executor']['type'] == 'workflow' and s['executor'].get('work_binding'))
        invocation['executor']['foreach']['max_concurrency'] = 2
        wf = publish(c, feature_source, feature_doc, 'Two independent work selections')
        sa = c.command('standard.capture', payload={'name': 'Reusable engineering snapshot A', 'root_workflow_refs': [wf['id']], 'scope': {'kind': 'global'}})

        extra = c.command('spec.create', payload={'name': 'Additional traceability check', 'data': {'schema_version': 3, 'body': '逐项核对本次评审所用材料及真实测试回执的确切来源。', 'checks': [{'key': 'standard_b_trace', 'name': '材料与测试来源', 'instructions': '检查当前输入材料和真实测试回执确实对应被评审版本，给出具体来源；不得将文字自述当作测试通过。'}]}})
        task_doc = json.loads(json.dumps(task_source['data']))
        review = next(s for s in task_doc['steps'] if s['executor']['type'] == 'agent' and any(port.get('content_checks') for port in s.get('outputs', {}).values()))
        review.setdefault('spec_refs', []).append(extra['id'])
        wb = publish(c, task_source, task_doc, 'Additional traceability requirement')
        sb = c.command('standard.capture', payload={'name': 'Reusable engineering snapshot B', 'root_workflow_refs': [wb['id']], 'scope': {'kind': 'global'}})
        context = next(r['data'] for r in defaults['records'] if r['id'] == defaults['permission_context_ref'])

        def permission(task_ref, name):
            data = json.loads(json.dumps(context))
            for grant in data['step_grants']:
                if grant['workflow_ref'] == feature_source['id']:
                    grant['workflow_ref'] = wf['id']
                if grant['workflow_ref'] == task_source['id']:
                    grant['workflow_ref'] = task_ref
            return c.command('permission_context.create', payload={'name': name, 'scope': {'kind': 'project', 'project_id': project}, 'data': data})['id']

        pa, pb = permission(task_source['id'], 'Original work authorization'), permission(wb['id'], 'Selected work authorization')
        current = c.rpc('query.workspace')['active_project']
        c.command('project.configuration.update', project, current['revision'], {'workflow_ref': wf['id'], 'standard_ref': sa['id'], 'role_agent_bindings': defaults['role_agent_bindings'], 'orchestrator_role_ref': defaults['orchestrator_role_ref'], 'permission_context_ref': pa})
        suffix = uuid.uuid4().hex[:8]
        directory = f'standard-{suffix}'
        goal = f'''在专用测试仓库实现两个互不依赖的 Task，稳定 key 为 add 和 twice，名称分别为 Add 和 Twice。
    Task add 仅实现 src/{directory}/add.mjs，export add(a,b)：两个有限数相加；非数值、NaN 或 Infinity 抛 TypeError。
    Task twice 仅实现 src/{directory}/twice.mjs，export twice(n)：有限数乘二；非数值、NaN 或 Infinity 抛 TypeError。
    Task 独立测试分别为 tests/{directory}-add/behavior.test.mjs 和 tests/{directory}-twice/behavior.test.mjs，静态 import 路径分别为 ../../src/{directory}/add.mjs 和 ../../src/{directory}/twice.mjs。
    开发职责自有增量测试分别位于 test/{directory}-add/increments.test.mjs 和 test/{directory}-twice/increments.test.mjs，分正常计算、无效输入拒绝两个增量保留真实 Red/Green。
    Feature 全量测试为 tests/{directory}-feature/behavior.test.mjs，覆盖两个导出；最终和候选配方同时运行两份 Task 独立测试及 Feature 全量测试。
    所有测试使用 node:test 和 node:assert/strict，无依赖安装；runner=node_test，image={image}，timeout_seconds=60。
    只允许上述源码、独立测试和各开发职责自有测试路径，禁止修改既有无关文件。缺少模块或导出由 red_baseline 准备最小可调用接口，不能用捕获 import 错误、伪造返回值或 assert.fail 制造 Red。
    按当前工作流完整提交文档、独立评审、测试与 Git 回执，不跳过、豁免或修改工作流及检查项。所有真实运行 JSON 和代码快照按当前 evidence 端口交付。业务事实完整，无需等待用户补充。'''
        feature = c.command('feature.create', payload={'project_id': project, 'title': 'Two Tasks with distinct immutable Standards', 'workflow_inputs': {'goal': {'instructions': goal}}})['id']
        root = c.rpc('query.workflow_instance', {'owner_id': feature})['record']['id']
        initial = c.rpc('query.workflow_instance', {'id': root})
        manager = c.command('orchestration.open', root, initial['record']['revision'])['id']
        (out / 'context.json').write_text(json.dumps({'project': project, 'feature': feature, 'root': root, 'manager': manager, 'sa': {'id': sa['id']}, 'sb': {'id': sb['id']}, 'wf': {'id': wf['id']}, 'wb': {'id': wb['id']}, 'pa': pa, 'pb': pb, 'original_binary_sha256': hashlib.sha256(private.read_bytes()).hexdigest()}, ensure_ascii=False, indent=2))
        manager_input(c, manager, '首个业务 Agent 的 inputs.goal 已包含完整任务事实。按当前工作流完成需求、设计、规划和真实 Task 创建并接纳创建结果。必须恰好创建 Add、Twice 两个 Task。之后立即 workflow.pause 根流程并结束 Turn，保持 Task 子调用未派发；等待验收入口设置第二项工作的规范。不要修改计划、绕过或豁免。')
        workspace, instances, paused = wait(c, root, manager, lambda ws, rows, parent: len(ws['detail']['tasks']) == 2 and parent['data']['paused'] and len(rows) == 1, 'Task creation')
        tasks = workspace['detail']['tasks']
        ta, tb = sorted(tasks, key=lambda t: t['title'])
        original_standard = c.rpc('query.work_configuration', {'owner_id': ta['id']})['configuration']['data']['standard_ref']
        assert original_standard != sb['id']
        c.command('work.configuration.update', tb['id'], tb['revision'], {'standard_ref': sb['id'], 'workflow_ref': wb['id'], 'permission_context_ref': pb, 'role_agent_bindings': defaults['role_agent_bindings']})
        assert c.rpc('query.workflow_instance', {'id': root})['runtime']['plan']['standard_ref'] == paused['data']['plan']['standard_ref']
        control(c, 'workflow.resume', root)
        manager_input(c, manager, '第二个 Task 已显式采用另一份 Standard，按各自冻结配置执行。先派发 Task 子调用；分别完成两个子流程的 workspace、tests 和 tests_snapshot 必要节点并真实接纳。只到独立测试源码快照已固定为止，不进入 red_baseline 或实现；两项均完成 tests_snapshot 后暂停根流程并结束 Turn 等待。两个子调用允许同时存在，项目并发额度仍由平台限制。禁止更改计划、检查、绑定或豁免。')
        def tests_frozen(ws, rows, parent):
            children = [r for r in rows if r['data']['owner_id'] in [ta['id'], tb['id']]]
            return parent['data']['paused'] and len(children) == 2 and all(any(s['step'] == 'tests_snapshot' and s['status'] == 'completed' for s in r['data']['steps']) for r in children)
        workspace, instances, paused = wait(c, root, manager, tests_frozen, 'Two real Task executions')
        child_a = next(r for r in instances if r['data']['owner_id'] == ta['id'])
        child_b = next(r for r in instances if r['data']['owner_id'] == tb['id'])
        assert child_a['data']['plan']['standard_ref'] == original_standard
        assert child_b['data']['plan']['standard_ref'] == sb['id']
        old_plan = child_a['data']['active_plan_revision_id']
        old_executions = [s for s in child_a['data']['steps'] if s['started']]
        old_view = c.rpc('query.workflow_instance', {'id': child_a['id']})
        (out / 'before-switch.json').write_text(json.dumps({'a': child_a, 'b': child_b, 'outputs': old_view['outputs']}, ensure_ascii=False, indent=2))
        changes = {'plan_revision_id': old_plan, 'reason': '本次修改显式切换规范并保留原始测试依据', 'operations': [{'type': 'adopt_standard', 'standard_ref': sb['id'], 'workflow_ref': wb['id'], 'permission_context_ref': pb}]}
        preview = c.rpc('workflow.change.preview', {'id': child_a['id'], 'changeset': changes})
        control(c, 'workflow.change.apply', child_a['id'], {'changeset': changes, 'preview_hash': preview['preview_hash']})
        for execution in old_executions:
            actual = c.rpc('workflow.execution.inspect', {'id': child_a['id'], 'execution_id': execution['id']})
            assert actual['plan_revision_id'] == execution['plan_revision_id'] and actual['input_digest'] == execution['input_digest'] and actual['outputs'] == execution['outputs']
        process.send_signal(signal.SIGINT)
        process.wait(timeout=30)
        c = boot()
        after = c.rpc('query.workflow_instance', {'id': child_a['id']})
        assert after['runtime']['plan']['standard_ref'] == sb['id']
        history = c.rpc('query.work_configuration', {'owner_id': ta['id']})
        assert next(r for r in history['history'] if r['id'] == old_plan)['data']['standard_ref'] == original_standard
        for execution in old_executions:
            actual = c.rpc('workflow.execution.inspect', {'id': child_a['id'], 'execution_id': execution['id']})
            assert actual['plan_revision_id'] == execution['plan_revision_id'] and actual['outputs'] == execution['outputs']
        control(c, 'workflow.resume', root)
        manager_row=c.rpc('query.orchestration', {'id':manager})['record']
        if manager_row['data']['status']=='paused':
            c.command('orchestration.resume',manager,manager_row['revision'],{'epoch':manager_row['data']['epoch']})
        manager_input(c, manager, '验收入口已为第一项 Task 显式切换 Standard，旧执行和测试快照已保存并经重启核验。继续当前活动计划，保留所有历史依据；按声明流程完成两项 Task 及 Feature 的真实 TDD、独立评审、全量和候选测试、Git 接纳与发布，显式 complete 所有子流程与根流程。不得绕过、豁免、改规范或伪造测试/Git/人类回执。')
    workspace, instances, final = wait(c, root, manager, lambda ws, rows, parent: parent['data']['status'] == 'completed', 'Engineering completion')
    assert len(workspace['detail']['tasks']) == 2 and all(t['status'] == 'completed' for t in workspace['detail']['tasks'])
    all_sessions = []
    for instance in instances:
        for execution in instance['data']['steps']:
            if execution.get('session_id'):
                session = c.rpc('query.session', {'id': execution['session_id'], 'limit': 100})
                all_sessions.extend(json.loads(t['input']) for t in session['turns'] if t['input'].startswith('{') and json.loads(t['input']).get('renderer_version') == 'agent-task/v6')
    assert any(t['source'].get('standard_ref') == sb['id'] and any(check['key'] == 'standard_b_trace' for spec in t['specifications'] for check in spec['data'].get('checks', [])) for t in all_sessions)
    # Actual receipts are read from the database ledger, never inferred from model text.
    import sqlite3
    database = out / 'data/projects' / project / 'engineering.sqlite3'
    with sqlite3.connect(database) as connection:
        calls = [json.loads(row[0]) for row in connection.execute("SELECT data FROM governance_records WHERE kind='platform_call'")]
    succeeded = [r for r in calls if r['status'] == 'succeeded']
    names = [r['request']['definition']['qualified_name'] for r in succeeded]
    assert names.count('tests.run') >= 6 and names.count('git.advance') >= 3 and names.count('git.publish') >= 3
    from verify_standard_history import verify
    integrity = verify(out)
    (out / 'integrity-report.json').write_text(json.dumps(integrity, ensure_ascii=False, indent=2))
    report['history_integrity'] = integrity
    report.update(passed=True, project=project, feature=feature, tasks=[ta['id'], tb['id']], initial_standards=[original_standard, sb['id']], switched_plan=old_plan, original_executions=[s['id'] for s in old_executions], native_task_count=len(all_sessions), test_receipts=names.count('tests.run'), git_advances=names.count('git.advance'), git_publications=names.count('git.publish'), binary_sha256=hashlib.sha256(private.read_bytes()).hexdigest())
    (out / 'final.json').write_text(json.dumps(workspace, ensure_ascii=False, indent=2))
    print('PASS real two-Task Standard adoption, historical switch, restart and engineering receipts', flush=True)
except BaseException as error:
    report['error'] = str(error)
    raise
finally:
    (out / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2))
    if process is not None and process.poll() is None:
        process.send_signal(signal.SIGINT)
        process.wait(timeout=30)
    shutil.rmtree(out / 'data/credentials', ignore_errors=True)
    for path in (out / 'data').rglob('auth.json'):
        path.unlink(missing_ok=True)
    print('Evidence:', out, flush=True)
