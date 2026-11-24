#!/usr/bin/env python3
"""Offline C2 terminal JSON and safe precommit HTTP error release contract."""
import argparse
import http.client
import json
from pathlib import Path
import sys
import time

import codex_responses_c1 as c1
from claude_contract_p2 import require

CASES = c1.ROOT/'scripts/fixtures/codex_responses/c2_cases.json'
SECRET = 'c2-SYNTHETIC-SECRET-never-disclose'
JSON_IDS = ['native', 'tool', 'incomplete', 'native_text', 'authoritative', 'empty_text', 'sparse',
            'no_usage', 'done', 'eof_terminal', 'hold', 'failed', 'error', 'missing',
            'contradiction', 'mismatch', 'malformed', 'limit']
ERROR_IDS = ['http400', 'http401', 'http403', 'http404', 'http429', 'http503',
             'retry_date', 'retry_obsolete', 'retry_asctime', 'retry_invalid',
             'retry_duplicate', 'retry_nominated', 'retry_negative',
             'retry_combined', 'transport', 'timeout_send', 'timeout_body', 'truncated', 'local']
REPAIR_IDS = ['r1_message', 'r1_function', 'r1_text_done', 'r1_delta_done', 'r1_order',
              'r1_terminal_output', 'r1_terminal_empty', 'r1_terminal_text',
              'r2_completed_status', 'r2_incomplete_status']
CASE_IDS = JSON_IDS + [name+'_'+mode for name in ERROR_IDS for mode in ('json', 'stream')] + REPAIR_IDS


def validate_cases(cases):
    require([c['id'] for c in cases] == CASE_IDS, 'mandatory C2 cases missing, duplicated or reordered')
    require(all(c.get('criterion') and c.get('expected_c1') and c.get('expected_c2') for c in cases), 'missing C2 expectation')
    return cases


def terminal_response(ident):
    response = {'id':'resp_native_c2', 'object':'response', 'model':'native-model-c2',
                'status':'incomplete' if ident == 'incomplete' else 'completed',
                'created_at':1234, 'opaque':{'native':42},
                'output':[{'id':'reasoning_c2','type':'reasoning','summary':[],'opaque':7},
                          {'id':'msg_c2','type':'message','role':'assistant','status':'completed',
                           'content':[{'type':'output_text','text':'final café','annotations':[{'type':'opaque','value':8}]},
                                      {'type':'refusal','refusal':'retained'},
                                      {'type':'output_text','text':' plus'}]},
                          {'id':'fc_c2','type':'function_call','call_id':'call_c2','name':'Read',
                           'arguments':'{"path":"final"}','status':'completed','opaque':9}],
                'usage':{'input_tokens':13,'output_tokens':7,'total_tokens':20,
                         'input_tokens_details':{'cached_tokens':5},
                         'output_tokens_details':{'reasoning_tokens':4},'opaque':11}}
    if ident == 'incomplete': response['incomplete_details'] = {'reason':'max_output_tokens','opaque':17}
    if ident == 'tool': response['output'] = [response['output'][2]]
    if ident == 'native_text': response['output_text'] = 'upstream text is authoritative'
    if ident == 'empty_text': response['output'][1]['content'] = [{'type':'output_text','text':''}]
    if ident == 'sparse': response = {'id':'resp_sparse','status':'completed','output':[]}
    if ident == 'no_usage': del response['usage']
    return response


