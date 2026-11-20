import unittest

from workflow_timing import audit, approval_windows, suspension_windows


class TimingAuditTests(unittest.TestCase):
    def test_overlapping_external_wait_and_sleep_are_not_deducted_twice(self):
        result = audit(0, 100, [{'requested_at_ms': 10, 'signed_at_ms': 50,
            'manager_preparation_ms': 5}], [{'started_at_ms': 30, 'finished_at_ms': 70}])
        self.assertEqual(result['excluded_union_ms'], 60)
        self.assertEqual(result['workflow_net_ms'], 40)
        self.assertEqual(result['manager_approval_preparation_ms'], 5)

    def test_no_os_evidence_does_not_turn_idle_time_into_an_exclusion(self):
        self.assertEqual(suspension_windows('', [0, 900000], 0, 900000), [])
        self.assertEqual(audit(0, 900000, [], [])['workflow_net_ms'], 900000)

    def test_approval_preparation_and_operator_response_have_distinct_boundaries(self):
        records = [('signature', '', '', {}, 90), ('request', '', '', {}, 40)]
        windows = approval_windows({'execution': {'id': 'execution', 'step': 'review', 'finished': 20}},
            [{'execution': 'execution', 'record_id': 'signature', 'request_id': 'request'}], records)
        self.assertEqual(windows[0]['manager_preparation_ms'], 20)
        self.assertEqual(windows[0]['external_response_ms'], 50)

    def test_sleep_transition_activity_is_retained_and_wake_requests_are_not_wakes(self):
        log = ('2026-11-20 09:09:48 +0800 Sleep               Entering Sleep\n'
               '2026-11-20 09:09:49 +0800 Wake Requests       request timer\n'
               '2026-11-20 09:24:49 +0800 DarkWake            DarkWake from Deep Idle')
        start = 1795136988000
        windows = suspension_windows(log, [start + 1900, start + 900900], start, start + 1000000)
        self.assertEqual(windows[0]['started_at_ms'], start + 1900)
        self.assertEqual(windows[0]['finished_at_ms'], start + 900900)


if __name__ == '__main__':
    unittest.main()
