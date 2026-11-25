#!/usr/bin/env python3
"""Quota-consuming checks. Uses an owned localhost gateway and temporary artifacts."""
import argparse
import contextlib
import json
import os
import pathlib
import secrets
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

MODELS = ['gpt-6.2-astra', 'gpt-6.2-sol', 'gpt-6.1-terra', 'gpt-6-luna', 'gpt-6-sol',
          'claude-fable-5-2', 'claude-opus-5-5', 'claude-sonnet-5-5', 'claude-haiku-5-5']
ROOT = pathlib.Path(__file__).resolve().parent.parent


class CheckFailure(Exception):
    pass


def events(raw):
    result = []
    for line in raw.splitlines():
        if line.startswith('data:'):
            data = line[5:].strip()
            if data and data != '[DONE]':
                result.append(json.loads(data))
    return result


def safe_error(raw):
    """Return an actionable upstream error while stripping bearer/API tokens."""
    import re
    try:
        value = json.loads(raw)
        error = value.get('error', value)
        text = str(error.get('message', error)) if isinstance(error, dict) else str(error)
    except (ValueError, AttributeError):
        text = 'non-JSON error response (inspect private temporary artifacts)'
    text = re.sub(r'sk-[A-Za-z0-9_-]+|eyJ[A-Za-z0-9_.-]+|Bearer\s+\S+', '[REDACTED]', text)
    return text[:500]


