"""Only an actual bounded manager change extends the observer's recovery grace."""
import copy
import hashlib
import unittest

from workflow_observer import manager_activity


class ManagerActivityTests(unittest.TestCase):
    def setUp(self):
        self.view = {"record": {"revision": 4, "data": {
            "session_id": "same-session", "status": "active"}}, "calls": []}

    def test_record_change_is_visible_without_internal_cursor_fields(self):
        changed = copy.deepcopy(self.view)
        changed["record"]["revision"] += 1
        self.assertNotEqual(manager_activity(self.view), manager_activity(changed))

    def test_actual_native_call_registration_and_completion_are_visible(self):
        changed = copy.deepcopy(self.view)
        changed["calls"] = [{"id": "call", "data": {"status": "prepared"}}]
        self.assertNotEqual(manager_activity(self.view), manager_activity(changed))
        finished = copy.deepcopy(changed)
        finished["calls"][0]["data"]["status"] = "succeeded"
        self.assertNotEqual(manager_activity(changed), manager_activity(finished))

    def test_unrelated_project_cursor_does_not_extend_manager_activity(self):
        changed = copy.deepcopy(self.view)
        changed["cursor"] = 10000
        self.assertEqual(manager_activity(self.view), manager_activity(changed))

    def test_same_view_remains_stable(self):
        self.assertEqual(manager_activity(self.view), manager_activity(copy.deepcopy(self.view)))



