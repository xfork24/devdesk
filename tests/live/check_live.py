#!/usr/bin/env python3
"""Run current native checks. Model-backed workbench checks require explicit selection."""
import argparse
import json
from pathlib import Path
import platform
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser()
parser.add_argument('--github-repo')
parser.add_argument('--providers', default='codex,dsh')
parser.add_argument('--groups', default='git,docker,permissions')
parser.add_argument('--url', default='http://127.0.0.1:51751')
parser.add_argument('--reports', type=Path)
args = parser.parse_args()
groups = args.groups.split(',')
providers = args.providers.split(',')
if 'workbench' in groups and not args.github_repo:
    parser.error('--groups workbench requires --github-repo for a dedicated test repository')
assert groups and set(groups) <= {'git', 'docker', 'permissions', 'workbench'}
assert providers and set(providers) <= {'codex', 'dsh'}
repo = Path(__file__).resolve().parents[2]
reports = args.reports or Path(tempfile.mkdtemp(prefix='devdesk-current-native-'))
reports.mkdir(parents=True, exist_ok=True)
report = {'passed': False, 'platform': platform.platform(), 'groups': groups, 'steps': [], 'started': time.time()}
output = reports / 'current-native-checks.json'

def run(name, command):
    log = reports / (name + '.log')
    with log.open('w') as stream:
        result = subprocess.run(command, cwd=repo, stdout=stream, stderr=subprocess.STDOUT)
    report['steps'].append({'name': name, 'exit_code': result.returncode, 'log': str(log)})
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    if result.returncode:
        raise RuntimeError(name + ' failed; see ' + str(log))

try:
    if 'git' in groups:
        run('git', ['cargo', 'test', '--locked', '-p', 'devdesk-adapters', '--test', 'git_resources'])
    if 'docker' in groups:
        run('docker', ['cargo', 'test', '--locked', '-p', 'devdesk-adapters', '--test', 'command_tests', '--test', 'docker_resources', '--', '--include-ignored'])
    if 'permissions' in groups:
        run('permissions', ['cargo', 'test', '--locked', '-p', 'devdesk-adapters', '--test', 'permissions', '--test', 'permission_profiles', '--', '--include-ignored'])
    if 'workbench' in groups:
        for provider in providers:
            run('workbench-' + provider, ['python3', 'tests/live/probe_workflow_workbench.py', '--url', args.url, '--github-repo', args.github_repo, '--provider', provider, '--report', str(reports / ('workbench-' + provider + '.md'))])
    report['passed'] = True
finally:
    report['finished'] = time.time()
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print(output)
