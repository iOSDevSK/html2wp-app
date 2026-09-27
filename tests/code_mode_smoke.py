"""Exercise pinned Codex 0.154 Code Mode over stdio, without auth or a model call.

The host wire protocol is intentionally confined to this regression test.
Production uses Codex App Server's public item/tool/call protocol.
"""
import argparse
import json
import os
from pathlib import Path
import queue
import shutil
import struct
import subprocess
import tempfile
import threading


def find_host():
    executable = shutil.which('codex')
    if not executable:
        raise RuntimeError('Install the pinned Codex CLI, or pass --host')
    package = Path(executable).resolve().parent.parent
    names = ['codex-code-mode-host', 'codex-code-mode-host.exe']
    for name in names:
        candidates = list(package.glob(f'node_modules/@openai/codex-*/vendor/*/bin/{name}'))
        candidates += list(package.glob(f'vendor/*/bin/{name}'))
        if candidates:
            return candidates[0]
    raise RuntimeError('Codex package is missing its Code Mode host; pass --host to its executable')


def check(host, tools_path):
    definitions = json.loads(Path(tools_path).read_text())
    enabled = [{'name': tool['name'], 'kind': 'function',
                'tool_name': {'name': tool['name'], 'namespace': None},
                'description': tool['description'], 'input_schema': tool['inputSchema'],
                'output_schema': None} for tool in definitions]
    with tempfile.TemporaryDirectory(prefix='html2wp-code-mode-') as home, tempfile.TemporaryFile() as errors:
        process = subprocess.Popen([str(host)], cwd=home, env=dict(os.environ, CODEX_HOME=home),
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors)
        frames = queue.Queue()

        def reader():
            try:
                while True:
                    header = process.stdout.read(4)
                    if not header:
                        raise EOFError('Code Mode host exited')
                    size = struct.unpack('<I', header)[0]
                    if size > 64 * 1024 * 1024:
                        raise RuntimeError('Oversized Code Mode frame')
                    frames.put(json.loads(process.stdout.read(size)))
            except Exception as error:
                frames.put(error)

        threading.Thread(target=reader, daemon=True).start()

        def send(value):
            data = json.dumps(value).encode()
            process.stdin.write(struct.pack('<I', len(data)) + data)
            process.stdin.flush()

        def receive():
            value = frames.get(timeout=15)
            if isinstance(value, Exception):
                raise value
            return value

        try:
            send({'type': 'connection/hello', 'supportedVersions': [1],
                  'requiredCapabilities': [], 'optionalCapabilities': []})
            assert receive()['type'] == 'connection/ready'
            send({'type': 'operation/request', 'id': 1,
                  'request': {'method': 'session/open', 'sessionId': 'html2wp-test'}})
            assert receive()['result']['value']['type'] == 'session/ready'
            source = '''
const results = await Promise.all([
  tools.project_shell({cmd: "pwd"}),
  tools.project_shell({cmd: "ls", cwd: "/input"})
]);
text({results, globals: {process: typeof process, require: typeof require, fetch: typeof fetch},
      shell: typeof tools.exec_command});
'''
            send({'type': 'operation/request', 'id': 2,
                  'request': {'method': 'session/execute', 'sessionId': 'html2wp-test',
                              'request': {'tool_call_id': 'inspection-batch', 'enabled_tools': enabled,
                                          'source': source, 'yield_time_ms': 10000, 'max_output_tokens': 2000}}})
            calls = []
            final = None
            for _ in range(10):
                message = receive()
                if message['type'] == 'delegate/request':
                    invocation = message['request']['invocation']
                    name = invocation['tool_name']['name']
                    if name == 'project_shell' and invocation['input'] == {'cmd': 'pwd'}:
                        result = {'exitCode': 0, 'output': '/work\n'}
                    elif name == 'project_shell':
                        assert invocation['input'] == {'cmd': 'ls', 'cwd': '/input'}
                        result = {'exitCode': 0, 'output': 'index.html\n'}
                    else:
                        raise AssertionError(f'Unexpected tool: {name}')
                    calls.append(name)
                    send({'type': 'delegate/response', 'id': message['id'],
                          'result': {'status': 'ok', 'value': {'type': 'tool/result', 'result': result}}})
                elif message['type'] == 'execute/initialResponse':
                    assert message['result']['status'] == 'ok', message
                    final = message['result']['value']['Result']
                    break
                else:
                    assert message['type'] == 'operation/response', message
                    assert message['result']['status'] == 'ok', message
            assert calls == ['project_shell', 'project_shell'], calls
            assert final and final['error_text'] is None, final
            output = json.loads(final['content_items'][0]['text'])
            assert output['results'][0]['output'] == '/work\n', output
            assert output['results'][1]['output'] == 'index.html\n', output
            assert output['globals'] == {'process': 'undefined', 'require': 'undefined', 'fetch': 'undefined'}, output
            assert output['shell'] == 'undefined', output
            return {'passed': True, 'tools': calls, 'modelCalls': 0, 'authUsed': False}
        finally:
            process.terminate()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            process.stdin.close()
            process.stdout.close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--host', type=Path)
    parser.add_argument('--tools', type=Path, default=Path(__file__).resolve().parents[1] / 'runtime/tools.json')
    args = parser.parse_args()
    print(json.dumps(check(args.host or find_host(), args.tools)))
