"""Offline fail-closed controls for mandatory P3 evidence."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import claude_contract_p3 as p3
from claude_contract_extract import digest


class P3Controls(unittest.TestCase):
    def setUp(self):
        self.cases = json.loads(p3.CASES.read_text())['cases']

    def test_all_positive_cases_required_in_order(self):
        self.assertEqual(len(p3.validate_cases(self.cases)),15)
        for mutation in ('missing','reorder','duplicate','family','auth','delay','decision','context'):
            cases = copy.deepcopy(self.cases)
            if mutation == 'missing':
                cases.pop()
            elif mutation == 'reorder':
                cases.reverse()
            elif mutation == 'duplicate':
                cases[1] = cases[0]
            elif mutation == 'family':
                cases[0]['family'] = 'unavailable'
            elif mutation == 'auth':
                cases[0]['auth'] = 'oauth'
            elif mutation == 'delay':
                cases[5]['ms'] = cases[5]['seconds']*1000
            elif mutation == 'decision':
                cases[4]['allow'] = True
            else:
                cases[7]['reject'] = False
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                p3.validate_cases(cases)

    def test_raw_byte_drift_fails_before_ranges_are_written(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = b'\xc3\xa9;original()'
            module = {'name':'module','source':source,'offset':100}
            span = {'name':'test','module_id':0,'module':'module','module_sha256':digest(source),
                    'start':2,'end':len(source),'binary_start':102,'sha256':digest(source[3:])}
            profile = root/'profile.json'
            profile.write_text(json.dumps({'binary_sha256':'synthetic','ranges':[span]}))
            with patch.object(p3,'verify_p2',return_value={'binary_sha256':'synthetic'}), \
                 patch.object(p3,'read_graph',return_value=({},[module])), patch.object(p3,'PROFILE',profile):
                with self.assertRaisesRegex(ValueError,'raw byte span'):
                    p3.verify_inventory(root/'binary',root)
            self.assertFalse((root/'p3-ranges.json').exists())

    def test_executable_manifest_names_original_layers(self):
        profile = json.loads(p3.PROFILE.read_text())
        self.assertEqual({r['name'] for r in profile['ranges']},
                         {'makeRequest','retry','turn','rejection','rejectionRegex','contextRemoval','contextGate'})
        for span in profile['ranges']:
            self.assertLess(span['start'],span['end'])
            self.assertEqual(len(span['sha256']),64)
            self.assertEqual(len(span['module_sha256']),64)

    def test_sse_terminal_is_held_separately(self):
        head,tail = p3.response_parts({'id':'sse_barrier'})
        self.assertIn(b'P3 terminal sse_barrier',head)
        self.assertNotIn(b'message_stop',head)
        self.assertNotIn(b'end_turn',head)
        self.assertIn(b'message_stop',tail)
        self.assertIn(b'11',tail)


if __name__ == '__main__':
    unittest.main()