def repair_case(ident, initial=False):
    message = {'id':'msg_final','type':'message','role':'assistant','status':'completed',
               'content':[{'type':'output_text','text':'final snapshot','annotations':[{'opaque':42}]}]}
    function = {'id':'fc_final','type':'function_call','call_id':'call_final','name':'Read',
                'arguments':'{"path":"fixture"}','status':'completed','opaque':7}
    response = {'id':'resp_final','model':'native-model','status':'completed'}
    def item(value, index=0): return {'type':'response.output_item.done','output_index':index,'item':value}
    events = []; expected_text = ''; expected_output = None
    if ident == 'r1_function': events = [item(function)]; expected_output = [function]
    elif ident == 'r1_text_done':
        events = [{'type':'response.output_text.done','output_index':0,'content_index':0,'text':'final snapshot'}]
        expected_text = 'final snapshot'
    elif ident == 'r1_order':
        first = dict(message,id='msg_first',content=[{'type':'output_text','text':'first'}])
        second = dict(message,id='msg_second',content=[{'type':'output_text','text':' second'}])
        events = [item(function,2),item(second,1),item(first,0),item(first,0)]
        events.append({'type':'response.output_text.done','output_index':3,'text':' tail'})
        expected_output = [first,second,function]; expected_text = 'first second tail'
    elif ident.startswith('r1_'):
        events = [item(message)]; expected_output = [message]; expected_text = 'final snapshot'
        if ident == 'r1_delta_done':
            events = [{'type':'response.output_text.delta','item_id':'msg_final','delta':'final '},
                      {'type':'response.output_text.done','item_id':'msg_final','output_index':0,
                       'content_index':0,'text':'final snapshot'}] + events
        if ident == 'r1_terminal_output':
            terminal_item = dict(message,id='msg_terminal',content=[{'type':'output_text','text':'terminal'}])
            response['output'] = [terminal_item]; expected_output = [terminal_item]; expected_text = 'terminal'
        if ident == 'r1_terminal_empty': response['output'] = []; expected_output = []
        if ident == 'r1_terminal_text': response['output_text'] = ''; expected_text = ''
    else:
        del response['status']
        if ident == 'r2_incomplete_status': response['incomplete_details'] = {'reason':'max_output_tokens','opaque':3}
    kind = 'response.incomplete' if ident == 'r2_incomplete_status' else 'response.completed'
    events.append({'type':kind,'response':response})
    expected = dict(response,object='response')
    if initial:
        expected.setdefault('output_text','terminal' if ident == 'r1_terminal_output' else 'final ' if ident == 'r1_delta_done' else '')
    else:
        expected.setdefault('status',kind.partition('.')[2])
        if expected_output is not None: expected.setdefault('output',expected_output)
        expected.setdefault('output_text',expected_text)
    return events, expected


class Fixture(c1.Fixture):
    def do_POST(self):
        server = self.server
        try:
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            require(self.path == '/codex/responses' and body['stream'], 'unexpected upstream request')
            server.attempts += 1
            ident = server.ident
            if ident in REPAIR_IDS:
                events, _ = repair_case(ident)
                self.send_response(200); self.send_header('Content-Type','text/event-stream'); self.end_headers()
                for event in events:
                    for byte in c1.frame(event): self.wfile.write(bytes([byte])); self.wfile.flush()
                return
            name = ident.rsplit('_', 1)[0] if ident not in JSON_IDS else ident
            if name == 'transport': self.close_connection = True; return
            if name == 'timeout_send': server.release.wait(12); return
            if name.startswith('http') or name.startswith('retry_'):
                status = int(name[4:]) if name.startswith('http') else 429
                self.send_response(status)
                self.send_header('X-C2-Secret', SECRET); self.send_header('Set-Cookie', SECRET)
                self.send_header('request-id', SECRET)
                hint = {'retry_date':'Wed, 21 Oct 2015 07:28:00 GMT',
                        'retry_obsolete':'Wednesday, 21-Oct-15 07:28:00 GMT',
                        'retry_asctime':'Wed Oct 21 07:28:00 2015',
                        'retry_invalid':SECRET,'retry_negative':'-3',
                        'retry_combined':'2, 3'}.get(name, '7')
                self.send_header('Retry-After', hint)
                if name == 'retry_duplicate': self.send_header('Retry-After','8')
                if name == 'retry_nominated':
                    self.send_header('Connection','keep-alive'); self.send_header('Connection','RETRY-AFTER')
                self.end_headers(); self.wfile.write(json.dumps({'error':{'message':SECRET}}).encode()); return
            self.send_response(200)
            self.send_header('Content-Type','text/event-stream')
            if name == 'truncated': self.send_header('Content-Length','100')
            self.end_headers()
            if name == 'truncated': return
            if name == 'timeout_body': server.release.wait(12); return
            def emit(raw):
                for offset in range(0,len(raw),3):
                    self.wfile.write(raw[offset:offset+3]); self.wfile.flush()
            emit(c1.frame({'type':'response.output_text.delta','item_id':'msg_c2','output_index':1,
                           'content_index':0,'delta':'' if ident == 'empty_text' else 'stale' if ident == 'authoritative' else 'final café'}))
            if ident != 'empty_text':
                emit(c1.frame({'type':'response.output_text.delta','item_id':'msg_c2','output_index':1,
                               'content_index':2,'delta':' plus'}))
            if ident == 'missing': return
            if ident == 'malformed': emit(b'data: {broken}\n\n'); return
            if ident == 'limit': self.wfile.write(b':'+b'x'*(1024*1024)+b'\n\n'); return
            if ident == 'error': emit(c1.frame({'type':'error','message':SECRET})); return
            response = terminal_response(ident)
            if ident == 'failed': response['status'] = 'failed'; response['error'] = {'message':SECRET}
            kind = 'response.done' if ident == 'done' else 'response.'+response['status']
            if ident == 'contradiction': kind = 'response.incomplete'
            raw = c1.frame({'type':kind,'response':response})
            if ident == 'mismatch': raw = b'event: response.failed\n'+raw
            if ident == 'eof_terminal': raw = raw.rstrip(b'\n')
            emit(raw)
            if ident == 'hold':
                self.connection.settimeout(.8)
                try: server.closed = self.connection.recv(1) == b''
                except TimeoutError: pass
        except (BrokenPipeError, ConnectionResetError): pass
        except Exception as error: server.failures.append(str(error))


