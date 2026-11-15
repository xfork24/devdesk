// Exercise the dependency gate with API responses; never call GitHub from tests.
import assert from 'node:assert/strict';
import test from 'node:test';
import gate from '../../.github/scripts/e2e-gate.cjs';

const repository = 'fixture/devdesk';
const run = (workflow, patch = {}) => ({
  id: workflow, workflow_id: workflow, run_number: 1, run_attempt: 1,
  head_sha: 'exact-sha', event: 'push', head_repository: { full_name: repository },
  status: 'completed', conclusion: 'success', ...patch,
});
async function ready(trigger, rust, web) {
  let result;
  const calls = [];
  await gate({
    context: { repo: { owner: 'fixture', repo: 'devdesk' }, payload: {
      repository: { full_name: repository }, workflow_run: trigger,
    } },
    github: { rest: { actions: { listWorkflowRuns: async (params) => {
      calls.push(params);
      assert.equal(params.head_sha, trigger.head_sha);
      assert.equal(params.event, trigger.event);
      return { data: { workflow_runs: params.workflow_id === 'rust.yml' ? rust : web } };
    } } } },
    core: { info() {}, setOutput(name, value) { assert.equal(name, 'ready'); result = value; } },
  });
  return { result, calls };
}
test('both workflows must succeed for the exact commit and event', async () => {
  assert.equal((await ready(run(1), [run(1)], [run(2)])).result, 'true');
  for (const patch of [
    { status: 'in_progress', conclusion: null }, { conclusion: 'failure' },
    { conclusion: 'cancelled' }, { head_sha: 'other-sha' }, { event: 'pull_request' },
    { head_repository: { full_name: 'fork/devdesk' } },
  ]) assert.equal((await ready(run(1), [run(1)], [run(2, patch)])).result, 'false');
  assert.equal((await ready(run(1), [run(1)], [])).result, 'false');
});
test('a failed or fork trigger does not inspect or execute source code', async () => {
  for (const patch of [{ conclusion: 'failure' }, { head_repository: { full_name: 'fork/devdesk' } }]) {
    const result = await ready(run(1, patch), [run(1)], [run(2)]);
    assert.equal(result.result, 'false');
    assert.deepEqual(result.calls, []);
  }
});
test('a newer failed run supersedes an older green run regardless of API ordering', async () => {
  assert.equal((await ready(run(1), [run(1)], [run(2), run(2, {
    id: 3, run_number: 2, conclusion: 'failure',
  })])).result, 'false');
  assert.equal((await ready(run(1), [run(1), run(1, {
    id: 3, run_number: 2,
  })], [run(2)])).result, 'false');
});
test('a stale completion cannot authorize a newer pending rerun', async () => {
  assert.equal((await ready(run(1), [run(1, {
    run_attempt: 2, status: 'in_progress', conclusion: null,
  })], [run(2)])).result, 'false');
  assert.equal((await ready(run(1, { run_attempt: 2 }), [run(1, {
    run_attempt: 2,
  })], [run(2)])).result, 'true');
});
