"""Offline verifier regressions; no model, credentials, Docker or network calls."""
import json
import copy
import hashlib
import sqlite3
import unittest
from engineering_evidence import frozen_document, verify_chunk_reads


class FrozenDocumentTests(unittest.TestCase):
    def test_original_source_is_selected_even_after_a_new_revision_exists(self):
        with sqlite3.connect(':memory:') as connection:
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
