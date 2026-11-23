"""Offline fail-closed controls for mandatory P5 evidence."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import claude_contract_p5 as p5
from claude_contract_extract import digest


class P5Controls(unittest.TestCase):
    def setUp(self):
        self.cases = json.loads(p5.CASES.read_text())['cases']

    def test_all_positive_cases_required_in_order(self):
        self.assertEqual(len(p5.validate_cases(self.cases)),26)
        for mutation in ('missing','reorder','duplicate','expectation'):
            cases = copy.deepcopy(self.cases)
            if mutation == 'missing': cases.pop()
            elif mutation == 'reorder': cases.reverse()
            elif mutation == 'duplicate': cases[1] = cases[0]
            else: cases[0]['expected'] = ''
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                p5.validate_cases(cases)

    def test_raw_byte_drift_fails_before_ranges_are_written(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = b'\xc3\xa9;original()'
            module = {'name':'module','source':source,'offset':100}
            span = {'name':'test','module_id':0,'module':'module','module_sha256':digest(source),
                    'start':2,'end':len(source),'binary_start':102,'sha256':digest(source[3:])}
            profile = root/'profile.json'
            profile.write_text(json.dumps({'binary_sha256':'synthetic','ranges':[span]}))
            with patch.object(p5,'verify_p2',return_value={'binary_sha256':'synthetic'}), \
                 patch.object(p5,'read_graph',return_value=({},[module])), patch.object(p5,'PROFILE',profile):
                with self.assertRaisesRegex(ValueError,'raw byte span'):
                    p5.verify_inventory(root/'binary',root)
            self.assertFalse((root/'p5-ranges.json').exists())

    def test_executable_manifest_names_original_layers(self):
        profile = json.loads(p5.PROFILE.read_text())
        self.assertEqual({r['name'] for r in profile['ranges']},
            {'decoder','accumulator','messages'})
        for span in profile['ranges']:
            self.assertLess(span['start'],span['end'])
            self.assertEqual(len(span['sha256']),64)
            self.assertEqual(len(span['module_sha256']),64)


if __name__ == '__main__':
    unittest.main()
