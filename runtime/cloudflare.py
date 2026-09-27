"""Cloudflare Pages client for the desktop app. Runs in its own container with
only the owner's wrangler credentials (/cfg) and the built site (/site).
The OAuth token stays in this process: it is never printed or returned."""
import json, os, re, subprocess, sys, urllib.error, urllib.parse, urllib.request

API=os.environ.get('H2WP_CF_API','https://api.cloudflare.com/client/v4')
WRANGLER=os.environ.get('H2WP_WRANGLER','wrangler')
SITE=os.environ.get('H2WP_CF_SITE','/site')
NAME=re.compile(r'^[a-z0-9](?:[a-z0-9-]{0,56}[a-z0-9])?$')
ACCOUNT=re.compile(r'^[0-9a-f]{32}$')
LABEL=re.compile(r'^(?!-)[a-z0-9-]{1,63}(?<!-)$')
ANSI=re.compile(r'\x1b\[[0-9;]*m')

class Failure(Exception):
    """A problem the owner can act on; the message is shown as is."""

def valid_domain(domain):
    labels=domain.split('.')
    if len(domain)>253 or len(labels)<2 or not all(LABEL.match(l) for l in labels) or labels[-1].isdigit():
        raise Failure('Enter a domain like www.example.com, without https:// or a path.')
    return domain

def wrangler(*args, account=None, credential=None):
    env=dict(os.environ)
    if account: env['CLOUDFLARE_ACCOUNT_ID']=account
    # Wrangler's Pages commands require an explicit token when invoked with
    # redirected stdio/CI, even while whoami and auth token see the saved OAuth
    # login. Keep the refreshed token in this child process only.
    if credential: env['CLOUDFLARE_API_TOKEN']=credential
    run=subprocess.run([WRANGLER,*args],capture_output=True,text=True,env=env,timeout=900)
    return run.returncode,ANSI.sub('',run.stdout),ANSI.sub('',run.stderr)

def whoami():
    code,out,_=wrangler('whoami','--json')
    try: data=json.loads(out[out.index('{'):])
    except ValueError: data={}
    if data.get('loggedIn') is False:
        return {'loggedIn':False,'email':None,'accounts':[]}
    if code!=0 or not data.get('loggedIn'):
        raise Failure('Cloudflare could not be reached to confirm your sign-in. Check your internet connection and try again.')
    accounts=[{'id':a.get('id'),'name':str(a.get('name') or '')[:200]} for a in data.get('accounts') or [] if ACCOUNT.match(str(a.get('id','')))]
    return {'loggedIn':True,'email':data.get('email'),'accounts':accounts}

def token():
    code,out,_=wrangler('auth','token','--json')
    try: value=json.loads(out[out.index('{'):]).get('token')
    except ValueError: value=None
    if code!=0 or not isinstance(value,str) or not value:
        raise Failure('You are signed out of Cloudflare. Sign in again to deploy.')
    return value

def api(key, method, path, body=None):
    request=urllib.request.Request(API+path,method=method,data=None if body is None else json.dumps(body).encode(),
        headers={'Authorization':'Bearer '+key,'Content-Type':'application/json','User-Agent':'html2wp-desktop'})
    try:
        with urllib.request.urlopen(request,timeout=60) as response: return json.loads(response.read() or b'{}')
    except urllib.error.HTTPError as e:
        try: return json.loads(e.read() or b'{}')|{'status':e.code}
        except ValueError: return {'success':False,'status':e.code,'errors':[]}
    except (urllib.error.URLError,TimeoutError,OSError):
        raise Failure('Cloudflare could not be reached. Check your internet connection and try again.')

def codes(response): return {e.get('code') for e in response.get('errors') or []}

def api_error(response, action):
    if response.get('status') in (401,403) or 10000 in codes(response):
        return Failure(f'Cloudflare refused to {action}. Sign out and sign in again, and choose an account where you can manage Pages.')
    first=next((e.get('message') for e in response.get('errors') or [] if e.get('message')),'unknown error')
    return Failure(f'Cloudflare could not {action}: {first}')

def pages_url(project): return 'https://'+(project.get('subdomain') or project['name']+'.pages.dev')

