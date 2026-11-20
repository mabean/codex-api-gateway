#!/usr/bin/env python3
"""Offline P2 baseline diagnostics. Known gaps pass; unavailable checks fail."""
import argparse
import errno
import http.server
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time

from claude_contract_extract import PROFILE, digest, extract, read_graph
from claude_contract_p1 import ROOT, free_port, get, minimal_env, sandbox_policy

FIXTURES = ROOT/'scripts/fixtures/claude_contract'
CASES = FIXTURES/'p2_cases.json'
INVENTORY = FIXTURES/'p2_inventory.json'
P2_PROFILE = FIXTURES/'2.1.329-macos-arm64-p2.json'
FAMILIES = ['tools', 'effort', 'clear_at', 'retry', 'sse', 'usage']
CASE_IDS = ([f'tools_{route}_{tool}' for route in ('anthropic','codex') for tool in ('Read','NotebookEdit')]
            + [f'effort_{route}_{mode}' for route in ('anthropic','codex') for mode in ('nested','only')]
            + [f'clear_at_{route}_{turn}' for route in ('anthropic','codex') for turn in ('first','next')]
            + [f'retry_anthropic_{path}_{decision}' for path in ('direct','gateway') for decision in ('deny','allow')]
            + [f'{family}_{route}_{suffix}' for family, suffix in (('sse','barrier'),('usage','nonzero'))
               for route in ('anthropic','codex')])


def require(value, message):
    if not value:
        raise ValueError(message)


def has_field(value, field):
    if isinstance(value, dict):
        return field in value or any(has_field(v, field) for v in value.values())
    if isinstance(value, list):
        return any(has_field(v, field) for v in value)
    return False


def validate_cases(cases):
    require(len(cases) == 20, 'expected exactly twenty ordered P2 cases')
    require(list(dict.fromkeys(c['family'] for c in cases)) == FAMILIES, 'P2 family order changed')
    ids = [c['id'] for c in cases]
    require(len(ids) == len(set(ids)), 'duplicate case correlation ID')
    require(ids == CASE_IDS, 'case order/correlation IDs changed')
    for case in cases:
        require(case['route'] in ('anthropic', 'codex'), 'unknown route')
        require(case['expected_outcome'] in ('known_gap', 'correct_behavior'), 'unavailable mandatory observation')
        gap = (case['route'] == 'codex' or case['family'] in ('effort','clear_at') or
               case['family'] == 'retry' and not case['direct'])
        require(case['expected_outcome'] == ('known_gap' if gap else 'correct_behavior'), 'baseline classification changed')
        require(bool(case['expected']), 'missing observable expectation')
        require(case['family']+'_'+case['route']+'_' in case['id'], 'case route/family mismatch')
        if case['family'] == 'tools':
            require(case['tool'] in ('Read','NotebookEdit') and case['id'].endswith(case['tool']), 'tool boundary mismatch')
        elif case['family'] == 'effort':
            require(case['effort_only'] == case['id'].endswith('only'), 'effort control mismatch')
            require(case['expected_error'] == (case['route'] == 'codex' and case['effort_only']), 'validation control mismatch')
        elif case['family'] == 'clear_at':
            require(case['followup'] == case['id'].endswith('next'), 'chronology control mismatch')
        elif case['family'] == 'retry':
            require(case['direct'] == ('_direct_' in case['id']) and case['allow'] == case['id'].endswith('allow'), 'retry control mismatch')
    return cases


