"""選択した読み順の未達を、全文の正解と区別して記録する。"""
import unittest

from semantic_gold import check_gold


class ReadingOrderGoldTests(unittest.TestCase):
    def test_ordered_anchors_preserve_scope_and_do_not_reuse_occurrences(self):
        gold = dict(kind='reading_order', anchors=['F01', 'F08', 'F01'],
                    scope='Three printed field labels only; not page prose or geometry')
        result = check_gold('prefix F01\nF08 suffix F01', gold)
        self.assertEqual(result, dict(kind='reading_order', scope=gold['scope'], matches=True,
                                     expected_anchors=gold['anchors'], matched_prefix=3))
        for actual, prefix in [('F08 F01', 1), ('F01 F08', 2), ('image only', 0)]:
            with self.subTest(actual=actual):
                result = check_gold(actual, gold)
                self.assertFalse(result['matches'])
                self.assertEqual(result['matched_prefix'], prefix)

    def test_only_canonical_spelling_and_whitespace_are_folded(self):
        gold = dict(kind='reading_order', anchors=['é A', '𐐀 B'], scope='Two labels')
        self.assertTrue(check_gold('e\u0301\u0085A then 𐐀\nB', gold)['matches'])
        for actual in ['e A 𐐀 B', 'é\ufeffA 𐐀 B', 'é A 𐐀\u200dB', '𐐀 B é A']:
            self.assertFalse(check_gold(actual, gold)['matches'])

    def test_invalid_or_unbounded_declarations_are_rejected(self):
        gold = dict(kind='reading_order', anchors=['A', 'B'], scope='Labels only')
        changes = [dict(anchors=value) for value in
                   (None, 'A', [], [None], [''], ['\u0085'], ['x' * 2049], ['A'] * 257,
                    ['x' * 2048] * 17)]
        changes += [dict(scope=value) for value in (None, '', '\u001c', 'x' * 2049)]
        for change in changes:
            with self.subTest(change=change), self.assertRaises(ValueError):
                check_gold('', gold | change)


if __name__ == '__main__':
    unittest.main()
