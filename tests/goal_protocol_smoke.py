"""Live, optional: pinned Codex continues a thread goal on its own (needs a Codex login).
Run: docker run --rm -v h2wpd-codex-auth:/a:ro -v $PWD/tests/goal_protocol_smoke.py:/probe.py:ro <runtime image> python3 /probe.py"""
import json, subprocess, selectors, time, sys, shutil, os
os.makedirs('/tmp/h',exist_ok=True); shutil.copy('/a/auth.json','/tmp/h/auth.json')
env=dict(os.environ,CODEX_HOME='/tmp/h')
p=subprocess.Popen(['codex','app-server','--stdio','-c','features.shell_tool=false','-c','features.unified_exec=false','-c','sandbox_mode="read-only"','-c','approval_policy="on-request"'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,env=env)
sel=selectors.DefaultSelector();sel.register(p.stdout,selectors.EVENT_READ)
n=[0]
def send(m,params=None,notify=False):
    msg={'method':m,'params':params or {}}
    if not notify: n[0]+=1;msg['id']=n[0]
    p.stdin.write((json.dumps(msg)+'\n').encode());p.stdin.flush();return msg.get('id')
def pump(until=None,timeout=60,want_id=None):
    end=time.time()+timeout;res=None
    while time.time()<end:
        if not sel.select(1):continue
        line=p.stdout.readline()
        if not line:break
        m=json.loads(line)
        meth=m.get('method')
        if meth and 'id' in m:
            print('SERVER-REQUEST',meth,json.dumps(m.get('params'))[:300]);send_resp=json.dumps({'id':m['id'],'result':{'decision':'decline'}})
            p.stdin.write((send_resp+'\n').encode());p.stdin.flush();continue
        if meth:
            if meth in('item/agentMessage/delta','item/reasoning/textDelta','item/reasoning/summaryTextDelta','thread/tokenUsage/updated','account/rateLimits/updated'):continue
            s=json.dumps(m['params'])
            print('NOTIFY',meth,s[:400])
            if until and meth==until:return m
        elif want_id is not None and m.get('id')==want_id:
            return m
    return res
i=send('initialize',{'clientInfo':{'name':'h2wp_probe','version':'0'},'capabilities':{'experimentalApi':True}});print(pump(want_id=i,timeout=20)['result'].keys())
send('initialized',notify=True)
i=send('thread/start',{'cwd':'/tmp','sandbox':'read-only','approvalPolicy':'on-request','developerInstructions':'You are a test agent. Be brief.'})
r=pump(want_id=i,timeout=30);tid=r['result']['thread']['id'];print('THREAD',tid)
i=send('turn/start',{'threadId':tid,'input':[{'type':'text','text':'Say GAMMA.','text_elements':[]}]});print('TURNSTART',json.dumps(pump(want_id=i,timeout=30))[:200])
i=send('thread/goal/set',{'threadId':tid,'objective':'After GAMMA, say DELTA in a new turn, then call update_goal with status complete.','status':'active'})
print('GOALSET-ACTIVE-TURN',json.dumps(pump(want_id=i,timeout=30))[:300])
pump(timeout=90)
print('--- reactivate a complete goal with new objective')
i=send('thread/goal/set',{'threadId':tid,'objective':'Say EPSILON, then call update_goal with status complete.','status':'active'})
print('GOALSET-AGAIN',json.dumps(pump(want_id=i,timeout=30))[:300])
pump(timeout=90)
p.terminate()
