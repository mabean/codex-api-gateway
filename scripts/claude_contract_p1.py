#!/usr/bin/env python3
"""Offline, fail-closed P1 SDK contract harness (macOS sandbox-exec + Node)."""
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
import urllib.request

from claude_contract_extract import extract, digest

ROOT = Path(__file__).resolve().parent.parent
FIXTURE = ROOT/'scripts/fixtures/claude_contract/p1_messages.json'
POLICY = '''(version 1)
(allow default)
(deny network*)
(allow network-bind (local ip "localhost:*"))
(allow network-inbound (local ip "localhost:*"))
(allow network-outbound (remote ip "localhost:*"))
(deny process-exec (literal "/usr/bin/security"))
(deny mach-lookup (global-name "com.apple.securityd"))
'''


def sandbox_policy(home, canary):
    paths = [home/'.codex/auth.json', home/'.claude', home/'.claude.json',
             home/'.aws', home/'.config', home/'Library/Keychains', canary]
    return POLICY + ''.join('(deny file-read* (subpath '+json.dumps(str(p.resolve()))+'))\n' for p in paths)


def minimal_env(directory):
    return {'PATH':'/usr/bin:/bin:/opt/homebrew/bin', 'HOME':str(directory),
            'TMPDIR':str(directory), 'XDG_CONFIG_HOME':str(directory/'config'),
            'RUST_LOG':'error', 'LANG':'en_US.UTF-8'}


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def get(url):
    return urllib.request.build_opener(urllib.request.ProxyHandler({})).open(url, timeout=1).read()


def events(text, invalid=False):
    delta = {'type':'content_block_delta','index':0,'delta':{'type':'text_delta','text':text}}
    sequence = [delta] if invalid else [
        {'type':'message_start','message':{'id':'msg_p1','type':'message','role':'assistant',
         'model':'claude-haiku-5-5','content':[],'stop_reason':None,'stop_sequence':None,
         'usage':{'input_tokens':3,'output_tokens':0}}},
        {'type':'content_block_start','index':0,'content_block':{'type':'text','text':''}}, delta,
        {'type':'content_block_stop','index':0},
        {'type':'message_delta','delta':{'stop_reason':'end_turn','stop_sequence':None},'usage':{'output_tokens':4}},
        {'type':'message_stop'}]
    return ''.join('event: '+e['type']+'\ndata: '+json.dumps(e)+'\n\n' for e in sequence).encode()


