#!/usr/bin/env python3
"""R0 real Docker/GitHub experiments. No model calls, tokens or inherited test secrets.

GitHub mode mutates only the explicitly named dedicated private acceptance repository.
Every remote step is recorded, and a failed/uncertain step is not automatically retried.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid


def run(argv, cwd=None, ok=True, timeout=60):
    result = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, timeout=timeout)
    if ok and result.returncode:
        # Tool diagnostics can contain credentials; persist only the operation and exit code.
        raise RuntimeError(f'{argv[0]} {argv[1]} exited {result.returncode}')
    return result


def docker(report):
    image = 'alpine@sha256:f3d55900449227ae9e237ede521e7b665d53a253b42065ba90b9db9275e198ad'
    name = 'devdesk-r0-' + uuid.uuid4().hex
    report.update(image=image, container=name, inference_tested=False)
    version = run(['docker', 'version', '--format', '{{.Server.Version}}']).stdout.strip()
    report['engine'] = version
    with tempfile.TemporaryDirectory(prefix='devdesk-docker-') as directory:
        work = Path(directory) / 'work'
        work.mkdir(mode=0o777)
        work.chmod(0o777)
        run(['docker', 'create', '--name', name, '--label', 'devdesk.probe=r0',
             '--network=none', '--read-only', '--user=65534:65534', '--cap-drop=ALL',
             '--security-opt=no-new-privileges', '--pids-limit=32', '--memory=64m', '--cpus=0.5',
             '--mount', f'type=bind,source={work},target=/work', '--workdir=/work', image,
             'sh', '-c', 'test ! -e /var/run/docker.sock && test ! -e /root/.codex && '
             'test -z "$DEEPSEEK_API_KEY$OPENAI_API_KEY$GH_TOKEN$GITHUB_TOKEN" && '
             '(! touch /outside 2>/dev/null) && printf boundary-ok > /work/proof'])
        try:
            inspection = json.loads(run(['docker', 'inspect', name]).stdout)[0]
            host = inspection['HostConfig']
            assert host['ReadonlyRootfs'] and not host['Privileged']
            assert host['NetworkMode'] == 'none' and len(inspection['Mounts']) == 1
            run(['docker', 'start', '-a', name])
            code = run(['docker', 'inspect', '--format', '{{.State.ExitCode}}', name]).stdout.strip()
            assert code == '0', 'container assertions failed'
            assert (work / 'proof').read_text() == 'boundary-ok'
            report.update(passed=True, uid=65534, read_only_root=True, network='none',
                          worktree_write=True, outside_write_denied=True, credential_free=True,
                          daemon_socket_absent=True, image_id=inspection['Image'])
        finally:
            run(['docker', 'rm', '-f', name], ok=False)
        report['cleanup_verified'] = run(['docker', 'inspect', name], ok=False).returncode != 0


def github(report, repo):
    # This probe must never treat a user project as its scratch repository.
    if not repo.endswith('/devdesk-integration-test'):
        raise ValueError('probe accepts only the dedicated devdesk-integration-test repository')
    metadata = json.loads(run(['gh', 'repo', 'view', repo, '--json', 'nameWithOwner,visibility,defaultBranchRef']).stdout)
    assert metadata['visibility'] == 'PRIVATE'
    base = metadata['defaultBranchRef']['name']
    run_id = uuid.uuid4().hex
    branch = 'devdesk-r0/' + run_id
    report.update(repository=repo, run_id=run_id, branch=branch, base=base)
    with tempfile.TemporaryDirectory(prefix='devdesk-github-') as directory:
        root = Path(directory) / 'repository'
        run(['git', '-c', 'credential.helper=', '-c', 'credential.helper=!gh auth git-credential',
             'clone', f'https://github.com/{repo}.git', str(root)])
        def git(*args, ok=True):
            return run(['git', '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false',
                        '-c', 'credential.helper=', '-c', 'credential.helper=!gh auth git-credential', *args], root, ok=ok)
        git('config', 'user.name', 'DevDesk Integration Probe')
        git('config', 'user.email', 'devdesk-probe@users.noreply.github.com')
        baseline = git('rev-parse', f'refs/heads/{base}').stdout.strip()
        report['baseline'] = baseline
        git('switch', '-c', branch)
        (root / 'integration-proof.txt').write_text('DevDesk R0 candidate ' + run_id + '\n')
        git('add', 'integration-proof.txt')
        git('commit', '-m', 'Verify DevDesk candidate publication and indirect PR merge')
        candidate = git('rev-parse', 'HEAD').stdout.strip()
        assert git('rev-list', '--parents', '-1', candidate).stdout.split() == [candidate, baseline]
        report['candidate'] = candidate
        git('push', 'origin', f'{candidate}:refs/heads/{branch}')
        body = Path(directory) / 'pr-body.md'
        body.write_text(f'R0 acceptance experiment for single-parent candidate publication.\n\n<!-- devdesk:r0:{run_id} -->\n')
        url = run(['gh', 'pr', 'create', '--repo', repo, '--base', base, '--head', branch,
                   '--title', 'Verify DevDesk candidate publication', '--body-file', str(body)], root).stdout.strip()
        report['pull_request'] = url
        head = json.loads(run(['gh', 'pr', 'view', url, '--json', 'headRefOid']).stdout)['headRefOid']
        assert head == candidate
        git('update-ref', f'refs/heads/{base}', candidate, baseline)
        report['local_cas_passed'] = True
        stale_local = git('update-ref', f'refs/heads/{base}', baseline, baseline, ok=False)
        assert stale_local.returncode != 0
        report['local_stale_cas_rejected'] = True
        git('merge-base', '--is-ancestor', baseline, candidate)
        git('push', f'--force-with-lease=refs/heads/{base}:{baseline}', 'origin', f'{candidate}:refs/heads/{base}')
        report['remote_lease_advance_passed'] = True
        assert git('ls-remote', 'origin', f'refs/heads/{base}').stdout.split()[0] == candidate
        stale = git('push', f'--force-with-lease=refs/heads/{base}:{baseline}', 'origin', f'{baseline}:refs/heads/{base}', ok=False)
        assert stale.returncode != 0, 'stale lease was accepted'
        assert git('ls-remote', 'origin', f'refs/heads/{base}').stdout.split()[0] == candidate
        report['stale_lease_rejected'] = True
        for _ in range(15):
            pr = json.loads(run(['gh', 'pr', 'view', url, '--json', 'state,mergedAt,headRefOid,mergeCommit']).stdout)
            if pr['state'] == 'MERGED':
                break
            time.sleep(2)
        report['pull_request_result'] = pr
        assert pr['state'] == 'MERGED' and pr['headRefOid'] == candidate
        report['passed'] = True


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--group', choices=['docker', 'github'], required=True)
    parser.add_argument('--github-repo')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    report = {'group': args.group, 'passed': False}
    try:
        if args.group == 'docker': docker(report)
        else: github(report, args.github_repo or '')
    except Exception as error:
        report['failure_type'] = type(error).__name__
        if isinstance(error, RuntimeError): report['failed_operation'] = str(error)
        # Omit raw external exceptions; identify the stage using the saved receipts.
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
    raise SystemExit(0 if report['passed'] else 1)