def run_case(server, port, ident, expect):
    initial = expect == 'initial-c2'
    if initial: expect = 'c2'
    server.ident = ident; server.attempts = 0; server.failures = []; server.closed = False
    server.release.clear()
    mode = ident.endswith('_stream')
    request = {'model':'gpt-6.2-sol','input':'synthetic C2','stream':mode}
    if ident.startswith('local_'): request['input'] = []
    started = time.monotonic()
    connection = http.client.HTTPConnection('127.0.0.1',port,timeout=14)
    try:
        connection.request('POST','/v1/responses',json.dumps(request),{'content-type':'application/json'})
        response = connection.getresponse(); wire = response.read()
        status = response.status; headers = response.getheaders()
    finally: connection.close(); server.release.set()
    require(not server.failures, ident+': fixture failure '+str(server.failures))
    baseline_disclosure = expect == 'c1' and ident in ('failed','error')
    require((SECRET.encode() in wire) == baseline_disclosure, ident+': unexpected secret disclosure behavior')
    require(all(SECRET not in v for _,v in headers), ident+': secret header disclosed')
    value = json.loads(wire)
    if ident in REPAIR_IDS:
        _, expected = repair_case(ident,initial=initial)
        require(status == 200 and value == expected, ident+': sparse terminal projection mismatch '+str(value))
        require(server.attempts == 1, ident+': unexpected replay')
        return {'id':ident,'result':0,'status':status,'attempts':server.attempts,
                'output_text':value.get('output_text'),'output_ids':[item.get('id') for item in value.get('output',[])],
                'response_status':value.get('status'),'elapsed_ms':round((time.monotonic()-started)*1000)}
    if ident in JSON_IDS:
        invalid = ident in ('failed','error','missing','contradiction','mismatch','malformed','limit') or (ident in ('authoritative','done') and expect == 'c1')
        if invalid:
            # C1 silently ignored mismatched SSE event names; C2 validates agreement.
            expected_status = 200 if expect == 'c1' and ident == 'mismatch' else 502
            require(status == expected_status, ident+': wrong terminal failure status '+str(status))
            if expected_status != 200: require(value['error']['code'] == 'upstream_protocol_error', ident+': missing protocol failure')
        elif expect == 'c2':
            native = terminal_response(ident)
            for key, item in native.items(): require(value.get(key) == item, ident+': native field lost '+key)
            require(('usage' in value) == ('usage' in native), ident+': fabricated usage')
            expected_text = native.get('output_text', '' if ident == 'empty_text' else 'final café plus')
            require(value['output_text'] == expected_text and value['object'] == 'response', ident+': text/object mismatch')
        else:
            require(status == 200 and value['id'] != terminal_response(ident)['id'], ident+': C1 projection changed '+str(status)+' '+str(value))
            require(value['status'] == 'completed' and value['usage']['output_tokens_details']['reasoning_tokens'] == 0, ident+': C1 projection changed')
            require(value['model'] == 'gpt-6.2-sol' and value['output_text'] == ('' if ident == 'empty_text' else 'final café plus'), ident+': C1 model/text projection changed')
        if ident == 'hold': require(time.monotonic()-started < .6 if expect == 'c2' else time.monotonic()-started >= .7, 'JSON terminal/EOF timing changed')
        expected_attempts = 1
    else:
        name = ident.rsplit('_',1)[0]
        upstream_status = int(name[4:]) if name.startswith('http') else 429 if name.startswith('retry_') else None
        expected_status = 400 if name == 'local' else 504 if expect == 'c2' and name.startswith('timeout_') else upstream_status if expect == 'c2' and upstream_status in (400,401,403,404,429) else 502
        require(status == expected_status, ident+': HTTP status '+str(status)+' != '+str(expected_status))
        code = value['error']['code']
        require(isinstance(value['error']['message'],str) and value['error']['type'], ident+': invalid error envelope')
        if expect == 'c2':
            expected_code = {'http400':'upstream_bad_request','http401':'upstream_unauthorized','http403':'upstream_forbidden',
                             'http404':'upstream_not_found','local':'validation_error','truncated':'upstream_protocol_error'}.get(name,
                             'upstream_rate_limit' if upstream_status == 429 else 'upstream_timeout' if name.startswith('timeout_') else 'upstream_unavailable')
            require(code == expected_code, ident+': error code '+code)
        else:
            expected_code = {'http400':'upstream_bad_request','http401':'upstream_unauthorized',
                             'http403':'upstream_unauthorized','http404':'upstream_protocol_error',
                             'local':'validation_error','truncated':'upstream_protocol_error'}.get(name,'upstream_unavailable')
            require(code == expected_code, ident+': C1 error code '+code)
        expected_hint = None
        if expect == 'c2':
            expected_hint = {'http429':'7','retry_date':'Wed, 21 Oct 2015 07:28:00 GMT',
                             'retry_obsolete':'Wednesday, 21-Oct-15 07:28:00 GMT',
                             'retry_asctime':'Wed Oct 21 07:28:00 2015'}.get(name)
        hints = [v for k,v in headers if k.lower() == 'retry-after']
        require(hints == ([] if expected_hint is None else [expected_hint]), ident+': unsafe/missing Retry-After')
        expected_attempts = 0 if name == 'local' else 1 if name == 'truncated' or upstream_status in (400,401,403,404) else 2
    require(server.attempts == expected_attempts, ident+': wrong retry count '+str(server.attempts))
    return {'id':ident,'result':0,'status':status,'attempts':server.attempts,
            'elapsed_ms':round((time.monotonic()-started)*1000)}


