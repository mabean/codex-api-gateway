#!/usr/bin/env python3
"""C3 offline native request/replay contract through the real release ingress."""
import argparse
import copy
import http.client
import json
from pathlib import Path
import sys
from types import SimpleNamespace

import codex_responses_c1 as c1
from claude_contract_p2 import require

CASES = c1.ROOT/'scripts/fixtures/codex_responses/c3_cases.json'
TOKEN = 'c3-synthetic-oauth'
ACCOUNT = 'c3-synthetic-account'
FUNCTION = {'type':'function','name':'weather_lookup_42','description':'Weather café',
            'parameters':{'type':'object','properties':{'city':{'type':'string'}},
                          'required':['city'],'additionalProperties':False},'strict':True}
REASONING = {'type':'reasoning','id':'rs_c3','summary':[],
             'encrypted_content':'opaque-replay-c3','opaque':{'preserve':42}}
CALL = {'type':'function_call','id':'fc_c3','call_id':'call_c3','name':FUNCTION['name'],
        'arguments':'{"city":"Berlin"}','status':'completed','opaque':7}
URLS = ['', '/', '/backend-api', '/backend-api/', '/backend-api/codex',
        '/backend-api/codex/', '/backend-api/codex/responses',
        '/backend-api/codex/responses///', '/tenant/backend-api/codex/responses/']


def base_request(mode):
    return {'model':'openai/gpt-5.4','input':'Weather in Berlin?','stream':mode,
            'tools':[copy.deepcopy(FUNCTION)],'parallel_tool_calls':False,
            'tool_choice':{'type':'function','name':FUNCTION['name']}}


def validate_cases(cases):
    expected = ['replay_json','replay_stream','nested','strict_default','defaults','object_input','scalar_input',
                'unknown_temperature','toolsearch','chat_allowlist','messages_allowlist']
    expected += [name+'_'+mode for name in NEGATIVES for mode in ('json','stream')]
    expected += REPAIR_IDS
    require([c['id'] for c in cases] == expected,'mandatory C3 case inventory changed')
    require(all(c.get('criterion') and c.get('expected_c2') and c.get('expected_c3') for c in cases),
            'missing C3 fixture expectation')
    require(all(c.get('expected_initial_c3') for c in cases if c['id'] in REPAIR_IDS),
            'missing initial C3 repair expectation')
    return cases


NULL_CONTROLS = ['instructions','prompt_cache_key','service_tier','store','stream',
                 'parallel_tool_calls','temperature','tools','include','reasoning','text','tool_choice']
REPAIR_NAMES = ['r1_null_'+field for field in NULL_CONTROLS] + [
    'r1_context','r1_mode','r1_summary_off','r1_summary_on','r1_inner_null','r1_text_null',
    'r2_flat_null','r2_nested_null','r2_nested_missing',
    'r3_dated','r3_folded','r3_trimmed','r3_unknown_dated','r3_invalid_suffix']
REPAIR_IDS = [name+'_'+mode for name in REPAIR_NAMES for mode in ('json','stream')]


