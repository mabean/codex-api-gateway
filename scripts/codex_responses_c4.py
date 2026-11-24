#!/usr/bin/env python3
"""C4 synthetic OAuth startup, account binding and owner restart contract."""
import argparse
import base64
import copy
import errno
import http.client
import http.server
import json
from pathlib import Path
import socket
import subprocess
import sys
import threading
import time

from claude_contract_p1 import ROOT, free_port, get, minimal_env
from claude_contract_p2 import require
from codex_responses_c1 import run

CASES = ROOT/'scripts/fixtures/codex_responses/c4_cases.json'
TOKEN = 'c4-secret-access'
ACCOUNT = 'c4-account'


def jwt(account=ACCOUNT, exp=4102444800):
    data = {'https://api.openai.com/auth':{'chatgpt_account_id':account},'exp':exp}
    return 'eyJhbGciOiJub25lIn0.'+base64.urlsafe_b64encode(json.dumps(data).encode()).decode().rstrip('=')+'.synthetic'


def native(token=TOKEN, account=ACCOUNT):
    return {'auth_mode':'chatgpt','tokens':{'access_token':token,'account_id':account,
            'refresh_token':'c4-secret-unused-refresh','id_token':'c4-secret-unused-id'}}


def legacy(profile=None):
    return {'profiles':{'chosen':profile or {'type':'oauth','provider':'openai-codex',
             'access':TOKEN,'accountId':ACCOUNT}}}


def scenarios():
    cases = {}
    def add(ident, auth, c3, c4): cases[ident] = (auth,c3,c4)
    add('native',native(),True,True)
    value = native(); value.pop('auth_mode'); add('native_historical',value,True,True)
    add('native_claim_account',native(jwt(),None),False,True)
    add('legacy',legacy(),True,True)
    value = legacy(); value['lastGood'] = {'openai-codex':'chosen'}; add('last_good',value,True,True)
    value = legacy(); value['profiles']['chosen'].pop('accountId'); value['profiles']['chosen']['access'] = jwt(); add('legacy_claim_account',value,True,True)
    add('api_key',{'OPENAI_API_KEY':'sk-c4-secret-api'},True,False)
    for ident, mode in [('native_api_mode','apikey'),('native_siwc','siwc'),('native_foreign','foreign')]:
        value = native(); value['auth_mode'] = mode; add(ident,value,True,False)
    for ident, provider in [('modern_openai','openai'),('foreign_provider','anthropic')]:
        value = legacy(); value['profiles']['chosen']['provider'] = provider; value['lastGood'] = {'openai-codex':'chosen'}; add(ident,value,True,False)
    value = legacy(); value['profiles']['chosen']['type'] = 'siwc'; value['lastGood'] = {'openai-codex':'chosen'}; add('legacy_siwc',value,False,False)
    value = legacy(); value['lastGood'] = {'openai-codex':'missing'}; add('dangling',value,True,False)
    value = legacy(); value['lastGood'] = {'openai-codex':7}; add('invalid_reference',value,False,False)
    value = legacy(); value['lastGood'] = []; add('invalid_last_good',value,False,False)
    value = legacy(); value['profiles']['second'] = copy.deepcopy(value['profiles']['chosen']); add('ambiguous',value,True,False)
    value = legacy(); value['profiles']['chosen']['expires'] = 1; add('expired_profile',value,True,False)
    add('expired_jwt',native(jwt(exp=1)),True,False)
    add('mismatch',native(jwt('other-account')),True,False)
    add('missing_account',native(TOKEN,None),False,False)
    add('empty_account',native(TOKEN,''),True,False)
    add('empty_access',native(''),True,False)
    add('invalid_access_header',native('c4-secret-access\r\nInjected: yes'),True,False)
    add('invalid_account_header',native(TOKEN,'c4-account\ninvalid'),True,False)
    add('platform_token',native('sk-c4-secret-api'),True,False)
    add('invalid_claims',native('a.not-base64!.c'),True,False)
    add('malformed','{"c4-secret-malformed":',False,False)
    add('empty',{},False,False)
    add('restart',native(),True,True)
    add('safe_401',native(),True,True)
    add('safe_403',native(),True,True)
    for ident, flow in [('legacy_siwc_token_sharing','chatgpt-token-sharing'),
                        ('legacy_siwc_identity','chatgpt-identity')]:
        value = legacy(); value['profiles']['chosen']['authFlow'] = flow
        value['lastGood'] = {'openai-codex':'chosen'}
        value['profiles']['fallback'] = legacy()['profiles']['chosen']
        add(ident,value,True,False)
    value = legacy(); value['profiles']['chosen']['access'] = 'c4-secret.opaque'
    add('legacy_opaque_dotted',value,True,True)
    value = native(); value['auth_mode'] = None; add('native_null_mode',value,True,True)
    for field in ('OPENAI_API_KEY','personal_access_token','bedrock_api_key','bedrock_access_keys'):
        value = native(); value.pop('auth_mode'); value[field] = 'c4-secret-alternative'
        add('native_mixed_'+field,value,True,False)
    value = native(); value['auth_mode'] = None; value['OPENAI_API_KEY'] = 'c4-secret-alternative'
    add('native_null_mixed',value,True,False)
    value = native(); value['OPENAI_API_KEY'] = 'c4-secret-alternative'
    add('native_explicit_precedence',value,True,True)
    return cases


