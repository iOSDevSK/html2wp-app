"""Live, optional: how long pinned Codex takes to answer Stop's two requests
while the model waits on one of our tool calls (a step still running), and
whether they answer only once that tool call does (needs a Codex login).
Run: docker run --rm -v h2wpd-codex-auth:/a:ro -v $PWD/tests/stop_protocol_smoke.py:/probe.py:ro -v $PWD/runtime/tools.json:/tools.json:ro <runtime image> python3 /probe.py"""
import json, subprocess, selectors, time, shutil, os
os.makedirs('/tmp/h', exist_ok=True); shutil.copy('/a/auth.json', '/tmp/h/auth.json')
env = dict(os.environ, CODEX_HOME='/tmp/h')
p = subprocess.Popen(['codex', 'app-server', '--stdio', '-c', 'features.shell_tool=false', '-c', 'features.unified_exec=false',
                      '-c', 'sandbox_mode="read-only"', '-c', 'approval_policy="on-request"'],
                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env)
sel = selectors.DefaultSelector(); sel.register(p.stdout, selectors.EVENT_READ)
n = [0]; held = []; answered = {}; T0 = time.time()


def send(method, params=None, notify=False, reply=None):
    msg = {'id': reply[0], 'result': reply[1]} if reply else {'method': method, 'params': params or {}}
    if not notify and not reply:
        n[0] += 1; msg['id'] = n[0]
    p.stdin.write((json.dumps(msg) + '\n').encode()); p.stdin.flush()
    return msg.get('id')


def pump(seconds, want=None, until=None):
    """Read for `seconds`; hold every tool call; note each answered request id."""
    end = time.time() + seconds
    while time.time() < end:
        if not sel.select(0.5): continue
        line = p.stdout.readline()
        if not line: break
        m = json.loads(line); meth = m.get('method')
        if meth and 'id' in m:
            if meth == 'item/tool/call':
                held.append(m); print(f'{time.time()-T0:6.1f}s TOOL CALL held: {m["params"].get("tool")}', flush=True)
            else:
                send(None, reply=(m['id'], {'decision': 'decline'}))
            continue
        if meth:
            if meth in ('turn/started', 'turn/completed', 'thread/goal/updated'):
                print(f'{time.time()-T0:6.1f}s NOTIFY {meth} {json.dumps(m["params"])[:160]}', flush=True)
            if until and meth == until: return m
        elif 'id' in m:
            answered[m['id']] = time.time()
            if want is not None and m['id'] == want: return m
    return None


tools = json.load(open('/tools.json'))
i = send('initialize', {'clientInfo': {'name': 'h2wp_stop_probe', 'version': '0'}, 'capabilities': {'experimentalApi': True}}); pump(20, want=i)
send('initialized', notify=True)
i = send('thread/start', {'cwd': '/tmp', 'sandbox': 'read-only', 'approvalPolicy': 'on-request', 'dynamicTools': tools,
                          'developerInstructions': 'You are a test agent. Do exactly what the user asks, nothing else.'})
tid = pump(30, want=i)['result']['thread']['id']
i = send('turn/start', {'threadId': tid, 'input': [{'type': 'text', 'text': 'Call project_shell with cmd "sleep 600" now. Do not write anything else.', 'text_elements': []}]})
turn = pump(30, want=i)['result']['turn']['id']
i = send('thread/goal/set', {'threadId': tid, 'objective': 'Run the command once.', 'status': 'active'}); pump(30, want=i)
t = time.time()
while not held and time.time() - t < 120: pump(2)
if not held: raise SystemExit('the model made no tool call')
print(f'--- tool call held (a step still running); Stop as 0.2.11 sends it: goal pause, then interrupt', flush=True)
t_stop = time.time()
pause = send('thread/goal/set', {'threadId': tid, 'status': 'paused'})
interrupt = send('turn/interrupt', {'threadId': tid, 'turnId': turn})
pump(30)
for name, rid in (('goal/set paused', pause), ('turn/interrupt', interrupt)):
    print(f'{name}: ' + (f'answered after {answered[rid]-t_stop:.1f}s' if rid in answered else 'NOT answered within 30s while the tool call is held'), flush=True)
if interrupt not in answered:
    print('--- now the tool call ends (the step was stopped)', flush=True)
    t_end = time.time()
    for m in held: send(None, reply=(m['id'], {'success': False, 'contentItems': [{'type': 'inputText', 'text': 'Conversation was stopped'}]}))
    pump(30)
    for name, rid in (('goal/set paused', pause), ('turn/interrupt', interrupt)):
        print(f'{name}: ' + (f'answered {answered[rid]-t_end:.1f}s after the tool call ended' if rid in answered else 'still not answered'), flush=True)
p.terminate()
