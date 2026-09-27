"""The Cloudflare Pages helper against a fake wrangler and a fake Cloudflare API.
Nothing here reaches Cloudflare; the real deploy needs the owner's account."""
import json,os,subprocess,sys,tempfile,threading,unittest
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse,parse_qs
ROOT=Path(__file__).resolve().parents[1]
ACCOUNT='0123456789abcdef0123456789abcdef'
TOKEN='fixture-oauth-token-never-printed'

FAKE_WRANGLER=r'''#!/usr/bin/env python3
import json,os,sys
args=sys.argv[1:]
with open(os.environ['FAKE_LOG'],'a') as f: f.write(json.dumps({'args':args,'account':os.environ.get('CLOUDFLARE_ACCOUNT_ID'),'credential_present':bool(os.environ.get('CLOUDFLARE_API_TOKEN'))})+'\n')
mode=os.environ.get('FAKE_MODE','ok')
if args[:2]==['whoami','--json']:
    if mode=='signed-out': print('{"loggedIn":false}'); sys.exit(1)
    if mode=='offline': print('fetch failed'); sys.exit(1)
    print(json.dumps({'loggedIn':True,'email':'owner@example.invalid','accounts':[{'id':'0123456789abcdef0123456789abcdef','name':'Owner'},{'id':'bad','name':'Ignored'}]}))
elif args[:2]==['auth','token']:
    if mode=='signed-out': print('Not logged in.'); sys.exit(1)
    print(json.dumps({'type':'oauth','token':'fixture-oauth-token-never-printed'}))
elif args[:2]==['pages','deploy']:
    if not os.environ.get('CLOUDFLARE_API_TOKEN'): print('Not logged in. No credentials in this non-interactive environment.',file=sys.stderr);sys.exit(1)
    site=args[2];files=sorted(os.path.relpath(os.path.join(d,f),site) for d,_,fs in os.walk(site) for f in fs)
    with open(os.environ['FAKE_LOG'],'a') as f: f.write(json.dumps({'uploaded':files})+'\n')
    if mode=='deploy-denied': print('A request to /accounts/ACCOUNT/pages/projects/my-site/deployments failed. Authentication error [code: 10000]',file=sys.stderr); sys.exit(1)
    if mode=='deploy-number': print('Upload failed for 100000 bytes in asset-10000.txt',file=sys.stderr); sys.exit(1)
    if mode=='deploy-secret': print('Upload failed Bearer fixture-secret-token oauth_token=fixture-private-token \"access_token\":\"fixture-json-token\"',file=sys.stderr); sys.exit(1)
    if mode=='deploy-fails': print('\x1b[31mX [ERROR] Upload failed: file too large\x1b[0m',file=sys.stderr); sys.exit(1)
    print('Uploaded 3 files\n✨ Deployment complete! Take a peek over at https://abc123.my-site.pages.dev')
elif args[:1]==['logout']: print('Successfully logged out.')
'''

class Fake(BaseHTTPRequestHandler):
    state=None
    def log_message(self,*a):pass
    def reply(self,code,body):
        raw=json.dumps(body).encode();self.send_response(code);self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(raw)
    def handle_any(self,method):
        s=Fake.state;url=urlparse(self.path);length=int(self.headers.get('Content-Length') or 0)
        body=json.loads(self.rfile.read(length) or b'{}') if length else None
        s['requests'].append((method,url.path,self.headers.get('Authorization')))
        if self.headers.get('Authorization')!='Bearer '+TOKEN:return self.reply(403,{'success':False,'errors':[{'code':10000,'message':'Authentication error'}]})
        parts=url.path.strip('/').split('/')
        if parts==['zones']:
            name=parse_qs(url.query)['name'][0]
            return self.reply(200,{'success':True,'result':[{'id':'z','name':name}] if name in s['zones'] else []})
        base=['accounts',ACCOUNT,'pages','projects']
        if parts[:4]!=base:return self.reply(404,{'success':False,'errors':[{'code':7003,'message':'No route'}]})
        rest=parts[4:]
        if method=='POST' and not rest:
            if body['name'] in s['projects']:return self.reply(409,{'success':False,'errors':[{'code':8000002,'message':'A project with this name already exists'}]})
            s['projects'][body['name']]={'id':'cf-'+body['name'],'name':body['name'],'subdomain':body['name']+'-7x.pages.dev','production_branch':body['production_branch'],'domains':{}}
            return self.reply(200,{'success':True,'result':s['projects'][body['name']]})
        project=s['projects'].get(rest[0])
        if not project:return self.reply(404,{'success':False,'errors':[{'code':8000007,'message':'Project not found'}]})
        if method=='DELETE' and len(rest)==1:
            if s.get('deny_delete'):return self.reply(403,{'success':False,'errors':[{'code':10000,'message':'Authentication error'}]})
            del s['projects'][rest[0]]
            return self.reply(200,{'success':True,'result':None})
        if len(rest)==1:return self.reply(200,{'success':True,'result':project})
        if method=='POST':
            project['domains'][body['name']]={'name':body['name'],'status':'pending','verification_data':{'status':'pending'}}
            return self.reply(200,{'success':True,'result':project['domains'][body['name']]})
        domain=project['domains'].get(rest[2])
        if not domain:return self.reply(404,{'success':False,'errors':[{'code':8000017,'message':'Domain not found'}]})
        if method=='PATCH' and s.get('activate'):domain['status']='active'
        return self.reply(200,{'success':True,'result':domain})
    def do_GET(self):self.handle_any('GET')
    def do_POST(self):self.handle_any('POST')
    def do_PATCH(self):self.handle_any('PATCH')
    def do_DELETE(self):self.handle_any('DELETE')

class CloudflareHelperTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();base=Path(self.tmp.name)
        self.wrangler=base/'wrangler';self.wrangler.write_text(FAKE_WRANGLER);self.wrangler.chmod(0o755)
        self.site=base/'site';self.site.mkdir();(self.site/'index.html').write_text('<h1>Built</h1>')
        self.log=base/'calls.jsonl';self.log.write_text('')
        Fake.state={'requests':[],'projects':{},'zones':{'example.com'}}
        self.server=ThreadingHTTPServer(('127.0.0.1',0),Fake);threading.Thread(target=self.server.serve_forever,daemon=True).start()
    def tearDown(self):
        self.server.shutdown();self.server.server_close();self.tmp.cleanup()
    def run_helper(self,request,mode='ok'):
        env=dict(os.environ,H2WP_CF_API=f'http://127.0.0.1:{self.server.server_port}',H2WP_WRANGLER=str(self.wrangler),
            H2WP_CF_SITE=str(self.site),FAKE_LOG=str(self.log),FAKE_MODE=mode)
        run=subprocess.run([sys.executable,str(ROOT/'runtime/cloudflare.py')],input=json.dumps(request),capture_output=True,text=True,env=env,timeout=60)
        self.assertEqual(run.returncode,0,run.stderr)
        self.assertNotIn(TOKEN,run.stdout+run.stderr)
        return json.loads(run.stdout.strip().splitlines()[-1])
    def calls(self):return [c for c in map(json.loads,self.log.read_text().splitlines()) if 'args' in c]

    def test_whoami_lists_only_valid_accounts_and_reports_signed_out_or_offline(self):
        me=self.run_helper({'action':'whoami'})
        self.assertEqual(me,{'loggedIn':True,'email':'owner@example.invalid','accounts':[{'id':ACCOUNT,'name':'Owner'}]})
        self.assertEqual(self.run_helper({'action':'whoami'},'signed-out')['loggedIn'],False)
        self.assertIn('could not be reached',self.run_helper({'action':'whoami'},'offline')['message'])

    def test_first_deploy_creates_project_uploads_and_attaches_zone_domain(self):
        req={'accountId':ACCOUNT,'projectName':'my-site','domain':'www.example.com'}
        project=self.run_helper(req|{'action':'project'})
        self.assertEqual(project,{'ok':True,'created':True,'pagesUrl':'https://my-site-7x.pages.dev','cfProjectId':'cf-my-site'})
        self.assertEqual(Fake.state['projects']['my-site']['production_branch'],'main')
        deployed=self.run_helper(req|{'action':'deploy'})
        self.assertEqual(deployed['deploymentUrl'],'https://abc123.my-site.pages.dev')
        upload=[c for c in self.calls() if c['args'][:2]==['pages','deploy']][0]
        self.assertEqual(upload['args'],['pages','deploy',str(self.site),'--project-name','my-site','--branch','main','--commit-dirty=true'])
        self.assertEqual(upload['account'],ACCOUNT)
        self.assertTrue(upload['credential_present'],'Pages deploy needs the refreshed OAuth token explicitly in CI/non-interactive mode')
        domain=self.run_helper(req|{'action':'domain'})['domain']
        self.assertEqual((domain['status'],domain['zoneInAccount']),('pending',True))
        self.assertEqual(domain['record'],{'type':'CNAME','name':'www','content':'my-site-7x.pages.dev'})
        self.assertEqual(domain['dashboardUrl'],f'https://dash.cloudflare.com/{ACCOUNT}/pages/view/my-site/domains')
        Fake.state['activate']=True
        self.assertEqual(self.run_helper(req|{'action':'domain-check'})['domain']['status'],'active')

    def test_existing_project_is_reused_only_when_this_app_deployed_it(self):
        Fake.state['projects']['taken']={'name':'taken','subdomain':'taken.pages.dev','domains':{}}
        req={'action':'project','accountId':ACCOUNT,'projectName':'taken'}
        self.assertIn('already exists in this Cloudflare account',self.run_helper(req)['message'])
        self.assertEqual(self.run_helper(req|{'known':True}),{'ok':True,'created':False,'pagesUrl':'https://taken.pages.dev','cfProjectId':None})

    def test_removal_checks_saved_identity_and_preserves_project_on_denial(self):
        project=self.run_helper({'action':'project','accountId':ACCOUNT,'projectName':'my-site'})
        req={'action':'remove','accountId':ACCOUNT,'projectName':'my-site','expectedProjectId':project['cfProjectId'],'expectedPagesUrl':project['pagesUrl']}
        self.assertIn('no longer matches',self.run_helper(req|{'expectedProjectId':'wrong'})['message'])
        self.assertIn('no longer matches',self.run_helper(req|{'expectedPagesUrl':'https://other.pages.dev'})['message'])
        self.assertIn('my-site',Fake.state['projects'])
        Fake.state['deny_delete']=True
        self.assertIn('refused to remove',self.run_helper(req)['message'])
        self.assertIn('my-site',Fake.state['projects'])
        Fake.state['deny_delete']=False
        self.assertEqual(self.run_helper(req),{'ok':True,'removed':True,'alreadyAbsent':False})
        self.assertNotIn('my-site',Fake.state['projects'])
        self.assertEqual(self.run_helper(req),{'ok':True,'removed':False,'alreadyAbsent':True})

    def test_external_domain_gets_exact_record_and_root_domain_is_explained(self):
        self.run_helper({'action':'project','accountId':ACCOUNT,'projectName':'shop'})
        domain=self.run_helper({'action':'domain','accountId':ACCOUNT,'projectName':'shop','domain':'www.other.org'})['domain']
        self.assertEqual((domain['zoneInAccount'],domain['record']),(False,{'type':'CNAME','name':'www.other.org','content':'shop-7x.pages.dev'}))
        self.assertIn('root domain',self.run_helper({'action':'domain','accountId':ACCOUNT,'projectName':'shop','domain':'other.org'})['message'])

    def test_invalid_input_never_reaches_wrangler_or_the_api(self):
        for bad in [{'accountId':'x','projectName':'ok'},{'accountId':ACCOUNT,'projectName':'-bad'},{'accountId':ACCOUNT,'projectName':'a;rm -rf /'},
                    {'accountId':ACCOUNT,'projectName':'ok','domain':'https://example.com/x'},{'accountId':ACCOUNT,'projectName':'ok','domain':'--help.com'}]:
            self.assertFalse(self.run_helper(bad|{'action':'deploy'})['ok'],bad)
        self.assertEqual(self.calls(),[]);self.assertEqual(Fake.state['requests'],[])

    def test_any_astro_output_shape_is_uploaded_whole(self):
        for file in ['about/index.html','kontakt.html','blog/2024/prvý-článok/index.html','404.html','_astro/app.Bx1-q.css','images/hero photo.webp','robots.txt','_redirects']:
            path=self.site/file;path.parent.mkdir(parents=True,exist_ok=True);path.write_text('x')
        self.assertTrue(self.run_helper({'action':'deploy','accountId':ACCOUNT,'projectName':'any-site'})['ok'])
        uploaded=[json.loads(l)['uploaded'] for l in self.log.read_text().splitlines() if 'uploaded' in l][0]
        self.assertEqual(set(uploaded),{'index.html','about/index.html','kontakt.html','blog/2024/prvý-článok/index.html','404.html','_astro/app.Bx1-q.css','images/hero photo.webp','robots.txt','_redirects'})

    def test_denied_deployment_does_not_claim_browser_login_failed(self):
        req={'action':'deploy','accountId':ACCOUNT,'projectName':'my-site'}
        denied=self.run_helper(req,'deploy-denied')['message']
        self.assertIn('deployment creation',denied)
        self.assertIn('10000',denied)
        self.assertIn('Browser sign-in may still be valid',denied)
        ordinary=self.run_helper(req,'deploy-number')['message']
        self.assertIn('100000 bytes',ordinary)
        self.assertNotIn('denied',ordinary)
        self.assertNotIn('Sign in again',ordinary)
        redacted=self.run_helper(req,'deploy-secret')['message']
        self.assertNotIn('fixture-secret-token',redacted)
        self.assertNotIn('fixture-private-token',redacted)
        self.assertNotIn('fixture-json-token',redacted)

    def test_errors_are_human_readable(self):
        req={'accountId':ACCOUNT,'projectName':'my-site'}
        self.assertIn('signed out',self.run_helper(req|{'action':'project'},'signed-out')['message'])
        self.assertIn('file too large',self.run_helper(req|{'action':'deploy'},'deploy-fails')['message'])
        (self.site/'index.html').unlink()
        self.assertIn('built site is missing',self.run_helper(req|{'action':'deploy'})['message'])
        self.server.shutdown();self.server.server_close()
        self.assertIn('could not be reached',self.run_helper(req|{'action':'project'})['message'])
        self.server=ThreadingHTTPServer(('127.0.0.1',0),Fake);threading.Thread(target=self.server.serve_forever,daemon=True).start()

if __name__=='__main__':unittest.main()
