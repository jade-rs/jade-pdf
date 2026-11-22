import unittest

from semantic_gold import check_gold, page_has_text_review, compact_fragment_matches


class SemanticGoldTests(unittest.TestCase):
    def test_selected_private_alias_must_not_remain_or_duplicate(self):
        gold=dict(kind='scalar_inventory',ranges=[[0x2264,0x2264]],count_each=1,
                  scope='One visible operator',forbidden_scalars=[0xf0a3])
        self.assertTrue(check_gold('≤\uf04b',gold)['matches'])
        self.assertFalse(check_gold('≤\uf0a3',gold)['matches'])
        for value in [None, 'x', [True], [0xd800], [0x110000], [0xf0a3,0xf0a3]]:
            with self.assertRaises(ValueError):
                check_gold('≤',gold | dict(forbidden_scalars=value))

    def test_optional_fragment_is_independent_of_inventory(self):
        self.assertTrue(compact_fragment_matches('…ℓ', {}))
        self.assertTrue(compact_fragment_matches('0 ≤ t\n≤ 79', dict(compact_fragment='0≤t≤79')))
        self.assertFalse(compact_fragment_matches('0≤t<79', dict(compact_fragment='0≤t≤79')))
        self.assertFalse(compact_fragment_matches('A\ufeffB', dict(compact_fragment='AB')))
        self.assertTrue(compact_fragment_matches('A\ufeffB', dict(compact_fragment='\ufeff')))
        for value in ['', ' \n', '\u0085', '\u001c', None, 4]:
            with self.assertRaises(ValueError):
                compact_fragment_matches('anything', dict(compact_fragment=value))

    def test_unrelated_page_or_geometry_review_cannot_guard_wrong_text(self):
        clean = dict(blocks=[dict(text='wrong')], user_unit=None)
        self.assertFalse(page_has_text_review(clean))
        for field in ['sdk_unicode_mapping_errors', 'sdk_text_objects_without_characters',
                      'source_text_characters_missing', 'source_unicode_scalars_unmatched', 'nonpainting_text_characters']:
            self.assertTrue(page_has_text_review(clean | {field: 1}))
        for field in ['source_text_accounting_unavailable',
                      'source_unicode_accounting_unavailable', 'reading_order_requires_review', 'text_visibility_accounting_unavailable']:
            self.assertTrue(page_has_text_review(clean | {field: True}))
        self.assertTrue(page_has_text_review(dict(blocks=[])))
        self.assertTrue(page_has_text_review(dict(blocks=[dict(text=' \n')])) )

    def test_full_text_preserves_order_marks_and_compatibility(self):
        gold = dict(text='é text')
        self.assertTrue(check_gold(' e\u0301\ntext ', gold)['matches'])
        for actual, expected in [('خل', 'لخ'), ('²', '2'), ('بَ', 'ب'), ('ب\u200cت', 'بت')]:
            self.assertFalse(check_gold(actual, dict(text=expected))['matches'])

    def test_supplementary_inventory_has_an_explicit_partial_scope(self):
        gold = dict(kind='scalar_inventory', ranges=[[0x10400, 0x10401]],
                    count_each=1, scope='Two printed chart cells only')
        result = check_gold('title\n𐐁𐐀\nprose', gold)
        self.assertTrue(result['matches'])
        self.assertEqual(result['kind'], 'scalar_inventory')
        self.assertEqual(result['scope'], gold['scope'])
        self.assertEqual(result['expected_inventory'], {'U+10400': 1, 'U+10401': 1})
        self.assertNotIn('actual_text', result)
        for actual in ['𐐀', '𐐀𐐁𐐁', '\u0400\u0401']:
            self.assertFalse(check_gold(actual, gold)['matches'])

    def test_inventory_is_literal_without_canonical_or_compatibility_folding(self):
        gold = dict(kind='scalar_inventory', ranges=[[0x1d400, 0x1d400]],
                    count_each=1, scope='One mathematical scalar')
        self.assertFalse(check_gold('A', gold)['matches'])
        self.assertTrue(check_gold('𝐀', gold)['matches'])

    def test_invalid_or_unbounded_gold_is_rejected(self):
        valid = dict(kind='scalar_inventory', ranges=[[0x10400, 0x1044f]],
                     count_each=1, scope='Grid only')
        invalid = [dict(ranges=[]), dict(ranges=[[2, 1]]),
                   dict(ranges=[[0xd7ff, 0xe000]]), dict(ranges=[[0, 0x110000]]),
                   dict(ranges=[[0, 4096]]), dict(ranges=[[1, 3], [3, 5]]),
                   dict(ranges=[[True, 2]]), dict(count_each=0), dict(count_each=True),
                   dict(count_each=30_000), dict(scope=''), dict(kind='guess')]
        for change in invalid:
            with self.subTest(change=change), self.assertRaises(ValueError):
                check_gold('', valid | change)


if __name__ == '__main__':
    unittest.main()