def verify_inventory(binary, directory):
    manifest = extract(binary, directory)
    _, modules = read_graph(binary)
    inventory = json.loads(INVENTORY.read_text())
    require(len(modules) == inventory['graph_module_count'], 'Bun graph size mismatch')
    hits = {}
    for marker in inventory['scan_markers']:
        hits[marker] = [{'id':m['id'], 'count':m['source'].count(marker.encode())}
                        for m in modules if m['loader'] == 1 and marker.encode() in m['source']]
    require(hits == inventory['scan_hits'], 'full graph endpoint/transport scan changed')
    for span in inventory['spans']:
        module = modules[span['module_id']]
        require(module['name'] == span['module'], 'inventory module identity mismatch')
        source = module['source']
        require(digest(source) == span['module_sha256'], 'inventory module fingerprint mismatch')
        require(0 <= span['start'] < span['end'] <= len(source), 'inventory span bounds')
        require(digest(source[span['start']:span['end']]) == span['sha256'], 'inventory raw byte span mismatch')
        require(module['offset']+span['start'] == span['binary_start'], 'inventory absolute position mismatch')
    profile = json.loads(P2_PROFILE.read_text())
    require(profile['binary_sha256'] == manifest['binary_sha256'], 'P2 binary mismatch')
    chunks = []
    for part in profile['ranges']:
        source = modules[part['module_id']]['source']
        chunk = source[part['start']:part['end']]
        require(digest(chunk) == part['sha256'], 'P2 executable range fingerprint mismatch')
        chunks.append(chunk)
    # Original method tokens are unmodified; only the surrounding class is an adapter.
    (directory/'p2-source.js').write_bytes(b'class Retry {'+chunks[0]+b'}\n'+chunks[1]+
                                         b'\nglobalThis.p2={Retry,SXn};\n')
    manifest['p2'] = profile
    manifest['inventory'] = {'spans_verified':len(inventory['spans']), 'modules_scanned':len(modules),
                             'inventory_sha256':digest(INVENTORY.read_bytes())}
    (directory/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
    return manifest


def sse_event(event):
    return ('event: '+event['type']+'\ndata: '+json.dumps(event)+'\n\n').encode()


def response_parts(case):
    text = 'P2 terminal '+case['id']
    if case['route'] == 'anthropic':
        head = [
            {'type':'message_start','message':{'id':'msg_'+case['id'],'type':'message','role':'assistant',
             'model':'claude-haiku-5-5','content':[],'stop_reason':None,'stop_sequence':None,
             'usage':{'input_tokens':17,'output_tokens':0,'cache_read_input_tokens':5,'cache_creation_input_tokens':3}}},
            {'type':'content_block_start','index':0,'content_block':{'type':'text','text':''}},
            {'type':'content_block_delta','index':0,'delta':{'type':'text_delta','text':text}}]
        tail = [{'type':'content_block_stop','index':0},
                {'type':'message_delta','delta':{'stop_reason':'end_turn','stop_sequence':None},'usage':{'output_tokens':11}},
                {'type':'message_stop'}]
        return b''.join(map(sse_event, head)), b''.join(map(sse_event, tail))
    head = [{'type':'response.created','response':{'id':'resp_'+case['id']}},
            {'type':'response.output_text.delta','delta':text,'output_index':0,'content_index':0}]
    tail = [{'type':'response.completed','response':{'id':'resp_'+case['id'],'status':'completed',
             'output':[{'type':'message','role':'assistant','content':[{'type':'output_text','text':text}]}],
             'usage':{'input_tokens':17,'output_tokens':11,'total_tokens':28,'input_tokens_details':{'cached_tokens':5}}}}]
    return b''.join(map(sse_event, head)), b''.join(map(sse_event, tail))


def validate_upstream(case, body, path, headers):
    anthropic = case['route'] == 'anthropic'
    expected_path = ('/v1/messages?beta=true' if case.get('direct') else '/v1/messages') if anthropic else '/codex/responses'
    require(path == expected_path, 'unexpected upstream path: '+path)
    require(body['model'] == ('claude-haiku-5-5' if anthropic else 'gpt-6.2-sol'), 'unexpected upstream model')
    require(body['stream'] is True, 'stream flag missing')
    if anthropic:
        require(headers.get('x-api-key') == 'p2-synthetic-upstream', 'unexpected upstream credential')
        require(body['messages'][0] == {'role':'user','content':case['id']}, 'case correlation/order changed')
        if not case.get('direct'):
            require(headers.get('anthropic-version') == '2023-06-01', 'version header lost')
            require('advanced-tool-use-2025-11-20' in headers.get('anthropic-beta',''), 'beta header lost')
    else:
        require(headers.get('Authorization') == 'Bearer p1-synthetic-codex', 'unexpected Codex credential')
        require(case['id'] in json.dumps(body['input']), 'Codex case correlation lost')
    family = case['family']
    if family == 'tools':
        name = case['tool']
        if anthropic:
            require([t['name'] for t in body['tools']] == ['ToolSearch', name], 'tool schema order changed')
            require(body['tools'][1]['defer_loading'] is True, 'deferred schema flag lost')
            require(body['tools'][1]['input_schema']['required'] == ['file_path'], 'schema field lost')
            use = body['messages'][1]['content'][0]
            result = body['messages'][2]['content'][0]
            require(use == {'type':'tool_use','id':'toolu_'+case['id'],'name':'ToolSearch','input':{'query':'select:'+name}}, 'tool call shape/order mismatch')
            require(result == {'type':'tool_result','tool_use_id':use['id'],'content':[{'type':'tool_reference','tool_name':name}]}, 'tool reference result mismatch')
        else:
            results = [x for x in body['input'] if x.get('type') == 'function_call_output']
            require(results == [{'type':'function_call_output','call_id':'toolu_'+case['id'],'output':''}], 'expected reference loss changed')
            calls = [x for x in body['input'] if x.get('type') == 'function_call']
            require(len(calls) == 1 and calls[0]['call_id'] == results[0]['call_id'], 'Codex tool correlation mismatch')
            require([x['name'] for x in body['tools']] == (['Read'] if name == 'Read' else []), 'supported/deferred schema boundary changed')
            if name == 'Read':
                require(body['tools'][0]['parameters']['required'] == ['file_path'], 'Read schema lost')
    elif family == 'effort':
        if anthropic:
            require(body['output_config'] == {'effort':'low'}, 'top-level effort changed')
            require([m['role'] for m in body['messages']] == ['user','assistant'], 'nested control not hoisted/dropped as baseline')
            require(all('output_config' not in m for m in body['messages']), 'nested effort baseline changed')
        else:
            require(body.get('reasoning') is None, 'Codex reasoning baseline changed')
    elif family == 'clear_at':
        if anthropic:
            require([m['role'] for m in body['messages']] == (['user','assistant','user'] if case['followup'] else ['user','assistant']), 'turn chronology mismatch')
            require(body['system'][-1] == {'type':'text','text':'P2 ephemeral'}, 'hoisted ephemeral baseline changed')
            require(not has_field(body, 'clear_at'), 'clear_at baseline loss changed')
        else:
            require('P2 ephemeral' in body['instructions'], 'Codex hoisting baseline changed')
            require(not has_field(body, 'clear_at'), 'Codex clear_at baseline changed')


class Fixture(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def json_response(self, body, status=200):
        self.send_response(status)
        self.send_header('content-type', 'application/json')
        self.end_headers()
        self.wfile.write(json.dumps(body).encode())

    def do_GET(self):
        if self.path == '/identity':
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b'p2-owned-fixture')
        else:
            self.send_error(404)

    def do_POST(self):
        try:
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            server = self.server
            if self.path == '/control':
                require(server.current is None, 'previous case not finished')
                case = server.cases[server.index]
                require(body == {'id':case['id']}, 'case order/correlation mismatch')
                server.current = case
                server.receipts = []
                server.fragment_sent = False
                server.release.clear()
                return self.json_response({'id':case['id']})
            if self.path == '/state':
                return self.json_response({'fragment_sent':server.fragment_sent,'closed':not server.release.is_set()})
            if self.path == '/release':
                require(server.fragment_sent and not server.release.is_set(), 'invalid barrier release')
                server.release.set()
                return self.json_response({'released':True})
            if self.path == '/finish':
                require(body == {'id':server.current['id']}, 'finish correlation mismatch')
                expected = 0 if server.current.get('expected_error') else 1
                require(len(server.receipts) == expected, 'unexpected upstream request count')
                require(not server.failures, 'fixture failure: '+str(server.failures))
                result = {'requests':len(server.receipts), 'validated':True,
                          'observations':server.receipts}
                server.completed.append({'id':server.current['id'],'requests':len(server.receipts),'validated':True})
                server.current = None
                server.index += 1
                return self.json_response(result)
            if self.path == '/complete':
                require(server.index == len(server.cases) and not server.failures, 'mandatory fixture cases missing')
                return self.json_response({'complete':True})
            case = server.current
            require(case is not None, 'unconfigured upstream request')
            validate_upstream(case, body, self.path, self.headers)
            server.receipts.append({'id':case['id'],'path':self.path,'body':body})
            if case['family'] == 'retry':
                self.send_response(429)
                for key, value in {'content-type':'application/json','retry-after-ms':'125','retry-after':'9',
                                   'x-should-retry':str(case['allow']).lower(), 'request-id':'req_p2',
                                   'anthropic-ratelimit-unified-status':'rejected'}.items():
                    self.send_header(key, value)
                self.end_headers()
                return self.wfile.write(b'{"type":"error","error":{"type":"rate_limit_error","message":"P2 limited"}}')
            self.send_response(200)
            self.send_header('content-type', 'text/event-stream')
            self.end_headers()
            head, tail = response_parts(case)
            self.wfile.write(head)
            self.wfile.flush()
            if case['family'] == 'sse':
                server.fragment_sent = True
                require(server.release.wait(5), 'terminal barrier release deadline')
            self.wfile.write(tail)
            self.wfile.flush()
        except Exception as error:
            self.server.failures.append(str(error))
            self.json_response({'error':str(error)}, 400)


def worker(args):
    directory = Path(args.worker)
    report = {'result':1, 'ports':[], 'checks':{}}
    server = gateway = None
    try:
        with socket.socket() as sock:
            sock.settimeout(1)
            try:
                sock.connect(('192.0.2.1', 9))
            except OSError as error:
                require(error.errno == errno.EPERM, 'network isolation probe not OS-denied')
            else:
                raise ValueError('external network allowed')
        try:
            Path(args.canary).read_bytes()
        except PermissionError:
            pass
        else:
            raise ValueError('credential canary readable')
        report['checks']['isolation'] = 0
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
        server.cases = validate_cases(json.loads(CASES.read_text())['cases'])
        server.current = None
        server.index = 0
        server.failures = []
        server.completed = []
        server.release = threading.Event()
        threading.Thread(target=server.serve_forever, daemon=True).start()
        upstream = 'http://127.0.0.1:'+str(server.server_port)
        port = free_port()
        report['ports'] = [server.server_port, port]
        auth = directory/'auth.json'
        auth.write_text('{"OPENAI_API_KEY":"p1-synthetic-codex"}')
        command = [args.gateway, '--port',str(port),'--auth-path',str(auth), '--upstream-base-url',upstream,
                   '--anthropic-upstream-base-url',upstream,'--anthropic-auth-token','p2-synthetic-upstream',
                   '--log-path',str(directory/'gateway.log')]
        with (directory/'gateway-output.log').open('w') as log:
            gateway = subprocess.Popen(command, env=minimal_env(directory), stdout=log, stderr=log)
        base = 'http://127.0.0.1:'+str(port)
        deadline = time.monotonic()+8
        while True:
            require(gateway.poll() is None, 'owned gateway exited at startup')
            try:
                get(base+'/health')
                break
            except OSError:
                require(time.monotonic() < deadline, 'gateway startup deadline')
                time.sleep(.05)
        result = subprocess.run([args.node, str(ROOT/'scripts/claude_contract_p2.mjs'),str(directory),base,upstream,str(CASES)],
                                env=minimal_env(directory),capture_output=True,text=True,timeout=35)
        (directory/'node-stderr.log').write_text(result.stderr)
        require(result.returncode == 0, 'extracted P2 SDK failed: '+result.stderr[-1800:])
        report['sdk'] = json.loads(result.stdout)
        require([x['id'] for x in report['sdk']['outcomes']] == [x['id'] for x in server.cases], 'outcome order changed')
        require(all(x['result'] == 0 for x in report['sdk']['outcomes']), 'mandatory outcome failed')
        require(len(server.completed) == len(server.cases) and not server.failures, 'fixture coverage incomplete')
        report['completed'] = server.completed
        report['checks']['six_families'] = 0
        report['result'] = 0
    except Exception as error:
        report['error'] = str(error)
        report['outcome'] = 'unavailable'
    finally:
        if gateway is not None:
            gateway.terminate()
            try:
                gateway.wait(timeout=3)
            except subprocess.TimeoutExpired:
                gateway.kill()
                gateway.wait(timeout=3)
        if server is not None:
            server.release.set()
            report['completed'] = server.completed
            report['fixture_failures'] = server.failures
            server.shutdown()
            server.server_close()
        report['owned_process_cleanup'] = gateway is None or gateway.poll() is not None
        (directory/'result.json').write_text(json.dumps(report, indent=2)+'\n')
    return report['result']


def run(args):
    require(sys.platform == 'darwin' and Path('/usr/bin/sandbox-exec').exists(), 'macOS sandbox-exec required')
    node = shutil.which('node')
    gateway = Path(args.gateway).resolve()
    require(node and gateway.is_file(), 'prebuilt gateway and local Node required')
    directory = Path(tempfile.mkdtemp(prefix='claude-contract-p2-')).resolve()
    print('local evidence: '+str(directory), flush=True)
    manifest = verify_inventory(args.binary, directory)
    manifest['gateway_sha256'] = digest(gateway.read_bytes())
    (directory/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
    canary = directory/'credential-canary'
    canary.write_text('synthetic protected credential canary')
    policy = directory/'sandbox.sb'
    policy.write_text(sandbox_policy(Path.home(), canary))
    # An independently owned listener detects accidental broad cleanup.
    sentinel = http.server.ThreadingHTTPServer(('127.0.0.1',0), Fixture)
    threading.Thread(target=sentinel.serve_forever,daemon=True).start()
    process = None
    try:
        command = ['/usr/bin/sandbox-exec','-f',str(policy),sys.executable,str(Path(__file__).resolve()),
                   '--worker',str(directory),'--node',node,'--gateway',str(gateway),'--canary',str(canary)]
        process = subprocess.Popen(command,env=minimal_env(directory),stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE,text=True,start_new_session=True)
        try:
            stdout, stderr = process.communicate(timeout=50)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid,signal.SIGKILL)
            process.communicate(timeout=3)
            raise ValueError('isolated P2 deadline; owned process group killed')
        path = directory/'result.json'
        require(path.exists(), 'isolated worker produced no report: '+stderr[-1000:])
        report = json.loads(path.read_text())
        report['worker_returncode'] = process.returncode
        report['unrelated_listener_alive'] = get('http://127.0.0.1:'+str(sentinel.server_port)+'/identity') == b'p2-owned-fixture'
        report['owned_ports_closed'] = True
        for port in report['ports']:
            with socket.socket() as sock:
                sock.settimeout(.5)
                if sock.connect_ex(('127.0.0.1',port)) == 0:
                    report['owned_ports_closed'] = False
        require(report['owned_process_cleanup'] and report['owned_ports_closed'] and report['unrelated_listener_alive'], 'owned cleanup failed')
        require(process.returncode == report['result'], 'worker/report result mismatch')
        path.write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps({'result':report['result'],'evidence':str(path), 'error':report.get('error'),
                          'cases':len(report.get('completed',[]))},indent=2))
        return report['result']
    finally:
        if process is not None and process.poll() is None:
            os.killpg(process.pid,signal.SIGKILL)
            process.communicate(timeout=3)
        sentinel.shutdown()
        sentinel.server_close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary')
    parser.add_argument('--gateway',default=str(ROOT/'target/release/codex-api-gateway'))
    parser.add_argument('--worker',help=argparse.SUPPRESS)
    parser.add_argument('--node',help=argparse.SUPPRESS)
    parser.add_argument('--canary',help=argparse.SUPPRESS)
    args = parser.parse_args()
    try:
        if args.worker:
            return worker(args)
        require(args.binary, '--binary required')
        return run(args)
    except Exception as error:
        print(json.dumps({'result':1,'outcome':'unavailable','error':str(error)}),file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