def run_repair(server,port,ident,expect):
    mode = ident.endswith('_stream'); name = ident.rsplit('_',1)[0]
    request = {'model':'gpt-5.4','input':'repair fixture','stream':mode}
    initial_param = None; repaired_status = 200
    if name.startswith('r1_null_'):
        field = name[len('r1_null_'):]; request[field] = None; initial_param = field
    elif name in ('r1_context','r1_mode','r1_summary_off','r1_summary_on','r1_inner_null'):
        reasoning = {'r1_context':{'context':'all_turns'},'r1_mode':{'mode':'standard'},
                     'r1_summary_off':{'summary':'off'},'r1_summary_on':{'summary':'on'},
                     'r1_inner_null':{'effort':None,'summary':None,'context':None}}[name]
        request['reasoning'] = reasoning
        initial_param = 'reasoning.'+('context' if name in ('r1_context','r1_inner_null') else 'mode' if name == 'r1_mode' else 'summary')
    elif name == 'r1_text_null':
        request['text'] = {'verbosity':None,'format':None}; initial_param = 'text.format'
    elif name.startswith('r2_'):
        declaration = {'name':'Read'}
        if name != 'r2_nested_missing': declaration.update(description=None,parameters=None)
        request['tools'] = [dict(declaration,type='function')] if name == 'r2_flat_null' else [{'type':'function','function':declaration}]
        if name != 'r2_nested_missing': initial_param = 'tools[0].description'
    elif name.startswith('r3_'):
        request['model'] = {'r3_dated':'openai/gpt-6.2-sol-2026-11-17','r3_folded':'GPT-6.2-SOL',
                            'r3_trimmed':'  OPENAI/GPT-6.2-SOL  ','r3_unknown_dated':'future-model-2026-11-17',
                            'r3_invalid_suffix':'gpt-6.2-sol-2026-1-17'}[name]
        request['temperature'] = .75
        if name not in ('r3_unknown_dated','r3_invalid_suffix'): repaired_status = 400
    server.attempts = 0; server.requests = []; server.failures = []; server.turn = 2
    status,value = post(port,request)
    expected_status = (400 if initial_param else 200) if expect == 'initial-c3' else repaired_status
    require(status == expected_status,ident+': status '+str(status)+' != '+str(expected_status))
    require(server.attempts == (1 if status == 200 else 0),ident+': attempts')
    if status == 400:
        require(value['error']['code'] == 'validation_error' and value['error']['param'] == (initial_param if expect == 'initial-c3' else 'temperature'),ident+': param')
    else:
        observed = server.requests[0]['body']
        if name.startswith('r1_null_'):
            require('prompt_cache_key' not in observed and observed['store'] is False and observed['stream'] is True,ident+': defaults')
            require(observed['instructions'] and observed['parallel_tool_calls'] is True,ident+': omitted control defaults')
        elif name.startswith('r1_'):
            field = 'text' if name == 'r1_text_null' else 'reasoning'
            require(observed[field] == request[field],ident+': supplied options changed')
        elif name.startswith('r2_'):
            tool = observed['tools'][0]
            if name == 'r2_flat_null': require(tool['parameters'] is None and tool['description'] is None,ident+': native null changed')
            elif expect == 'initial-c3': require('parameters' not in tool,ident+': initial missing schema changed')
            else: require(tool['parameters'] == {'type':'object','properties':{}},ident+': nested default')
            require(tool['strict'] is False,ident+': strict default')
        elif name.startswith('r3_'):
            expected_model = request['model'].rsplit('/',1)[-1]
            require(observed['model'] == expected_model and observed['temperature'] == .75,ident+': model or temperature rewritten')
    require(not server.failures,ident+': fixture errors')
    return {'id':ident,'status':status,'attempts':server.attempts,'requests':server.requests}


# Each mutation records the field and the immutable C2 behavior (HTTP status).
NEGATIVES = {
    'store':('store',True,'store',200),
    'previous':('previous_response_id','resp_old','previous_response_id',200),
    'previous_null':('previous_response_id',None,'previous_response_id',200),
    'metadata':('metadata',{'secret':'synthetic'},'metadata',200),
    'metadata_null':('metadata',None,'metadata',200),
    'truncation_null':('truncation',None,'truncation',200),
    'top_p_null':('top_p',None,'top_p',200),
    'user_null':('user',None,'user',200),
    'unsupported_tool':('tools',[{'type':'web_search'}],'tools[0].type',200),
    'bad_name':('tools',[dict(FUNCTION,name='bad name')],'tools[0].name',200),
    'bad_strict':('tools',[dict(FUNCTION,strict='yes')],'tools[0].strict',200),
    'bad_schema':('tools',[dict(FUNCTION,parameters=[])],'tools[0].parameters',200),
    'missing_choice':('tool_choice',{'type':'function','name':'undeclared'},'tool_choice',200),
    'bad_parallel':('parallel_tool_calls','false','parallel_tool_calls',400),
    'bad_tier':('service_tier',{},'service_tier',200),
    'bad_include':('include',[1],'include',400),
    'bad_cache':('prompt_cache_key',42,'prompt_cache_key',400),
    'bad_reasoning':('reasoning',{'effort':42},'reasoning.effort',200),
    'bad_text':('text',{'format':{'type':'bogus'}},'text.format',200),
    'temperature_56':('model','openai/gpt-5.6','temperature',200),
    'temperature_62':('model','openai/gpt-6.2-sol','temperature',200),
    'temperature_manifest':('model','gpt-daybreak-red-latest','temperature',200),
}


class Fixture(c1.Fixture):
    def do_POST(self):
        server = self.server
        try:
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            server.requests.append({'path':self.path,'body':body,
                                    'authorization':self.headers.get('Authorization'),
                                    'account':self.headers.get('chatgpt-account-id')})
            server.attempts += 1
            output = [REASONING,CALL] if server.turn == 1 else [
                {'type':'message','id':'msg_c3','role':'assistant','status':'completed',
                 'content':[{'type':'output_text','text':'Berlin: 18°C'}]}]
            response = {'id':'resp_c3_'+str(server.turn),'model':'gpt-5.4','status':'completed','output':output}
            self.send_response(200); self.send_header('Content-Type','text/event-stream'); self.end_headers()
            self.wfile.write(c1.frame({'type':'response.completed','response':response})); self.wfile.flush()
        except (BrokenPipeError,ConnectionResetError): pass
        except Exception as error: server.failures.append(str(error))


