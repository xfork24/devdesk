"""Negative evidence tests: the verifier must reject external workflow steering."""
import hashlib
import json
import sqlite3
import unittest
from autonomous_orchestration import verify


class AutonomyTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.executescript(
            "CREATE TABLE governance_records(id TEXT,type TEXT,name TEXT,data TEXT);"
            "CREATE TABLE turns(id TEXT,session_id TEXT,input TEXT);")
        self.record("manager", "workflow_controller",
                    {"root_instance_id": "root", "epoch": 1, "session_id": "session"})
        self.db.execute("INSERT INTO turns VALUES (?,?,?)", (
            "turn", "session", json.dumps({"bootstrap": True, "delivery_id": "delivery",
                                          "root_instance_id": "root", "user_messages": []})))
        self.record("start", "workflow_decision", {"actor": {"kind": "human"}, "operation": "start", "instance_id": "root", "root_instance_id": "root"})
        self.record("dispatch", "workflow_decision",
                    {"actor": {"kind": "controller", "controller_id": "manager", "epoch": 1}})
        self.record("root", "workflow_instance", {"owner_id":"feature", "workflow_ref":"workflow:1"})
        self.record("start-command", "command_context", {
            "context":{"actor":{"kind":"human"}}, "target":"feature",
            "request_digest":json.dumps(["workflow.start","feature",1,{"workflow_ref":"workflow:1"}])
        }, "workflow.start")

    def tearDown(self):
        self.db.close()

    def record(self, identity, kind, data, name=""):
        self.db.execute("INSERT INTO governance_records VALUES (?,?,?,?)",
                        (identity, kind, name, json.dumps(data)))

    def test_one_goal_and_native_manager_decision_pass(self):
        self.assertEqual(verify(self.db, "root")["external_steering_count"], 0)

    def test_native_layout_start_without_legacy_start_decision_passes(self):
        self.db.execute("DELETE FROM governance_records WHERE id='start'")
        self.assertEqual(verify(self.db,"root")["initial_human_start_commands"],1)

    def test_default_start_is_bound_to_its_original_command_result(self):
        self.db.execute("ALTER TABLE governance_records ADD COLUMN definition_id TEXT")
        self.db.execute("CREATE TABLE operations(id TEXT,fingerprint TEXT,result TEXT)")
        request = json.dumps(["workflow.start", "feature", 1, {"title": "原开发目标" * 24}],
                             ensure_ascii=False, separators=(",", ":"))
        self.db.execute("UPDATE governance_records SET definition_id='command',data=? WHERE id='start-command'", (json.dumps({"context":{"actor":{"kind":"human"}},"target":"feature","request_digest":request}),))
        result = {"id":"root","type":"workflow_instance","data":{"owner_id":"feature","workflow_ref":"workflow:1"}}
        self.db.execute("INSERT INTO operations VALUES (?,?,?)", ("command",request,json.dumps(result)))
        self.assertTrue(verify(self.db,"root")["passed"])
        encoded = json.dumps(request, ensure_ascii=False, separators=(",", ":")).encode()
        fingerprint = "devdesk-command-sha256/v1:" + hashlib.sha256(encoded).hexdigest()
        self.db.execute("UPDATE operations SET fingerprint=?", (fingerprint,))
        self.assertTrue(verify(self.db,"root")["passed"])
        self.db.execute("UPDATE operations SET fingerprint=?", (fingerprint[:-1],))
        with self.assertRaisesRegex(AssertionError, "lacks its exact durable command receipt"):
            verify(self.db,"root")
        self.db.execute("UPDATE operations SET fingerprint=?", (fingerprint,))
        result["data"]["workflow_ref"] = "foreign:1"
        self.db.execute("UPDATE operations SET result=?", (json.dumps(result),))
        with self.assertRaisesRegex(AssertionError, "different root or Workflow"):
            verify(self.db,"root")

    def test_missing_actual_start_command_is_rejected(self):
        self.db.execute("DELETE FROM governance_records WHERE id='start-command'")
        with self.assertRaisesRegex(AssertionError,"exactly one initial Human start"):
            verify(self.db,"root")

    def test_start_for_another_owner_is_rejected(self):
        self.db.execute("UPDATE governance_records SET data=json_set(data,'$.target','foreign') WHERE id='start-command'")
        with self.assertRaises(AssertionError):
            verify(self.db,"root")

    def test_extra_human_guidance_is_rejected(self):
        self.record("input", "controller_input", {"input": "现在派发设计步骤"})
        with self.assertRaisesRegex(AssertionError, "external steering"):
            verify(self.db, "root")

    def test_script_acceptance_is_rejected(self):
        self.record("human-action", "command_context",
                    {"context": {"actor": {"kind": "human"}}}, "workflow.step.accept")
        with self.assertRaisesRegex(AssertionError, "performed manager work"):
            verify(self.db, "root")

    def test_runtime_step_instructions_are_rejected(self):
        self.db.execute("UPDATE turns SET input=?", (json.dumps({
            "bootstrap": True, "delivery_id": "delivery", "root_instance_id": "root",
            "instructions": "派发下一步"}),))
        with self.assertRaisesRegex(AssertionError, "repeated orchestration"):
            verify(self.db, "root")

    def test_confirmation_without_manager_request_is_rejected(self):
        self.record("signature", "workflow_human_decision", {"approved": True})
        with self.assertRaisesRegex(AssertionError, "lacks manager request"):
            verify(self.db, "root")

    def test_event_without_actual_trigger_is_rejected(self):
        self.db.execute("INSERT INTO turns VALUES (?,?,?)", (
            "another-turn", "session", json.dumps({"delivery_id": "another", "root_instance_id": "root"})))
        with self.assertRaisesRegex(AssertionError, "without a cause"):
            verify(self.db, "root")

    def event_turn(self, identity, events):
        self.db.execute("INSERT INTO turns VALUES (?,?,?)", (
            identity, "session", json.dumps({"delivery_id": identity,
                                             "root_instance_id": "root", "events": events})))

    def test_foreign_root_event_is_rejected(self):
        self.event_turn("foreign", [{"id": 1, "source": {"root_instance_id": "another"}}])
        with self.assertRaisesRegex(AssertionError, "Foreign root"):
            verify(self.db, "root")

    def test_repeated_event_in_a_second_turn_is_rejected(self):
        event = {"id": 1, "source": {"root_instance_id": "root"}}
        self.event_turn("first", [event])
        self.event_turn("second", [event])
        with self.assertRaisesRegex(AssertionError, "multiple manager turns"):
            verify(self.db, "root")

    def test_oversized_event_delivery_is_rejected(self):
        self.event_turn("large", [{"id": 1, "source": {"root_instance_id": "root", "details": "x" * 65536}}])
        with self.assertRaisesRegex(AssertionError, "count or byte bound"):
            verify(self.db, "root")

    def test_notification_without_actual_manager_tool_call_is_rejected(self):
        self.record('notice', 'controller_interaction',
                    {'turn_id': 'turn', 'interaction': {'title': '伪造反馈'}})
        with self.assertRaisesRegex(AssertionError, 'actual native manager call'):
            verify(self.db, 'root')


if __name__ == "__main__":
    unittest.main()
