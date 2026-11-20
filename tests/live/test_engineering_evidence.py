"""Offline verifier regressions; no model, credentials, Docker or network calls."""
import json
import copy
import hashlib
import sqlite3
import unittest
from contextlib import closing
from engineering_evidence import frozen_document, verify_chunk_reads, verify_source_contract


class FrozenDocumentTests(unittest.TestCase):
    def test_original_source_is_selected_even_after_a_new_revision_exists(self):
        with closing(sqlite3.connect(':memory:')) as connection:
            connection.execute('CREATE TABLE source_definitions(reference TEXT, type TEXT, data TEXT)')
            original = {'steps': [{'key': 'required', 'confirmation': 'human'}]}
            changed = {'steps': []}
            connection.executemany('INSERT INTO source_definitions VALUES (?, ?, ?)', [
                ('flow:1', 'workflow', json.dumps(original)),
                ('flow:2', 'workflow', json.dumps(changed)),
            ])
            frozen = {'workflow_ref': 'flow:1', 'plan': {'$source': 'workflow_plan', 'workflow_ref': 'flow:1'}}
            self.assertEqual(frozen_document(connection, frozen), original)
            frozen['plan']['workflow_ref'] = 'flow:2'
            with self.assertRaisesRegex(AssertionError, 'does not match'):
                frozen_document(connection, frozen)
            frozen['plan']['workflow_ref'] = 'flow:1'
            connection.execute("DELETE FROM source_definitions WHERE reference='flow:1'")
            with self.assertRaisesRegex(AssertionError, 'must remain available'):
                frozen_document(connection, frozen)


class PreservedContractTests(unittest.TestCase):
    def setUp(self):
        self.source = {'steps': [{'key': 'review', 'confirmation': 'human',
                                 'completion': {'op': 'always'}, 'dependencies': [],
                                 'spec_refs': [], 'required_skill_refs': [],
                                 'executor': {'type': 'control'}, 'outputs': {}}]}
        self.plan = copy.deepcopy(self.source)

    def test_original_human_gate_passes(self):
        verify_source_contract(self.source, self.plan)

    def test_removing_human_gate_is_rejected(self):
        self.plan['steps'][0]['confirmation'] = 'automatic'
        with self.assertRaisesRegex(AssertionError, 'Human confirmation'):
            verify_source_contract(self.source, self.plan)

    def test_removing_mandatory_node_is_rejected(self):
        self.plan['steps'] = []
        with self.assertRaisesRegex(AssertionError, 'mandatory source node'):
            verify_source_contract(self.source, self.plan)

    def test_replacing_completion_rule_is_rejected(self):
        self.plan['steps'][0]['completion'] = {'op': 'any', 'rules': []}
        with self.assertRaisesRegex(AssertionError, 'completion rule'):
            verify_source_contract(self.source, self.plan)

    def test_inserted_producer_preserves_mandatory_ordering(self):
        prepare = {**copy.deepcopy(self.source['steps'][0]), 'key': 'prepare', 'dependencies': []}
        self.source['steps'].insert(0, prepare)
        self.source['steps'][1]['dependencies'] = ['prepare']
        self.plan = copy.deepcopy(self.source)
        self.plan['steps'].append({'key': 'extra', 'dependencies': ['prepare']})
        self.plan['steps'][1]['dependencies'] = ['extra']
        verify_source_contract(self.source, self.plan)
        self.plan['steps'][-1]['dependencies'] = []
        with self.assertRaisesRegex(AssertionError, 'prerequisite'):
            verify_source_contract(self.source, self.plan)

    def test_prerequisite_cycle_cannot_substitute_for_ordering(self):
        self.plan['steps'][0]['dependencies'] = ['review']
        with self.assertRaisesRegex(AssertionError, 'cycle'):
            verify_source_contract(self.source, self.plan)

    def test_workflow_independence_and_delivery_requirements_cannot_be_removed(self):
        for field, value in [('completion', {'op': 'independent', 'author': 'tests', 'reviewer': 'implementation'}),
                             ('delivery_policy', {'independent_green_step': 'green'})]:
            source = {**self.source, field: value}
            plan = {**self.plan, field: None}
            with self.subTest(field=field), self.assertRaisesRegex(AssertionError, 'mandatory workflow'):
                verify_source_contract(source, plan)

    def test_domain_defaults_do_not_look_like_a_changed_contract(self):
        source = {**self.source, 'delivery_policy': {'code_review_step': 'review'},
                  'outputs': {'result': {'step': 'review', 'port': 'result'}}}
        plan = {**self.plan, 'delivery_policy': {'code_review_step': 'review', 'children_step': None},
                'outputs': {'result': {'step': 'review', 'port': 'result', 'pointer': ''}}}
        verify_source_contract(source, plan)

    def test_altering_an_independent_review_prompt_is_rejected(self):
        self.source['steps'][0]['task_instructions'] = '独立核对原始材料与证据'
        self.plan['steps'][0]['task_instructions'] = '无条件批准'
        with self.assertRaisesRegex(AssertionError, 'review prompt'):
            verify_source_contract(self.source, self.plan, True)