def result_text(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return ''.join(block.get('text', '') for block in content
                       if isinstance(block, dict) and block.get('type') == 'text')
    return ''


def validate_claude_tools(parsed, scenario, work, marker, challenge):
    """Require evidence from matching successful tool results in execution order."""
    calls = {}
    stage = 0
    verified = False
    target = (work / 'note.txt').resolve()
    for event in parsed:
        content = event.get('message', {}).get('content', [])
        for block in content if isinstance(content, list) else []:
            if block.get('type') == 'tool_use':
                call_id = block.get('id')
                if not call_id or call_id in calls:
                    raise CheckFailure('missing or repeated tool call ID')
                args = block.get('input', {})
                eligible = False
                if scenario == 'edit-read':
                    path = args.get('file_path')
                    matches_target = isinstance(path, str) and (work / path).resolve() == target
                    eligible = matches_target and (
                        (stage == 0 and block.get('name') == 'Read') or
                        (stage == 1 and block.get('name') == 'Edit' and marker in args.get('new_string', '')) or
                        (stage == 2 and block.get('name') == 'Read'))
                else:
                    eligible = block.get('name') == 'Bash'
                # Capture stage at CALL time: a Read issued before Edit completes cannot verify it.
                calls[call_id] = (block, stage, eligible)
            elif block.get('type') == 'tool_result':
                matched = calls.pop(block.get('tool_use_id'), None)
                if matched is None or block.get('is_error'):
                    continue
                call, issued_stage, eligible = matched
                if not eligible or issued_stage != stage:
                    continue
                text = result_text(block.get('content'))
                if scenario == 'bash':
                    verified |= marker in text
                elif stage == 0:
                    if not challenge or challenge in text:
                        raise CheckFailure('first target Read disclosed the read-back challenge')
                    stage = 1
                elif stage == 1:
                    stage = 2
                elif marker in text and challenge in text:
                    verified = True
    if not verified:
        raise CheckFailure('missing successful target Read/Edit/Read read-back with both values'
                           if scenario == 'edit-read' else 'Bash result lacks command output marker')
    if scenario == 'edit-read':
        expected = marker + '\n' + challenge
        if (work / 'note.txt').read_text().strip() != expected:
            raise CheckFailure('file does not contain the expected two lines')
        terminal = [event for event in parsed if event.get('type') == 'result']
        if (not terminal or terminal[-1].get('is_error') or
                terminal[-1].get('subtype') != 'success' or
                terminal[-1].get('result', '').strip() != expected):
            raise CheckFailure('successful terminal answer does not contain the exact two lines')


class Suite:
    def __init__(self, args, directory):
        self.args, self.directory, self.results = args, directory, []
        self.base = None
        self.blockers = {}
        self.alias_targets = {"sonnet": "claude-sonnet-5-5", "opus": "claude-opus-5-5", "haiku": "claude-haiku-5-5"}

    def check(self, model, scenario, function):
        started = time.monotonic()
        target = self.alias_targets.get(model, model)
        family = 'anthropic' if target.startswith('claude-') else 'codex'
        key = family if family in self.blockers else model
        if key in self.blockers:
            row = {'model': model, 'scenario': scenario, 'status': 'BLOCKED', 'seconds': 0, 'detail': self.blockers[key]}
            self.results.append(row)
            (self.directory / 'summary.json').write_text(json.dumps(self.results, indent=2) + '\n')
            print(json.dumps(row), flush=True)
            return
        try:
            function()
            status, detail = 'PASS', ''
        except Exception as exc:
            status, detail = 'FAIL', str(exc)[:500]
            lowered = detail.lower()
            if any(term in lowered for term in ('no anthropic upstream auth', 'invalid bearer token', 'token has expired', 'http 401:', 'authentication_error', 'unauthorized')):
                self.blockers[family] = detail
            elif any(term in lowered for term in ('not supported', 'not available', 'does not have access', 'not found')):
                self.blockers[model] = detail
        row = {'model': model, 'scenario': scenario, 'status': status,
               'seconds': round(time.monotonic() - started, 2), 'detail': detail}
        self.results.append(row)
        (self.directory / 'summary.json').write_text(json.dumps(self.results, indent=2) + '\n')
        print(json.dumps(row), flush=True)

    def request(self, path, payload):
        name = f'{len(self.results):03d}-{secrets.token_hex(3)}'
        (self.directory / f'{name}-request.json').write_text(json.dumps(payload))
        req = urllib.request.Request(self.base + path, json.dumps(payload).encode(),
                                     {'Content-Type': 'application/json', 'anthropic-version': '2023-06-01',
                                      'x-api-key': 'local-proxy'})
        try:
            with urllib.request.urlopen(req, timeout=self.args.timeout) as response:
                raw = response.read().decode()
        except urllib.error.HTTPError as exc:
            raw = exc.read().decode()
            (self.directory / f'{name}-error.json').write_text(raw)
            raise CheckFailure(f'HTTP {exc.code}: {safe_error(raw)}') from exc
        (self.directory / f'{name}-response.txt').write_text(raw)
        data = events(raw) if payload.get('stream') else json.loads(raw)
        if isinstance(data, list):
            for event in data:
                if event.get('type') in ('error', 'response.failed', 'response.incomplete'):
                    raise CheckFailure('SSE terminal failure: ' + safe_error(json.dumps(event)))
            if path == '/v1/messages' and not any(e.get('type') == 'message_stop' for e in data):
                raise CheckFailure('missing message_stop')
            if path == '/v1/responses' and not any(e.get('type') == 'response.completed' for e in data):
                raise CheckFailure('missing response.completed')
            if path == '/v1/chat/completions':
                if '[DONE]' not in raw:
                    raise CheckFailure('missing [DONE]')
                choices = [c for e in data for c in e.get('choices', [])]
                has_tools = any(c.get('delta', {}).get('tool_calls') for c in choices)
                expected = 'tool_calls' if has_tools else 'stop'
                reasons = [c['finish_reason'] for c in choices if c.get('finish_reason') is not None]
                if reasons != [expected] or not choices or choices[-1].get('finish_reason') != expected:
                    raise CheckFailure('invalid Chat SSE terminal finish_reason; expected ' + expected)
        return data

    @staticmethod
    def text(path, response, stream):
        if not stream:
            if path == '/v1/messages':
                return ''.join(b.get('text', '') for b in response.get('content', []))
            if path == '/v1/chat/completions':
                return response['choices'][0]['message'].get('content', '')
            return response.get('output_text', '')
        if path == '/v1/messages':
            return ''.join(e.get('delta', {}).get('text', '') for e in response)
        if path == '/v1/chat/completions':
            return ''.join(c.get('delta', {}).get('content', '') for e in response for c in e.get('choices', []))
        return ''.join(e.get('delta', '') for e in response if e.get('type') == 'response.output_text.delta')

    @staticmethod
    def content(path, response, stream):
        if not stream:
            if path == '/v1/messages':
                return response['content']
            if path == '/v1/chat/completions':
                return response['choices'][0]['message']
            return response['output']
        if path == '/v1/messages':
            blocks = {}
            for e in response:
                idx = e.get('index')
                if e.get('type') == 'content_block_start':
                    blocks[idx] = dict(e['content_block'])
                    if blocks[idx].get('type') == 'tool_use':
                        blocks[idx]['_arguments'] = ''
                if e.get('type') == 'content_block_delta' and idx in blocks:
                    d = e['delta']
                    if d.get('type') == 'input_json_delta':
                        blocks[idx]['_arguments'] += d['partial_json']
                    elif d.get('type') == 'text_delta':
                        blocks[idx]['text'] = blocks[idx].get('text', '') + d['text']
                    elif d.get('type') == 'thinking_delta':
                        blocks[idx]['thinking'] = blocks[idx].get('thinking', '') + d['thinking']
                    elif d.get('type') == 'signature_delta':
                        blocks[idx]['signature'] = blocks[idx].get('signature', '') + d['signature']
            for b in blocks.values():
                if '_arguments' in b:
                    raw = b.pop('_arguments')
                    if raw:
                        b['input'] = json.loads(raw)
            return list(blocks.values())
        if path == '/v1/responses':
            # Preserve reasoning and tool items exactly as the completed response provides them.
            complete = next(e for e in response if e.get('type') == 'response.completed')
            return complete['response'].get('output') or [e['item'] for e in response if e.get('type') == 'response.output_item.done']
        calls, text = {}, ''
        for e in response:
            for c in e.get('choices', []):
                delta = c.get('delta', {})
                text += delta.get('content', '') or ''
                for call in delta.get('tool_calls', []):
                    b = calls.setdefault(call.get('index', 0), {'id': '', 'type': 'function', 'function': {'name': '', 'arguments': ''}})
                    b['id'] = call.get('id') or b['id']
                    for key in ('name', 'arguments'):
                        b['function'][key] += call.get('function', {}).get(key, '')
        return {'role': 'assistant', 'content': text or None, 'tool_calls': list(calls.values())}

    @staticmethod
    def payload(model, path, stream, prompt):
        payload = {'model': model, 'stream': stream}
        if path == '/v1/responses':
            payload.update(input=[{'role': 'user', 'content': prompt}], reasoning={'effort': 'low'})
        else:
            payload['messages'] = [{'role': 'user', 'content': prompt}]
            if path == '/v1/messages':
                payload['max_tokens'] = 2048
                if model.startswith('claude-') or model in ('sonnet', 'opus'):
                    payload['output_config'] = {'effort': 'low'}
            else:
                payload['reasoning_effort'] = 'low'
        return payload

    def text_check(self, model, path, stream):
        marker = 'TEXT_' + secrets.token_hex(5)
        payload = self.payload(model, path, stream, f'Reply with exactly {marker}.')
        response = self.request(path, payload)
        if marker not in self.text(path, response, stream):
            raise CheckFailure('missing text marker')
        if model in self.alias_targets:
            actual = next((e.get('message', {}).get('model') for e in response if e.get('type') == 'message_start'), None) if stream else response.get('model')
            if actual != self.alias_targets[model]:
                raise CheckFailure('alias target mismatch')

    def tool_check(self, model, path, stream):
        # The result nonce is generated only AFTER the call, so it cannot be guessed from the prompt.
        payload = self.payload(model, path, stream,
                               'Call the Read tool once with file_path="probe.txt". After its result, reply with exactly the content returned by the tool.')
        schema = {'type': 'object', 'properties': {'file_path': {'type': 'string'}}, 'required': ['file_path'], 'additionalProperties': False}
        definition = {'name': 'Read', 'description': 'Read the named test file.', 'parameters': schema}
        if path == '/v1/messages':
            payload['tools'] = [{'name': 'Read', 'description': definition['description'], 'input_schema': schema}]
            payload['tool_choice'] = {'type': 'auto'}
        elif path == '/v1/responses':
            payload['tools'] = [dict(type='function', **definition)]
            payload['tool_choice'] = 'auto'
        else:
            payload['tools'] = [{'type': 'function', 'function': definition}]
            payload['tool_choice'] = 'auto'
        response = self.request(path, payload)
        content = self.content(path, response, stream)
        if path == '/v1/messages':
            calls = [b for b in content if b.get('type') == 'tool_use']
            if len(calls) != 1 or calls[0]['name'] != 'Read' or calls[0].get('input', {}).get('file_path') != 'probe.txt':
                raise CheckFailure('expected exactly one valid Read tool_use')
            call_id = calls[0]['id']
        elif path == '/v1/responses':
            calls = [b for b in content if b.get('type') == 'function_call']
            if len(calls) != 1 or calls[0]['name'] != 'Read' or json.loads(calls[0]['arguments']).get('file_path') != 'probe.txt':
                raise CheckFailure('expected exactly one valid Read function_call')
            call_id = calls[0]['call_id']
        else:
            calls = content.get('tool_calls', [])
            if len(calls) != 1 or calls[0]['function']['name'] != 'Read' or json.loads(calls[0]['function']['arguments']).get('file_path') != 'probe.txt':
                raise CheckFailure('expected exactly one valid Read tool_call')
            call_id = calls[0]['id']
        if not call_id:
            raise CheckFailure('empty tool call ID')
        marker = 'RESULT_' + secrets.token_hex(8)
        if path == '/v1/messages':
            payload['messages'].extend([{'role': 'assistant', 'content': content},
                                        {'role': 'user', 'content': [{'type': 'tool_result', 'tool_use_id': call_id, 'content': marker}]}])
        elif path == '/v1/responses':
            payload['input'].extend(content + [{'type': 'function_call_output', 'call_id': call_id, 'output': marker}])
        else:
            payload['messages'].extend([dict(role='assistant', **{k: v for k, v in content.items() if k != 'role'}),
                                        {'role': 'tool', 'tool_call_id': call_id, 'content': marker}])
        final = self.request(path, payload)
        if marker not in self.text(path, final, stream):
            raise CheckFailure('matching tool result was not reflected in the final response')

    def claude_check(self, model, scenario):
        work = self.directory / model / scenario
        work.mkdir(parents=True)
        marker = 'CLAUDE_' + secrets.token_hex(6)
        challenge = ''
        if scenario == 'edit-read':
            challenge = 'READBACK_' + secrets.token_hex(12)
            (work / 'note.txt').write_text('before\n' + challenge + '\n')
            prompt = f'Use Read with file_path="note.txt", offset=1 and limit=1 to inspect only the first line. Then use Edit to replace before with {marker}, preserving the second line. After Edit completes, use a separate Read with offset=1 and limit=2 to read both lines of note.txt. Reply with exactly those two lines, including the previously unseen second-line code, after completing that Read.'
        else:
            prompt = f'Use Bash to run printf {marker}. Reply with exactly the command output.'
        env = dict(os.environ, ANTHROPIC_BASE_URL=self.base, ANTHROPIC_API_KEY='local-proxy',
                   CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC='1', API_TIMEOUT_MS=str(int(self.args.timeout * 1000)))
        # Child process groups ensure only this suite\'s processes are terminated on timeout.
        out = work / 'transcript.jsonl'
        with out.open('w') as output:
            process = subprocess.Popen(['claude', '-p', '--dangerously-skip-permissions', '--output-format', 'stream-json',
                                        '--verbose', '--max-turns', '8', '--model', model, prompt], cwd=work, env=env,
                                       stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                code = process.wait(timeout=self.args.claude_timeout)
            except subprocess.TimeoutExpired as exc:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
                raise CheckFailure(f'Claude Code exceeded {self.args.claude_timeout}s timeout') from exc
        parsed = []
        for line in out.read_text().splitlines():
            try:
                parsed.append(json.loads(line))
            except ValueError:
                continue
        terminal = [e for e in parsed if e.get('type') == 'result']
        if code != 0 or not terminal or terminal[-1].get('is_error') or terminal[-1].get('subtype') != 'success':
            errors = [str(e.get('result', '')) for e in terminal]
            raise CheckFailure(f'Claude Code exit={code}; ' + safe_error(json.dumps({'error': {'message': ' '.join(errors)}})))
        if marker not in terminal[-1].get('result', ''):
            raise CheckFailure('missing terminal marker')
        validate_claude_tools(parsed, scenario, work, marker, challenge)

    @contextlib.contextmanager
    def gateway(self, overrides=None):
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', self.args.port or 0))
            port = sock.getsockname()[1]
        env = dict(os.environ, CODEX_PROXY_VERBOSE=os.environ.get('CODEX_PROXY_VERBOSE', '0'),
                   CODEX_GATEWAY_CODEX_TIMEOUT_SECS=str(int(self.args.timeout)),
                   CODEX_GATEWAY_ANTHROPIC_FIRST_CHUNK_TIMEOUT_SECS=str(min(120, int(self.args.timeout))))
        if overrides:
            env.update(overrides)
        # Auth remains in environment/local credential stores, never command-line arguments.
        token = env.get('ANTHROPIC_AUTH_TOKEN')
        if token:
            env['ANTHROPIC_UPSTREAM_API_KEY'] = token
        args = [str(ROOT / 'target/release/codex-api-gateway'), '--port', str(port), '--auth-path', self.args.auth_path,
                '--upstream-base-url', self.args.upstream, '--anthropic-upstream-base-url', self.args.anthropic_upstream,
                '--log-path', str(self.directory / f'gateway-{port}.log')]
        old_base = self.base
        old_alias_targets = self.alias_targets.copy()
        for alias in ('sonnet', 'opus', 'haiku'):
            target = env.get('CODEX_GATEWAY_ALIAS_' + alias.upper())
            if target:
                self.alias_targets[alias] = target.rsplit('/', 1)[-1]
        self.base = f'http://127.0.0.1:{port}'
        with (self.directory / f'gateway-{port}.stdio').open('w') as output:
            process = subprocess.Popen(args, cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT)
            try:
                ready = False
                for _ in range(100):
                    if process.poll() is not None:
                        raise CheckFailure('gateway exited before readiness; inspect temporary stdio')
                    try:
                        urllib.request.urlopen(self.base + '/health', timeout=1).close()
                        ready = True
                        break
                    except urllib.error.URLError:
                        time.sleep(.1)
                if not ready:
                    raise CheckFailure('gateway readiness timeout')
                yield
            finally:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                self.base = old_base
                self.alias_targets = old_alias_targets

    def run(self):
        with self.gateway():
            for model in self.args.models:
                if self.args.suite == 'claude-code':
                    if model.startswith('claude-') and 'anthropic' not in self.blockers:
                        self.check(model, 'claude-code/auth-preflight', lambda m=model: self.text_check(m, '/v1/messages', False))
                    for scenario in ('edit-read', 'bash'):
                        self.check(model, 'claude-code/' + scenario, lambda m=model, s=scenario: self.claude_check(m, s))
                    continue
                paths = ['/v1/messages']
                if model.startswith('gpt-'):
                    paths += ['/v1/responses', '/v1/chat/completions']
                for path in paths:
                    for stream in (False, True):
                        mode = 'SSE' if stream else 'JSON'
                        self.check(model, f'{path}/{mode}/text', lambda m=model, p=path, s=stream: self.text_check(m, p, s))
                        self.check(model, f'{path}/{mode}/tool-roundtrip', lambda m=model, p=path, s=stream: self.tool_check(m, p, s))
            if self.args.suite == 'protocol':
                for alias in ('sonnet', 'opus', 'haiku'):
                    self.check(alias, '/v1/messages/JSON/default-alias', lambda a=alias: self.text_check(a, '/v1/messages', False))
        if self.args.suite == 'protocol':
            # Overrides switch upstream families; clear probe blockers keyed by aliases.
            for alias in ('sonnet', 'opus', 'haiku'):
                self.blockers.pop(alias, None)
            with self.gateway({'CODEX_GATEWAY_ALIAS_SONNET': 'gpt-6.2-sol', 'CODEX_GATEWAY_ALIAS_OPUS': 'claude-fable-5-2',
                               'CODEX_GATEWAY_ALIAS_HAIKU': 'gpt-6-luna'}):
                for alias in ('sonnet', 'opus', 'haiku'):
                    self.check(alias, '/v1/messages/SSE/override-alias', lambda a=alias: self.text_check(a, '/v1/messages', True))
        failures = sum(r['status'] != 'PASS' for r in self.results)
        counts = {status: sum(r['status'] == status for r in self.results) for status in ('PASS', 'FAIL', 'BLOCKED')}
        print(f"{counts['PASS']} passed; {counts['FAIL']} failed; {counts['BLOCKED']} blocked; private artifacts: {self.directory}", flush=True)
        return int(bool(failures))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--suite', choices=['protocol', 'claude-code'], required=True)
    parser.add_argument('--models', nargs='+', default=os.environ.get('MODELS', ' '.join(MODELS)).split())
    parser.add_argument('--port', type=int, default=int(os.environ.get('PORT', '0')))
    parser.add_argument('--timeout', type=float, default=float(os.environ.get('REQUEST_TIMEOUT', '90')))
    parser.add_argument('--claude-timeout', type=float, default=float(os.environ.get('CLAUDE_TIMEOUT', '180')))
    parser.add_argument('--auth-path', default=os.environ.get('AUTH_PATH', str(pathlib.Path.home() / '.codex/auth.json')))
    parser.add_argument('--upstream', default=os.environ.get('UPSTREAM_BASE_URL', 'https://chatgpt.com/backend-api'))
    parser.add_argument('--anthropic-upstream', default=os.environ.get('ANTHROPIC_UPSTREAM_BASE_URL', 'https://api.anthropic.com'))
    args = parser.parse_args()
    directory = pathlib.Path(os.environ.get('OUT_DIR') or tempfile.mkdtemp(prefix='codex-gateway-' + args.suite + '-'))
    directory.mkdir(parents=True, exist_ok=True)
    return Suite(args, directory).run()


if __name__ == '__main__':
    sys.exit(main())
