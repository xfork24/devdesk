"""Only an actual bounded manager change extends the observer's recovery grace."""
import copy
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


if __name__ == "__main__":
    unittest.main()
