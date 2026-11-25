"""Local adversarial fixtures for the quota-consuming acceptance runner."""
import argparse
import json
import pathlib
import tempfile
import unittest
from unittest.mock import patch

from live_gateway_checks import CheckFailure, Suite, validate_claude_tools


def trace():
    return [
        {'message': {'content': [{'type': 'tool_use', 'id': 'first', 'name': 'Read', 'input': {'file_path': 'note.txt', 'offset': 1, 'limit': 1}}]}},
        {'message': {'content': [{'type': 'tool_result', 'tool_use_id': 'first', 'content': '1→before'}]}},
        {'message': {'content': [{'type': 'tool_use', 'id': 'edit', 'name': 'Edit', 'input': {'file_path': 'note.txt', 'old_string': 'before', 'new_string': 'NONCE'}}]}},
        {'message': {'content': [{'type': 'tool_result', 'tool_use_id': 'edit', 'content': 'File updated successfully.'}]}},
        {'message': {'content': [{'type': 'tool_use', 'id': 'last', 'name': 'Read', 'input': {'file_path': 'note.txt', 'offset': 1, 'limit': 2}}]}},
        {'message': {'content': [{'type': 'tool_result', 'tool_use_id': 'last', 'content': [{'type': 'text', 'text': '1→NONCE\n2→CHALLENGE'}]}]}},
        {'type': 'result', 'subtype': 'success', 'result': 'NONCE\nCHALLENGE'},
    ]


def block(events, index):
    return events[index]['message']['content'][0]


class ClaudeEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.work = pathlib.Path(self.directory.name)
        # File and terminal marker deliberately remain correct in every counterexample.
        (self.work / 'note.txt').write_text('NONCE\nCHALLENGE\n')

    def validate(self, events):
        validate_claude_tools(events, 'edit-read', self.work, 'NONCE', 'CHALLENGE')

    def test_valid_relative_and_absolute_target_paths(self):
        self.validate(trace())
        events = trace()
        for index in (0, 2, 4):
            block(events, index)['input']['file_path'] = str(self.work / 'note.txt')
        self.validate(events)

    def test_other_file_cannot_pass_even_when_target_changed_separately(self):
        for indices in [(0, 2, 4), (0,), (2,), (4,)]:
            with self.subTest(indices=indices):
                events = trace()
                for index in indices:
                    block(events, index)['input']['file_path'] = 'other.txt'
                with self.assertRaises(CheckFailure):
                    self.validate(events)

    def test_stale_read_cannot_use_terminal_or_file_marker(self):
        events = trace()
        block(events, 5)['content'] = '1→before'
        with self.assertRaises(CheckFailure):
            self.validate(events)

    def test_first_read_must_not_disclose_challenge(self):
        events = trace()
        block(events, 1)['content'] = '1→before\n2→CHALLENGE'
        with self.assertRaises(CheckFailure):
            self.validate(events)

    def test_read_back_requires_both_values_from_same_result(self):
        for text in ('1→NONCE', '2→CHALLENGE'):
            with self.subTest(text=text):
                events = trace()
                block(events, 5)['content'] = text
                with self.assertRaises(CheckFailure):
                    self.validate(events)

    def test_disk_requires_both_lines(self):
        for text in ('NONCE\n', 'CHALLENGE\n', 'NONCE\nwrong\n'):
            with self.subTest(text=text):
                (self.work / 'note.txt').write_text(text)
                with self.assertRaises(CheckFailure):
                    self.validate(trace())

    def test_terminal_requires_exact_two_lines_and_success(self):
        for defect in ('missing', 'failed', 'wrong-subtype', 'marker-only', 'challenge-only', 'extra'):
            with self.subTest(defect=defect):
                events = trace()
                if defect == 'missing':
                    events.pop()
                elif defect == 'failed':
                    events[-1]['is_error'] = True
                elif defect == 'wrong-subtype':
                    events[-1]['subtype'] = 'error_max_turns'
                else:
                    events[-1]['result'] = {'marker-only': 'NONCE', 'challenge-only': 'CHALLENGE',
                                            'extra': 'NONCE\nCHALLENGE\nextra'}[defect]
                with self.assertRaises(CheckFailure):
                    self.validate(events)

    def test_bash_still_requires_correlated_successful_command_output(self):
        events = [
            {'message': {'content': [{'type': 'tool_use', 'id': 'bash', 'name': 'Bash', 'input': {}}]}},
            {'message': {'content': [{'type': 'tool_result', 'tool_use_id': 'bash', 'content': 'NONCE'}]}},
        ]
        validate_claude_tools(events, 'bash', self.work, 'NONCE', '')
        block(events, 1)['is_error'] = True
        with self.assertRaises(CheckFailure):
            validate_claude_tools(events, 'bash', self.work, 'NONCE', '')

    def test_read_issued_before_edit_result_is_not_read_back(self):
        events = trace()
        events[3], events[4] = events[4], events[3]
        with self.assertRaises(CheckFailure):
            self.validate(events)

    def test_missing_failed_or_wrong_id_result_at_each_stage(self):
        for index in (1, 3, 5):
            for defect in ('missing', 'failed', 'wrong-id'):
                with self.subTest(index=index, defect=defect):
                    events = trace()
                    if defect == 'missing':
                        del events[index]
                    elif defect == 'failed':
                        block(events, index)['is_error'] = True
                    else:
                        block(events, index)['tool_use_id'] = 'unmatched'
                    with self.assertRaises(CheckFailure):
                        self.validate(events)

    def test_edit_args_must_contain_expected_replacement(self):
        events = trace()
        block(events, 2)['input']['new_string'] = 'different'
        with self.assertRaises(CheckFailure):
            self.validate(events)

    def test_result_marker_must_be_text_content(self):
        events = trace()
        block(events, 5)['content'] = [{'type': 'image', 'text': 'NONCE'}]
        with self.assertRaises(CheckFailure):
            self.validate(events)


class ChatTerminalTests(unittest.TestCase):
    def test_chat_terminal_reason_must_match_actual_tool_or_text_stream(self):
        with tempfile.TemporaryDirectory() as directory:
            suite = Suite(argparse.Namespace(timeout=1), pathlib.Path(directory))
            suite.base = 'http://127.0.0.1:1'
            for has_tool, expected in [(True, 'tool_calls'), (False, 'stop')]:
                delta = {'tool_calls': [{'index': 0, 'id': 'call', 'function': {'name': 'Read', 'arguments': '{}'}}]} if has_tool else {'content': 'Done'}
                for reason in (expected, 'tool_use', None, 'length'):
                    with self.subTest(has_tool=has_tool, reason=reason):
                        raw = '\n\n'.join('data: ' + json.dumps(e) for e in [
                            {'choices': [{'delta': delta}]},
                            {'choices': [{'delta': {}, 'finish_reason': reason}]},
                        ]) + '\n\ndata: [DONE]\n\n'
                        class Response:
                            def __enter__(self): return self
                            def __exit__(self, *args): pass
                            def read(self): return raw.encode()
                        with patch('urllib.request.urlopen', return_value=Response()):
                            if reason == expected:
                                suite.request('/v1/chat/completions', {'stream': True})
                            else:
                                with self.assertRaises(CheckFailure):
                                    suite.request('/v1/chat/completions', {'stream': True})


if __name__ == '__main__':
    unittest.main()