def ensure_project(key, account, name, known):
    """Create the Pages project, or reuse one this app deployed before.
    Another project with the same name is never overwritten silently."""
    found=api(key,'GET',f'/accounts/{account}/pages/projects/{name}')
    if found.get('success'):
        if not known:
            raise Failure(f'A Pages project named "{name}" already exists in this Cloudflare account. Choose another project name, or confirm that this site should replace it.')
        return found['result'],False
    if found.get('status')!=404 and 8000007 not in codes(found):
        raise api_error(found,'check the Pages project')
    created=api(key,'POST',f'/accounts/{account}/pages/projects',{'name':name,'production_branch':'main'})
    if not created.get('success'):
        if 8000002 in codes(created) or 'already' in json.dumps(created.get('errors')).lower():
            raise Failure(f'The project name "{name}" is taken. Choose another project name.')
        raise api_error(created,'create the Pages project')
    return created['result'],True

def zone_for(key, account, domain):
    """The account's zone that contains the domain, if any."""
    labels=domain.split('.')
    for i in range(len(labels)-1):
        candidate='.'.join(labels[i:])
        found=api(key,'GET','/zones?'+urllib.parse.urlencode({'name':candidate,'account.id':account}))
        if found.get('success') and found.get('result'):
            return found['result'][0]
    return None

def domain_state(domain, project, zone, result, account):
    target=(project.get('subdomain') or project['name']+'.pages.dev')
    host=domain[:-len(zone['name'])-1] if zone and domain!=zone['name'] else ('@' if zone else domain)
    apex=bool(zone and domain==zone['name']) or len(domain.split('.'))==2
    status=(result or {}).get('status') or 'pending'
    detail=((result or {}).get('verification_data') or {}).get('error_message') or ((result or {}).get('validation_data') or {}).get('error_message')
    return {'name':domain,'status':status,'detail':detail,'zoneInAccount':bool(zone),'apex':apex,
        'record':{'type':'CNAME','name':host,'content':target},
        'dashboardUrl':f'https://dash.cloudflare.com/{account}/pages/view/{project["name"]}/domains',
        'url':'https://'+domain}

def attach_domain(key, account, project, domain):
    name=project['name']
    zone=zone_for(key,account,domain)
    if len(domain.split('.'))==2 and not zone:
        raise Failure(f'{domain} is a root domain. Cloudflare Pages can serve a root domain only when its DNS is in your Cloudflare account. Add the domain to Cloudflare first, or use a subdomain such as www.{domain}.')
    current=api(key,'GET',f'/accounts/{account}/pages/projects/{name}/domains/{domain}')
    if not current.get('success'):
        current=api(key,'POST',f'/accounts/{account}/pages/projects/{name}/domains',{'name':domain})
        if not current.get('success'):
            if 8000018 in codes(current) or 'already' in json.dumps(current.get('errors')).lower():
                raise Failure(f'{domain} is already connected to another Pages project. Remove it there first, then deploy again.')
            raise api_error(current,f'connect {domain}')
    return domain_state(domain,project,zone,current.get('result'),account)

def check_domain(key, account, name, domain):
    found=api(key,'GET',f'/accounts/{account}/pages/projects/{name}')
    if not found.get('success'): raise api_error(found,'read the Pages project')
    # PATCH asks Cloudflare to validate again now instead of on its own schedule.
    current=api(key,'PATCH',f'/accounts/{account}/pages/projects/{name}/domains/{domain}',{})
    if not current.get('success'):
        current=api(key,'GET',f'/accounts/{account}/pages/projects/{name}/domains/{domain}')
    if not current.get('success'): raise api_error(current,f'check {domain}')
    return domain_state(domain,found['result'],zone_for(key,account,domain),current['result'],account)

def deploy_failure(text):
    # A Pages operation can be denied while OAuth/whoami remains valid.
    # Match actual diagnostic codes, not numbers in file names or byte counts.
    found=re.findall(r'\[code:\s*(\d+)\]|["\']code["\']\s*:\s*(\d+)',text,re.I)
    error_codes=list(dict.fromkeys(a or b for a,b in found))
    operation='Pages upload'
    for path,label in [('/pages/assets/check-missing','asset verification'),
                       ('/pages/assets/upload','asset upload'),
                       ('/upload-token','upload authorization'),
                       ('/deployments','deployment creation')]:
        if path in text: operation=label
    if 'not logged in' in text.lower():
        return Failure('Wrangler is signed out. Sign in to Cloudflare again, then deploy.')
    if '10000' in error_codes or 'Authentication error' in text:
        code=f" (Cloudflare error {', '.join(error_codes)})" if error_codes else ''
        return Failure(f'Cloudflare denied {operation}{code}. Browser sign-in may still be valid. Check access to this Pages project and retry; signing in again alone may not resolve it.')
    if any(word in text for word in ('ENOTFOUND','fetch failed','ECONNRESET')):
        return Failure('Cloudflare could not be reached. Check your internet connection and try again.')
    # Keep useful provider detail, but never expose an authorization value.
    text=re.sub(r'(?i)(bearer\s+)[^\s"\']+',r'\1[redacted]',text)
    text=re.sub(r'''(?i)(["']?(?:oauth_token|access_token|refresh_token|api_token|authorization)["']?\s*[:=]\s*)(?:"[^"]*"|'[^']*'|[^\s,}]+)''',r'\1[redacted]',text)
    text=re.sub(r'eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+','[redacted]',text)
    last=[l.strip() for l in text.splitlines() if l.strip() and not l.startswith('🪵')][-3:]
    code=f" (Cloudflare error {', '.join(error_codes)})" if error_codes else ''
    return Failure(f'The {operation} failed{code}: '+' '.join(last)[-600:])