def post(port, body, path='/v1/responses'):
    connection = http.client.HTTPConnection('127.0.0.1',port,timeout=8)
    try:
        connection.request('POST',path,json.dumps(body),{'Content-Type':'application/json',
                           'Authorization':'Bearer caller-cannot-override','chatgpt-account-id':'caller-account'})
        response = connection.getresponse(); wire = response.read()
        if response.status == 200 and body.get('stream'):
            events = [json.loads(line[6:]) for line in wire.decode().splitlines() if line.startswith('data: ')]
            value = events[-1]['response']
        else: value = json.loads(wire)
        return response.status,value
    finally: connection.close()


def run_case(server, port, ident, expect):
    if ident in REPAIR_IDS: return run_repair(server,port,ident,expect)
    server.attempts = 0; server.requests = []; server.failures = []; server.turn = 1
    mode = ident.endswith('_stream'); request = base_request(mode)
    negative = ident.rsplit('_',1)[0] in NEGATIVES
    param = None; path = '/v1/responses'
    if negative:
        key,value,param,old_status = NEGATIVES[ident.rsplit('_',1)[0]]
        request[key] = copy.deepcopy(value)
        if ident.startswith('temperature_'): request['temperature'] = .25
    elif ident == 'nested':
        request['tools'] = [{'type':'function','function':{k:v for k,v in FUNCTION.items() if k != 'type'}}]
        request['tool_choice'] = {'type':'function','function':{'name':FUNCTION['name']}}
    elif ident == 'defaults': request = {'model':'gpt-5.4','input':'defaults'}
    elif ident == 'strict_default': del request['tools'][0]['strict']
    elif ident == 'object_input': request['input'] = {'role':'user','content':'object'}
    elif ident == 'scalar_input': request['input'] = 42
    elif ident == 'unknown_temperature': request.update(model='future-unknown-model',temperature=.75)
    elif ident == 'toolsearch':
        request['tools'] = [dict(FUNCTION,name='ToolSearch',_codex_client_discovery=True)]
        request['tool_choice'] = 'auto'
    elif ident in ('chat_allowlist','messages_allowlist'):
        path = '/v1/chat/completions' if ident == 'chat_allowlist' else '/v1/messages'
        request = {'model':'gpt-5.4','messages':[{'role':'user','content':'fixture'}],
                   'tools':[{'type':'function','function':{'name':FUNCTION['name'],'parameters':FUNCTION['parameters']}}]
                   if ident == 'chat_allowlist' else [{'name':FUNCTION['name'],'input_schema':FUNCTION['parameters']}],
                   'max_tokens':100}
        server.turn = 2
    elif ident.startswith('replay_'):
        request.update(instructions='Exact fixture instructions',service_tier='priority',
                       reasoning={'effort':'low','summary':None},include=['reasoning.encrypted_content'],
                       text={'verbosity':'high','format':{'type':'json_schema','name':'weather',
                             'schema':FUNCTION['parameters'],'strict':True}},prompt_cache_key='explicit-c3')
    else: raise ValueError('unknown case '+ident)
    # Caller-controlled destinations/credentials must remain inert.
    request.update(upstream_base_url='http://192.0.2.1/',authorization='evil',account_id='evil')
    status,value = post(port,request,path)
    if negative:
        require(status == (400 if expect == 'c3' else old_status),ident+': status '+str(status))
        require(server.attempts == (0 if status == 400 else 1),ident+': invalid attempt count')
        if expect == 'c3':
            require(value['error']['param'] == param and value['error']['code'] == 'validation_error',ident+': missing safe field error')
            require('synthetic' not in json.dumps(value),ident+': reflected request data')
    else:
        require(status == 200 and server.attempts == 1,ident+': request failed '+str(value))
        observed = server.requests[-1]['body']
        require(observed['stream'] is True and observed['store'] is False and isinstance(observed['input'],list),ident+': profile')
        if ident in ('chat_allowlist','messages_allowlist'): require(observed['tools'] == [],ident+': allowlist changed')
        elif expect == 'c3':
            if ident == 'defaults': require('prompt_cache_key' not in observed and observed['instructions'],ident+': defaults')
            else:
                name = 'ToolSearch' if ident == 'toolsearch' else FUNCTION['name']
                require(observed['tools'][0]['name'] == name and observed['tools'][0]['strict'] is (ident != 'strict_default'),ident+': arbitrary function lost')
                require(observed['tools'][0]['parameters'] == FUNCTION['parameters'],ident+': schema changed')
                require(observed['parallel_tool_calls'] is False,ident+': parallel setting')
            if ident == 'unknown_temperature': require(observed['temperature'] == .75,'unknown temperature lost')
        else:
            require(observed['tools'] == [] and 'prompt_cache_key' in observed,ident+': baseline conversion changed')
        if ident.startswith('replay_'):
            if expect == 'c3':
                for key in ('instructions','service_tier','reasoning','include','text','prompt_cache_key'):
                    require(observed[key] == request[key],ident+': option lost '+key)
            history = observed['input'] + value['output']
            actual_call = next(item for item in value['output'] if item['type'] == 'function_call')
            history.append({'type':'function_call_output','call_id':actual_call['call_id'],'output':'18°C','opaque':99})
            second = dict(request,input=history,tool_choice='auto'); server.turn = 2
            status,final = post(port,second)
            require(status == 200 and server.attempts == 2 and final['output'][0]['content'][0]['text'] == 'Berlin: 18°C',ident+': second turn')
            require(server.requests[-1]['body']['input'] == history,ident+': replay content changed')
        if ident == 'object_input': require(observed['input'] == [request['input']],ident+': object input changed')
        if ident == 'scalar_input': require(observed['input'] == [{'role':'user','content':[{'type':'input_text','text':'42'}]}],ident+': scalar input changed')
    require(not server.failures,ident+': fixture errors '+str(server.failures))
    for attempt in server.requests:
        require(attempt['authorization'] == 'Bearer '+TOKEN and attempt['account'] == ACCOUNT,ident+': configured auth changed')
        base = server.url_suffix.rstrip('/')
        expected_path = base if base.endswith('/codex/responses') else base+'/responses' if base.endswith('/codex') else base+'/codex/responses'
        if expect == 'c2': expected_path = base+'/codex/responses'
        require(attempt['path'] == expected_path,ident+': URL normalization')
    return {'id':ident,'status':status,'attempts':server.attempts,'param':param,'requests':server.requests}


