import unittest
from profile_workflow import partition_ms, union_ms


class TimelineTests(unittest.TestCase):
    def test_parallel_and_nested_intervals_cover_the_root_once(self):
        groups = {'human': [[-10, 20], [10, 30]],
                  'business': [[10, 70], [50, 80]],
                  'manager': [[60, 90]], 'workers': [[0, 90]]}
        result = partition_ms(groups, 0, 100)
        self.assertEqual(result['exclusive_ms'],
                         {'human': 30, 'business': 50, 'manager': 10,
                          'unattributed': 10})
        self.assertEqual(result['covered_wall_ms'], 100)
        self.assertEqual(union_ms(groups['business'], 0, 100), 70)

    def test_missing_and_out_of_range_spans_stay_visible(self):
        self.assertEqual(partition_ms({'sdk': [[-10, -1], [120, 130], [20, 10]]}, 0, 100)['exclusive_ms'], {'unattributed': 100})


if __name__ == '__main__':
    unittest.main()