class ReviewRequestTests(unittest.TestCase):
    def setUp(self):
        import json
        from types import SimpleNamespace
        self.json = json
        self.current = {'id': 'gate', 'step': 'gate', 'plan_revision_id': 'plan', 'status': 'waiting_confirmation', 'input_digest': 'digest',
                        'outputs': {'report': 'output'}, 'session_id': 'session', 'turn_id': 'turn'}
        self.tree = {'root': {'runtime': {'project_id': 'project', 'owner_id': 'owner',
                     'active_plan_revision_id': 'plan', 'steps': [self.current]}}}
        self.reference = {'label': 'report', 'purpose': 'review', 'instance_id': 'root',
                          'execution_id': 'gate', 'output_id': 'output', 'owner_id': 'owner',
                          'session_id': 'session', 'turn_id': 'turn'}
        self.request = {'id': 'request', 'definition_id': 'root', 'data': {'status': 'pending',
                        'plan_revision_id': 'plan', 'request': {'arguments': {
                            'execution_id': 'gate', 'plan_revision_id': 'plan',
                            'input_digest': 'digest', 'outputs': {'report': 'output'}},
                        'interaction': {**{k: 'review' for k in ('title', 'background',
                            'requested_action', 'completed_work', 'review_summary')},
                            'materials': [self.reference], 'evidence': []}}}}
        self.contract = {'inputs': {}}
        self.reads = []
        def rpc(method, params):
            self.reads.append((method, params))
            self.assertEqual(method, 'workflow.execution.inspect')
            self.assertEqual(params['project_id'], 'project')
            if params.get('part') == 'contract':
                body = json.dumps(self.contract)
                return {'value_chunk': body, 'next_offset': None, 'offset': 0,
                        'execution_id': 'gate', 'plan_revision_id': 'plan', 'total_bytes': len(body.encode()),
                        'value_digest': hashlib.sha256(body.encode()).hexdigest()}
            return self.current
        self.client = SimpleNamespace(rpc=rpc)

    def check(self, request=None):
        from workflow_observer import check_human_request
        check_human_request(self.client, self.tree, request or self.request)

    def test_current_execution_and_output_can_be_in_either_category(self):
        self.check()
        packet = self.request['data']['request']['interaction']
        packet['materials'], packet['evidence'] = [], [self.reference]
        self.check()
        packet['materials'] = [self.reference]
        self.check()

    def test_plan_change_checks_exact_preview_without_a_step_gate(self):
        from types import SimpleNamespace
        from workflow_observer import ReviewBasisError
        self.tree['root']['runtime']['control_version'] = 2
        arguments = {'instance_id': 'root', 'plan_revision_id': 'plan', 'control_version': 2,
                     'changeset': {'plan_revision_id': 'plan', 'reason': 'repair',
                                   'operations': [{'type': 'remove_step', 'node_id': 'extra'}]},
                     'preview_hash': 'exact-preview'}
        self.request['data']['request'].update(operation='workflow.change.apply', arguments=arguments)
        original = self.client.rpc
        self.client = SimpleNamespace(rpc=lambda method, params:
            {'preview_hash': 'exact-preview', 'changeset': arguments['changeset']}
            if method == 'workflow.change.preview' else original(method, params))
        self.check()
        arguments['preview_hash'] = 'different-preview'
        with self.assertRaises(ReviewBasisError):
            self.check()

    def test_missing_current_execution_or_output_never_allows_review(self):
        from workflow_observer import ReviewBasisError
        packet = self.request['data']['request']['interaction']
        packet['materials'] = []
        with self.assertRaises(ReviewBasisError): self.check()
        packet['evidence'] = [{k: v for k, v in self.reference.items() if k != 'output_id'}]
        with self.assertRaises(ReviewBasisError): self.check()

    def test_wrong_root_or_context_is_rejected(self):
        from workflow_observer import ReviewBasisError
        for key in ('instance_id', 'owner_id', 'session_id', 'turn_id', 'output_id'):
            with self.subTest(key=key):
                request = copy.deepcopy(self.request)
                request['data']['request']['interaction']['materials'][0][key] = 'foreign'
                with self.assertRaises((ReviewBasisError, KeyError)): self.check(request)

    def test_wrong_project_is_rejected(self):
        from workflow_observer import ReviewBasisError
        foreign = copy.deepcopy(self.tree['root'])
        foreign['runtime']['project_id'] = 'foreign'
        self.tree['foreign'] = foreign
        self.request['data']['request']['interaction']['evidence'] = [{**self.reference, 'instance_id': 'foreign'}]
        with self.assertRaises(ReviewBasisError): self.check()

    def test_stale_plan_input_outputs_or_replaced_execution(self):
        from workflow_observer import ReviewBasisError
        for key, value in [('plan_revision_id', 'old'), ('input_digest', 'old'), ('outputs', {})]:
            with self.subTest(key=key):
                request = copy.deepcopy(self.request)
                request['data']['request']['arguments'][key] = value
                with self.assertRaises(ReviewBasisError): self.check(request)
        self.tree['root']['runtime']['steps'] = []
        with self.assertRaises(ReviewBasisError): self.check()

    def test_missing_upstream_and_invalid_chunk_cursor_block_review(self):
        from workflow_observer import ReviewBasisError
        self.contract['inputs'] = {'source': {'type': 'source', 'source': {'step': 'producer', 'port': 'result'}}}
        self.tree['root']['runtime']['steps'].append({'id': 'upstream', 'step': 'producer', 'outputs': {'result': 'original'}})
        with self.assertRaises(ReviewBasisError): self.check()
        self.contract['inputs'] = {}
        old_rpc = self.client.rpc
        self.client.rpc = lambda method, p: ({'value_chunk': '{}', 'next_offset': 65536} if p.get('part') else old_rpc(method, p))
        with self.assertRaises(ReviewBasisError): self.check()

    def test_unicode_contract_chunks_keep_the_same_exact_source(self):
        from workflow_observer import ReviewBasisError
        body = self.json.dumps({'inputs': {}, 'task_instructions': '材料😀' * 20}, ensure_ascii=False)
        chunks = [body[:16], body[16:55], body[55:]]
        offsets = [0, len(chunks[0].encode()), len((chunks[0]+chunks[1]).encode())]
        digest = hashlib.sha256(body.encode()).hexdigest()
        original = self.client.rpc
        def read(method, params):
            if not params.get('part'): return original(method, params)
            index = offsets.index(params.get('offset', 0))
            if index: self.assertEqual(params['value_digest'], digest)
            return {'offset': offsets[index], 'value_chunk': chunks[index], 'value_digest': digest,
                    'next_offset': offsets[index+1] if index < 2 else None, 'total_bytes': len(body.encode()),
                    'execution_id': 'gate', 'plan_revision_id': 'plan'}
        self.client.rpc = read
        self.check()
        chunks[-1] = chunks[-1].replace('材料', '伪造')
        with self.assertRaises(ReviewBasisError): self.check()

    def test_both_probe_call_sites_preserve_before_checking(self):
        import ast
        from pathlib import Path
        for name in ('probe_streamlined_workflow.py', 'probe_child_approvals.py'):
            tree = ast.parse(Path(__file__).with_name(name).read_text())
            calls = [n for n in ast.walk(tree) if isinstance(n, ast.Call) and
                     isinstance(n.func, ast.Name) and n.func.id == 'observe_human_request']
            self.assertEqual(len(calls), 1, name)
            self.assertIsInstance(calls[0].args[-1], ast.Lambda)
            self.assertEqual(calls[0].args[-1].body.func.id, 'check_human_request')

    def test_failed_diagnostic_preserves_packet_and_observes_the_next_gate(self):
        import tempfile
        from pathlib import Path
        from workflow_observer import observe_human_request
        with tempfile.TemporaryDirectory() as directory:
            self.contract['inputs'] = {'source': {'type': 'source', 'source': {'step': 'producer', 'port': 'result'}}}
            self.tree['root']['runtime']['steps'].append({'id': 'upstream', 'step': 'producer', 'outputs': {'result': 'original'}})
            first = observe_human_request(directory, self.request, 'http://local:1', self.check)
            self.assertFalse(first['diagnostic_passed'])
            self.assertEqual(self.json.loads(Path(first['path']).read_text())['request'], self.request)
            self.assertIn('Missing upstream output', Path(first['path']).with_suffix('.validation.jsonl').read_text())
            self.contract['inputs'] = {}
            second_request = copy.deepcopy(self.request)
            second_request['id'] = 'next-request'
            second = observe_human_request(directory, second_request, 'http://local:1', self.check)
            self.assertTrue(second['diagnostic_passed'])
            self.assertEqual(self.request['data']['status'], 'pending')

    def test_resume_port_change_preserves_original_packet_and_still_checks_identity(self):
        import tempfile
        from pathlib import Path
        from workflow_observer import observe_human_request
        with tempfile.TemporaryDirectory() as directory:
            first = observe_human_request(directory, self.request, 'http://local:1', self.check)
            original = Path(first['path']).read_bytes()
            resumed = observe_human_request(directory, self.request, 'http://local:2', self.check)
            self.assertTrue(resumed['diagnostic_passed'])
            changed = copy.deepcopy(self.request)
            changed['data']['request']['arguments']['input_digest'] = 'changed'
            self.assertFalse(observe_human_request(directory, changed, 'http://local:2', self.check)['diagnostic_passed'])
            self.assertEqual(Path(first['path']).read_bytes(), original)

    def test_invalid_request_identity_cannot_escape_the_evidence_directory(self):
        import tempfile
        from pathlib import Path
        from workflow_observer import observe_human_request
        with tempfile.TemporaryDirectory() as directory:
            changed = copy.deepcopy(self.request)
            changed['id'] = '../outside'
            result = observe_human_request(directory, changed, 'http://local', self.check)
            self.assertFalse(result['diagnostic_passed'])
            self.assertIsNone(result['path'])
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_original_is_durable_before_any_failure_and_never_overwritten(self):
        import tempfile
        from pathlib import Path
        from workflow_observer import preserve_human_request, ReviewBasisError
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'human-request-request.json'
            def fail(saved):
                self.assertEqual(self.json.loads(path.read_text())['request'], saved)
                raise RuntimeError('read failed')
            with self.assertRaises(ReviewBasisError):
                preserve_human_request(directory, self.request, 'http://local', fail)
            self.assertIn('read failed', path.with_suffix('.validation.jsonl').read_text())
            preserve_human_request(directory, self.request, 'http://local', lambda r: self.check(r))
            changed = copy.deepcopy(self.request)
            changed['data']['status'] = 'stale'
            with self.assertRaises(ReviewBasisError):
                preserve_human_request(directory, changed, 'http://local', lambda r: None)
            self.assertEqual(self.json.loads(path.read_text())['request'], self.request)


if __name__ == "__main__":
    unittest.main()
