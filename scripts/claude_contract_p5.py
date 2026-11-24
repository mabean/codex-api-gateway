#!/usr/bin/env python3
"""Mandatory positive offline incremental Codex streaming contract; no model calls."""
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
PROFILE = FIXTURES/'2.1.329-macos-arm64-p5.json'
CASES = FIXTURES/'p5_cases.json'
CASE_IDS = ['text', 'tool', 'tool_error', 'start_error', 'limit', 'eof', 'error', 'failed', 'malformed', 'contradiction', 'duplicate_terminal', 'timeout', 'abort', 'disconnect', 'retry', 'direct_extended', 'raw_responses', 'chat_json', 'chat_stream', 'responses_json', 'unfinished_tool', 'finalized_tool_whitespace', 'finalized_tool_second_json', 'parallel_tools', 'terminal_tool_snapshot', 'partial_tool_limit']


def validate_cases(cases):
    require([c['id'] for c in cases] == CASE_IDS, 'missing, reordered or duplicate mandatory P5 case')
    for c in cases:
        require(isinstance(c.get('expected'),str) and bool(c['expected']), 'missing positive expectation')
    return cases


def verify_inventory(binary, directory):
    manifest = verify_p2(binary, directory)
    _, modules = read_graph(binary)
    profile = json.loads(PROFILE.read_text())
    require(profile['binary_sha256'] == manifest['binary_sha256'], 'P5 binary fingerprint mismatch')
    chunks = {}
    for part in profile['ranges']:
        module = modules[part['module_id']]
        source = module['source']
        require(module['name'] == part['module'] and digest(source) == part['module_sha256'], 'P5 module mismatch')
        require(module['offset']+part['start'] == part['binary_start'], 'P5 absolute byte offset mismatch')
        chunk = source[part['start']:part['end']]
        require(digest(chunk) == part['sha256'], 'P5 raw byte span fingerprint mismatch')
        chunks[part['name']] = chunk.decode()
    (directory/'p5-ranges.json').write_text(json.dumps(chunks))
    manifest['p5'] = profile
    return manifest


