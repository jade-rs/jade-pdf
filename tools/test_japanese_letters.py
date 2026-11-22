"""Unicode既定値や未確認データを、和字の字間の根拠へ昇格させない。"""
import unittest
from generate_japanese_letters import selected,generate,SOURCES


class JapaneseEvidenceTests(unittest.TestCase):
    def test_explicit_properties_do_not_borrow_missing_defaults(self):
        data=b'# @missing: 0000..10FFFF; Han\n0041; Latin\n7533; Han\n8ACB; Han # explicit\n'
        self.assertEqual(selected(data,lambda value:value=='Han'),{0x7533,0x8acb})
        for span in [b'D800..DFFF',b'110000',b'8ACB..7533']:
            with self.assertRaises(ValueError):selected(span+b'; Han\n',lambda value:True)

    def test_unreviewed_unicode_inputs_are_rejected_before_generation(self):
        with self.assertRaisesRegex(ValueError,'SHA-256 mismatch'):
            generate({name:b'# unreviewed\n' for name in SOURCES})


if __name__=='__main__':unittest.main()
