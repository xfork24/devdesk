#!/usr/bin/env python3
"""Isolated normal-session acceptance. DSH metadata discovery; Codex inference is opt-in."""
from client import Client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import argparse
import json
import os
import platform
import shutil
import signal
import subprocess
import tempfile
import threading
import uuid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--codex-auth-source', type=Path)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    rows = ['# 多模型接入 Host 验收', '', '真实模型仅使用 Codex；DSH 仅做本地模拟目录查询，不发推理请求。', '']
    server_calls = []

    class Directory(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_GET(self):
            server_calls.append(('GET', self.path))
            body = json.dumps({'data': [{'id': 'host-fixture-model'}]}).encode()
            self.send_response(200 if self.path == '/v1/models' else 404)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            server_calls.append(('POST', self.path))
            self.send_error(405)

    with tempfile.TemporaryDirectory(prefix='devdesk-model-acceptance-') as temporary:
        dataset = Path(temporary) / 'data'
        dataset.mkdir(mode=0o700)
        server = ThreadingHTTPServer(('0.0.0.0', 0), Directory)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        host = None

        def start():
            nonlocal host
            host = subprocess.Popen([str(args.binary.resolve()), '--data', str(dataset), '--port', '0'], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
            assert host.stdout
            host.stdout.readline()
            url = host.stdout.readline().strip()
            assert url.startswith('http://127.0.0.1:'), 'Host did not announce a local URL'
            return Client(url)

        def stop():
            nonlocal host
            if host is not None:
                host.send_signal(signal.SIGINT)
                try:
                    host.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    host.kill()
                    host.wait(timeout=5)
                host = None

        try:
            c = start()
            if args.codex_auth_source:
                home = dataset / 'credentials' / 'codex'
                home.mkdir(parents=True, mode=0o700, exist_ok=True)
                home.parent.chmod(0o700)
                shutil.copyfile(args.codex_auth_source, home / 'auth.json')
                (home / 'auth.json').chmod(0o600)
            assert len(c.rpc('query.model_providers')['providers']) == 5
            assert c.rpc('credentials.status', {'model_provider': 'ollama'})['auth_kind'] == 'none'
            project = c.command('project.create', payload={'title': 'Isolated provider acceptance'})['id']
            c.open(project)
            model = c.command('model.create', payload={'name': 'Host Ollama fixture', 'data': {'provider': 'ollama', 'model': 'host-fixture-model', 'endpoint': f'http://127.0.0.1:{server.server_port}/v1', 'context_window': 32768, 'max_output_tokens': 4096, 'parameters': {'max_tokens': 2048}}})['id']
            cap = c.command('capability.create', payload={'model_ref':model,'harness_ref':'builtin-harness-dsh-multi-provider:1','tools':['builtin-tool-dsh-native:1'],'skills':[],'name': 'Host Ollama capability', 'data': {}})['id']
            agent = c.command('agent.create', payload={'role_ref':'builtin-role-analyst:1','capability_ref':cap,'name': 'Host Ollama Agent', 'data': {}})['id']
            catalog = c.rpc('models.list', {'model_provider': 'ollama', 'endpoint': f'http://127.0.0.1:{server.server_port}/v1'})
            assert catalog['models'] == [{'id': 'host-fixture-model'}] and not catalog['inference_verified']
            project_row = c.rpc('query.workspace')['active_project']
            session = c.command('session.open', project, project_row['revision'], {'agent': agent, 'work_type': 'discussion', 'force_new': True})['id']
            state = c.rpc('query.session', {'id': session})
            binding = state['session']['runtime_binding']
            assert binding['credential_ref'] is None
            assert binding['model_connection']['provider'] == 'ollama'
            assert binding['model_connection']['worker_endpoint'].startswith('http://host.docker.internal:')
            assert binding['model_profile_digest']
            assert state['turns'] == []
            assert server_calls == [('GET', '/v1/models')], server_calls
            c.command('model.revise', model, 1, {'name': 'Updated fixture endpoint', 'data': {'provider': 'ollama', 'model': 'host-fixture-model', 'endpoint': 'http://127.0.0.1:11435/v1', 'context_window': 32768, 'max_output_tokens': 4096}})
            stop()
            c = start()
            c.open(project)
            assert c.rpc('query.session', {'id': session})['session']['runtime_binding'] == binding
            rows += ['- 模型目录读取及普通 Model → Capability → Agent → Session 固定 DSH 绑定通过。', '- Ollama 无凭据；连接和 profile 摘要冻结，Model 新修订与 Host 重启不改旧 Session。', '- DSH 模拟服务只收到一次 GET /v1/models，没有推理请求。']
            if args.codex_auth_source:
                project_row = c.rpc('query.workspace')['active_project']
                session = c.command('session.open', project, project_row['revision'], {'agent': 'builtin-agent-sol-high-analyst:4', 'work_type': 'discussion', 'force_new': True})['id']
                current = c.rpc('query.session', {'id': session})
                marker = 'provider-session-' + uuid.uuid4().hex
                turn = c.command('conversation.send', session, current['session']['revision'], {'client_message_id': str(uuid.uuid4()), 'input': f'Use the native Bash tool once to print {marker}, then reply with that exact marker. Do not modify files.'})['id']

                def completed():
                    view = c.rpc('query.session', {'id': session, 'limit': 100})
                    current = next(t for t in view['turns'] if t['id'] == turn)
                    if current['status'] in ['failed', 'cancelled', 'needs_reconciliation']:
                        raise AssertionError('Codex turn failed: ' + current['status'])
                    return view if current['status'] == 'completed' else None

                view = c.wait(completed, bool, timeout=300)
                assert marker in '\n'.join(e['content'] for e in view['events'] if e['kind'] == 'assistant')
                assert any(e['type'] == 'tool' for e in view['events'])
                original = view['session']['runtime_binding']
                native = view['session']['external_session']
                stop()
                c = start()
                c.open(project)
                view = c.rpc('query.session', {'id': session})
                turn = c.command('conversation.send', session, view['session']['revision'], {'client_message_id': str(uuid.uuid4()), 'input': 'Reply with the exact previous marker from this conversation. Do not use tools.'})['id']
                resumed = c.wait(completed, bool, timeout=300)
                assert marker in '\n'.join(e['content'] for e in resumed['events'] if e['kind'] == 'assistant' and e['turn_id'] == turn)
                assert resumed['session']['runtime_binding'] == original
                assert resumed['session']['external_session'] == native
                rows += [f'- Codex 真实模型：{original["model"]}；普通讨论的原生工具结果消费、Host 重启与原生会话恢复通过。', f'- Codex Session：`{session}`；恢复 Turn：`{turn}`。']
            assert all(method == 'GET' for method, _ in server_calls)
            secrets = []
            for auth in dataset.rglob('auth.json'):
                value = json.loads(auth.read_text())
                secrets.extend(token.encode() for name, token in value.get('tokens', {}).items() if name in ['access_token', 'refresh_token', 'id_token'] and isinstance(token, str) and len(token) > 16)
            for file in dataset.rglob('*'):
                if not file.is_file() or file.name == 'auth.json' or 'credentials' in file.relative_to(dataset).parts:
                    continue
                body = file.read_bytes()
                assert not any(secret in body for secret in secrets), 'OAuth secret leaked into persisted non-credential data'
            if secrets:
                rows += ['- Codex OAuth token 扫描通过：非凭据持久化文件未发现 token，报告不包含秘密。']
            rows += ['', f'结果：通过。宿主：{platform.system()} / {platform.machine()}；执行器：Linux Docker；DSH 真实推理未验收。']
        finally:
            stop()
            server.shutdown()
            server.server_close()
    args.report.write_text('\n'.join(rows) + '\n')
    print('PASS isolated provider Host acceptance; DSH inference requests: 0')


if __name__ == '__main__':
    main()