def worker(args):
    root = Path(args.worker); reports = []
    for index,suffix in enumerate(URLS):
        if args.repair_only and index: break
        directory = root/str(index); directory.mkdir()
        local = SimpleNamespace(**vars(args)); local.worker = str(directory); local.upstream_suffix = suffix
        local.synthetic_auth = {'tokens':{'access_token':TOKEN,'account_id':ACCOUNT}}
        def runner(server,port,ident,expect):
            server.url_suffix = suffix
            return run_case(server,port,ident,expect)
        def selected(cases):
            cases = validate_cases(cases)
            if args.repair_only: return [c for c in cases if c['id'] in REPAIR_IDS]
            if index: return [c for c in cases if c['id'] == 'defaults']
            return [c for c in cases if args.expect != 'c2' or c['id'] not in REPAIR_IDS]
        result = c1.worker(local,fixture=Fixture,validate=selected,case_runner=runner,cases_path=CASES)
        report = json.loads((directory/'result.json').read_text()); report['url_suffix'] = suffix
        reports.append(report)
        if result: break
    combined = {'result':result,'ports':[p for r in reports for p in r['ports']],
                'isolation':all(r.get('isolation') for r in reports),
                'owned_process_cleanup':all(r['owned_process_cleanup'] for r in reports),'runs':reports}
    if result: combined['error'] = reports[-1].get('error')
    (root/'result.json').write_text(json.dumps(combined,indent=2)+'\n')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gateway',required=True)
    parser.add_argument('--expect',choices=('c2','initial-c3','c3'),required=True)
    parser.add_argument('--repair-only',action='store_true')
    parser.add_argument('--verify-failure-cleanup',action='store_true')
    parser.add_argument('--worker',help=argparse.SUPPRESS)
    parser.add_argument('--canary',help=argparse.SUPPRESS)
    parser.add_argument('--fail-after-start',action='store_true',help=argparse.SUPPRESS)
    args = parser.parse_args()
    try:
        require(args.expect != 'initial-c3' or args.repair_only,'initial-c3 requires repair-only')
        require(not (args.expect == 'c2' and args.repair_only),'C2 comparison retains original cases')
        return worker(args) if args.worker else c1.run(args,script=__file__,cases_path=CASES,worker_args=['--repair-only'] if args.repair_only else [])
    except Exception as error:
        print(json.dumps({'result':1,'error':str(error)}),file=sys.stderr); return 1


if __name__ == '__main__': sys.exit(main())
