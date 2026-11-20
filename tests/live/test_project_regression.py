"""Counterexamples to false-green end-to-end acceptance; no live services."""
import hashlib
import unittest

from verify_project_regression import check_confirmation, check_merge, check_test, check_unassisted, read_output


class ActualExecutionTests(unittest.TestCase):
    def setUp(self):
        self.result = {
            'status': 'succeeded', 'skipped': False, 'candidate': 'tested',
            'container_id': 'actual-container', 'writer_revoked': True,
            'image_digest': 'sha256:pinned', 'expected_failure': False,
            'counts': {'tests': 2, 'passed': 2, 'failed': 0}, 'exit_code': 0,
            'execution': {'command': 'node --test suite.cjs', 'cwd': '/work',
                          'files': [{'path': 'suite.cjs', 'content': 'original'}], 'exit_code': 0},
            'report': 'original report',
            'report_sha256': hashlib.sha256(b'original report').hexdigest(),
        }

    def test_actual_execution_passes(self):
        check_test(self.result, 'tested')

    def test_successfully_skipped_node_cannot_prove_regression(self):
        self.result.update(skipped=True, counts={'tests': 0, 'passed': 0, 'failed': 0})
        with self.assertRaisesRegex(AssertionError, 'skipped'):
            check_test(self.result, 'tested')

    def test_different_candidate_cannot_reuse_green_result(self):
        with self.assertRaisesRegex(AssertionError, 'candidate mismatch'):
            check_test(self.result, 'new candidate')

    def test_missing_execution_or_stop_proof_is_rejected(self):
        for key, value in [('execution', None), ('container_id', None), ('writer_revoked', False)]:
            result = {**self.result, key: value}
            with self.subTest(key=key), self.assertRaises(AssertionError):
                check_test(result, 'tested')

    def test_empty_or_partial_test_suite_is_rejected(self):
        for counts in [{'tests': 0}, {'tests': 2, 'passed': 1, 'failed': 0, 'skipped': 1}]:
            with self.subTest(counts=counts), self.assertRaises(AssertionError):
                check_test({**self.result, 'counts': counts}, 'tested')

    def test_environment_failure_is_not_real_red(self):
        result = {**self.result, 'expected_failure': True, 'exit_code': 1,
                  'counts': {'tests': 2, 'failed': 2}, 'assertion_failed': False}
        with self.assertRaisesRegex(AssertionError, 'business assertion'):
            check_test(result, 'tested', red=True)
        result['assertion_failed'] = True
        check_test(result, 'tested', red=True)

    def test_modified_report_is_rejected(self):
        self.result['report'] += ' invented success'
        with self.assertRaisesRegex(AssertionError, 'digest'):
            check_test(self.result, 'tested')


class ExactDeliveryTests(unittest.TestCase):
    def test_maintenance_cannot_be_reported_as_an_unassisted_run(self):
        controller = {'id': 'manager', 'epoch': 1}
        confirm = ('workflow_decision', 'root', {'root_instance_id': 'root',
                   'actor': {'kind': 'human'}, 'operation': 'confirm'})
        check_unassisted([confirm], 'root', {'root', 'task'}, controller)
        for item in [('controller_input', 'manager', {}), ('workflow_change', 'task', {}),
                     ('workflow_decision', 'root', {**confirm[2], 'operation': 'dispatch'}),
                     ('workflow_human_decision', 'decision', {'request': {'instance_id': 'task', 'operation': 'workflow.change.apply'}})]:
            with self.subTest(item=item), self.assertRaises(AssertionError):
                check_unassisted([confirm, item], 'root', {'root', 'task'}, controller)

    def test_stale_human_signature_and_exception_are_rejected(self):
        step = {'id': 'current', 'input_digest': 'digest', 'outputs': {'result': 'output'}}
        decision = {'execution_id': 'current', 'input_digest': 'digest',
                    'outputs': {'result': 'output'}, 'actor': {'kind': 'human'}}
        check_confirmation(decision, step)
        for changed in [{'execution_id': 'old'}, {'outputs': {'result': 'old'}},
                        {'actor': {'kind': 'controller'}}, {'override_completion': True}]:
            with self.subTest(changed=changed), self.assertRaises(AssertionError):
                check_confirmation({**decision, **changed}, step)

    def test_merge_and_sync_must_both_use_tested_candidate(self):
        publication = {'revision': 'tested'}
        merged = {'publication': publication, 'revision': 'tested', 'synchronized': True,
                  'pull_request': {'merged': True, 'merge_commit': 'tested'},
                  'sync': {'revision': 'tested', 'synchronized': True}}
        check_merge(merged, publication)
        for changed in [{'publication': {'revision': 'different'}}, {'synchronized': False},
                        {'sync': {'revision': 'old', 'synchronized': True}},
                        {'pull_request': {'merged': False, 'merge_commit': None}}]:
            with self.subTest(changed=changed), self.assertRaises(AssertionError):
                check_merge({**merged, **changed}, publication)


class OutputReadTests(unittest.TestCase):
    def test_large_utf8_output_is_read_completely_and_corruption_rejected(self):
        body = '"' + '中' * 30000 + '"'
        chunks = [body[:15001], body[15001:]]
        digest = hashlib.sha256(body.encode()).hexdigest()
        class FakeClient:
            def __init__(self):
                self.index = 0
            def rpc(self, method, params):
                chunk = chunks[self.index]
                self.index += 1
                end = params['offset'] + len(chunk.encode())
                return {'outputs': [{'id': 'output', 'offset': params['offset'],
                    'next_offset': end if self.index == 1 else None, 'total_bytes': len(body.encode()),
                    'value_chunk': chunk, 'data': {'provenance': {'value_digest': digest}}}]}
        self.assertEqual(read_output(FakeClient(), 'instance', 'output')['value'], '中' * 30000)
        chunks[1] = chunks[1].replace('中', '错', 1)
        with self.assertRaisesRegex(AssertionError, 'Incomplete output'):
            read_output(FakeClient(), 'instance', 'output')


if __name__ == '__main__':
    unittest.main()
