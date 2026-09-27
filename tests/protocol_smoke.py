"""No model call or login. Verify pinned Codex stdio with an empty auth home."""
import json, os, pathlib, selectors, subprocess, tempfile
with tempfile.TemporaryDirectory(prefix='h2wp-protocol-') as home:
    env=dict(os.environ,CODEX_HOME=home)
    p=subprocess.Popen(['codex','app-server','--stdio'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,env=env)
    selector=selectors.DefaultSelector();selector.register(p.stdout,selectors.EVENT_READ)
    def send(value):
        p.stdin.write((json.dumps(value)+'\n').encode());p.stdin.flush()
    def response(id):
        while selector.select(20):
            line=p.stdout.readline()
            if not line:break
            result=json.loads(line)
            if result.get('id')==id:return result
        raise AssertionError('Protocol response timed out')
    try:
        send({'id':1,'method':'initialize','params':{'clientInfo':{'name':'html2wp_desktop_test','version':'0.1.0'},'capabilities':{'experimentalApi':True}}})
        assert 'result' in response(1)
        send({'method':'initialized'})
        send({'id':2,'method':'account/read','params':{'refreshToken':False}})
        assert response(2)['result']['account'] is None
        send({'id':3,'method':'desktop/unsupported','params':{}})
        assert 'error' in response(3)
        print('PASS: initialize, initialized, account/read, unknown request; no login or model call.')
    finally:p.terminate();p.wait(timeout=10)
