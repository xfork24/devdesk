"""Scheduling diagnostics retain overlaps, failures and incomplete observations."""
import copy
import unittest
from summarize_engineering_scene import scheduling_metrics


class SchedulingMetricsTests(unittest.TestCase):
    def setUp(self):
        self.records = {'governance_records': [
            {'id': 'manager', 'type': 'workflow_controller', 'data': {'session_id': 'session'}},
        ], 'turns': [
            {'id': 'one', 'session_id': 'session', 'started': 10, 'finished': 30},
            {'id': 'two', 'session_id': 'session', 'started': 20, 'finished': 40},
            {'id': 'worker', 'session_id': 'other', 'started': 0, 'finished': 1000},
        ]}

    def call(self, identity, status='succeeded', arguments=None, session='session'):
        self.records['governance_records'].append({'id': identity, 'type': 'controller_tool_call',
            'definition_id': 'manager', 'data': {'method': 'workflow.inspect', 'turn_id': 'one',
            'arguments': arguments or {'id': 'root'}, 'session_id': session, 'status': status}})

    def test_overlapping_manager_turns_are_counted_once_and_worker_is_excluded(self):
        before = copy.deepcopy(self.records)
        result = scheduling_metrics(self.records)
        self.assertEqual(result['manager_turns'], 2)
        self.assertEqual(result['manager_turn_union_ms'], 30)
        self.assertEqual(result['manager_turn_intervals'], [[10, 40]])
        self.assertEqual(self.records, before)

    def test_unfinished_and_invalid_intervals_remain_unmeasured(self):
        self.records['turns'].extend([
            {'id': 'open', 'session_id': 'session', 'started': 50, 'finished': None},
            {'id': 'backwards', 'session_id': 'session', 'started': 50, 'finished': 40}])
        result = scheduling_metrics(self.records)
        self.assertEqual(result['unmeasured_turn_ids'], ['open', 'backwards'])
        self.assertEqual(result['manager_turn_union_ms'], 30)

    def test_repeated_candidates_do_not_discard_failures_or_changed_parameters(self):
        self.call('one')
        self.call('rejection', status='failed')
        self.call('new-page', arguments={'id': 'root', 'offset': 32})
        result = scheduling_metrics(self.records)
        self.assertEqual(result['repeated_read_candidates'], 1)
        self.assertEqual(result['native_calls_by_status'], {'succeeded': 2, 'failed': 1})

    def test_historical_manager_session_and_missing_turn_are_explicit(self):
        self.call('history', session='previous-session')
        self.records['governance_records'][-1]['data']['turn_id'] = 'missing'
        result = scheduling_metrics(self.records)
        self.assertIn('previous-session', result['manager_session_ids'])
        self.assertEqual(result['missing_native_turn_ids'], ['missing'])

    def test_a_call_without_its_controller_is_not_silently_dropped(self):
        self.call('orphan', status='failed')
        self.records['governance_records'][-1]['definition_id'] = 'absent-controller'
        result = scheduling_metrics(self.records)
        self.assertEqual(result['unattributed_controller_call_ids'], ['orphan'])


if __name__ == '__main__':
    unittest.main()