class Fixture(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_): pass
    def respond(self, body, status=200, content_type='application/json'):
        raw = body if isinstance(body,bytes) else json.dumps(body).encode()
        self.send_response(status); self.send_header('content-type',content_type); self.end_headers(); self.wfile.write(raw)
    def do_GET(self):
        if self.path == '/redirect':
            self.send_response(302); self.send_header('location','http://192.0.2.1:9/forbidden'); self.end_headers()
        else: self.respond(b'p5-owned-fixture')
    def do_POST(self):
        server=self.server
        try:
            body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            if self.path=='/control':
                server.current=body;server.receipts=[];server.failures=[];server.release.clear();server.fragment=False;server.cancelled=False;server.events=[];server.wire=""
                return self.respond({'ready':True})
            if self.path=='/state': return self.respond({'fragment_sent':server.fragment,'closed':not server.release.is_set(),'cancelled':server.cancelled,'events':server.events,'wire':server.wire})
            if self.path=='/release': server.release.set();return self.respond({'released':True})
            if self.path=='/finish': return self.respond({'receipts':server.receipts,'failures':server.failures,'cancelled':server.cancelled,'events':server.events,'wire':server.wire})
            server.receipts.append({'path':self.path,'body':body})
            ident=server.current['id']
            require(self.path.split('?')[0] in ('/codex/responses','/v1/messages'),'unexpected upstream path')
            if ident=='retry' and len(server.receipts)==1: return self.respond({'error':'temporary'},503)
            self.send_response(200);self.send_header('content-type','text/event-stream');self.end_headers()
            def emit(events):
                raw=b''.join(map(sse_event,events))
                server.events.extend(events);server.wire+=raw.decode()
                # Segment inside UTF-8 and JSON as well as between frames.
                for i in range(0,len(raw),7): self.wfile.write(raw[i:i+7]);self.wfile.flush()
            if ident=='direct_extended':
                emit(direct_events());return
            if ident in ('unfinished_tool','finalized_tool_whitespace','finalized_tool_second_json','parallel_tools','terminal_tool_snapshot','partial_tool_limit'):
                item={'type':'function_call','id':'fc_round1','call_id':'toolu_round1','name':'Read','arguments':''}
                final={**item,'arguments':'{"file_path":"/fixture"}'}
                emit([{'type':'response.output_item.added','output_index':0,'item':item}])
                if ident=='parallel_tools':
                    other={**item,'id':'fc_round1_b','call_id':'toolu_round1_b'}
                    other_final={**other,'arguments':'{"file_path":"/second"}'}
                    emit([{'type':'response.output_item.added','output_index':1,'item':other},
                          {'type':'response.function_call_arguments.delta','item_id':item['id'],'delta':'{"file_path":'},
                          {'type':'response.function_call_arguments.delta','item_id':other['id'],'delta':'{"file_path":'},
                          {'type':'response.function_call_arguments.done','item_id':item['id'],'arguments':final['arguments']},
                          {'type':'response.function_call_arguments.done','item_id':other['id'],'arguments':other_final['arguments']},
                          {'type':'response.completed','response':{'status':'completed','output':[final,other_final],'usage':{'input_tokens':17,'output_tokens':11}}}])
                    return
                if ident=='partial_tool_limit':
                    emit([{'type':'response.function_call_arguments.delta','item_id':item['id'],'delta':'{"file_path":'}])
                elif ident!='unfinished_tool':
                    emit([{'type':'response.function_call_arguments.delta','item_id':item['id'],'delta':'{"file_path":'}])
                    if ident!='terminal_tool_snapshot':
                        emit([{'type':'response.function_call_arguments.done','item_id':item['id'],'arguments':final['arguments']}])
                output=[]
                if ident in ('finalized_tool_whitespace','finalized_tool_second_json'):
                    suffix=' ' if ident=='finalized_tool_whitespace' else '{}'
                    emit([{'type':'response.output_item.done','item':{**final,'arguments':final['arguments']+suffix}}])
                if ident=='terminal_tool_snapshot': output=[final]
                limited=ident=='partial_tool_limit'
                emit([{'type':'response.incomplete' if limited else 'response.completed','response':{
                    'status':'incomplete' if limited else 'completed','output':output,
                    'incomplete_details':{'reason':'max_output_tokens'},'usage':{'input_tokens':17,'output_tokens':11}}}])
                return
            tool=ident in ('tool','tool_error')
            text='P5 café'
            item={'type':'function_call','id':'fc_p5','call_id':'toolu_p5','name':'Read','arguments':''}
            first=[{'type':'response.output_item.added','output_index':0,'item':item},
                   {'type':'response.function_call_arguments.delta','item_id':'fc_p5','delta':'{"file_path":'}] if tool else [{'type':'response.output_text.delta','output_index':0,'content_index':0,'delta':text}]
            if ident=='start_error': first=[{'type':'response.output_item.added','item':{'type':'message','id':'msg_p5','content':[]}}]
            if ident=='raw_responses':
                first += [{'type':'response.compaction.compacting','item_id':'compact_p5','output_index':2,'sequence_number':0},
                          {'type':'response.reasoning_summary_text.delta','delta':'summary'},
                          {'type':'response.output_text.annotation.added','annotation':{'type':'url_citation','url':'https://example.invalid','title':'fixture','start_index':0,'end_index':2}},
                          {'type':'response.output_item.added','item':{'type':'compaction','encrypted_content':'synthetic-opaque'}}]
            emit(first);server.fragment=True
            if ident in ('abort','disconnect'):
                self.connection.settimeout(3)
                try:
                    server.cancelled=self.connection.recv(1)==b''
                except (ConnectionResetError,BrokenPipeError): server.cancelled=True
                return
            if ident=='timeout': server.release.wait(6);return
            if ident in ('text','tool'):
                require(server.release.wait(5),'terminal barrier deadline')
            if ident=='eof': return
            if ident in ('error','start_error','tool_error'): emit([{'type':'error','message':'P5 deadline exceeded after committed frame'}]);return
            if ident=='failed': emit([{'type':'response.failed','response':{'error':{'message':'P5 failed'}}}]);return
            if ident=='malformed': self.wfile.write(b'data: {broken}\n\n');return
            if ident=='contradiction': emit([{'type':'response.output_text.done','output_index':0,'text':'different'}]);return
            if tool:
                emit([{'type':'response.output_item.done','output_index':0,'item':{**item,'arguments':'{"file_path":"/fixture"}'}}])
            terminal={'type':'response.incomplete' if ident=='limit' else 'response.completed','response':{'status':'incomplete' if ident=='limit' else 'completed','usage':{'input_tokens':17,'output_tokens':11},'incomplete_details':{'reason':'max_output_tokens'}}}
            emit([terminal])
            if ident=='duplicate_terminal': emit([terminal])
        except (BrokenPipeError,ConnectionResetError): server.cancelled=True
        except Exception as error:
            server.failures.append(str(error))


