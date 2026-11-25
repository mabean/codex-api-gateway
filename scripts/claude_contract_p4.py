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
PROFILE = FIXTURES/'2.1.329-macos-arm64-p4.json'
CASES = FIXTURES/'p4_cases.json'
CASE_IDS = ['effort_'+x for x in ('low','medium','high','xhigh','override','only','followup','absent')] + ['scope_first','scope_followup','mid_history','bad_effort','max_effort','format','context','format_context','unknown_output','choice_auto','choice_none','choice_any','choice_named','choice_parallel','choice_parallel_false','choice_bad','choice_unavailable','choice_malformed','discovery_chain','legacy_read','standard_read','unsupported_reference','undeclared_reference','unrelated_reference']


def validate_cases(cases):
    require([c['id'] for c in cases] == CASE_IDS, 'missing, reordered or duplicate mandatory P4 case')
    for c in cases:
        require(isinstance(c.get('expected'),str) and bool(c['expected']), 'missing positive expectation')
    return cases


def verify_inventory(binary, directory):
    manifest = verify_p2(binary, directory)
    _, modules = read_graph(binary)
    profile = json.loads(PROFILE.read_text())
    require(profile['binary_sha256'] == manifest['binary_sha256'], 'P4 binary fingerprint mismatch')
    chunks = {}
    for part in profile['ranges']:
        module = modules[part['module_id']]
        source = module['source']
        require(module['name'] == part['module'] and digest(source) == part['module_sha256'], 'P4 module mismatch')
        require(module['offset']+part['start'] == part['binary_start'], 'P4 absolute byte offset mismatch')
        chunk = source[part['start']:part['end']]
        require(digest(chunk) == part['sha256'], 'P4 raw byte span fingerprint mismatch')
        chunks[part['name']] = chunk.decode()
    (directory/'p4-ranges.json').write_text(json.dumps(chunks))
    manifest['p4'] = profile
    return manifest


class Fixture(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def respond(self, body, status=200, content_type='application/json'):
        raw = body if isinstance(body,bytes) else json.dumps(body).encode()
        self.send_response(status)
        self.send_header('content-type',content_type)
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        if self.path == '/redirect':
            self.send_response(302)
            self.send_header('location','http://192.0.2.1:9/forbidden')
            self.end_headers()
        else:
            self.respond(b'p4-owned-fixture')

    def do_POST(self):
        server = self.server
        try:
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            if self.path == '/control':
                server.current = body
                server.receipts = []
                return self.respond({'ready':True})
            if self.path == '/finish':
                return self.respond({'receipts':server.receipts,'failures':server.failures})
            require(self.path == '/codex/responses','unexpected upstream path '+self.path)
            require(self.headers.get('Authorization') == 'Bearer p4-synthetic-codex','unexpected credential')
            server.receipts.append({'path':self.path,'body':body})
            stage = server.current.get('stage','text')
            ident = 'resp_p4'
            if stage in ('search','read'):
                name = 'ToolSearch' if stage == 'search' else 'Read'
                call = {'type':'function_call','id':'fc_p4_'+stage,'call_id':'toolu_p4_'+stage,
                        'name':name,'arguments':json.dumps({'query':'select:Read'} if stage == 'search' else {'file_path':'/fixture/p4.txt'})}
                output = [call]
            else:
                output = [{'type':'message','role':'assistant','content':[{'type':'output_text','text':'P4 terminal '+server.current['id']}]}]
            events = [{'type':'response.created','response':{'id':ident}},
                      *([{'type':'response.output_item.done','item':output[0]}] if stage in ('search','read') else
                        [{'type':'response.output_text.delta','delta':output[0]['content'][0]['text'],'output_index':0,'content_index':0}]),
                      {'type':'response.completed','response':{'id':ident,'status':'completed','output':output,
                       'usage':{'input_tokens':17,'output_tokens':11,'total_tokens':28}}}]
            self.respond(b''.join(map(sse_event,events)),content_type='text/event-stream')
        except Exception as error:
            server.failures.append(str(error))
            self.respond({'error':str(error)},500)


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
        auth.write_text('{"tokens":{"access_token":"p4-synthetic-codex","account_id":"synthetic-account"}}')
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
        result = subprocess.run([args.node,'--experimental-vm-modules',str(ROOT/'scripts/claude_contract_p4.mjs'),str(directory),base,upstream,str(CASES)],
                                env=minimal_env(directory),capture_output=True,text=True,timeout=65)
        (directory/'node-stderr.log').write_text(result.stderr)
        require(result.stdout.strip(), 'extracted P4 execution unavailable: '+result.stderr[-1800:])
        report['sdk'] = json.loads(result.stdout)
        outcomes = report['sdk']['outcomes']
        require([x['id'] for x in outcomes] == CASE_IDS, 'mandatory case missing')
        require(result.returncode == 0 and all(x['result']==0 for x in outcomes), 'mandatory positive P4 case failed')
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
    directory = Path(tempfile.mkdtemp(prefix='claude-contract-p4-')).resolve()
    print('local evidence: '+str(directory),flush=True)
    manifest = verify_inventory(args.binary,directory)
    manifest['gateway_sha256'] = digest(gateway.read_bytes())
    manifest['harness_sha256'] = {str(path.relative_to(ROOT)):digest(path.read_bytes()) for path in
        (Path(__file__).resolve(),ROOT/'scripts/claude_contract_p4.mjs',CASES,PROFILE)}
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
            raise ValueError('isolated P4 deadline; owned process group killed')
        path = directory/'result.json'
        require(path.exists(), 'isolated worker produced no report: '+stderr[-1000:])
        report = json.loads(path.read_text())
        report['worker_returncode'] = process.returncode
        report['unrelated_listener_alive'] = get('http://127.0.0.1:'+str(sentinel.server_port)+'/identity') == b'p4-owned-fixture'
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
            print(report.get('error','mandatory P4 failure'),file=sys.stderr)
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
