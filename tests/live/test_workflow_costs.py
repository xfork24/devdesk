import json
import unittest

from workflow_costs import allocations, observed_native, partition


class CostAttributionTests(unittest.TestCase):
    def test_nested_and_concurrent_tools_are_not_added_twice(self):
        slices=partition(0,100,[('external',[(40,60)]),('tools',[(20,50),(30,70)]),
            ('agent',[(0,90)]),('lifecycle',[(0,100)])])
        self.assertEqual(allocations(slices,[(0,100)]),
            {'agent':40,'tools':30,'external':20,'lifecycle':10})

    def test_overlapping_phase_windows_are_unioned(self):
        slices=partition(0,100,[('agent',[(0,100)])])
        self.assertEqual(allocations(slices,[(10,50),(30,70)]),{'agent':60})

    def test_host_observations_survive_a_stale_vm_clock(self):
        def row(at, observations):
            return json.dumps({'basis':{'turn_id':'turn'},'value':{'observations':observations}}),at
        def tool(suffix):
            return {'type':'message','kind':'tool','key':'turn:call:'+suffix,'item':'call',
                'content':json.dumps({'type':'dynamicToolCall','startedAtMs':-900000,'completedAtMs':-899900})}
        native,tools=observed_native([row(10,[{'type':'admitted'}]),row(20,[tool('started')]),
            row(30,[tool('complete')]),row(40,[{'type':'terminal'}])],{'turn'})
        self.assertEqual(native['turn'],{'started':10,'finished':40})
        self.assertEqual((tools[0]['started'],tools[0]['finished']),(20,30))
        self.assertEqual(tools[0]['producer_reported_duration_ms'],100)

    def test_missing_tool_start_cannot_be_reported_as_zero_cost(self):
        content=json.dumps({'type':'commandExecution'})
        rows=[(json.dumps({'basis':{'turn_id':'turn'},'value':{'observations':[
            {'type':'admitted'},{'type':'message','kind':'tool','key':'call:complete','item':'call','content':content},
            {'type':'terminal'}]}}),10)]
        with self.assertRaisesRegex(ValueError,'tool boundary'):
            observed_native(rows,{'turn'})


if __name__=='__main__':
    unittest.main()