REPAIR_INITIAL = {'legacy_siwc_token_sharing':True,'legacy_siwc_identity':True,
                  'legacy_opaque_dotted':False,'native_null_mode':False,
                  **{'native_mixed_'+field:True for field in ('OPENAI_API_KEY','personal_access_token','bedrock_api_key','bedrock_access_keys')},
                  'native_null_mixed':False,'native_explicit_precedence':True}


CASE_IDS = list(scenarios())


def validate_cases(cases):
    require([c['id'] for c in cases] == CASE_IDS,'mandatory C4 cases missing, duplicated or reordered')
    expected = scenarios()
    for case in cases:
        require(isinstance(case.get('expected_initial_c4'),bool) and case['expected_initial_c4'] == REPAIR_INITIAL.get(case['id'],expected[case['id']][2]),'invalid initial C4 expectation')
        require(case.get('criterion') and all(isinstance(case.get('expected_'+v),bool) and case['expected_'+v] == expected[case['id']][i] for v,i in [('c3',1),('c4',2)]),'invalid C4 expectation')
    return cases


class Fixture(http.server.BaseHTTPRequestHandler):
    def log_message(self,*_): pass
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.server.receipts.append((self.headers.get('Authorization'),self.headers.get('ChatGPT-Account-ID'),body))
        status = self.server.status
        self.send_response(status); self.send_header('content-type','text/event-stream'); self.end_headers()
        if status != 200:
            self.wfile.write(b'c4-secret-upstream-diagnostic'); return
        data = {'type':'response.completed','response':{'id':'c4','status':'completed','output':[]}}
        self.wfile.write(('data: '+json.dumps(data)+'\n\n').encode())


def request(port, server, token=TOKEN, account=ACCOUNT):
    before = len(server.receipts)
    connection = http.client.HTTPConnection('127.0.0.1',port,timeout=3)
    body = {'model':'gpt-6.2-sol','input':'synthetic C4','stream':False,
            'authorization':'c4-secret-inbound','chatgpt_account_id':'hostile-account'}
    connection.request('POST','/v1/responses',json.dumps(body),
                       {'Content-Type':'application/json','Authorization':'Bearer c4-secret-inbound',
                        'ChatGPT-Account-ID':'hostile-account','x-api-key':'sk-c4-secret-inbound'})
    response = connection.getresponse(); wire = response.read(); status = response.status; connection.close()
    require(status == server.status,'wrong HTTP outcome')
    require(len(server.receipts) == before+1,'unexpected attempt count')
    bearer, actual_account, _ = server.receipts[-1]
    require(bearer == 'Bearer '+token and actual_account == account,'configured account/bearer authority lost')
    require(b'c4-secret' not in wire,'credential or diagnostic leaked in response')
    require(isinstance(json.loads(wire),dict),'missing JSON envelope')
    return status