def worker(args):
    def selected(cases):
        cases = validate_cases(cases)
        if args.repair_only: return [case for case in cases if case['id'] in REPAIR_IDS]
        if args.expect == 'c1': return [case for case in cases if case['id'] not in REPAIR_IDS]
        return cases
    result = c1.worker(args,fixture=Fixture,validate=selected,case_runner=run_case,cases_path=CASES)
    directory = Path(args.worker)
    path = directory/'result.json'; report = json.loads(path.read_text())
    report['scope'] = 'repair-round-1' if args.repair_only else 'original-c1-56' if args.expect == 'c1' else 'full-c2-66'
    clean = all(SECRET not in p.read_text() for p in directory.glob('*.log'))
    report['ordinary_logs_secret_free'] = clean
    if not clean: report.update(result=1,error='synthetic secret in ordinary log'); result = 1
    path.write_text(json.dumps(report,indent=2)+'\n')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gateway',required=True)
    parser.add_argument('--expect',choices=('c1','initial-c2','c2'),required=True)
    parser.add_argument('--repair-only',action='store_true',help='Run the ten mandatory C2-R1/R2 repair cases only')
    parser.add_argument('--verify-failure-cleanup',action='store_true')
    parser.add_argument('--worker',help=argparse.SUPPRESS)
    parser.add_argument('--canary',help=argparse.SUPPRESS)
    parser.add_argument('--fail-after-start',action='store_true',help=argparse.SUPPRESS)
    args = parser.parse_args()
    # Reuse C1/P1 OS isolation, process ownership, canary and cleanup explicitly.
    try:
        require(not (args.repair_only and args.expect == 'c1'), 'repair comparison requires initial-c2 or c2 expectation')
        return worker(args) if args.worker else c1.run(args,script=__file__,cases_path=CASES,worker_args=['--repair-only'] if args.repair_only else [])
    except Exception as error:
        print(json.dumps({'result':1,'error':str(error)}),file=sys.stderr); return 1


if __name__ == '__main__': sys.exit(main())