class Fixture(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        if self.path == '/identity':
            self.send_response(200)
            self.end_headers()
            self.wfile.write(self.server.identity.encode())
        elif self.path == '/redirect':
            self.send_response(302)
            self.send_header('Location', 'http://192.0.2.1:9/forbidden')
            self.end_headers()
        else:
            self.send_error(404)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.server.receipts.append({'path':self.path,'body':body,
                                     'key_is_synthetic':self.headers.get('x-api-key') == 'p1-synthetic-upstream'})
        valid = (self.path.split('?')[0] == '/v1/messages' and
                 body.get('model') == 'claude-haiku-5-5' and body.get('stream') is True and
                 body.get('max_tokens') == 32 and self.headers.get('x-api-key') == 'p1-synthetic-upstream')
        if not valid:
            self.send_error(400, 'unexpected P1 request')
            return
        invalid = body['messages'][0]['content'] == 'p1-invalid-order'
        self.send_response(200)
        self.send_header('Content-Type','text/event-stream')
        self.end_headers()
        self.wfile.write(events('P1 terminal text', invalid))


def worker(args):
    directory = Path(args.worker)
    report = {'checks':{},'ports':[]}
    gateway = None
    server = None
    try:
        with socket.socket() as sock:
            sock.settimeout(1)
            try:
                sock.connect(('192.0.2.1',9))
            except OSError as error:
                if error.errno != errno.EPERM:
                    raise RuntimeError('isolation probe was not rejected by OS policy: '+repr(error)) from error
                report['checks']['external_network'] = {'errno':error.errno,'result':0}
            else:
                raise RuntimeError('external network unexpectedly allowed')
        try:
            Path(args.canary).read_bytes()
        except PermissionError:
            report['checks']['credential_file_canary'] = 0
        else:
            raise RuntimeError('credential file canary readable')
        server = http.server.ThreadingHTTPServer(('127.0.0.1',0),Fixture)
        server.identity = 'p1-owned-fixture'
        server.receipts = []
        threading.Thread(target=server.serve_forever,daemon=True).start()
        upstream = 'http://127.0.0.1:'+str(server.server_port)
        report['ports'].append(server.server_port)
        if get(upstream+'/identity') != b'p1-owned-fixture':
            raise RuntimeError('fixture identity mismatch')
        report['checks']['loopback_identity'] = 0
        node = args.node
        probe = subprocess.run([node,'--input-type=module','-e',
            'try {await fetch(process.argv[1],{redirect:"error",signal:AbortSignal.timeout(2000)});process.exit(2)} catch(e) {if(!/unexpected redirect|redirect count exceeded/.test(String(e.cause))) throw e;console.log("redirect rejected")}',
            upstream+'/redirect'], capture_output=True,text=True,timeout=5,env=minimal_env(directory))
        if probe.returncode:
            raise RuntimeError('redirect policy probe failed: '+probe.stderr[-1000:])
        report['checks']['external_redirect'] = 0
        port = free_port()
        report['ports'].append(port)
        auth = directory/'auth.json'
        auth.write_text('{"tokens":{"access_token":"p1-synthetic-codex","account_id":"synthetic-account"}}')
        command = [args.gateway,'--port',str(port),'--auth-path',str(auth),
                   '--upstream-base-url',upstream,'--anthropic-upstream-base-url',upstream,
                   '--anthropic-auth-token','p1-synthetic-upstream','--log-path',str(directory/'gateway.log')]
        with (directory/'gateway-output.log').open('w') as log:
            gateway = subprocess.Popen(command,env=minimal_env(directory),stdout=log,stderr=log)
        deadline = time.monotonic()+8
        while True:
            if gateway.poll() is not None:
                raise RuntimeError('owned gateway exited during startup')
            try:
                get('http://127.0.0.1:'+str(port)+'/health')
                break
            except OSError:
                if time.monotonic() > deadline:
                    raise RuntimeError('gateway startup deadline')
                time.sleep(.05)
        if args.fail_after_start:
            raise RuntimeError('intentional owned-cleanup failure')
        result = subprocess.run([node,str(ROOT/'scripts/claude_contract_sdk.mjs'),
            str(directory/'sdk-source.js'),'http://127.0.0.1:'+str(port),str(FIXTURE)],
            capture_output=True,text=True,timeout=18,env=minimal_env(directory))
        if result.returncode:
            raise RuntimeError('extracted SDK failed: '+result.stderr[-1800:])
        report['sdk'] = json.loads(result.stdout)
        if len(server.receipts) != 2 or not all(r['key_is_synthetic'] for r in server.receipts):
            raise RuntimeError('fixture request count/auth mismatch')
        report['fixture_receipts'] = server.receipts
        report['expected'] = json.loads(FIXTURE.read_text())['scenarios']
        report['gateway_path'] = 'POST /v1/messages -> Anthropic local fixture'
        report['checks']['sdk_gateway_contract'] = 0
        report['result'] = 0
    except Exception as error:
        report.update(result=1, error=str(error))
    finally:
        if gateway is not None:
            gateway.terminate()
            try:
                gateway.wait(timeout=3)
            except subprocess.TimeoutExpired:
                gateway.kill()
                gateway.wait(timeout=3)
        if server is not None:
            server.shutdown()
            server.server_close()
        report['owned_process_cleanup'] = gateway is None or gateway.poll() is not None
        (directory/'result.json').write_text(json.dumps(report,indent=2)+'\n')
    return report['result']


def run(args):
    if sys.platform != 'darwin' or not Path('/usr/bin/sandbox-exec').exists():
        raise RuntimeError('P1 requires macOS sandbox-exec; no unconfined fallback')
    node = shutil.which('node')
    gateway = Path(args.gateway).resolve()
    if not node or not gateway.is_file():
        raise RuntimeError('prebuild gateway release and install Node before offline P1 run')
    directory = Path(tempfile.mkdtemp(prefix='claude-contract-p1-')).resolve()
    print('local evidence: '+str(directory), flush=True)
    manifest = extract(args.binary,directory)
    manifest['gateway_sha256'] = digest(gateway.read_bytes())
    (directory/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    canary = directory/'credential-canary'
    canary.write_text('synthetic protected credential canary')
    policy = directory/'sandbox.sb'
    policy.write_text(sandbox_policy(Path.home(),canary))
    sentinel = http.server.ThreadingHTTPServer(('127.0.0.1',0),Fixture)
    sentinel.identity = 'unrelated-listener'
    threading.Thread(target=sentinel.serve_forever,daemon=True).start()
    command = ['/usr/bin/sandbox-exec','-f',str(policy),sys.executable,str(Path(__file__).resolve()),
               '--worker',str(directory),'--node',node,'--gateway',str(gateway),'--canary',str(canary)]
    if args.fail_after_start:
        command.append('--fail-after-start')
    process = None
    try:
        process = subprocess.Popen(command,env=minimal_env(directory),stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE,text=True,start_new_session=True)
        try:
            stdout, stderr = process.communicate(timeout=35)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid,signal.SIGKILL)
            process.communicate(timeout=3)
            raise RuntimeError('isolated worker deadline; owned process group killed')
        result = subprocess.CompletedProcess(command,process.returncode,stdout,stderr)
        path = directory/'result.json'
        if not path.exists():
            raise RuntimeError('isolated worker produced no result: '+result.stderr[-1500:])
        report = json.loads(path.read_text())
        report['worker_returncode'] = result.returncode
        report['unrelated_listener_alive'] = get('http://127.0.0.1:'+str(sentinel.server_port)+'/identity') == b'unrelated-listener'
        report['owned_ports_closed'] = True
        for port in report['ports']:
            with socket.socket() as sock:
                sock.settimeout(.5)
                if sock.connect_ex(('127.0.0.1',port)) == 0:
                    report['owned_ports_closed'] = False
        passed = (result.returncode == 0 and report['result'] == 0 and report['owned_process_cleanup']
                  and report['owned_ports_closed'] and report['unrelated_listener_alive'])
        report['result'] = 0 if passed else 1
        path.write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps(report,indent=2))
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
    parser.add_argument('--fail-after-start',action='store_true',help='exercise owned cleanup after startup')
    args = parser.parse_args()
    try:
        if args.worker:
            return worker(args)
        if not args.binary:
            parser.error('--binary is required')
        return run(args)
    except Exception as error:
        print('P1 BLOCKED: '+str(error),file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
