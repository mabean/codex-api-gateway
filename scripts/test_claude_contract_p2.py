"""Fail-closed P2 controls; no official binary, credentials or network required."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import claude_contract_p2 as p2
from claude_contract_extract import digest


class P2Controls(unittest.TestCase):
    def setUp(self):
        self.cases = json.loads(p2.CASES.read_text())['cases']

    def test_complete_case_order_and_all_six_families(self):
        self.assertEqual(len(p2.validate_cases(self.cases)), 20)
        self.assertEqual({c['family'] for c in self.cases}, set(p2.FAMILIES))

    def test_missing_duplicate_reordered_or_unavailable_case_fails(self):
        for mutation in ('missing','duplicate','reordered','unavailable','wrong_route','wrong_control'):
            cases = copy.deepcopy(self.cases)
            if mutation == 'missing':
                cases.pop()
            elif mutation == 'duplicate':
                cases[1]['id'] = cases[0]['id']
            elif mutation == 'reordered':
                cases[0], cases[1] = cases[1], cases[0]
            elif mutation == 'unavailable':
                cases[0]['expected_outcome'] = 'unavailable'
            elif mutation == 'wrong_route':
                cases[0]['route'] = 'codex'
            else:
                cases[4]['effort_only'] = True
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                p2.validate_cases(cases)

    def tool_body(self):
        case = self.cases[0]
        return {'model':'claude-haiku-5-5','stream':True,
                'messages':[{'role':'user','content':case['id']},
                            {'role':'assistant','content':[{'type':'tool_use','id':'toolu_'+case['id'],
                             'name':'ToolSearch','input':{'query':'select:Read'}}]},
                            {'role':'user','content':[{'type':'tool_result','tool_use_id':'toolu_'+case['id'],
                             'content':[{'type':'tool_reference','tool_name':'Read'}]}]}],
                'tools':[{'name':'ToolSearch'}, {'name':'Read','defer_loading':True,
                         'input_schema':{'required':['file_path']}}]}

    def validate_tool(self, body):
        p2.validate_upstream(self.cases[0],body,'/v1/messages',
                            {'x-api-key':'p2-synthetic-upstream','anthropic-version':'2023-06-01',
                             'anthropic-beta':'advanced-tool-use-2025-11-20'})

    def test_tool_fixture_rejects_broken_ids_order_and_schema(self):
        self.validate_tool(self.tool_body())
        for mutation in ('id','order','schema','reference'):
            body = self.tool_body()
            if mutation == 'id':
                body['messages'][2]['content'][0]['tool_use_id'] = 'wrong'
            elif mutation == 'order':
                body['messages'][1],body['messages'][2] = body['messages'][2],body['messages'][1]
            elif mutation == 'schema':
                body['tools'][1]['input_schema']['required'] = []
            else:
                body['messages'][2]['content'][0]['content'][0]['tool_name'] = 'NotebookEdit'
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                self.validate_tool(body)

    def test_wrong_endpoint_and_non_synthetic_key_fail(self):
        for path, key in [('/v1/messages?beta=true','p2-synthetic-upstream'),('/v1/messages','other')]:
            with self.assertRaises(ValueError):
                p2.validate_upstream(self.cases[0], self.tool_body(), path, {'x-api-key':key})

    def test_nonzero_terminal_counters_are_supplied_before_comparison(self):
        for route in ('anthropic','codex'):
            case = next(c for c in self.cases if c['family']=='usage' and c['route']==route)
            head, tail = p2.response_parts(case)
            self.assertIn(b'17',head+tail)
            self.assertIn(b'11',tail)
            self.assertIn(('P2 terminal '+case['id']).encode(),head)
            self.assertNotIn(b'message_stop',head)
            self.assertNotIn(b'response.completed',head)

    def test_field_detection_distinguishes_keys_from_correlation_text(self):
        self.assertFalse(p2.has_field({'messages':[{'content':'clear_at_anthropic_first'}]},'clear_at'))
        self.assertTrue(p2.has_field({'messages':[{'clear_at':'next_user_message'}]},'clear_at'))

    def test_inventory_maps_every_future_criterion_without_acceptance_claim(self):
        inventory = json.loads(p2.INVENTORY.read_text())
        mapped = {ac for record in inventory['records'] for ac in record['acceptance']}
        self.assertEqual(mapped,{f'P{phase}-AC{ac}' for phase in (3,4,5) for ac in (1,2,3)})
        self.assertIn('not P3-P5 acceptance',inventory['status'])
        spans = {span['id'] for span in inventory['spans']}
        for record in inventory['records']:
            self.assertTrue(set(record['source_spans']) <= spans)
            for key in ('activation','request_or_events','caller_transport_consumer','next_outcome',
                        'anthropic','codex','proof','remaining_condition'):
                self.assertTrue(record[key])

    def test_raw_byte_span_drift_fails_before_executable_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = b'\xc3\xa9;function SXn(){}'
            module = {'id':0,'name':'module','source':source,'loader':1,'offset':100}
            inventory = {'graph_module_count':1,'scan_markers':['SXn'],'scan_hits':{'SXn':[{'id':0,'count':1}]},
                         'spans':[{'module_id':0,'module':'module','module_sha256':digest(source),
                                   'start':3,'end':len(source),'binary_start':103,'sha256':digest(source[3:])}]}
            path = root/'inventory.json'
            # A decoded character position points at a different raw byte after é.
            inventory['spans'][0]['start'] = 2
            path.write_text(json.dumps(inventory))
            with patch.object(p2,'extract',return_value={'binary_sha256':'synthetic'}), \
                 patch.object(p2,'read_graph',return_value=({},[module])), patch.object(p2,'INVENTORY',path):
                with self.assertRaisesRegex(ValueError,'raw byte span mismatch'):
                    p2.verify_inventory(root/'binary',root)
            self.assertFalse((root/'p2-source.js').exists())


if __name__ == '__main__':
    unittest.main()