class ChunkReadTests(unittest.TestCase):
    def setUp(self):
        self.expected = {'text': '中' * 15000}
        raw = json.dumps(self.expected, ensure_ascii=False, separators=(',', ':')).encode()
        digest = hashlib.sha256(raw).hexdigest()
        self.current = {'id': 'execution', 'turn_id': 'turn', 'session_id': 'session'}
        self.reference = {'read_tool': 'devdesk_workflow_outputs_read', 'complete': False,
                          'source_output_id': 'output', 'pointer': '', 'total_bytes': len(raw),
                          'value_digest': digest,
                          'arguments': {'ids': ['output'], 'max_bytes': 32768, 'offset': 0, 'pointer': ''}}
        self.calls, self.wire = [], []
        offset = 0
        while offset < len(raw):
            end = min(offset + 32768, len(raw))
            while True:
                try:
                    body = raw[offset:end].decode()
                    break
                except UnicodeDecodeError:
                    end -= 1
            arguments = {'ids': ['output'], 'max_bytes': 32768, 'offset': offset, 'pointer': ''}
            caller = {'execution_id': 'execution', 'turn_id': 'turn', 'session_id': 'session', 'call_id': str(offset)}
            receipt = {'outputs': [{'id': 'output', 'offset': offset, 'next_offset': end if end < len(raw) else None,
                                    'total_bytes': len(raw), 'value_chunk': body,
                                    'data': {'provenance': {'value_digest': digest}}}]}
            self.calls.append({'status': 'succeeded', 'request': {'caller': caller, 'arguments': arguments,
                               'definition': {'qualified_name': 'workflow.outputs.read'}}, 'receipt': receipt})
            self.wire.append({'journal': 'native.jsonl', 'item': {'id': str(offset), 'type': 'dynamicToolCall',
                              'tool': 'devdesk_workflow_outputs_read', 'status': 'completed', 'success': True,
                              'arguments': copy.deepcopy(arguments),
                              'contentItems': [{'type': 'inputText', 'text': json.dumps(receipt)}]}})
            offset = end

    def verify(self):
        return verify_chunk_reads(self.reference, self.expected, 'output', self.current, self.calls, self.wire)

    def test_complete_native_reads_reconstruct_utf8_input(self):
        proof = self.verify()
        self.assertTrue(proof['native_delivery_verified'])
        self.assertEqual(len(proof['chunks']), 2)
        self.assertEqual(sum(chunk['utf8_bytes'] for chunk in proof['chunks']), self.reference['total_bytes'])

    def batched_first_read(self):
        raw = json.dumps(self.expected, ensure_ascii=False, separators=(',', ':')).encode()
        end = 16384
        while True:
            try:
                raw[:end].decode()
                break
            except UnicodeDecodeError:
                end -= 1
        self.calls[0]['request']['arguments']['ids'].append('another-output')
        first = self.calls[0]['receipt']['outputs'][0]
        first.update(value_chunk=raw[:end].decode(), next_offset=end)
        self.calls[0]['receipt']['outputs'].append({'id': 'another-output', 'data': {'value': True}})
        self.calls[1]['request']['arguments']['offset'] = end
        second = self.calls[1]['receipt']['outputs'][0]
        second.update(offset=end, value_chunk=raw[end:].decode(), next_offset=None)
        self.sync_wire()

    def sync_wire(self):
        for call, wire in zip(self.calls, self.wire):
            wire['item']['arguments'] = copy.deepcopy(call['request']['arguments'])
            wire['item']['contentItems'][0]['text'] = json.dumps(call['receipt'])

    def test_batched_read_shares_budget_and_reconstructs_complete_input(self):
        self.batched_first_read()
        self.assertTrue(self.verify()['native_delivery_verified'])

    def test_batched_read_still_requires_actual_native_delivery(self):
        self.batched_first_read()
        self.wire.pop(0)
        with self.assertRaisesRegex(AssertionError, 'absent from native journal'):
            self.verify()

    def test_batched_read_cannot_use_the_entire_budget_for_one_input(self):
        self.batched_first_read()
        raw = json.dumps(self.expected, ensure_ascii=False, separators=(',', ':')).encode()
        end = 32768
        while True:
            try:
                raw[:end].decode()
                break
            except UnicodeDecodeError:
                end -= 1
        self.calls[0]['receipt']['outputs'][0].update(value_chunk=raw[:end].decode(), next_offset=end)
        self.sync_wire()
        with self.assertRaisesRegex(AssertionError, 'Returned end differs'):
            self.verify()

    def test_batched_read_requires_every_requested_output(self):
        self.batched_first_read()
        self.calls[0]['receipt']['outputs'].pop()
        self.sync_wire()
        with self.assertRaisesRegex(AssertionError, 'outputs differ from requested IDs'):
            self.verify()

    def test_requested_offset_inside_utf8_character_uses_returned_boundary(self):
        self.calls[-1]['request']['arguments']['offset'] += 1
        self.wire[-1]['item']['arguments']['offset'] += 1
        self.assertTrue(self.verify()['native_delivery_verified'])

    def test_normalized_offset_must_match_the_actual_utf8_boundary(self):
        self.calls[-1]['request']['arguments']['offset'] += 4
        self.wire[-1]['item']['arguments']['offset'] += 4
        with self.assertRaisesRegex(AssertionError, 'UTF-8 boundary'):
            self.verify()

    def rejected_read(self, code='schema_validation'):
        arguments = {'ids': ['output'], 'max_bytes': 1000, 'offset': 0, 'pointer': ''}
        error = {'code': code, 'message': 'invalid read argument'}
        caller = {**self.calls[0]['request']['caller'], 'call_id': 'rejected'}
        self.calls.insert(0, {'status': 'failed', 'error': error, 'request': {
            'caller': caller, 'arguments': arguments,
            'definition': {'qualified_name': 'workflow.outputs.read'}}})
        self.wire.insert(0, {'journal': 'native.jsonl', 'item': {
            'id': 'rejected', 'type': 'dynamicToolCall', 'tool': 'devdesk_workflow_outputs_read',
            'status': 'failed', 'success': False, 'arguments': copy.deepcopy(arguments),
            'contentItems': [{'type': 'inputText', 'text': json.dumps(error)}]}})

    def test_rejected_schema_read_is_preserved_before_complete_successful_reads(self):
        self.rejected_read()
        proof = self.verify()
        self.assertTrue(proof['native_delivery_verified'])
        self.assertEqual(proof['rejected_reads'][0]['error']['code'], 'schema_validation')

    def test_unknown_read_failure_cannot_be_ignored(self):
        self.rejected_read('provider_timeout')
        with self.assertRaises(AssertionError):
            self.verify()

    def test_rejected_read_also_requires_its_native_reply(self):
        self.rejected_read()
        self.wire.pop(0)
        with self.assertRaisesRegex(AssertionError, 'absent from native journal'):
            self.verify()

    def test_missing_chunk_is_rejected(self):
        self.calls.pop()
        with self.assertRaisesRegex(AssertionError, 'missing chunk'):
            self.verify()

    def test_another_turn_cannot_supply_missing_reads(self):
        self.calls[-1]['request']['caller']['turn_id'] = 'another-turn'
        with self.assertRaisesRegex(AssertionError, 'missing chunk'):
            self.verify()

    def test_durable_receipt_without_native_delivery_is_rejected(self):
        self.wire.pop()
        with self.assertRaisesRegex(AssertionError, 'absent from native journal'):
            self.verify()

    def test_truncated_native_response_is_rejected(self):
        self.wire[0]['item']['contentItems'][0]['text'] = '{}'
        with self.assertRaisesRegex(AssertionError, 'Complete read receipt absent'):
            self.verify()

    def test_tampered_digest_is_rejected(self):
        self.reference['value_digest'] = '0' * 64
        with self.assertRaisesRegex(AssertionError, 'provenance digest differs'):
            self.verify()

    def test_wrong_source_is_rejected(self):
        self.reference['source_output_id'] = 'different-output'
        with self.assertRaises(AssertionError):
            self.verify()

    def test_wrong_handoff_is_rejected(self):
        self.expected = {'text': 'different handoff'}
        with self.assertRaisesRegex(AssertionError, 'sealed handoff'):
            self.verify()


if __name__ == '__main__':
    unittest.main()
