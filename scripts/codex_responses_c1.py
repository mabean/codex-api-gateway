#!/usr/bin/env python3
"""Offline C1 release HTTP contract; synthetic credentials, OS loopback sandbox."""
import argparse
import errno
import hashlib
import http.client
import http.server
import json
import os
from pathlib import Path
import queue
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time

from claude_contract_p1 import ROOT, free_port, get, minimal_env, sandbox_policy
from claude_contract_p2 import require

CASES = ROOT/'scripts/fixtures/codex_responses/c1_cases.json'
CASE_IDS = ['text', 'tool', 'completed_hold', 'done_completed', 'done_incomplete',
            'done_failed', 'incomplete', 'failed', 'error', 'eof', 'malformed',
            'contradiction', 'event_mismatch', 'bad_done', 'limit', 'timeout',
            'disconnect', 'retry', 'exhaustion', 'precommit_retry', 'eof_terminal',
            'eof_terminal_no_newline', 'eof_missing_terminal', 'eof_malformed_terminal']


def validate_cases(cases):
    require([c['id'] for c in cases] == CASE_IDS, 'mandatory C1 cases missing, duplicated or reordered')
    require(all(c.get('criterion') and c.get('expected') for c in cases), 'missing C1 expectation')
    return cases


def frame(value):
    return ('data: '+json.dumps(value, ensure_ascii=False)+'\n\n').encode()


class Fixture(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_): pass

    def do_GET(self):
        self.send_response(200); self.end_headers(); self.wfile.write(b'c1-owned-fixture')

    def do_POST(self):
        server = self.server
        try:
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            require(self.path == '/codex/responses' and body['stream'], 'unexpected upstream request')
            server.attempts += 1
            ident = server.ident
            if ident == 'exhaustion' or (ident == 'retry' and server.attempts == 1):
                self.send_response(503); self.end_headers(); return
            if ident == 'precommit_retry' and server.attempts == 1:
                self.send_response(200); self.send_header('Content-Length', '100'); self.end_headers()
                return
            self.send_response(200); self.send_header('content-type', 'text/event-stream'); self.end_headers()
            def emit(raw):
                server.wire.extend(raw)
                size = 1 if len(raw) < 4096 else 4093
                for offset in range(0, len(raw), size):
                    self.wfile.write(raw[offset:offset+size]); self.wfile.flush()
            if ident == 'tool':
                initial = (b'id: native-tool\r\nevent: response.function_call_arguments.delta\r\n'
                           b'data: {"type":"response.function_call_arguments.delta",\r\n'
                           b'data: "call_id":"call_c1","delta":"{\\"x\\":","opaque":{"keep":true}}\r\n\r\n')
            else:
                initial = (': native comment\r\nid: native-text\r\nevent: response.output_text.delta\r\n'
                           'data: {"type":"response.output_text.delta",\r\n'
                           'data: "delta":"C1 café","opaque":{"keep":true}}\r\n\r\n').encode()
            emit(initial); server.fragment.set()
            if ident in ('text', 'tool'):
                require(server.release.wait(4), 'terminal barrier not released')
            if ident == 'disconnect':
                self.connection.settimeout(4)
                server.closed = self.connection.recv(1) == b''
                return
            if ident == 'timeout':
                server.release.wait(8); return
            if ident == 'eof': return
            if ident == 'eof_missing_terminal':
                emit(frame({'type':'opaque','extension':42}).rstrip(b'\n')); return
            if ident == 'eof_malformed_terminal':
                emit(b'data: {"type":"response.completed","response":'); return
            if ident == 'malformed': emit(b'data: {broken}\n\n'); return
            if ident == 'limit': emit(b':'+b'x'*(1024*1024)+b'\n\n'); return
            if ident == 'event_mismatch':
                emit(b'event: response.failed\ndata: {"type":"response.completed","response":{"status":"completed"}}\n\n'); return
            if ident == 'bad_done':
                emit(frame({'type':'response.done','response':{'status':'in_progress'}})); return
            if ident == 'error': emit(frame({'type':'error','message':'synthetic failure','opaque':7})); return
            status = 'incomplete' if ident in ('incomplete', 'done_incomplete') else 'failed' if ident in ('failed', 'done_failed', 'contradiction') else 'completed'
            kind = 'response.done' if ident.startswith('done_') else 'response.completed' if ident == 'contradiction' else 'response.'+status
            terminal = {'type':kind, 'response':{'id':'resp_c1','status':status,'output':[],
                        'usage':{'input_tokens':3,'output_tokens':2},'opaque':{'keep':42}}}
            terminal_wire = frame(terminal)
            if ident == 'eof_terminal': terminal_wire = terminal_wire[:-1]
            if ident == 'eof_terminal_no_newline': terminal_wire = terminal_wire[:-2]
            emit(terminal_wire)
            if ident == 'completed_hold':
                self.connection.settimeout(4)
                server.closed = self.connection.recv(1) == b''
        except (BrokenPipeError, ConnectionResetError): server.closed = True
        except Exception as error: server.failures.append(str(error))


