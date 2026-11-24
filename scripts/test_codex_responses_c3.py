"""Fail-closed fixture inventory and real two-turn replay inputs."""
import copy
import json
import unittest

import codex_responses_c3 as c3


class C3Controls(unittest.TestCase):
    def test_inventory(self):
        cases = json.loads(c3.CASES.read_text())['cases']
        self.assertEqual(len(c3.validate_cases(cases)),55+len(c3.REPAIR_IDS))
        for mutation in ('missing','duplicate','reorder','criterion','expected_c2','expected_c3'):
            changed = copy.deepcopy(cases)
            if mutation == 'missing': changed.pop()
            elif mutation == 'duplicate': changed[1] = changed[0]
            elif mutation == 'reorder': changed.reverse()
            else: changed[0][mutation] = ''
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                c3.validate_cases(changed)
        changed = copy.deepcopy(cases)
        changed[-1]['expected_initial_c3'] = ''
        with self.assertRaises(ValueError): c3.validate_cases(changed)

    def test_native_replay_data_and_options(self):
        self.assertEqual(c3.CALL['name'],c3.FUNCTION['name'])
        self.assertTrue(c3.FUNCTION['strict'])
        self.assertTrue(c3.REASONING['encrypted_content'])
        request = c3.base_request(False)
        request['tools'][0]['parameters']['properties'].clear()
        self.assertIn('city',c3.FUNCTION['parameters']['properties'])
        self.assertGreater(len(c3.URLS),6)


if __name__ == '__main__': unittest.main()
