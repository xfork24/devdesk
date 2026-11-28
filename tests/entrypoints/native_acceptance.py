#!/usr/bin/env python3
"""Explicit Docker protocol acceptance, optionally including an existing dedicated GitHub PR.

No paid model calls. Missing resources and failed tests fail the run; nothing is
silently skipped. Default Rust unit tests remain portable and credential-free.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
SUITES = (
    'command_tests',
    'delivery_acceptance',
    'docker_resources',
    'model_providers',
    'permission_profiles',
    'permissions',
    'runtime_resources',
    'tool_integrations',
)


def source_fingerprint():
    names = subprocess.check_output(['git', 'ls-files', '--cached', '--others', '--exclude-standard'], cwd=ROOT, text=True).splitlines()
    digest = hashlib.sha256()
    for name in sorted(set(names)):
        if name in {'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'} or name.startswith(('crates/', 'plugins/', 'permissions/', 'tests/', 'sql/')):
            path = ROOT / name
            if path.is_file():
                digest.update(name.encode() + b'\0' + path.read_bytes() + b'\0')
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    scope = parser.add_mutually_exclusive_group(required=True)
    scope.add_argument('--docker-only', action='store_true', help='run the Docker tests; no GitHub credentials required')
    scope.add_argument('--github-request', type=Path, help='existing dedicated PR request; include GitHub acceptance')
    parser.add_argument('--github-repo', help='dedicated acceptance repository, required with --github-request')
    parser.add_argument('--prepare', action='store_true', help='build the selected packaged runtime recipes')
    parser.add_argument('--reports', type=Path)
    args = parser.parse_args()
    if bool(args.github_request) != bool(args.github_repo):
        parser.error('--github-request and --github-repo must be supplied together')
    if args.github_request:
        args.github_request = args.github_request.resolve(strict=True)
        request = json.loads(args.github_request.read_text())
        if request['repository'] != args.github_repo or not request.get('number') or args.github_repo == 'xfork24/devdesk':
            parser.error('GitHub acceptance requires a known existing PR in a dedicated repository')
    reports = (args.reports or Path(tempfile.mkdtemp(prefix='devdesk-native-'))).resolve()
    reports.mkdir(parents=True, exist_ok=True)
    report = {'passed': False, 'scope': 'docker-and-github' if args.github_request else 'docker', 'expected_tests': 0, 'tests': {}, 'started': time.time(), 'images': {}, 'steps': [], 'source': {
        'git_head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'inputs_sha256': source_fingerprint(),
    }}
    env = os.environ.copy()

    def save():
        (reports / 'native-acceptance.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')

    def run(name, command):
        print(f'Running {name}', flush=True)
        log = reports / f'{name}.log'
        with log.open('w') as stream:
            result = subprocess.run(command, cwd=ROOT, env=env, stdout=stream, stderr=subprocess.STDOUT)
        report['steps'].append({'name': name, 'exit_code': result.returncode, 'log': str(log)})
        save()
        if result.returncode:
            raise RuntimeError(f'{name} failed: {log}')
        return log.read_text()

    def inspect(reference):
        value = json.loads(subprocess.check_output(['docker', 'image', 'inspect', reference], cwd=ROOT))[0]
        if value['Os'] != 'linux' or not re.fullmatch(r'sha256:[0-9a-f]{64}', value['Id']):
            raise RuntimeError('acceptance requires an immutable Linux image ID')
        return value['Id']

    try:
        run('pi-host-build', ['cargo', 'build', '--locked', '-p', 'devdesk-host', '--bin', 'devdesk-host'])
        host = Path(env.get('CARGO_TARGET_DIR', 'target')) / 'debug/devdesk-host'
        pi_reports = reports / 'pi-host'
        run('pi-host', ['python3', 'tests/entrypoints/pi_harness.py', '--binary', str(host), '--reports', str(pi_reports)])
        pi_image = json.loads((pi_reports / 'runtime.json').read_text())['artifact']['image_id']
        report['images']['DEVDESK_TEST_PI_IMAGE'] = inspect(pi_image)
        if args.prepare:
            recipes = json.loads(run('recipes', ['cargo', 'run', '--quiet', '--locked', '-p', 'devdesk-adapters', '--example', 'export_runtime_recipes']))
            for provider in ['codex', 'dsh']:
                resource = recipes[provider]
                tag = f'devdesk/native-acceptance:{provider}'
                if resource.get('image'):
                    run(f'pull-{provider}', ['docker', 'pull', resource['image']])
                    tag = resource['image']
                else:
                    with tempfile.TemporaryDirectory(prefix=f'devdesk-native-{provider}-') as directory:
                        context = Path(directory)
                        for item in resource['files']:
                            target = context / item['path']
                            target.resolve().relative_to(context.resolve())
                            target.parent.mkdir(parents=True, exist_ok=True)
                            target.write_text(item['content'])
                        run(f'build-{provider}', ['docker', 'build', '--tag', tag, str(context)])
                        if provider == 'codex':
                            # The delivery fixture needs Node only, with the same
                            # pinned base as the selected runtime recipe.
                            base = re.search(r'^FROM (\S+)', (context / 'Dockerfile').read_text(), re.MULTILINE).group(1)
                            run('pull-node', ['docker', 'pull', base])
                            env['DEVDESK_ACCEPTANCE_NODE_IMAGE'] = inspect(base)
                env[f'DEVDESK_TEST_{provider.upper()}_IMAGE'] = inspect(tag)
            env['DEVDESK_DSH_MOCK_IMAGE'] = env['DEVDESK_TEST_DSH_IMAGE']
            env['DEVDESK_TOOL_TEST_IMAGE'] = env['DEVDESK_TEST_CODEX_IMAGE']
        for variable in ['DEVDESK_TEST_CODEX_IMAGE', 'DEVDESK_TEST_DSH_IMAGE', 'DEVDESK_DSH_MOCK_IMAGE', 'DEVDESK_TOOL_TEST_IMAGE', 'DEVDESK_ACCEPTANCE_NODE_IMAGE']:
            reference = env.get(variable, '')
            if not re.fullmatch(r'sha256:[0-9a-f]{64}', reference):
                raise RuntimeError(f'set immutable {variable} or use --prepare')
            report['images'][variable] = inspect(reference)
        env['DEVDESK_DSH_TEST_IMAGE'] = env['DEVDESK_TEST_DSH_IMAGE']
        env['DEVDESK_TEST_PROJECT_IMAGE'] = env['DEVDESK_TEST_CODEX_IMAGE']
        suites = list(SUITES)
        if args.github_request:
            env['DEVDESK_LIVE_PR_REQUEST'] = str(args.github_request)
            env['DEVDESK_LIVE_GITHUB_REPO'] = args.github_repo
            suites.append('github_resources')
        for suite in suites:
            listing = run(suite + '-list', ['cargo', 'test', '--locked', '-p', 'devdesk-adapters', '--features', 'test-fixtures,fixture-catalog', '--test', suite, '--', '--ignored', '--list'])
            tests = re.findall(r'^(.+): test$', listing, re.MULTILINE)
            if not tests:
                raise RuntimeError(f'{suite} did not enumerate any external tests')
            expected = len(tests)
            report['tests'][suite] = tests
            report['expected_tests'] += expected
            output = run(suite, ['cargo', 'test', '--locked', '-p', 'devdesk-adapters', '--features', 'test-fixtures,fixture-catalog', '--test', suite, '--', '--ignored', '--test-threads=1'])
            if not re.search(rf'test result: ok\. {expected} passed; 0 failed; 0 ignored;', output):
                raise RuntimeError(f'{suite} did not execute the expected {expected} external tests')
        if source_fingerprint() != report['source']['inputs_sha256']:
            raise RuntimeError('acceptance inputs changed during execution; rerun a stable checkout')
        report['passed'] = True
    finally:
        report['finished'] = time.time()
        save()
        print(reports / 'native-acceptance.json', flush=True)


if __name__ == '__main__':
    main()