def consume(port, events, result, stop):
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=7)
    try:
        connection.request('POST', '/v1/responses', json.dumps({'model':'gpt-6.2-sol','stream':True,'input':'synthetic C1'}), {'content-type':'application/json'})
        response = connection.getresponse()
        result['status'] = response.status
        result['content_type'] = response.getheader('content-type')
        raw = bytearray(); lines = []
        while True:
            line = response.readline()
            if not line: break
            raw.extend(line)
            if line.rstrip(b'\r\n'):
                lines.append(line)
            else:
                data = b'\n'.join(x.rstrip(b'\r\n')[5:].lstrip(b' ') for x in lines if x.startswith(b'data:'))
                if data:
                    try: events.put(json.loads(data))
                    except ValueError: events.put({'invalid':True})
                lines = []
                if stop: break
        result['wire'] = bytes(raw)
    except Exception as error: result['read_error'] = type(error).__name__
    finally: connection.close(); result['ended'] = True


def run_case(server, port, ident, expect):
    server.ident = ident; server.attempts = 0; server.wire = bytearray(); server.closed = False
    server.fragment = threading.Event(); server.release = threading.Event(); server.failures = []
    events = queue.Queue(); result = {}; observed = []
    thread = threading.Thread(target=consume, args=(port, events, result, ident == 'disconnect'), daemon=True)
    started = time.monotonic(); thread.start()
    early = None
    if ident in ('text', 'tool'):
        require(server.fragment.wait(3), 'upstream delta not sent')
        try: observed.append(events.get(timeout=.8)); early = True
        except queue.Empty: early = False
        require(not server.release.is_set(), 'barrier opened before observation')
        server.release.set()
    thread.join(6)
    if thread.is_alive():
        server.release.set(); thread.join(2)
    require(not thread.is_alive(), ident+': consumer deadline')
    if ident in ('disconnect', 'completed_hold'):
        deadline = time.monotonic()+1
        while not server.closed and time.monotonic() < deadline: time.sleep(.01)
    server.release.set()
    while not events.empty(): observed.append(events.get_nowait())
    require(not server.failures, ident+': fixture failed '+str(server.failures))
    if ident == 'precommit_retry':
        require(result.get('status') == 502 and server.attempts == 1, 'C1-R1: truncated length must fail with HTTP 502 after one attempt')
        require(json.loads(result['wire'])['error']['code'] == 'upstream_protocol_error', 'C1-R1: non-timeout body failure lost protocol error')
    elif expect == 'baseline':
        require(early is False, ident+': baseline unexpectedly incremental')
        require(result.get('status') == 200 and observed and observed[-1].get('type') == 'response.completed', 'baseline did not complete after release')
        require(observed[0].get('delta') == ('{"x":' if ident == 'tool' else 'C1 café') and server.attempts == 1, 'baseline delta/attempt mismatch')
    elif ident == 'exhaustion':
        require(result.get('status') != 200 and server.attempts == 2, 'retry exhaustion not structured failure')
    else:
        require(result.get('status') == 200 and result.get('content_type') == 'text/event-stream', ident+': missing SSE')
        require(observed and observed[0].get('opaque') == {'keep':True}, ident+': native fields lost')
        if ident == 'tool':
            require(observed[0].get('delta') == '{"x":' and observed[0].get('call_id') == 'call_c1', 'tool delta identity lost')
        else: require(observed[0].get('delta') == 'C1 café', ident+': decoded text lost')
        if early is not None: require(early, ident+': delta arrived after barrier')
        types = [event.get('type') for event in observed]
        if ident == 'disconnect': require(server.closed, 'upstream not closed after downstream drop')
        elif ident in ('eof','malformed','contradiction','event_mismatch','bad_done','limit','timeout','eof_missing_terminal','eof_malformed_terminal'):
            require(types[-1] == 'error' and 'response.completed' not in types and server.attempts == 1, ident+': no explicit unsuccessful end')
            if ident == 'timeout': require(4.5 < time.monotonic()-started < 6, 'configured 5s deadline not observed')
        else:
            outcome = 'error' if ident == 'error' else 'response.incomplete' if ident in ('incomplete','done_incomplete') else 'response.failed' if ident in ('failed','done_failed') else 'response.completed'
            require(types[-1] == outcome and types.count(outcome) == 1, ident+': wrong terminal outcome')
            if outcome != 'error': require(observed[-1]['response']['opaque']['keep'] == 42, ident+': terminal payload lost')
            if ident == 'completed_hold': require(server.closed and time.monotonic()-started < 2, 'terminal waited for EOF')
            if ident in ('eof_terminal','eof_terminal_no_newline'):
                require(result['wire'] == bytes(server.wire)+ (b'\n' if ident == 'eof_terminal' else b'\n\n'), 'C1-R2: EOF terminal bytes or appended boundary changed')
            elif not ident.startswith('done_'): require(result['wire'] == bytes(server.wire), ident+': native wire changed')
        expected_attempts = 2 if ident == 'retry' else 1
        require(server.attempts == expected_attempts, ident+': attempt count mismatch')
    return {'id':ident,'result':0,'early_delta':early,'attempts':server.attempts,
            'outcome':observed[-1].get('type') if observed else None,'upstream_closed':server.closed,
            'elapsed_ms':round((time.monotonic()-started)*1000)}


