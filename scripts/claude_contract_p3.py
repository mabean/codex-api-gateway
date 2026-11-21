#!/usr/bin/env python3
"""Mandatory positive offline Anthropic forwarding contract; no model calls."""
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

from claude_contract_extract import digest, read_graph
from claude_contract_p1 import ROOT, free_port, get, minimal_env, sandbox_policy
from claude_contract_p2 import require, sse_event, verify_inventory as verify_p2

FIXTURES = ROOT/'scripts/fixtures/claude_contract'
PROFILE = FIXTURES/'2.1.329-macos-arm64-p3.json'
CASES = FIXTURES/'p3_cases.json'
CASE_IDS = (['fidelity_'+a+'_'+t for a in ('key','oauth') for t in ('first','followup')]
            + ['retry_deny','retry_allow_short','retry_allow_long','context_reject','context_unrelated',
               'count_key','count_oauth','count_codex','json_messages','sse_barrier','catalog'])


def validate_cases(cases):
    require([c['id'] for c in cases] == CASE_IDS, 'missing, reordered or duplicate mandatory P3 case')
    for c in cases:
        require(isinstance(c.get('expected'),str) and bool(c['expected']), 'missing positive expectation')
        require(c['family'] in ('fidelity','retry','context','count','unsupported','json','sse','catalog'), 'unknown family')
        require(c['family'] == c['id'].split('_')[0] or
                (c['id'],c['family']) in [('count_codex','unsupported')], 'case family mismatch')
        if c['family'] == 'fidelity':
            require(c['auth'] == c['id'].split('_')[1] and c['followup'] == c['id'].endswith('followup'), 'fidelity control mismatch')
        if c['family'] == 'count':
            require(c['auth'] == c['id'].split('_')[1], 'count auth mismatch')
        if c['family'] == 'retry':
            require(c['allow'] == (c['id'] != 'retry_deny'), 'retry decision mismatch')
            require((c['ms'],c['seconds']) == ((275,1) if c['id'].endswith('long') else (125,9)), 'delay control mismatch')
        if c['family'] == 'context':
            require(c['reject'] == c['id'].endswith('reject'), 'context control mismatch')
    return cases


def verify_inventory(binary, directory):
    manifest = verify_p2(binary, directory)
    _, modules = read_graph(binary)
    profile = json.loads(PROFILE.read_text())
    require(profile['binary_sha256'] == manifest['binary_sha256'], 'P3 binary fingerprint mismatch')
    chunks = {}
    for part in profile['ranges']:
        module = modules[part['module_id']]
        source = module['source']
        require(module['name'] == part['module'] and digest(source) == part['module_sha256'], 'P3 module mismatch')
        require(module['offset']+part['start'] == part['binary_start'], 'P3 absolute byte offset mismatch')
        chunk = source[part['start']:part['end']]
        require(digest(chunk) == part['sha256'], 'P3 raw byte span fingerprint mismatch')
        chunks[part['name']] = chunk.decode()
    (directory/'p3-ranges.json').write_text(json.dumps(chunks))
    manifest['p3'] = profile
    return manifest


def response_parts(case):
    text = 'P3 terminal '+case['id']
    head = [dict(type='message_start',message=dict(id='msg_p3',type='message',role='assistant',model='claude-haiku-5-5',
             content=[],stop_reason=None,stop_sequence=None,usage=dict(input_tokens=17,output_tokens=0))),
            dict(type='content_block_start',index=0,content_block=dict(type='text',text='')),
            dict(type='content_block_delta',index=0,delta=dict(type='text_delta',text=text))]
    tail = [dict(type='content_block_stop',index=0),dict(type='message_delta',delta=dict(stop_reason='end_turn',stop_sequence=None),
             usage=dict(output_tokens=11)),dict(type='message_stop')]
    return b''.join(map(sse_event,head)), b''.join(map(sse_event,tail))


