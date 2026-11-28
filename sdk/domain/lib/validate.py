"""Validate instances against the SDK's JSON Schemas in one process.

Reads a JSON list of {"schema": <file in the SDK's schemas directory>, "instance": ...,
"label": ...} from stdin and writes a list of the same length: null for a valid instance,
otherwise the first error with its label and where it is in the instance.
"""
import json
import sys
from pathlib import Path

try:
    import jsonschema
except ImportError:  # The kernel's own check scripts need the same package.
    raise SystemExit("需要 jsonschema：python3 -m pip install jsonschema")

schemas = Path(sys.argv[1])
validators = {}


def validator(name):
    if name not in validators:
        schema = json.loads((schemas / name).read_text())
        # Every reference stays inside its schema, so nothing is fetched.
        validators[name] = jsonschema.validators.validator_for(schema)(schema)
    return validators[name]


results = []
for item in json.load(sys.stdin):
    error = next(validator(item["schema"]).iter_errors(item["instance"]), None)
    if error is None:
        results.append(None)
        continue
    where = "/".join(str(part) for part in error.absolute_path)
    results.append(f"{item.get('label', item['schema'])}: {where} {error.message}")
print(json.dumps(results, ensure_ascii=False))