def worker(args, fixture=Fixture, validate=validate_cases, case_runner=run_case, cases_path=CASES):
    directory = Path(args.worker); server = gateway = None
    report = {'result':1,'ports':[],'outcomes':[]}
    try:
        with socket.socket() as sock:
            sock.settimeout(1)
            try: sock.connect(('192.0.2.1',9))
            except OSError as error: require(error.errno == errno.EPERM, 'external network not OS-denied')
            else: raise ValueError('external network allowed')
        try: Path(args.canary).read_bytes()
        except PermissionError: pass
        else: raise ValueError('credential canary readable')
        report['isolation'] = True
        server = http.server.ThreadingHTTPServer(('127.0.0.1',0),fixture)
        server.daemon_threads = True; server.release = threading.Event()
        threading.Thread(target=server.serve_forever,daemon=True).start()
        port = free_port(); report['ports'] = [port,server.server_port]
        auth = directory/'auth.json'; auth.write_text(json.dumps(getattr(args,'synthetic_auth',{'OPENAI_API_KEY':'c1-synthetic'})))
        environment = minimal_env(directory)
        environment.update(CODEX_GATEWAY_DISABLE_CLAUDE_KEYCHAIN='1',CODEX_GATEWAY_CODEX_TIMEOUT_SECS='5',CODEX_GATEWAY_CODEX_MAX_ATTEMPTS='2')
        with (directory/'gateway-output.log').open('w') as log:
            gateway = subprocess.Popen([args.gateway,'--port',str(port),'--auth-path',str(auth),
                '--upstream-base-url','http://127.0.0.1:'+str(server.server_port)+getattr(args,'upstream_suffix',''),'--log-path',str(directory/'gateway.log')],env=environment,stdout=log,stderr=log)
        deadline = time.monotonic()+8
        while True:
            require(gateway.poll() is None, 'gateway exited at startup')
            try: get('http://127.0.0.1:'+str(port)+'/health'); break
            except OSError:
                require(time.monotonic() < deadline, 'startup deadline'); time.sleep(.05)
        require(not args.fail_after_start, 'intentional failure after owned services start')
        cases = validate(json.loads(cases_path.read_text())['cases'])
        for case in cases:
            if args.expect == 'baseline' and case['id'] not in ('text','tool','precommit_retry'): continue
            report['outcomes'].append(case_runner(server,port,case['id'],args.expect))
        report['result'] = 0
    except Exception as error: report['error'] = str(error)
    finally:
        if gateway is not None:
            gateway.terminate()
            try: gateway.wait(timeout=3)
            except subprocess.TimeoutExpired: gateway.kill(); gateway.wait(timeout=3)
        if server is not None:
            server.release.set(); server.shutdown(); server.server_close()
        report['owned_process_cleanup'] = gateway is None or gateway.poll() is not None
        (directory/'result.json').write_text(json.dumps(report,indent=2)+'\n')
    return report['result']