def worker(args):
    directory = Path(args.worker); server = None; gateway = None
    report = {'result':1,'ports':[],'outcomes':[]}
    def stop():
        nonlocal gateway
        if gateway is not None:
            gateway.terminate()
            try: gateway.wait(timeout=3)
            except subprocess.TimeoutExpired: gateway.kill(); gateway.wait(timeout=3)
            gateway = None
    try:
        with socket.socket() as sock:
            sock.settimeout(1)
            try: sock.connect(('192.0.2.1',9))
            except OSError as error: require(error.errno == errno.EPERM,'external network not OS-denied')
            else: raise ValueError('external network allowed')
        try: Path(args.canary).read_bytes()
        except PermissionError: pass
        else: raise ValueError('credential canary readable')
        report['isolation'] = True
        server = http.server.ThreadingHTTPServer(('127.0.0.1',0),Fixture)
        server.daemon_threads = True; server.receipts = []; server.status = 200
        threading.Thread(target=server.serve_forever,daemon=True).start()
        report['ports'].append(server.server_port)
        auth_path = directory/'auth.json'
        def start(ident, auth):
            nonlocal gateway
            auth_path.write_text(auth if isinstance(auth,str) else json.dumps(auth))
            port = free_port(); report['ports'].append(port)
            env = minimal_env(directory); env['CODEX_GATEWAY_DISABLE_CLAUDE_KEYCHAIN'] = '1'
            with (directory/(ident+'-output.log')).open('w') as log:
                gateway = subprocess.Popen([args.gateway,'--port',str(port),'--auth-path',str(auth_path),
                    '--upstream-base-url','http://127.0.0.1:'+str(server.server_port),
                    '--log-path',str(directory/(ident+'-gateway.log'))],env=env,stdout=log,stderr=log)
            deadline = time.monotonic()+4
            while gateway.poll() is None:
                try: get('http://127.0.0.1:'+str(port)+'/health'); return port,True
                except OSError:
                    require(time.monotonic() < deadline,'startup deadline'); time.sleep(.025)
            return port,False
        for case in validate_cases(json.loads(CASES.read_text())['cases']):
            ident = case['id']; auth,_,_ = scenarios()[ident]
            before = len(server.receipts)
            port, started = start(ident,auth)
            require(started == case['expected_'+args.expect.replace('-','_')],ident+': wrong startup result')
            if args.fail_after_start and started:
                raise ValueError('intentional failure after owned services start')
            if started:
                if ident == 'restart':
                    request(port,server)
                    auth_path.write_text(json.dumps(native('c4-secret-replacement','replacement-account')))
                    request(port,server)  # The original process retains its startup snapshot.
                    stop(); port, started = start('restarted',native('c4-secret-replacement','replacement-account'))
                    require(started,'owned restart failed'); request(port,server,'c4-secret-replacement','replacement-account')
                elif ident.startswith('safe_'):
                    server.status = int(ident[-3:]); request(port,server); server.status = 200
                elif args.expect == 'c4' or (args.expect == 'initial-c4' and ident in REPAIR_INITIAL):
                    token = auth.get('tokens',{}).get('access_token') if 'tokens' in auth else auth['profiles']['chosen']['access']
                    request(port,server,token,ACCOUNT)
            stop()
            if args.expect == 'c4':
                logs = ''.join(p.read_text() for p in directory.glob('*log'))
                require('c4-secret' not in logs,'secret in ordinary diagnostics')
                if not started: require('Codex auth:' in logs,'missing categorical startup diagnostic')
            report['outcomes'].append({'id':ident,'started':started,'outbound_count':len(server.receipts)-before,'result':0})
        report['result'] = 0
    except Exception as error: report['error'] = str(error)
    finally:
        stop()
        if server is not None: server.shutdown(); server.server_close()
        report['owned_process_cleanup'] = gateway is None
        (directory/'result.json').write_text(json.dumps(report,indent=2)+'\n')
    return report['result']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gateway',required=True)
    parser.add_argument('--expect',choices=('c3','initial-c4','c4'),required=True)
    parser.add_argument('--verify-failure-cleanup',action='store_true')
    parser.add_argument('--worker'); parser.add_argument('--canary')
    parser.add_argument('--fail-after-start',action='store_true')
    args = parser.parse_args()
    try: return worker(args) if args.worker else run(args,script=__file__,cases_path=CASES)
    except Exception as error:
        print(json.dumps({'result':1,'error':str(error)}),file=sys.stderr); return 1


if __name__ == '__main__': sys.exit(main())