class Fixture(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def respond(self, body, status=200, headers=None):
        raw = body if isinstance(body,bytes) else json.dumps(body).encode()
        self.send_response(status)
        for key,value in (headers or {'content-type':'application/json'}).items():
            self.send_header(key,str(value))
        self.end_headers()
        self.wfile.write(raw)
        self.wfile.flush()

    def do_GET(self):
        if self.path == '/redirect':
            return self.respond(b'',302,{'location':'http://192.0.2.1:9/forbidden'})
        self.respond(b'p3-owned-fixture')

    def do_POST(self):
        server = self.server
        try:
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            if self.path == '/control':
                server.current = body
                server.receipts = []
                server.fragment_sent = False
                server.release.clear()
                return self.respond({'ready':True})
            if self.path == '/state':
                return self.respond({'fragment_sent':server.fragment_sent,'closed':not server.release.is_set()})
            if self.path == '/release':
                server.release.set()
                return self.respond({'released':True})
            if self.path == '/finish':
                server.release.set()
                return self.respond({'receipts':server.receipts,'failures':server.failures})
            case = server.current
            require(case is not None, 'unexpected upstream traffic')
            server.receipts.append({'path':self.path,'body':body,'headers':dict(self.headers)})
            attempt = len(server.receipts)
            headers = {'content-type':'application/json','request-id':'req_p3','anthropic-ratelimit-unified-status':'allowed',
                       'connection':'close, anthropic-ratelimit-unified-hidden',
                       'anthropic-ratelimit-unified-hidden':'secret','x-private-upstream':'secret'}
            if case['family'] == 'retry' and (not case['allow'] or attempt == 1):
                headers.update({'retry-after-ms':case['ms'],'Retry-After':case['seconds'],'x-should-retry':str(case['allow']).lower()})
                return self.respond(b'{"type":"error","error":{"type":"rate_limit_error","message":"P3 limited"}}',429,headers)
            if case['family'] == 'context' and attempt == 1:
                message = ('Unexpected value(s) `context-management-2025-06-27` for the `anthropic-beta`; context_management rejected'
                           if case['reject'] else 'unrelated invalid request')
                return self.respond({'type':'error','error':{'type':'invalid_request_error','message':message}},400,headers)
            if self.path.startswith('/v1/messages/count_tokens'):
                return self.respond(b'{"input_tokens":37}',headers=headers)
            if not body.get('stream'):
                return self.respond(b'{"id":"msg_json","type":"message","role":"assistant","content":[{"type":"text","text":"P3 JSON"}],"stop_reason":"end_turn","usage":{"input_tokens":17,"output_tokens":11}}',headers=headers)
            headers['content-type'] = 'text/event-stream'
            self.send_response(200)
            for k,v in headers.items():
                self.send_header(k,v)
            self.end_headers()
            head,tail = response_parts(case)
            self.wfile.write(head)
            self.wfile.flush()
            if case['family'] == 'sse':
                server.fragment_sent = True
                require(server.release.wait(8), 'terminal barrier deadline')
            self.wfile.write(tail)
            self.wfile.flush()
        except Exception as error:
            server.failures.append(str(error))


def worker(args):
    directory = Path(args.worker)
    report = {'result':1,'ports':[]}
    server = gateway = None
    try:
        with socket.socket() as sock:
            sock.settimeout(1)
            try:
                sock.connect(('192.0.2.1',9))
            except OSError as error:
                require(error.errno == errno.EPERM, 'external network not OS-denied')
            else:
                raise ValueError('external network allowed')
        try:
            Path(args.canary).read_bytes()
        except PermissionError:
            pass
        else:
            raise ValueError('credential canary readable')
        report['isolation'] = True
        server = http.server.ThreadingHTTPServer(('127.0.0.1',0),Fixture)
        server.current = None
        server.receipts = []
        server.failures = []
        server.release = threading.Event()
        threading.Thread(target=server.serve_forever,daemon=True).start()
        port = free_port()
        report['ports'] = [port,server.server_port]
        upstream = 'http://127.0.0.1:'+str(server.server_port)
        base = 'http://127.0.0.1:'+str(port)
        auth = directory/'auth.json'
        auth.write_text('{"OPENAI_API_KEY":"p3-synthetic-codex"}')
        command = [args.gateway,'--port',str(port),'--auth-path',str(auth),'--upstream-base-url',upstream,
                   '--anthropic-upstream-base-url',upstream,'--log-path',str(directory/'gateway.log')]
        environment = minimal_env(directory)
        environment['CODEX_GATEWAY_DISABLE_CLAUDE_KEYCHAIN'] = '1'
        with (directory/'gateway-output.log').open('w') as log:
            gateway = subprocess.Popen(command,env=environment,stdout=log,stderr=log)
        deadline = time.monotonic()+8
        while True:
            require(gateway.poll() is None, 'gateway exited at startup')
            try:
                get(base+'/health')
                break
            except OSError:
                require(time.monotonic()<deadline,'gateway startup deadline')
                time.sleep(.05)
        require(not args.fail_after_start, 'intentional failure after owned services start')
        validate_cases(json.loads(CASES.read_text())['cases'])
        result = subprocess.run([args.node,str(ROOT/'scripts/claude_contract_p3.mjs'),str(directory),base,upstream,str(CASES)],
                                env=minimal_env(directory),capture_output=True,text=True,timeout=65)
        (directory/'node-stderr.log').write_text(result.stderr)
        require(result.stdout.strip(), 'extracted P3 execution unavailable: '+result.stderr[-1800:])
        report['sdk'] = json.loads(result.stdout)
        outcomes = report['sdk']['outcomes']
        require([x['id'] for x in outcomes] == CASE_IDS, 'mandatory case missing')
        require(result.returncode == 0 and all(x['result']==0 for x in outcomes), 'mandatory positive P3 case failed')
        require(not server.failures, 'fixture failure')
        report['result'] = 0
    except Exception as error:
        report['error'] = str(error)
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
            server.shutdown()
            server.server_close()
            report['fixture_failures'] = server.failures
        report['owned_process_cleanup'] = gateway is None or gateway.poll() is not None
        (directory/'result.json').write_text(json.dumps(report,indent=2)+'\n')
    return report['result']


def run(args):
    require(sys.platform == 'darwin' and Path('/usr/bin/sandbox-exec').exists(), 'macOS sandbox-exec required')
    node = shutil.which('node')
    gateway = Path(args.gateway).resolve()
    require(node and gateway.is_file(), 'prebuilt gateway and local Node required')
    directory = Path(tempfile.mkdtemp(prefix='claude-contract-p3-')).resolve()
    print('local evidence: '+str(directory),flush=True)
    manifest = verify_inventory(args.binary,directory)
    manifest['gateway_sha256'] = digest(gateway.read_bytes())
    manifest['harness_sha256'] = {str(path.relative_to(ROOT)):digest(path.read_bytes()) for path in
        (Path(__file__).resolve(),ROOT/'scripts/claude_contract_p3.mjs',CASES,PROFILE)}
    (directory/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    canary = directory/'credential-canary'
    canary.write_text('synthetic protected canary')
    policy = directory/'sandbox.sb'
    policy.write_text(sandbox_policy(Path.home(),canary))
    sentinel = http.server.ThreadingHTTPServer(('127.0.0.1',0),Fixture)
    threading.Thread(target=sentinel.serve_forever,daemon=True).start()
    process = None
    try:
        command = ['/usr/bin/sandbox-exec','-f',str(policy),sys.executable,str(Path(__file__).resolve()),
                   '--worker',str(directory),'--node',node,'--gateway',str(gateway),'--canary',str(canary)]
        if args.fail_after_start:
            command.append('--fail-after-start')
        process = subprocess.Popen(command,env=minimal_env(directory),stdout=subprocess.PIPE,stderr=subprocess.PIPE,
                                   text=True,start_new_session=True)
        try:
            stdout,stderr = process.communicate(timeout=85)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid,signal.SIGKILL)
            process.communicate(timeout=3)
            raise ValueError('isolated P3 deadline; owned process group killed')
        path = directory/'result.json'
        require(path.exists(), 'isolated worker produced no report: '+stderr[-1000:])
        report = json.loads(path.read_text())
        report['worker_returncode'] = process.returncode
        report['unrelated_listener_alive'] = get('http://127.0.0.1:'+str(sentinel.server_port)+'/identity') == b'p3-owned-fixture'
        report['owned_ports_closed'] = True
        for port in report['ports']:
            with socket.socket() as sock:
                sock.settimeout(.5)
                if sock.connect_ex(('127.0.0.1',port)) == 0:
                    report['owned_ports_closed'] = False
        path.write_text(json.dumps(report,indent=2)+'\n')
        require(all(report[k] for k in ('owned_process_cleanup','owned_ports_closed','unrelated_listener_alive')), 'cleanup failed')
        require(process.returncode == report['result'], 'worker/report result mismatch')
        print(json.dumps({'result':report['result'],'evidence':str(path),'error':report.get('error')}))
        if report['result']:
            print(report.get('error','mandatory P3 failure'),file=sys.stderr)
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
    parser.add_argument('--fail-after-start',action='store_true')
    parser.add_argument('--worker',help=argparse.SUPPRESS)
    parser.add_argument('--node',help=argparse.SUPPRESS)
    parser.add_argument('--canary',help=argparse.SUPPRESS)
    args = parser.parse_args()
    try:
        if args.worker:
            return worker(args)
        require(args.binary,'--binary required')
        return run(args)
    except Exception as error:
        print(json.dumps({'result':1,'outcome':'unavailable','error':str(error)}),file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