def run(args, script=None, cases_path=CASES, worker_args=()):
    require(sys.platform == 'darwin', 'macOS OS sandbox required')
    gateway = Path(args.gateway).resolve(); require(gateway.is_file(), 'prebuilt gateway required')
    directory = Path(tempfile.mkdtemp(prefix='codex-responses-c1-')).resolve()
    print('local evidence: '+str(directory),flush=True)
    script = Path(script or __file__).resolve()
    manifest = {'gateway_sha256':hashlib.sha256(gateway.read_bytes()).hexdigest(),'expect':args.expect,
                'harness_sha256':hashlib.sha256(script.read_bytes()).hexdigest(),
                'lifecycle_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                'cases_sha256':hashlib.sha256(cases_path.read_bytes()).hexdigest()}
    (directory/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    canary = directory/'credential-canary'; canary.write_text('synthetic protected canary')
    policy = directory/'sandbox.sb'; policy.write_text(sandbox_policy(Path.home(),canary))
    sentinel = http.server.ThreadingHTTPServer(('127.0.0.1',0),Fixture)
    threading.Thread(target=sentinel.serve_forever,daemon=True).start()
    process = None
    try:
        command = ['/usr/bin/sandbox-exec','-f',str(policy),sys.executable,str(script),
                   '--worker',str(directory),'--gateway',str(gateway),'--expect',args.expect,'--canary',str(canary)]
        command.extend(worker_args)
        if args.verify_failure_cleanup: command.append('--fail-after-start')
        process = subprocess.Popen(command,env=minimal_env(directory),stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,start_new_session=True)
        try: _,stderr = process.communicate(timeout=100)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid,signal.SIGKILL); process.communicate(timeout=3)
            raise ValueError('worker deadline; owned process group killed')
        path = directory/'result.json'
        require(path.exists(), 'worker report missing: '+stderr[-1000:])
        report = json.loads(path.read_text())
        report['unrelated_listener_alive'] = get('http://127.0.0.1:'+str(sentinel.server_port)+'/identity') == b'c1-owned-fixture'
        report['owned_ports_closed'] = True
        for port in report['ports']:
            with socket.socket() as sock:
                sock.settimeout(.5)
                if sock.connect_ex(('127.0.0.1',port)) == 0: report['owned_ports_closed'] = False
        path.write_text(json.dumps(report,indent=2)+'\n')
        require(all(report[k] for k in ('owned_process_cleanup','owned_ports_closed','unrelated_listener_alive')), 'cleanup failed')
        require(process.returncode == report['result'], 'worker/report mismatch')
        if args.verify_failure_cleanup:
            require(report['result'] == 1 and report.get('error') == 'intentional failure after owned services start', 'wrong deliberate failure')
            outcome = 0
        else: outcome = report['result']
        print(json.dumps({'result':outcome,'evidence':str(path),'error':report.get('error')}))
        return outcome
    finally:
        if process is not None and process.poll() is None:
            os.killpg(process.pid,signal.SIGKILL); process.communicate(timeout=3)
        sentinel.shutdown(); sentinel.server_close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gateway',required=True)
    parser.add_argument('--expect',choices=('baseline','c1'),required=True)
    parser.add_argument('--verify-failure-cleanup',action='store_true')
    parser.add_argument('--worker',help=argparse.SUPPRESS)
    parser.add_argument('--canary',help=argparse.SUPPRESS)
    parser.add_argument('--fail-after-start',action='store_true',help=argparse.SUPPRESS)
    args = parser.parse_args()
    try: return worker(args) if args.worker else run(args)
    except Exception as error:
        print(json.dumps({'result':1,'error':str(error)}),file=sys.stderr); return 1


if __name__ == '__main__': sys.exit(main())