def direct_events():
    return [
      {'type':'message_start','message':{'id':'msg_direct','type':'message','role':'assistant','content':[],'model':'claude-haiku-5-5','stop_reason':None,'stop_sequence':None,'usage':{'input_tokens':17,'output_tokens':0}}},
      {'type':'content_block_start','index':0,'content_block':{'type':'thinking','thinking':'','signature':''}},
      {'type':'content_block_delta','index':0,'delta':{'type':'thinking_delta','thinking':'provider thinking'}},
      {'type':'content_block_delta','index':0,'delta':{'type':'signature_delta','signature':'provider-signature'}},
      {'type':'content_block_stop','index':0},
      {'type':'content_block_start','index':1,'content_block':{'type':'text','text':'','citations':[]}},
      {'type':'content_block_delta','index':1,'delta':{'type':'text_delta','text':'direct text'}},
      {'type':'content_block_delta','index':1,'delta':{'type':'citations_delta','citation':{'type':'char_location','cited_text':'quote','document_index':0,'document_title':'fixture','start_char_index':0,'end_char_index':5}}},
      {'type':'content_block_stop','index':1},
      {'type':'content_block_start','index':2,'content_block':{'type':'compaction','content':'provider compacted content'}},
      {'type':'content_block_stop','index':2},
      {'type':'message_delta','delta':{'stop_reason':'end_turn','stop_sequence':None},'usage':{'output_tokens':11,'input_tokens':17}},
      {'type':'message_stop'}]


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
        server.fragment = False
        server.cancelled = False
        threading.Thread(target=server.serve_forever,daemon=True).start()
        port = free_port()
        report['ports'] = [port,server.server_port]
        upstream = 'http://127.0.0.1:'+str(server.server_port)
        base = 'http://127.0.0.1:'+str(port)
        auth = directory/'auth.json'
        auth.write_text('{"tokens":{"access_token":"p5-synthetic-codex","account_id":"synthetic-account"}}')
        command = [args.gateway,'--port',str(port),'--auth-path',str(auth),'--upstream-base-url',upstream,
                   '--anthropic-upstream-base-url',upstream,'--log-path',str(directory/'gateway.log')]
        environment = minimal_env(directory)
        environment['CODEX_GATEWAY_DISABLE_CLAUDE_KEYCHAIN'] = '1'
        environment['CODEX_GATEWAY_CODEX_TIMEOUT_SECS'] = '5'
        environment['CODEX_GATEWAY_CODEX_MAX_ATTEMPTS'] = '2'
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
        result = subprocess.run([args.node,'--experimental-vm-modules',str(ROOT/'scripts/claude_contract_p5.mjs'),str(directory),base,upstream,str(CASES)],
                                env=minimal_env(directory),capture_output=True,text=True,timeout=100)
        (directory/'node-stderr.log').write_text(result.stderr)
        require(result.stdout.strip(), 'extracted P5 execution unavailable: '+result.stderr[-1800:])
        report['sdk'] = json.loads(result.stdout)
        outcomes = report['sdk']['outcomes']
        require([x['id'] for x in outcomes] == CASE_IDS, 'mandatory case missing')
        require(result.returncode == 0 and all(x['result']==0 for x in outcomes), 'mandatory positive P5 case failed')
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
    directory = Path(tempfile.mkdtemp(prefix='claude-contract-p5-')).resolve()
    print('local evidence: '+str(directory),flush=True)
    manifest = verify_inventory(args.binary,directory)
    manifest['gateway_sha256'] = digest(gateway.read_bytes())
    manifest['harness_sha256'] = {str(path.relative_to(ROOT)):digest(path.read_bytes()) for path in
        (Path(__file__).resolve(),ROOT/'scripts/claude_contract_p5.mjs',CASES,PROFILE)}
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
            stdout,stderr = process.communicate(timeout=125)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid,signal.SIGKILL)
            process.communicate(timeout=3)
            raise ValueError('isolated P5 deadline; owned process group killed')
        path = directory/'result.json'
        require(path.exists(), 'isolated worker produced no report: '+stderr[-1000:])
        report = json.loads(path.read_text())
        report['worker_returncode'] = process.returncode
        report['unrelated_listener_alive'] = get('http://127.0.0.1:'+str(sentinel.server_port)+'/identity') == b'p5-owned-fixture'
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
            print(report.get('error','mandatory P5 failure'),file=sys.stderr)
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
