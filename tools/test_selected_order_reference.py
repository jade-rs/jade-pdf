"""登録時に画像内の未達ルールを落とすことを防ぐ。"""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from selected_order_reference import validate_selected_order_reference


class SelectedOrderReferenceTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.rules = [dict(case_id='image', page=1, anchors=['F01', 'F08'], scope='Image labels'),
                      dict(case_id='table', page=2, anchors=['Type', 'Name'], scope='Table labels')]
        self.gold = dict(source_id='owned', source_sha256='a'*64, source_url='https://example.test/a.pdf',
                         product_output_inspected_before_initial_declaration=False, reading_order=self.rules)
        self.review = dict(id='owned', source_sha256='a'*64, original_pages_reviewed=[1, 2],
                           product_output_inspected_before_declaration=False)
        self.entry = dict(id='owned', sha256='a'*64, url=self.gold['source_url'], pages=2,
                          gold=dict(self.rules[0], kind='reading_order'), reading_order=[self.rules[1]])
        self.pin()

    def pin(self):
        refs = {}
        for kind, value in [('gold', self.gold), ('review', self.review)]:
            data = json.dumps(value).encode()
            (self.root/(kind+'.json')).write_bytes(data)
            refs[kind+'_file'] = kind+'.json'
            refs[kind+'_sha256'] = hashlib.sha256(data).hexdigest()
        self.entry['selected_order_reference'] = refs

    def test_declared_primary_failure_and_positive_rules_are_all_retained(self):
        validate_selected_order_reference(self.entry, self.root)
        validate_selected_order_reference(dict(id='legacy'), self.root)

    def test_missing_swapped_or_changed_rules_and_source_are_rejected(self):
        for change in [dict(reading_order=[]), dict(gold=dict(self.entry['gold'], anchors=['F08', 'F01'])),
                       dict(gold=dict(self.entry['gold'], kind='scalar_inventory')),
                       dict(sha256='b'*64), dict(pages=1), dict(url='https://example.test/b.pdf')]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                validate_selected_order_reference(self.entry | change, self.root)

    def test_references_cannot_escape_or_change_without_new_hashes(self):
        for change in [dict(gold_file='../gold.json'), dict(review_file='/review.json'),
                       dict(gold_sha256='b'*64), dict(review_sha256='b'*64)]:
            entry = copy.deepcopy(self.entry)
            entry['selected_order_reference'].update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                validate_selected_order_reference(entry, self.root)

    def test_duplicate_unbounded_or_unreviewed_declarations_are_rejected(self):
        for change in [dict(reading_order=self.rules*2), dict(reading_order=[]),
                       dict(reading_order=[dict(self.rules[0], page=True)]),
                       dict(product_output_inspected_before_initial_declaration=True)]:
            saved = self.gold
            self.gold = saved | change
            self.pin()
            with self.subTest(change=change), self.assertRaises(ValueError):
                validate_selected_order_reference(self.entry, self.root)
            self.gold = saved
        self.review['original_pages_reviewed'] = [1]
        self.pin()
        with self.assertRaises(ValueError):
            validate_selected_order_reference(self.entry, self.root)


class RegisteredSourceReferences(unittest.TestCase):
    def test_english_image_failure_and_both_tables_stay_in_pending_cohort(self):
        corpus = Path(__file__).resolve().parents[1]/'corpus'
        manifest = json.loads((corpus/'manifest.json').read_text())
        identifier = 'nta-invoice-domestic-english-2026'
        self.assertFalse(any(entry['id'] == identifier for entry in manifest['documents']))
        entries = [entry for entry in manifest['pending_semantic'] if entry['id'] == identifier]
        self.assertEqual(len(entries), 1)
        entry = entries[0]
        self.assertIn('selected_order_reference', entry)
        validate_selected_order_reference(entry, corpus)
        self.assertEqual(entry['gold']['page'], 1)
        self.assertEqual(len(entry['gold']['anchors']), 14)
        self.assertEqual([rule['page'] for rule in entry['reading_order']], [4, 5])
        self.assertEqual(sum(len(rule['anchors']) for rule in [entry['gold'], *entry['reading_order']]), 32)


if __name__ == '__main__':
    unittest.main()
