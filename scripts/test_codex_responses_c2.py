"""Fail-closed C2 fixture inventory and lifecycle reuse controls."""
import copy
import json
import unittest

import codex_responses_c2 as c2


class C2Controls(unittest.TestCase):
    def test_mandatory_cases_and_both_expectations(self):
        cases = json.loads(c2.CASES.read_text())['cases']
        self.assertEqual(len(c2.validate_cases(cases)), 66)
        for mutation in ('missing','duplicate','reorder','criterion','expected_c1','expected_c2'):
            altered = copy.deepcopy(cases)
            if mutation == 'missing': altered.pop()
            elif mutation == 'duplicate': altered[1] = altered[0]
            elif mutation == 'reorder': altered.reverse()
            else: altered[0][mutation] = ''
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                c2.validate_cases(altered)

    def test_native_payload_contains_discriminating_fields(self):
        response = c2.terminal_response('incomplete')
        self.assertEqual(response['incomplete_details']['reason'],'max_output_tokens')
        self.assertEqual([x['id'] for x in response['output']],['reasoning_c2','msg_c2','fc_c2'])
        self.assertGreater(response['usage']['output_tokens_details']['reasoning_tokens'],0)
        self.assertNotIn('usage',c2.terminal_response('no_usage'))
        self.assertNotIn('object',c2.terminal_response('sparse'))

    def test_repair_cases_distinguish_original_losses_from_authority_controls(self):
        for ident in c2.REPAIR_IDS:
            events, initial = c2.repair_case(ident,initial=True)
            same_events, fixed = c2.repair_case(ident)
            self.assertEqual(events,same_events)
            self.assertNotIn('usage',fixed)
            if ident == 'r1_terminal_output': self.assertEqual(initial,fixed)
            else: self.assertNotEqual(initial,fixed)
        self.assertEqual(c2.repair_case('r1_terminal_empty')[1]['output'],[])
        self.assertEqual(c2.repair_case('r1_terminal_text')[1]['output_text'],'')


if __name__ == '__main__': unittest.main()
