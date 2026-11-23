"""Fail-closed controls for the C1 acceptance harness."""
import copy
import json
import unittest

import codex_responses_c1 as c1


class C1Controls(unittest.TestCase):
    def test_cases_are_mandatory(self):
        cases = json.loads(c1.CASES.read_text())['cases']
        self.assertEqual(len(c1.validate_cases(cases)), 24)
        for mutation in ('missing', 'reorder', 'duplicate', 'expected', 'criterion'):
            altered = copy.deepcopy(cases)
            if mutation == 'missing': altered.pop()
            elif mutation == 'reorder': altered.reverse()
            elif mutation == 'duplicate': altered[1] = altered[0]
            else: altered[0][mutation] = ''
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                c1.validate_cases(altered)

    def test_fixture_utf8_is_native(self):
        raw = c1.frame({'type':'response.output_text.delta','delta':'café'})
        self.assertIn('café'.encode(), raw)
        self.assertEqual(json.loads(raw.decode().split('data: ')[1])['delta'], 'café')


if __name__ == '__main__': unittest.main()
