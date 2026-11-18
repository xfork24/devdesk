"""Trusted project runner. Child stdout is always encoded as diagnostic data."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import unittest


def emit(event, **fields):
    print(json.dumps(dict(devdesk_test_report=1, event=event, **fields)), flush=True)


def diagnostic(output, source='test:stdout'):
    if output:
        emit('diagnostic', source=source, diagnostic={'message': output})


def run_command(command):
    root = Path('/work').resolve()
    cwd = (root / command.get('cwd', '.')).resolve(strict=True)
    if not cwd.is_relative_to(root):
        raise ValueError('project command cwd escapes the frozen revision')
    environment = dict(os.environ, **command.get('environment', {}))
    result = subprocess.run(command['argv'], cwd=cwd, env=environment,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    diagnostic(result.stdout)
    diagnostic(result.stderr, 'test:stderr')
    return result.returncode


def main(recipe):
    for command in recipe.get('preparation', []):
        if run_command(command) != 0:
            emit('fail', classification='environment')
            emit('summary', counts=dict(tests=1, failed=1, passed=0))
            return 1
    if recipe['runner'] == 'command':
        code = run_command(recipe['command'])
        if code:
            emit('fail', classification='environment')
        emit('summary', counts=dict(tests=1, failed=int(code != 0), passed=int(code == 0)))
        return int(code != 0)
    suite = unittest.TestSuite()
    captured = io.StringIO()
    with contextlib.redirect_stdout(captured), contextlib.redirect_stderr(captured):
        sys.path.insert(0, '/work')
        for index, file in enumerate(recipe['files']):
            path = (Path('/work') / file).resolve(strict=True)
            if not path.is_relative_to(Path('/work')):
                raise ValueError('test file escapes the frozen revision')
            spec = importlib.util.spec_from_file_location('devdesk_test_' + str(index), path)
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            suite.addTests(unittest.defaultTestLoader.loadTestsFromModule(module))
        result = unittest.TextTestRunner(stream=captured, verbosity=2).run(suite)
    diagnostic(captured.getvalue())
    for _, message in result.failures:
        emit('fail', classification='assertion', diagnostic={'message': message})
    for _, message in result.errors:
        emit('fail', classification='environment', diagnostic={'message': message})
    # Skips and expected failures cannot establish newly working behavior.
    if result.skipped or result.expectedFailures or result.unexpectedSuccesses:
        emit('fail', classification='environment')
    failed = len(result.failures) + len(result.errors) + len(result.unexpectedSuccesses)
    failed += bool(result.skipped or result.expectedFailures)
    emit('summary', counts=dict(tests=result.testsRun, failed=failed,
                               passed=result.testsRun - failed))
    return int(failed != 0)


if __name__ == '__main__':
    try:
        code = main(json.loads(sys.argv[1]))
    except Exception as error:
        emit('fail', classification='environment', diagnostic={'message': repr(error)})
        emit('summary', counts=dict(tests=1, failed=1, passed=0))
        code = 1
    sys.exit(code)
