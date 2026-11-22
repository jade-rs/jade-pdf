import unittest
from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import sys
import tempfile
from unittest.mock import patch

import audit_page_gold
from audit_page_gold import regular_cases, check_page


class PageGoldTests(unittest.TestCase):
    def test_regular_gold_excludes_unresolved_cohort(self):
        regular = dict(id='regular', gold=dict(page=5, text='Disclaimer'))
        pending = dict(id='unresolved', gold=dict(page=1, text='not recovered'))
        manifest = dict(documents=[regular, dict(id='no-gold')], pending_semantic=[pending])
        self.assertEqual(regular_cases(manifest), [(regular, regular['gold'])])
        self.assertEqual(manifest['pending_semantic'], [pending])

    def test_legacy_whole_document_gold_requires_exactly_one_page(self):
        entry = dict(id='legacy', pages=1, full_page_text='one page')
        self.assertEqual(regular_cases(dict(documents=[entry]))[0][1],
                         dict(page=1, kind='full_page_text', text='one page'))
        entry['pages'] = 5
        with self.assertRaises(ValueError):
            regular_cases(dict(documents=[entry]))

    def test_selected_page_is_full_text_not_a_document_fragment(self):
        ir = dict(pages=[dict(blocks=[dict(text='other page')]) for _ in range(4)]
                  + [dict(blocks=[dict(text='Disclaimer'), dict(text='exact text')])])
        check = check_page(ir, dict(page=5, kind='full_page_text', text='Disclaimer exact text'))
        self.assertTrue(check['matches'])
        self.assertFalse(check_page(ir, dict(page=1, text='Disclaimer exact text'))['matches'])
        ir['pages'][4]['blocks'].append(dict(text='unaccounted text'))
        self.assertFalse(check_page(ir, dict(page=5, text='Disclaimer exact text'))['matches'])

    def test_invalid_page_cannot_select_last_page_or_boolean_index(self):
        ir = dict(pages=[dict(blocks=[dict(text='text')])])
        for page in [0, -1, 2, True, '1', None, 1.0]:
            with self.subTest(page=page), self.assertRaises(ValueError):
                check_page(ir, dict(page=page, text='text'))

    def test_both_gold_forms_are_rejected_instead_of_silently_choosing(self):
        with self.assertRaises(ValueError):
            regular_cases(dict(documents=[dict(id='ambiguous', pages=1,
                                               full_page_text='old', gold=dict(page=1, text='new'))]))

    def test_guarded_regular_mismatch_is_still_a_failing_gate(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'corpus/cache').mkdir(parents=True)
            (root/'tools').mkdir()
            (root/'tools/semantic_gold.py').write_text('checker source')
            entry = dict(id='regular', pages=1, sha256='pinned', gold=dict(page=1, text='correct'))
            manifest = dict(documents=[entry], pending_semantic=[dict(id='pending')])
            (root/'corpus/manifest.json').write_text(json.dumps(manifest))
            (root/'corpus/cache/regular.pdf').write_bytes(b'%PDF-1.7')
            binary, report = root/'binary', root/'report.json'
            binary.write_bytes(b'binary')
            ir = dict(pages=[dict(blocks=[dict(text='wrong')], sdk_unicode_mapping_errors=1)])
            equivalent = dict(summary=dict(changed=0), diagnostics=dict())
            with patch.object(audit_page_gold, 'ROOT', root), \
                    patch.object(audit_page_gold, 'fetch'), \
                    patch.object(audit_page_gold, 'invoke', side_effect=[(3, ir), (3, equivalent)]), \
                    patch.object(sys, 'argv', ['audit', '--binary', str(binary), '--report', str(report)]), \
                    redirect_stdout(io.StringIO()):
                self.assertEqual(audit_page_gold.main(), 1)
            result = json.loads(report.read_text())
            self.assertEqual((result['cases'], result['correct_cases'], result['failed_cases']), (1, 0, 1))
            self.assertTrue(result['results'][0]['selected_page_has_text_review'])
            self.assertEqual(manifest['pending_semantic'], [dict(id='pending')])


if __name__ == '__main__':
    unittest.main()
