"""数値の意味を検査し、分数の字形差と誤った量を区別する。"""
import unittest
from semantic_gold import check_gold


class QuantityContextGoldTests(unittest.TestCase):
    gold = dict(kind='quantity_context', context='units of material', numerator=1,
                denominator=8, scope='One printed quantity only')

    def test_equivalent_notation_is_exact_without_float_or_global_compatibility_folding(self):
        for number in ['⅛', '1/8', '1 / 8', '1⁄8', '2/16', '0.125', '+1/8']:
            with self.subTest(number=number):
                value = check_gold(number+' units\nof material', self.gold)
                self.assertTrue(value['matches'])
                self.assertEqual(value['actual_values'], ['1/8'])
                self.assertEqual(value['expected_value'], '1/8')

    def test_wrong_values_and_partial_numeric_tokens_never_pass(self):
        for number in ['¼', '1/4', '11/8', '-1/8', '−1/8', '1 ⅛', '1⅛', '1 1/8',
                       '1.125', '1/0', '0.124999999999', 'v1/8', '1/8\ufeff',
                       '11111111111111111111111111111111111111111111111111111111111111111/8']:
            with self.subTest(number=number):
                self.assertFalse(check_gold(number+' units of material', self.gold)['matches'])

    def test_context_is_unique_and_canonical_without_erasing_marks(self):
        self.assertFalse(check_gold('1/8 units of material 1/8 units of material', self.gold)['matches'])
        self.assertFalse(check_gold('1/8 units of materials', self.gold)['matches'])
        self.assertFalse(check_gold('1/8 units of mater\u200dial', self.gold)['matches'])
        self.assertFalse(check_gold('1/8 UNITS of material', self.gold)['matches'])
        gold=self.gold | dict(context='café ℓ')
        self.assertTrue(check_gold('⅛ cafe\u0301 ℓ', gold)['matches'])
        self.assertFalse(check_gold('⅛ cafe l', gold)['matches'])

    def test_mixed_numbers_and_zero_preserve_full_value(self):
        for number in ['1 ⅛', '1⅛', '1 1/8', '9/8', '1.125']:
            self.assertTrue(check_gold(number+' units of material', self.gold | dict(numerator=9))['matches'])
        self.assertTrue(check_gold('0 units of material', self.gold | dict(numerator=0))['matches'])

    def test_equivalent_fraction_property(self):
        for denominator in range(1, 33):
            for numerator in range(0, 17):
                gold=self.gold | dict(numerator=numerator, denominator=denominator)
                for multiple in [1, 2, 7]:
                    self.assertTrue(check_gold(f'{numerator*multiple}/{denominator*multiple} units of material', gold)['matches'])

    def test_invalid_specs_and_budgets_are_rejected(self):
        changes=[dict(context=x) for x in [None, '', ' ', 'x'*257]]
        changes += [dict(scope=x) for x in [None, '', 'x'*2049]]
        changes += [dict(numerator=x) for x in [True, -1, 1.5, '1', 1_000_001]]
        changes += [dict(denominator=x) for x in [True, 0, -1, 1.5, 1_000_001]]
        for change in changes:
            with self.subTest(change=change), self.assertRaises(ValueError):
                check_gold('', self.gold | change)
        for text in ['x'*2_000_001, '1/8 units of material '*33]:
            with self.assertRaises(ValueError):
                check_gold(text, self.gold)


if __name__=='__main__':
    unittest.main()
