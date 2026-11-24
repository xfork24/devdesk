#!/usr/bin/env python3
"""Regression for the permission gate's relative source-envelope reference."""
from copy import deepcopy
from pathlib import Path
import runpy
import unittest

from jsonschema import ValidationError

gate = runpy.run_path(str(Path(__file__).resolve().parents[2] / 'scripts/check/check-permissions.py'))


class PermissionSchema(unittest.TestCase):
    def test_source_envelope_is_validated_through_the_relative_reference(self):
        for name, value in [('identity', ''), ('revision', 0), ('scope', 123)]:
            invalid = deepcopy(gate['presets'][0])
            invalid[name] = value
            with self.subTest(field=name), self.assertRaises(ValidationError):
                gate['validate_source'](invalid)

    def test_permission_schema_rejects_the_wrong_source_kind(self):
        invalid = deepcopy(gate['presets'][0])
        invalid['type'] = 'role'
        with self.assertRaises(ValidationError):
            gate['validate_source'](invalid)


if __name__ == '__main__':
    unittest.main()