def deploy(account, name, credential):
    if not os.path.isfile(os.path.join(SITE,'index.html')):
        raise Failure('The built site is missing. Run the conversion until the static build is complete, then deploy.')
    code,out,err=wrangler('pages','deploy',SITE,'--project-name',name,'--branch','main','--commit-dirty=true',account=account,credential=credential)
    text=out+'\n'+err
    if code!=0:
        raise deploy_failure(text)
    urls=re.findall(r'https://[a-z0-9.-]+\.pages\.dev',out)
    return urls[-1] if urls else None

def remove_project(key, account, name, expected_id, expected_url):
    """Remove only the project this app recorded, after a fresh identity check."""
    path=f'/accounts/{account}/pages/projects/{name}'
    found=api(key,'GET',path)
    if not found.get('success'):
        if found.get('status')==404 or 8000007 in codes(found):
            return {'ok':True,'removed':False,'alreadyAbsent':True}
        raise api_error(found,'check the Pages project before removal')
    project=found['result']
    if project.get('name')!=name or (expected_id and project.get('id')!=expected_id) or pages_url(project)!=expected_url:
        raise Failure('This Pages project no longer matches the saved deployment. Nothing was removed.')
    deleted=api(key,'DELETE',path)
    if not deleted.get('success'):
        if deleted.get('status')==404 or 8000007 in codes(deleted):
            return {'ok':True,'removed':False,'alreadyAbsent':True}
        raise api_error(deleted,'remove the Pages project')
    return {'ok':True,'removed':True,'alreadyAbsent':False}

def main(request):
    action=request.get('action')
    if action=='whoami': return whoami()
    if action=='logout':
        wrangler('logout')
        return {'ok':True}
    account=str(request.get('accountId',''))
    if not ACCOUNT.match(account): raise Failure('Choose a Cloudflare account first.')
    name=str(request.get('projectName',''))
    if not NAME.match(name): raise Failure('Use 1–58 lowercase letters, digits or hyphens for the project name, not starting or ending with a hyphen.')
    domain=request.get('domain') or None
    if domain: domain=valid_domain(str(domain))
    key=token()
    if action=='project':
        project,created=ensure_project(key,account,name,bool(request.get('known')))
        return {'ok':True,'created':created,'pagesUrl':pages_url(project),'cfProjectId':project.get('id')}
    if action=='deploy':
        return {'ok':True,'deploymentUrl':deploy(account,name,key)}
    if action=='remove':
        expected_url=str(request.get('expectedPagesUrl') or '')
        expected_id=str(request.get('expectedProjectId') or '')
        if not expected_url.startswith('https://') or not expected_url.endswith('.pages.dev'):
            raise Failure('This app has no verified Pages address for the project. Nothing was removed.')
        return remove_project(key,account,name,expected_id,expected_url)
    if action=='domain':
        found=api(key,'GET',f'/accounts/{account}/pages/projects/{name}')
        if not found.get('success'): raise api_error(found,'read the Pages project')
        return {'ok':True,'domain':attach_domain(key,account,found['result'],domain)} if domain else {'ok':True,'domain':None}
    if action=='domain-check':
        if not domain: raise Failure('No custom domain is set for this site.')
        return {'ok':True,'domain':check_domain(key,account,name,domain)}
    raise Failure('Unknown Cloudflare action')

if __name__=='__main__':
    try: result=main(json.loads(sys.stdin.read() or '{}'))
    except Failure as e: result={'ok':False,'message':str(e)}
    except subprocess.TimeoutExpired: result={'ok':False,'message':'Cloudflare took too long to answer. Try again.'}
    print(json.dumps(result))
