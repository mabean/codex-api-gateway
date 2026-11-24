"""Mandatory C4 coverage controls."""
import copy
import json
import unittest
import codex_responses_c4 as c4


class C4Controls(unittest.TestCase):
    def test_inventory(self):
        cases = json.loads(c4.CASES.read_text())['cases']
        self.assertEqual(len(c4.validate_cases(cases)),42)
        for mutation in ('missing','duplicate','order','criterion','expected_c3','expected_c4','expected_initial_c4'):
            changed = copy.deepcopy(cases)
            if mutation == 'missing': changed.pop()
            elif mutation == 'duplicate': changed[1] = changed[0]
            elif mutation == 'order': changed.reverse()
            elif mutation == 'criterion': changed[0]['criterion'] = ''
            else: changed[0][mutation] = not changed[0][mutation]
            with self.subTest(mutation=mutation), self.assertRaises(ValueError): c4.validate_cases(changed)

    def test_native_owner_fields_and_ambiguity(self):
        self.assertIn('refresh_token',c4.native()['tokens'])
        auth,old,new = c4.scenarios()['ambiguous']
        self.assertEqual(len(auth['profiles']),2)
        self.assertTrue(old); self.assertFalse(new)


if __name__ == '__main__': unittest.main()
