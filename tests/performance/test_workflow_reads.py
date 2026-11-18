"""Diagnostic read samples must not drop a failed request from their ledger."""
import io
import json
import unittest
from types import SimpleNamespace
from workflow_reads import measure_rpc


class ReadSampleTests(unittest.TestCase):
    def response(self, body):
        stream = io.BytesIO(body)
        stream.headers = {'x-devdesk-request-id': 'actual-request'}
        return SimpleNamespace(open=lambda *args, **kwargs: stream)

    def test_success_and_domain_error_are_both_samples(self):
        for packet in [{'result': {'status': 'running'}}, {'error': {'code': 'conflict'}}]:
            row = measure_rpc(self.response(json.dumps(packet).encode()), 'http://local', 'project', 'query.workflow_control', {})
            self.assertEqual(row['error'], packet.get('error'))
            self.assertGreater(row['bytes'], 0)
            self.assertEqual(row['request_id'], 'actual-request')

    def test_transport_failure_and_invalid_json_remain_failed_samples(self):
        def fail(*args, **kwargs):
            raise OSError('connection closed')
        for client in [SimpleNamespace(open=fail), self.response(b'not JSON')]:
            row = measure_rpc(client, 'http://local', 'project', 'query.workflow_control', {})
            self.assertIn('transport_or_decode', row['error'])
            self.assertGreaterEqual(row['ms'], 0)
            self.assertIsNone(row['digest'])


if __name__ == '__main__':
    unittest.main()
